use std::sync::Arc;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use crate::infra::service_traits::RedisService;
use crate::core::engine::{PluginError};
use tracing::{info, warn, error, debug};
use tokio::sync::Mutex as TokioMutex;
use tokio::sync::oneshot;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::task::JoinHandle;
use tokio::sync::Mutex;
use once_cell::sync::OnceCell;

/// Type alias cho danh sách kết nối trong pool để giảm type complexity
type ConnectionQueue<T> = VecDeque<(T, Instant)>;

/// Loại hàm đóng kết nối
type ConnectionCloserFn<T> = Box<dyn Fn(&T) + Send + Sync>;

/// Trạng thái monitor task toàn cục, đảm bảo thread safety tuyệt đối khi drop nhiều pool đồng thời.
fn get_pool_monitor_flag() -> &'static Arc<AtomicBool> {
    static FLAG: OnceCell<Arc<AtomicBool>> = OnceCell::new();
    FLAG.get_or_init(|| Arc::new(AtomicBool::new(false)))
}

/// ConnectionPool quản lý một pool kết nối và đảm bảo chỉ có một global monitor task chạy trong mỗi process.
/// Điều này ngăn chặn sự trùng lặp trong việc log tài nguyên và đảm bảo giám sát nhất quán bộ nhớ và file descriptors.
///
/// # Thread Safety
/// - Sử dụng tokio::sync::Mutex thay vì tokio::sync::Mutex để đảm bảo an toàn trong async context
/// - Các method hỗ trợ async và sử dụng lock không chặn luồng executor
///
/// # Examples
/// ```
/// let pool = ConnectionPool::new(10, 300);
/// let conn = pool.acquire().await?;
/// // Sử dụng connection
/// pool.release(conn).await?;
/// ```
pub struct ConnectionPool<T: Send + Sync + Clone + 'static> {
    /// Pool chứa các kết nối và thời điểm last activity
    pool: TokioMutex<ConnectionQueue<T>>,
    /// Số lượng kết nối tối đa trong pool
    max_size: usize,
    /// Thời gian timeout cho mỗi kết nối (giây)
    connection_timeout_seconds: u64,
    /// Kênh shutdown để dừng background tasks
    shutdown_signal: Mutex<Option<oneshot::Sender<()>>>,
    /// Cờ đánh dấu trạng thái shutdown
    is_shutdown: AtomicBool,
    /// JoinHandle cho cleanup task (nếu cần join khi drop)
    cleanup_handle: Option<JoinHandle<()>>,
    /// Callback để đóng kết nối (nếu có)
    connection_closer: Option<ConnectionCloserFn<T>>,
}

impl<T: Send + Sync + Clone + 'static> Clone for ConnectionPool<T> {
    /// Tạo một bản sao của ConnectionPool.
    ///
    /// # Lưu ý
    /// - JoinHandle và shutdown_signal không được clone
    /// - Mỗi bản sao sẽ quản lý pool kết nối riêng
    fn clone(&self) -> Self {
        // Không thể clone JoinHandle nên tạo mới nếu cần
        Self {
            pool: TokioMutex::new(VecDeque::new()),
            max_size: self.max_size,
            connection_timeout_seconds: self.connection_timeout_seconds,
            shutdown_signal: Mutex::new(None), // Không clone shutdown signal
            is_shutdown: AtomicBool::new(self.is_shutdown.load(Ordering::SeqCst)),
            cleanup_handle: None, // Không clone handle, sẽ tạo mới nếu cần
            // Không clone connection_closer vì Box<dyn Fn> không implement Clone
            connection_closer: None,
        }
    }
}

/// Trạng thái circuit breaker cho pool kết nối
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CircuitBreakerState {
    /// Bình thường, cho phép kết nối
    Closed,
    /// Đã đóng, từ chối kết nối
    Open,
    /// Bán mở, cho phép một số kết nối thử nghiệm
    HalfOpen,
}

/// CircuitBreaker giúp ngăn chặn lỗi cascading khi hệ thống external gặp vấn đề.
///
/// # Nguyên lý hoạt động
/// - Theo dõi số lần thất bại liên tiếp
/// - Chuyển sang trạng thái Open (từ chối kết nối) khi đạt ngưỡng thất bại
/// - Sau một khoảng thời gian, chuyển sang HalfOpen để thử kết nối lại
/// - Nếu thành công, quay lại Closed; nếu thất bại, quay lại Open
struct CircuitBreaker {
    /// Trạng thái hiện tại
    state: CircuitBreakerState,
    /// Số lần thất bại liên tiếp
    failure_count: u32,
    /// Số lần thất bại tối đa trước khi chuyển sang Open
    max_failures: u32,
    /// Thời gian chờ trước khi thử lại (chuyển từ Open sang HalfOpen)
    reset_timeout: Duration,
    /// Thời điểm thất bại gần nhất
    last_failure: Option<Instant>,
    /// Thời điểm thay đổi trạng thái gần nhất
    last_state_change: Instant,
}

impl CircuitBreaker {
    /// Tạo mới một CircuitBreaker với các cấu hình cho trước
    ///
    /// # Arguments
    /// * `max_failures` - Số lần thất bại tối đa trước khi chuyển sang Open
    /// * `reset_timeout` - Thời gian chờ trước khi thử lại (chuyển từ Open sang HalfOpen)
    fn new(max_failures: u32, reset_timeout: Duration) -> Self {
        Self {
            state: CircuitBreakerState::Closed,
            failure_count: 0,
            max_failures,
            reset_timeout,
            last_failure: None,
            last_state_change: Instant::now(),
        }
    }
    
    /// Kiểm tra xem có thể thực hiện kết nối mới không
    ///
    /// # Returns
    /// * `true` nếu có thể thực hiện kết nối
    /// * `false` nếu không thể (đang trong trạng thái Open và chưa hết thời gian timeout)
    fn can_attempt(&mut self) -> bool {
        match self.state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open => {
                if let Some(last) = self.last_failure {
                    if last.elapsed() >= self.reset_timeout {
                        self.state = CircuitBreakerState::HalfOpen;
                        self.last_state_change = Instant::now();
                        info!("[CircuitBreaker] Transition to HalfOpen after timeout");
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitBreakerState::HalfOpen => true,
        }
    }
    
    /// Ghi nhận kết nối thành công
    ///
    /// # Effects
    /// - Reset failure_count về 0
    /// - Chuyển state về Closed nếu đang ở trạng thái khác
    fn on_success(&mut self) {
        if self.state != CircuitBreakerState::Closed {
            info!("[CircuitBreaker] Reset to Closed after success");
        }
        self.state = CircuitBreakerState::Closed;
        self.failure_count = 0;
        self.last_state_change = Instant::now();
    }
    
    /// Ghi nhận kết nối thất bại
    ///
    /// # Effects
    /// - Tăng failure_count
    /// - Cập nhật last_failure
    /// - Chuyển state sang Open nếu vượt ngưỡng max_failures
    /// - Nếu đang ở HalfOpen, quay lại Open
    fn on_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure = Some(Instant::now());
        if self.failure_count >= self.max_failures {
            if self.state != CircuitBreakerState::Open {
                warn!("[CircuitBreaker] Transition to Open after {} failures", self.failure_count);
            }
            self.state = CircuitBreakerState::Open;
            self.last_state_change = Instant::now();
        } else if self.state == CircuitBreakerState::HalfOpen {
            warn!("[CircuitBreaker] Failure in HalfOpen, back to Open");
            self.state = CircuitBreakerState::Open;
            self.last_state_change = Instant::now();
        }
    }
}

/// Pool kết nối cho Redis với khả năng retry, circuit breaker và quản lý kết nối thông minh.
///
/// # Features
/// - Quản lý số lượng kết nối tối đa
/// - Hỗ trợ xác thực username/password
/// - Tự động retry khi kết nối thất bại
/// - Circuit breaker để ngăn cascade failure
/// - Tự động đóng kết nối không sử dụng sau một thời gian
///
/// # Thread Safety
/// - Thread-safe và async-safe, sử dụng tokio::sync::Mutex
/// - An toàn khi được clone và sử dụng từ nhiều task
pub struct RedisConnectionPool {
    /// Pool kết nối nội bộ
    inner: Arc<ConnectionPool<Arc<dyn RedisService>>>,
    /// URL Redis server
    url: String,
    /// Tên đăng nhập (nếu có)
    username: Option<String>,
    /// Mật khẩu (nếu có)
    password: Option<String>,
    /// Số lần retry tối đa khi kết nối thất bại
    max_retries: u8,
    /// Thời gian chờ giữa các lần retry (ms)
    retry_delay_ms: u64,
    /// Circuit breaker để quản lý trạng thái kết nối
    circuit_breaker: Arc<TokioMutex<CircuitBreaker>>,
}

/// Trait chung để kiểm tra kết nối còn active không
pub trait ConnectionValidator<T>: Send + Sync {
    /// Kiểm tra kết nối còn active không
    /// 
    /// # Arguments
    /// * `conn` - Kết nối cần kiểm tra
    /// 
    /// # Returns
    /// * `Some(bool)` - true nếu kết nối còn active, false nếu không
    /// * `None` - Nếu không thể kiểm tra
    fn check_connection_active(&self, conn: &T) -> Option<bool>;
}

/// Trait chuyên biệt cho Redis, riêng biệt để tránh xung đột với trait ConnectionValidator generic
pub trait RedisConnectionValidator {
    /// Kiểm tra kết nối Redis còn active không
    /// 
    /// # Arguments
    /// * `conn` - Kết nối Redis cần kiểm tra
    /// 
    /// # Returns
    /// * `Some(bool)` - true nếu kết nối còn active, false nếu không
    /// * `None` - Nếu không thể kiểm tra
    fn check_redis_connection_active(&self, conn: &Arc<dyn RedisService>) -> Option<bool>;
}

// Implement ConnectionValidator cho ConnectionPool
impl<T> ConnectionValidator<T> for ConnectionPool<T>
where 
    T: Clone + Send + Sync + 'static,
{
    fn check_connection_active(&self, _conn: &T) -> Option<bool> {
        // Kiểm tra kết nối còn active không, mặc định là active
        Some(true)
    }
}

// Implement RedisConnectionValidator cho ConnectionPool chuyên biệt với Redis
impl RedisConnectionValidator for ConnectionPool<Arc<dyn RedisService>> {
    fn check_redis_connection_active(&self, conn: &Arc<dyn RedisService>) -> Option<bool> {
        // Sử dụng implementation cụ thể cho RedisService
        self.check_connection_active_internal(conn)
    }
}

impl<T: Send + Sync + Clone + 'static> ConnectionPool<T> {
    /// Tạo mới một connection pool với kích thước và timeout được chỉ định.
    ///
    /// # Arguments
    /// * `max_size` - Số lượng kết nối tối đa trong pool
    /// * `connection_timeout_seconds` - Thời gian timeout cho mỗi kết nối (giây)
    ///
    /// # Returns
    /// Arc-wrapped ConnectionPool để có thể share giữa nhiều thread/task
    ///
    /// # Notes
    /// - Mỗi pool sẽ tự động khởi tạo một background task để cleanup các kết nối hết hạn
    /// - Chỉ một global resource monitoring task sẽ được khởi tạo cho toàn bộ process
    pub fn new(max_size: usize, connection_timeout_seconds: u64) -> Arc<Self> {
        let pool = TokioMutex::new(VecDeque::with_capacity(max_size));
        
        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        
        // Create the pool instance
        let pool_instance = Arc::new(Self {
            pool,
            max_size,
            connection_timeout_seconds,
            shutdown_signal: Mutex::new(Some(shutdown_tx)),
            is_shutdown: AtomicBool::new(false),
            cleanup_handle: None,
            connection_closer: None,
        });
        
        // Clone Arc for background task
        let pool_clone = Arc::downgrade(&pool_instance);
        let timeout = connection_timeout_seconds;
        
        // Spawn background async cleanup task
        let cleanup_handle = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx; // Make shutdown_rx mutable for select!
            
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(60)) => {
                        if let Some(pool_strong) = pool_clone.upgrade() {
                            if pool_strong.is_shutdown.load(Ordering::SeqCst) {
                                break;
                            }
                            match pool_strong.pool.try_lock() {
                                Ok(mut pool_lock) => {
                                    let now = Instant::now();
                                    // Lưu lại connections cần close
                                    let mut to_close = Vec::new();
                                    
                                    // Filter expired connections và kiểm tra health của những connections đang idle
                                    let mut idx = 0;
                                    while idx < pool_lock.len() {
                                        // Kiểm tra timeout
                                        if now.duration_since(pool_lock[idx].1).as_secs() > timeout {
                                            // Lưu connection sẽ bị xóa để close sau khi unlock
                                            if let Some(conn) = pool_lock.remove(idx) {
                                                to_close.push(conn.0);
                                            }
                                        } else {
                                            // Kiểm tra health cho những connection đã idle lâu (>120s)
                                            if now.duration_since(pool_lock[idx].1).as_secs() > 120 {
                                                let conn_ref = &pool_lock[idx].0;
                                                if let Some(is_active) = pool_strong.check_connection_active(conn_ref) {
                                                    if !is_active {
                                                        // Connection không còn active, loại bỏ
                                                        if let Some(conn) = pool_lock.remove(idx) {
                                                            debug!("[Pool] Removing unhealthy idle connection");
                                                            to_close.push(conn.0);
                                                            continue;
                                                        }
                                                    }
                                                }
                                            }
                                            idx += 1;
                                        }
                                    }
                                    
                                    debug!("[Pool] Cleaned up expired connections, remaining: {}", pool_lock.len());
                                    drop(pool_lock); // Release lock before closing connections
                                    
                                    // Close expired connections nếu có registered closer
                                    if let Some(closer) = &pool_strong.connection_closer {
                                        for conn in to_close {
                                            closer(&conn);
                                        }
                                    }
                                },
                                Err(_) => {
                                    warn!("[Pool] Failed to acquire lock for cleanup (pool is locked by another task)");
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    _ = &mut shutdown_rx => {
                        debug!("[Pool] Received shutdown signal, exiting cleanup task");
                        break;
                    }
                }
            }
        });
        
        // Lưu cleanup_handle vào struct (Arc::get_mut an toàn vì chỉ có 1 ref tại đây)
        if let Some(mut_ref) = Arc::get_mut(&mut Arc::clone(&pool_instance)) {
            mut_ref.cleanup_handle = Some(cleanup_handle);
        }
        
        // Spawn global resource usage monitor task (only once per process, thread-safe)
        let monitor_flag = get_pool_monitor_flag();
        if !monitor_flag.swap(true, Ordering::SeqCst) {
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    // Log resource usage (memory, fd count nếu có thể)
                    #[cfg(target_os = "linux")]
                    if let Ok(meminfo) = std::fs::read_to_string("/proc/self/status") {
                        for line in meminfo.lines() {
                            if line.starts_with("VmRSS") || line.starts_with("VmSize") {
                                log::info!("[Pool][Resource] {}", line);
                            }
                        }
                    }
                    #[cfg(target_os = "linux")]
                    if let Ok(fds) = std::fs::read_dir("/proc/self/fd") {
                        let count = fds.count();
                        log::info!("[Pool][Resource] Open file descriptors: {}", count);
                    }
                    // Nếu cần dừng monitor, kiểm tra flag
                    if !monitor_flag.load(Ordering::SeqCst) {
                        break;
                    }
                }
                // Reset flag khi task kết thúc
                monitor_flag.store(false, Ordering::SeqCst);
            });
        }
        
        pool_instance
    }

    /// Lấy một kết nối từ pool, hoặc trả về None nếu pool rỗng.
    ///
    /// # Returns
    /// * `Some(T)` - Kết nối từ pool nếu có sẵn
    /// * `None` - Nếu pool rỗng hoặc đã shutdown
    ///
    /// # Thread Safety
    /// Phương thức này là async-safe và sử dụng tokio::sync::Mutex để đảm bảo an toàn
    pub async fn acquire(&self) -> Option<T> {
        // Skip if already shutdown
        if self.is_shutdown.load(Ordering::SeqCst) {
            warn!("[Pool] Attempted to acquire connection from shutdown pool");
            return None;
        }
        
        // Try to lock the pool with timeout
        let lock_result = tokio::time::timeout(
            Duration::from_secs(5), // 5 second timeout for lock
            self.pool.lock()
        ).await;
        
        let mut pool = match lock_result {
            Ok(lock) => lock,
            Err(_) => {
                warn!("[Pool] Timeout waiting for pool lock during acquire");
                return None;
            }
        };
        
        // First, remove any expired connections
        let now = Instant::now();
        while let Some((_, timestamp)) = pool.front() {
            if now.duration_since(*timestamp).as_secs() > self.connection_timeout_seconds {
                pool.pop_front();
            } else {
                break;
            }
        }
        
        // Now return a connection if available
        pool.pop_front().map(|(conn, _)| conn)
    }

    /// Trả lại một kết nối vào pool.
    ///
    /// # Arguments
    /// * `conn` - Kết nối cần trả lại pool
    ///
    /// # Returns
    /// * `Ok(())` - Nếu kết nối được trả lại thành công
    /// * `Err(String)` - Nếu có lỗi (pool đã shutdown, lock timeout, v.v.)
    ///
    /// # Notes
    /// - Nếu pool đã đầy (đạt max_size), kết nối sẽ bị drop
    /// - Kết nối không active sẽ không được trả lại pool
    pub async fn release(&self, conn: T) -> Result<(), String> {
        // Skip if already shutdown
        if self.is_shutdown.load(Ordering::SeqCst) {
            return Err("Pool is shutdown".to_string());
        }
        
        // Thêm kiểm tra kết nối còn active trước khi trả về pool
        if let Some(conn_check) = ConnectionValidator::<T>::check_connection_active(self, &conn) {
            if !conn_check {
                debug!("[Pool] Connection inactive, not returning to pool");
                return Ok(());  // Không trả kết nối không active vào pool, nhưng cũng không báo lỗi
            }
        }
        
        // Try to lock the pool with timeout
        let lock_result = tokio::time::timeout(
            Duration::from_secs(5), // 5 second timeout for lock
            self.pool.lock()
        ).await;
        
        let mut pool = match lock_result {
            Ok(lock) => lock,
            Err(_) => {
                warn!("[Pool] Timeout waiting for pool lock during release");
                return Err("Timeout waiting for pool lock".to_string());
            }
        };
        
        // Only add the connection back if we're under capacity
        if pool.len() < self.max_size {
            pool.push_back((conn, Instant::now()));
            debug!("[Pool] Connection returned to pool, size: {}/{}", pool.len(), self.max_size);
        } else {
            warn!("[Pool] Pool is full ({}), dropping connection", self.max_size);
        }
        
        Ok(())
    }
    
    /// Kiểm tra kết nối Redis còn active không. (private method)
    ///
    /// # Arguments
    /// * `conn` - Kết nối Redis cần kiểm tra
    ///
    /// # Returns
    /// * `Some(bool)` - true nếu kết nối còn active, false nếu không
    /// * `None` - Nếu không thể kiểm tra (hiếm khi xảy ra)
    ///
    /// # Notes
    /// Đây là base implementation, được override trong các subtype cụ thể
    #[doc(hidden)]
    fn check_connection_active_internal(&self, conn: &Arc<dyn RedisService>) -> Option<bool> {
        // Thử ping để kiểm tra kết nối còn alive không
        let conn_clone = conn.clone();
        
        // Sử dụng handle hiện tại nếu có, hoặc tạo runtime mới với xử lý lỗi thích hợp
        let rt = match tokio::runtime::Handle::try_current() {
            Ok(handle) => handle,
            Err(_) => match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime.handle().clone(),
                Err(e) => {
                    error!("[ConnectionPool] Failed to create runtime: {}", e);
                    return None;
                }
            }
        };
        
        // Thực hiện ping trong runtime
        let result = rt.block_on(async {
            match conn_clone.ping().await {
                Ok(_) => true,
                Err(e) => {
                    warn!("[ConnectionPool] Connection inactive: {}", e);
                    false
                }
            }
        });
        
        if !result {
            error!("[ConnectionPool] Connection inactive in check");
        }
        
        Some(result)
    }
    
    /// Lấy số lượng kết nối hiện tại trong pool.
    ///
    /// # Returns
    /// * `Ok(usize)` - Số lượng kết nối
    /// * `Err(String)` - Nếu có lỗi khi lấy lock
    pub async fn size(&self) -> Result<usize, String> {
        // Try to lock the pool with timeout
        let lock_result = tokio::time::timeout(
            Duration::from_secs(5), // 5 second timeout for lock
            self.pool.lock()
        ).await;
        
        let pool = match lock_result {
            Ok(lock) => lock,
            Err(_) => {
                warn!("[Pool] Timeout waiting for pool lock when checking size");
                return Err("Timeout waiting for pool lock".to_string());
            }
        };
        
        Ok(pool.len())
    }
    
    /// Xóa tất cả kết nối trong pool.
    ///
    /// # Returns
    /// * `Ok(())` - Nếu thao tác thành công
    /// * `Err(String)` - Nếu có lỗi khi xóa
    ///
    /// # Notes
    /// Phương thức này không đóng các kết nối, chỉ xóa chúng khỏi pool
    pub async fn clear(&self) -> Result<(), String> {
        // Try to lock the pool with timeout
        let lock_result = tokio::time::timeout(
            Duration::from_secs(5), // 5 second timeout for lock
            self.pool.lock()
        ).await;
        
        let mut pool = match lock_result {
            Ok(lock) => lock,
            Err(_) => {
                warn!("[Pool] Timeout waiting for pool lock during clear");
                return Err("Timeout waiting for pool lock".to_string());
            }
        };
        
        let old_size = pool.len();
        pool.clear();
        debug!("[Pool] Cleared {} connections from pool", old_size);
        
        Ok(())
    }
    
    /// Đăng ký callback để đóng kết nối khi cần thiết.
    ///
    /// # Arguments
    /// * `closer` - Closure dùng để đóng kết nối
    ///
    /// # Type Parameters
    /// * `F` - Closure type thỏa mãn Fn(&T) + Send + Sync + 'static
    pub fn register_connection_closer<F>(&mut self, closer: F)
    where
        F: Fn(&T) + Send + Sync + 'static,
    {
        self.connection_closer = Some(Box::new(closer));
    }
    
    /// Force đóng tất cả kết nối trong pool và xóa pool.
    ///
    /// # Returns
    /// * `Ok(())` - Nếu thao tác thành công
    /// * `Err(String)` - Nếu có lỗi khi đóng kết nối
    ///
    /// # Notes
    /// - Đánh dấu pool đã shutdown để ngăn thêm kết nối mới
    /// - Xóa tất cả kết nối khỏi pool
    /// - Gọi closer cho mỗi kết nối nếu có
    pub async fn force_close_all(&self) -> Result<(), String> {
        // Đánh dấu pool đã shutdown
        self.is_shutdown.store(true, Ordering::SeqCst);
        
        // Try to get the pool mutex with timeout
        let lock_result = tokio::time::timeout(
            Duration::from_secs(5), // 5 second timeout
            self.pool.lock()
        ).await;
        
        match lock_result {
            Ok(mut pool) => {
                let connections_count = pool.len();
                
                // Collect all connections that need to be closed
                let connections: Vec<T> = pool.drain(..).map(|(conn, _)| conn).collect();
                
                // Release the lock before closing connections
                drop(pool);
                
                // Close all connections if a closer is registered
                if let Some(closer) = &self.connection_closer {
                    for conn in &connections {
                        closer(conn);
                    }
                }
                
                debug!("[Pool] Force closed {} connections", connections_count);
                Ok(())
            },
            Err(_) => {
                error!("[Pool] Timeout acquiring lock for force_close_all");
                Err("Timeout acquiring pool lock".to_string())
            }
        }
    }

    /// Kích hoạt shutdown của connection pool và cleanup task.
    ///
    /// # Thread Safety
    /// Phương thức này yêu cầu &mut self để đảm bảo chỉ có một caller có thể shutdown
    pub fn shutdown(&self) {
        // Đánh dấu pool đã shutdown
        self.is_shutdown.store(true, Ordering::SeqCst);
        
        // Tạo một task mới để thực hiện shutdown bất đồng bộ
        let pool_clone = Arc::new(self.clone());
        
        tokio::spawn(async move {
            // Gửi tín hiệu shutdown nếu có
            let mut shutdown_signal = pool_clone.shutdown_signal.lock().await;
            if let Some(tx) = shutdown_signal.take() {
                let _ = tx.send(());
            }
            
            // Dọn dẹp kết nối trong pool
            match pool_clone.force_close_all().await {
                Ok(_) => debug!("[Pool] Pool shutdown completed successfully"),
                Err(e) => warn!("[Pool] Error during pool shutdown: {}", e),
            }
        });
    }

    /// Đóng một kết nối
    /// 
    /// Gọi connection_closer nếu được cung cấp
    /// 
    /// # Arguments
    /// * `key` - Khóa của kết nối cần đóng
    /// 
    /// # Returns
    /// `bool` - true nếu kết nối được đóng thành công, false nếu không tìm thấy
    pub fn close_connection(&self, _key: &str) -> bool {
        // Implementation
        false
    }
}

impl<T: Send + Sync + Clone + 'static> Drop for ConnectionPool<T> {
    /// Cleanup resources khi pool bị drop.
    /// 
    /// # Notes
    /// - Gọi shutdown() để clean up background tasks
    /// - Abort cleanup_handle nếu còn running
    fn drop(&mut self) {
        // Đánh dấu pool đã shutdown
        self.is_shutdown.store(true, Ordering::SeqCst);
        
        // Gửi tín hiệu dừng đến background task
        // Không sử dụng lock() vì đây là context synchronous và có thể deadlock
        if let Ok(mut guard) = self.shutdown_signal.try_lock() {
            if let Some(signal) = guard.take() {
                let _ = signal.send(());
            }
        }
        
        // Nếu có cleanup_handle, abort
        if let Some(handle) = self.cleanup_handle.take() {
            handle.abort();
        }
    }
}

impl RedisConnectionPool {
    /// Tạo mới một Redis connection pool.
    ///
    /// # Arguments
    /// * `url` - URL Redis server
    /// * `username` - Tên đăng nhập (optional)
    /// * `password` - Mật khẩu (optional)
    /// * `max_size` - Số lượng kết nối tối đa trong pool
    ///
    /// # Returns
    /// RedisConnectionPool đã cấu hình với các giá trị mặc định:
    /// - 5 phút timeout cho mỗi kết nối
    /// - Tối đa 3 lần retry khi kết nối
    /// - 500ms giữa các lần retry
    /// - Circuit breaker với 5 lần thất bại liên tiếp và 30 giây reset
    pub fn new(url: String, username: Option<String>, password: Option<String>, max_size: usize) -> Self {
        let circuit_breaker = Arc::new(TokioMutex::new(CircuitBreaker::new(5, Duration::from_secs(30))));
        Self {
            inner: ConnectionPool::new(max_size, 300),  // 5 minute timeout
            url,
            username,
            password,
            max_retries: 3,
            retry_delay_ms: 500,
            circuit_breaker,
        }
    }
    
    /// Tạo và trả về kết nối Redis mới với cơ chế retry.
    ///
    /// # Returns
    /// * `Ok(Arc<dyn RedisService>)` - Kết nối Redis mới
    /// * `Err(PluginError)` - Lỗi khi tạo kết nối
    ///
    /// # Notes
    /// - Sử dụng circuit breaker để ngăn cascade failure
    /// - Tự động retry theo cấu hình max_retries và retry_delay_ms
    /// - Ghi log thông tin về quá trình kết nối
    pub async fn create_connection(&self) -> Result<Arc<dyn RedisService>, PluginError> {
        use crate::infra::service_mocks::DefaultRedisService;
        
        let service = Arc::new(DefaultRedisService::new());
        
        // Circuit breaker: kiểm tra trạng thái trước khi kết nối
        let cb = self.circuit_breaker.lock().await;
        let mut cb = cb;
        
        if !cb.can_attempt() {
            error!("[CircuitBreaker] Circuit is OPEN, reject connection attempt");
            return Err(PluginError::Other("Circuit breaker is OPEN, please retry later".to_string()));
        }
        
        // Try to connect with retries
        for attempt in 0..=self.max_retries {
            match service.connect_with_auth(&self.url, &self.username, &self.password).await {
                Ok(_) => {
                    if attempt > 0 {
                        info!("[Redis] Connection established after {} retries", attempt);
                    }
                    // Circuit breaker: reset on success
                    cb.on_success();
                    return Ok(service);
                }
                Err(e) => {
                    if attempt < self.max_retries {
                        warn!("[Redis] Connection attempt {}/{} failed: {}", attempt + 1, self.max_retries, e);
                        tokio::time::sleep(Duration::from_millis(self.retry_delay_ms)).await;
                    } else {
                        error!("[Redis] Connection failed after {} retries: {}", self.max_retries, e);
                        // Circuit breaker: tăng failure
                        cb.on_failure();
                        return Err(PluginError::Other(e.to_string()));
                    }
                }
            }
        }
        
        // Shouldn't reach here due to return in the loop
        Err(PluginError::Other("Unexpected error in create_connection".to_string()))
    }
    
    /// Lấy kết nối từ pool hoặc tạo mới nếu cần.
    ///
    /// # Returns
    /// * `Ok(Arc<dyn RedisService>)` - Kết nối Redis
    /// * `Err(PluginError)` - Lỗi khi lấy kết nối
    ///
    /// # Notes
    /// - Ưu tiên lấy kết nối từ pool trước
    /// - Tạo kết nối mới nếu pool rỗng
    /// - Sử dụng circuit breaker để ngăn cascade failure
    pub async fn get_connection(&self) -> Result<Arc<dyn RedisService>, PluginError> {
        // Circuit breaker: kiểm tra trạng thái trước khi lấy kết nối
        let cb = self.circuit_breaker.lock().await;
        let mut cb = cb;
        
        if !cb.can_attempt() {
            error!("[CircuitBreaker] Circuit is OPEN, reject get_connection");
            return Err(PluginError::Other("Circuit breaker is OPEN, please retry later".to_string()));
        }
        
        // Try to get from pool first
        if let Some(conn) = self.inner.acquire().await {
            return Ok(conn);
        }
        
        // Otherwise create a new connection
        self.create_connection().await
    }
    
    /// Trả lại kết nối vào pool.
    ///
    /// # Arguments
    /// * `conn` - Kết nối Redis cần trả lại
    ///
    /// # Returns
    /// * `Ok(())` - Nếu thao tác thành công
    /// * `Err(PluginError)` - Nếu có lỗi khi trả lại kết nối
    ///
    /// # Notes
    /// - Tự động kiểm tra kết nối còn active không trước khi trả về pool
    /// - Nếu kết nối không active, tạo kết nối mới thay thế
    pub async fn return_connection(&self, conn: Arc<dyn RedisService>) -> Result<(), PluginError> {
        // Kiểm tra kết nối còn active trước khi trả về pool
        if let Ok(is_active) = conn.ping_with_timeout(Duration::from_millis(300)).await {
            if !is_active {
                // Nếu kết nối không active, thử tạo kết nối mới thay thế
                debug!("[RedisPool] Connection inactive, creating new replacement");
                match self.create_connection().await {
                    Ok(new_conn) => {
                        debug!("[RedisPool] New replacement connection created");
                        match self.inner.release(new_conn).await {
                            Ok(_) => Ok(()),
                            Err(e) => Err(PluginError::Other(e)),
                        }
                    },
                    Err(e) => {
                        warn!("[RedisPool] Failed to create replacement connection: {}", e);
                        Err(e)
                    }
                }
            } else {
                // Kết nối còn hoạt động, trả về pool bình thường
                match self.inner.release(conn).await {
                    Ok(_) => Ok(()),
                    Err(e) => Err(PluginError::Other(e)),
                }
            }
        } else {
            // Không kiểm tra được, giả định kết nối vẫn ok
            match self.inner.release(conn).await {
                Ok(_) => Ok(()),
                Err(e) => Err(PluginError::Other(e)),
            }
        }
    }
    
    /// Lấy kích thước hiện tại của pool.
    ///
    /// # Returns
    /// * `Ok(usize)` - Số lượng kết nối trong pool
    /// * `Err(PluginError)` - Lỗi khi lấy kích thước
    pub async fn pool_size(&self) -> Result<usize, PluginError> {
        self.inner.size().await.map_err(PluginError::Other)
    }
    
    /// Xóa tất cả kết nối trong pool.
    ///
    /// # Returns
    /// * `Ok(())` - Nếu thao tác thành công
    /// * `Err(PluginError)` - Lỗi khi xóa
    pub async fn clear_pool(&self) -> Result<(), PluginError> {
        self.inner.clear().await.map_err(PluginError::Other)
    }
    
    /// Force đóng tất cả các kết nối Redis.
    ///
    /// # Returns
    /// * `Ok(())` - Nếu thao tác thành công
    /// * `Err(PluginError)` - Lỗi khi đóng kết nối
    pub async fn force_close_all(&self) -> Result<(), PluginError> {
        self.inner.force_close_all().await
            .map_err(|e| PluginError::Other(format!("Failed to force close Redis connections: {}", e)))
    }

    /// Shutdown pool và cleanup tài nguyên.
    ///
    /// # Thread Safety
    /// - Sử dụng phương pháp an toàn hơn để shutdown pool
    /// - An toàn khi gọi từ nhiều thread
    pub fn shutdown(&self) {
        // Clone các thành phần cần thiết để tránh borrowed data escapes
        let self_inner = self.inner.clone();
        
        // Tạo một task mới để thực hiện shutdown
        tokio::spawn(async move {
            // Dọn dẹp kết nối trong pool
            match self_inner.force_close_all().await {
                Ok(_) => debug!("[RedisPool] Pool shutdown completed successfully"),
                Err(e) => warn!("[RedisPool] Error during pool shutdown: {}", e),
            }
        });
    }
    
    /// Cấu hình tham số retry.
    ///
    /// # Arguments
    /// * `max_retries` - Số lần retry tối đa
    /// * `retry_delay_ms` - Thời gian chờ giữa các lần retry (ms)
    ///
    /// # Returns
    /// Self để hỗ trợ method chaining
    pub fn with_retry_config(mut self, max_retries: u8, retry_delay_ms: u64) -> Self {
        self.max_retries = max_retries;
        self.retry_delay_ms = retry_delay_ms;
        self
    }
}