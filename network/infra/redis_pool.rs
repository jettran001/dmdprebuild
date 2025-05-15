use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore, Notify};
use redis::{Client, Connection, RedisError, RedisResult};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};
use tokio::time::timeout;
use std::sync::atomic::{AtomicBool, Ordering};

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
        self.connection.as_ref().expect("PooledConnection: Kết nối không tồn tại khi deref")
    }
}

impl DerefMut for PooledConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.connection.as_mut().expect("PooledConnection: Kết nối không tồn tại khi deref_mut")
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
    pub fn default() -> Result<Arc<Self>, RedisError> {
        Self::new(RedisPoolConfig::default())
    }
    
    /// Trả một kết nối về pool
    async fn return_connection(&self, conn: Connection) {
        let now = std::time::Instant::now();
        let mut available_connections = self._available_connections.lock().await;
        
        // Xóa các kết nối hết hạn (đã idle quá lâu)
        while let Some((_, created_at)) = available_connections.front() {
            if created_at.elapsed() > Duration::from_secs(self.config.idle_timeout_secs) {
                available_connections.pop_front();
                // Không cần release semaphore vì release được gọi khi connection bị drop
            } else {
                break;
            }
        }
        
        available_connections.push_back((conn, now));
        drop(available_connections);
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
        
        Err(last_error.unwrap_or_else(|| {
            RedisError::from((redis::ErrorKind::IoError, "Không thể tạo kết nối Redis"))
        }))
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
        let lock_result = timeout(Duration::from_secs(1), self._available_connections.lock()).await;
        let _available_connections = match lock_result {
            Ok(guard) => guard,
            Err(_) => return Err(PoolError::Timeout),
        };
        
        // Lấy permit từ semaphore với timeout
        let permit = match tokio::time::timeout(
            Duration::from_millis(self.config.connection_timeout_ms),
            self.semaphore.acquire(),
        ).await {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => return Err(PoolError::MaxRetriesExceeded(self.config.max_retries)),
            Err(_) => return Err(PoolError::Timeout),
        };
        
        // Thử lấy kết nối từ pool
        let connection = {
            let mut available_connections = self._available_connections.lock().await;
            
            // Xóa các kết nối hết hạn
            while let Some((_, created_at)) = available_connections.front() {
                if created_at.elapsed() > Duration::from_secs(self.config.idle_timeout_secs) {
                    available_connections.pop_front();
                } else {
                    break;
                }
            }
            
            if let Some((conn, _)) = available_connections.pop_front() {
                drop(available_connections);
                Ok(conn)
            } else {
                drop(available_connections);
                // Tạo kết nối mới
                self.create_connection().await.map_err(PoolError::Redis)
            }
        };
        
        // Tạo PooledConnection với kết nối và permit
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
        
        let pool = ConnectionPool::new(config).expect("Failed to create pool");
        
        // Kiểm tra health check
        pool.health_check().await.expect("Health check failed");
        
        // Lấy một số kết nối
        let conn1 = pool.get_connection().await?;
        let conn2 = pool.get_connection().await?;
        
        // Kiểm tra metrics
        let metrics = pool.metrics().await;
        assert_eq!(metrics.active_connections, 2);
        assert_eq!(metrics.idle_connections, 0);
        
        // Trả kết nối về pool
        drop(conn1);
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        let metrics = pool.metrics().await;
        assert_eq!(metrics.active_connections, 1);
        assert_eq!(metrics.idle_connections, 1);
        
        // Thực hiện lệnh Redis
        pool.with_connection(|conn| {
            redis::cmd("SET").arg("test_key").arg("test_value").query(conn)
        }).await?;
        
        let result: String = pool.with_connection(|conn| {
            redis::cmd("GET").arg("test_key").query(conn)
        }).await?;
        
        assert_eq!(result, "test_value");
        
        // Dọn dẹp
        pool.with_connection(|conn| {
            redis::cmd("DEL").arg("test_key").query(conn)
        }).await?;
        
        drop(conn2);
        pool.shutdown().await;
        
        Ok(())
    }
} 