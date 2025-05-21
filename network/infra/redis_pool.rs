use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore, Notify};
use redis::{Client, Connection, RedisError, RedisResult};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};
use tokio::time::timeout;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{warn, error};

/// Kết nối Redis được quản lý bởi pool
pub struct PooledConnection {
    connection: Option<Connection>,
    pool: Arc<ConnectionPool>,
}

impl Drop for PooledConnection {
    fn drop(&mut self) {
        if let Some(conn) = self.connection.take() {
            let pool = self.pool.clone();
            tokio::spawn(async move {
                pool.return_connection(conn).await;
            });
        }
    }
}

impl Deref for PooledConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        match self.connection.as_ref() {
            Some(conn) => conn,
            None => {
                // Ghi log và phản hồi panic có thông tin rõ ràng hơn
                tracing::error!("PooledConnection: Kết nối không tồn tại khi deref");
                // Trong trường hợp này, panic vẫn là cần thiết vì đây là lỗi lập trình không thể khôi phục
                // Tuy nhiên chúng ta đã ghi log và cung cấp thông tin chi tiết hơn
                panic!("PooledConnection: Kết nối không tồn tại khi deref. Đây là lỗi logic không thể khôi phục, vui lòng kiểm tra code.")
            }
        }
    }
}

impl DerefMut for PooledConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self.connection.as_mut() {
            Some(conn) => conn,
            None => {
                // Ghi log và phản hồi panic có thông tin rõ ràng hơn
                tracing::error!("PooledConnection: Kết nối không tồn tại khi deref_mut");
                // Trong trường hợp này, panic vẫn là cần thiết vì đây là lỗi lập trình không thể khôi phục
                // Tuy nhiên chúng ta đã ghi log và cung cấp thông tin chi tiết hơn
                panic!("PooledConnection: Kết nối không tồn tại khi deref_mut. Đây là lỗi logic không thể khôi phục, vui lòng kiểm tra code.")
            }
        }
    }
}

/// Cấu hình Redis pool
#[derive(Debug, Clone)]
pub struct RedisPoolConfig {
    /// URL kết nối Redis
    pub url: String,
    /// Số lượng kết nối tối đa
    pub max_connections: u32,
    /// Thời gian chờ lấy kết nối (milliseconds)
    pub connection_timeout_ms: u64,
    /// Thời gian tối đa một kết nối có thể tồn tại (seconds)
    pub max_lifetime_secs: u64,
    /// Thời gian một kết nối có thể idle (seconds)
    pub idle_timeout_secs: u64,
    /// Thời gian chờ giữa các lần thử lại kết nối (milliseconds)
    pub retry_interval_ms: u64,
    /// Số lần thử lại tối đa
    pub max_retries: u32,
    /// Tên người dùng (cho Redis 6.0+)
    pub username: Option<String>,
    /// Mật khẩu
    pub password: Option<String>,
}

impl Default for RedisPoolConfig {
    fn default() -> Self {
        RedisPoolConfig {
            url: "redis://127.0.0.1:6379".to_string(),
            max_connections: 20,
            connection_timeout_ms: 1000,
            max_lifetime_secs: 300,
            idle_timeout_secs: 60,
            retry_interval_ms: 500,
            max_retries: 3,
            username: None,
            password: None,
        }
    }
}

/// Pool kết nối Redis
pub struct ConnectionPool {
    /// Client Redis
    client: Client,
    /// Semaphore để giới hạn số lượng kết nối
    semaphore: Semaphore,
    /// Danh sách các kết nối sẵn sàng
    _available_connections: Mutex<VecDeque<(Connection, std::time::Instant)>>,
    /// Cấu hình cho pool
    config: RedisPoolConfig,
    /// Cờ dừng background cleanup
    stop_cleanup: Arc<AtomicBool>,
    /// Notify để dừng task
    notify_cleanup: Arc<Notify>,
}

/// Lỗi pool kết nối
#[derive(Debug, thiserror::Error)]
pub enum PoolError {
    #[error("Lỗi Redis: {0}")]
    Redis(#[from] RedisError),
    #[error("Timeout khi lấy kết nối")]
    Timeout,
    #[error("Không thể lấy kết nối sau {0} lần thử")]
    MaxRetriesExceeded(u32),
}

/// Interface Redis pool
#[async_trait]
pub trait RedisPool: Send + Sync + 'static {
    /// Lấy một kết nối từ pool
    async fn get_connection(&self) -> Result<PooledConnection, PoolError>;
    
    /// Thực hiện một pipeline lệnh Redis
    async fn with_connection<F, T>(&self, f: F) -> Result<T, PoolError>
    where
        F: FnOnce(&mut Connection) -> RedisResult<T> + Send + 'static,
        T: Send + 'static;
        
    /// Kiểm tra sức khỏe của pool
    async fn health_check(&self) -> Result<(), PoolError>;
    
    /// Lấy số liệu (metrics) về pool
    async fn metrics(&self) -> PoolMetrics;
    
    /// Đóng tất cả kết nối và dọn dẹp tài nguyên
    async fn shutdown(&self);
}

/// Thông số về pool
#[derive(Debug, Clone)]
pub struct PoolMetrics {
    /// Số lượng kết nối hiện tại đang sử dụng
    pub active_connections: usize,
    /// Số lượng kết nối sẵn sàng trong pool
    pub idle_connections: usize,
    /// Số lượng kết nối tối đa
    pub max_connections: usize,
    /// Số lượng yêu cầu chờ lấy kết nối
    pub waiting_requests: usize,
}

impl ConnectionPool {
    /// Tạo một pool kết nối mới với cấu hình được chỉ định
    pub fn new(config: RedisPoolConfig) -> Result<Arc<Self>, RedisError> {
        let client = redis::Client::open(config.url.as_str())?;
        let pool = Arc::new(ConnectionPool {
            client,
            semaphore: Semaphore::new(config.max_connections as usize),
            _available_connections: Mutex::new(VecDeque::with_capacity(config.max_connections as usize)),
            config,
            stop_cleanup: Arc::new(AtomicBool::new(false)),
            notify_cleanup: Arc::new(Notify::new()),
        });
        // Spawn background cleanup task
        let pool_clone = pool.clone();
        tokio::spawn(async move {
            loop {
                if pool_clone.stop_cleanup.load(Ordering::SeqCst) {
                    break;
                }
                pool_clone.clean_expired_connections().await;
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {},
                    _ = pool_clone.notify_cleanup.notified() => { break; }
                }
            }
        });
        Ok(pool)
    }
    
    /// Tạo pool với cấu hình mặc định
    pub fn create_default() -> Result<Arc<Self>, RedisError> {
        Self::new(RedisPoolConfig::default())
    }
    
    /// Trả một kết nối về pool
    async fn return_connection(&self, conn: Connection) {
        let now = std::time::Instant::now();
        
        // Sử dụng timeout khi lấy lock để tránh deadlock
        let lock_result = tokio::time::timeout(
            Duration::from_secs(1),
            self._available_connections.lock()
        ).await;
        
        match lock_result {
            Ok(mut available_connections) => {
                // Giảm thiểu công việc trong khi giữ lock
                // Chỉ kiểm tra xem có quá nhiều kết nối đang idle không
                let should_clean_expired = available_connections.len() > (self.config.max_connections as usize / 2) &&
                                          available_connections.front()
                                              .map(|(_, time)| time.elapsed() > Duration::from_secs(self.config.idle_timeout_secs))
                                              .unwrap_or(false);
                                          
                if should_clean_expired {
                    // Chỉ loại bỏ tối đa 1 kết nối cũ mỗi lần để tránh giữ lock quá lâu
                    if let Some((_, time)) = available_connections.front() {
                        if time.elapsed() > Duration::from_secs(self.config.idle_timeout_secs) {
                            available_connections.pop_front();
                        }
                    }
                }
                
                // Thêm kết nối hiện tại vào pool
                available_connections.push_back((conn, now));
                
                // Nếu có quá nhiều kết nối, hãy lên lịch dọn dẹp bất đồng bộ
                if should_clean_expired {
                    let self_clone = self.clone();
                    tokio::spawn(async move {
                        self_clone.clean_expired_connections().await;
                    });
                }
            },
            Err(_) => {
                // Nếu không thể lấy lock, không trả kết nối về pool mà để nó tự đóng
                warn!("Không thể trả kết nối về pool do timeout khi lấy lock");
            }
        }
    }
    
    /// Tạo một kết nối mới
    async fn create_connection(&self) -> Result<Connection, RedisError> {
        let mut last_error = None;
        
        for _ in 0..self.config.max_retries {
            match self.client.get_connection() {
                Ok(conn) => return Ok(conn),
                Err(e) => {
                    last_error = Some(e);
                    tokio::time::sleep(Duration::from_millis(self.config.retry_interval_ms)).await;
                }
            }
        }
        
        // Thay vì sử dụng unwrap, chúng ta sử dụng match để xử lý an toàn
        match last_error {
            Some(error) => Err(error),
            None => {
                tracing::error!("Không thể tạo kết nối Redis sau nhiều lần thử");
                Err(RedisError::from((redis::ErrorKind::IoError, "Không thể tạo kết nối Redis sau nhiều lần thử")))
            }
        }
    }
    
    /// Loại bỏ các kết nối đã hết hạn
    async fn clean_expired_connections(&self) -> usize {
        let mut available_connections = self._available_connections.lock().await;
        let before_len = available_connections.len();
        
        // Xóa các kết nối idle quá lâu
        available_connections.retain(|(_, created_at)| {
            created_at.elapsed() <= Duration::from_secs(self.config.idle_timeout_secs)
        });
        
        before_len - available_connections.len()
    }
}

#[async_trait]
impl RedisPool for ConnectionPool {
    async fn get_connection(&self) -> Result<PooledConnection, PoolError> {
        // Lấy permit từ semaphore với timeout - thực hiện trong scope riêng
        let permit = match tokio::time::timeout(
            Duration::from_millis(self.config.connection_timeout_ms),
            self.semaphore.acquire(),
        ).await {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => return Err(PoolError::MaxRetriesExceeded(self.config.max_retries)),
            Err(_) => return Err(PoolError::Timeout),
        };
        
        // Thử lấy kết nối từ pool trong scope riêng
        let connection = {
            // Sử dụng scope riêng cho việc lock _available_connections
            let connection_from_pool = {
                // Timeout để tránh deadlock khi lấy lock
                let lock_result = timeout(Duration::from_secs(1), self._available_connections.lock()).await;
                let mut available_connections = match lock_result {
                    Ok(guard) => guard,
                    Err(_) => {
                        // Nếu không lấy được lock, trả lại permit và báo lỗi
                        drop(permit);
                        return Err(PoolError::Timeout);
                    }
                };
                
                // Xóa các kết nối hết hạn
                while let Some((_, created_at)) = available_connections.front() {
                    if created_at.elapsed() > Duration::from_secs(self.config.idle_timeout_secs) {
                        available_connections.pop_front();
                    } else {
                        break;
                    }
                }
                
                // Lấy kết nối từ pool nếu có
                available_connections.pop_front().map(|(conn, _)| conn)
            }; // drop available_connections lock ở đây
            
            // Tạo kết nối mới nếu không có sẵn trong pool - làm việc này ngoài lock để tránh giữ lock quá lâu
            if let Some(conn) = connection_from_pool {
                Ok(conn)
            } else {
                // Tạo kết nối mới - thực hiện ngoài lock để không block threads khác
                self.create_connection().await.map_err(PoolError::Redis)
            }
        };
        
        // Xử lý kết quả và trả về PooledConnection
        match connection {
            Ok(conn) => {
                // Không giải phóng permit, nó sẽ được tự động giải phóng khi PooledConnection được drop
                permit.forget();
                
                Ok(PooledConnection {
                    connection: Some(conn),
                    pool: Arc::new(self.clone()),
                })
            },
            Err(err) => {
                // Trả lại permit vì chúng ta không lấy được kết nối
                drop(permit);
                Err(err)
            }
        }
    }
    
    async fn with_connection<F, T>(&self, f: F) -> Result<T, PoolError>
    where
        F: FnOnce(&mut Connection) -> RedisResult<T> + Send + 'static,
        T: Send + 'static,
    {
        let mut conn = self.get_connection().await?;
        f(&mut conn).map_err(PoolError::Redis)
    }
    
    async fn health_check(&self) -> Result<(), PoolError> {
        self.with_connection(|conn| {
            redis::cmd("PING").query::<String>(conn)
        }).await.map(|_| ())
    }
    
    async fn metrics(&self) -> PoolMetrics {
        let available = self._available_connections.lock().await;
        let idle_count = available.len();
        drop(available);
        
        let max = self.config.max_connections as usize;
        let active = max - self.semaphore.available_permits();
        let waiting = active.saturating_sub(max);
        
        PoolMetrics {
            active_connections: active,
            idle_connections: idle_count,
            max_connections: max,
            waiting_requests: waiting,
        }
    }
    
    async fn shutdown(&self) {
        self.stop_cleanup.store(true, Ordering::SeqCst);
        self.notify_cleanup.notify_waiters();
        let mut available_connections = self._available_connections.lock().await;
        available_connections.clear();
    }
}

impl Clone for ConnectionPool {
    fn clone(&self) -> Self {
        ConnectionPool {
            client: self.client.clone(),
            semaphore: Semaphore::new(self.config.max_connections as usize),
            _available_connections: Mutex::new(VecDeque::with_capacity(self.config.max_connections as usize)),
            config: self.config.clone(),
            stop_cleanup: Arc::new(AtomicBool::new(false)),
            notify_cleanup: Arc::new(Notify::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    
    #[tokio::test]
    async fn test_connection_pool() -> Result<(), Box<dyn std::error::Error>> {
        let config = RedisPoolConfig {
            url: "redis://127.0.0.1:6379".to_string(),
            max_connections: 5,
            connection_timeout_ms: 1000,
            max_lifetime_secs: 300,
            idle_timeout_secs: 60,
            retry_interval_ms: 500,
            max_retries: 3,
            username: None,
            password: None,
        };
        
        let pool = ConnectionPool::new(config)?;
        
        pool.health_check().await?;
        
        let conn1 = pool.get_connection().await?;
        let conn2 = pool.get_connection().await?;
        
        let metrics = pool.metrics().await;
        assert_eq!(metrics.active_connections, 2);
        assert_eq!(metrics.idle_connections, 0);
        
        drop(conn1);
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        let metrics = pool.metrics().await;
        assert_eq!(metrics.active_connections, 1);
        assert_eq!(metrics.idle_connections, 1);
        
        pool.with_connection(|conn| {
            redis::cmd("SET").arg("test_key").arg("test_value").query(conn)
        }).await?;
        
        let result: String = pool.with_connection(|conn| {
            redis::cmd("GET").arg("test_key").query(conn)
        }).await?;
        
        assert_eq!(result, "test_value");
        
        pool.with_connection(|conn| {
            redis::cmd("DEL").arg("test_key").query(conn)
        }).await?;
        
        drop(conn2);
        pool.shutdown().await;
        
        Ok(())
    }
} 