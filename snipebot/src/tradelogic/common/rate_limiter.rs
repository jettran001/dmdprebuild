/// Rate Limiter Module - Quản lý giới hạn API call đến các dịch vụ bên ngoài
///
/// Module này cung cấp cơ chế giới hạn tần suất gọi API (rate limiting) để tránh bị giới hạn hoặc 
/// bị chặn bởi các exchange và API bên ngoài. Module hỗ trợ nhiều loại giới hạn (token bucket, 
/// sliding window, fixed window) và quản lý tự động.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock, Semaphore};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};
use anyhow::{Result, anyhow, Context};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use std::future::Future;

/// Kiểu giới hạn API
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitType {
    /// Token bucket - giới hạn tổng số lượng request trong một khoảng thời gian, 
    /// với tốc độ nạp lại token nhất định
    TokenBucket,
    
    /// Sliding window - giới hạn số lượng request trong một cửa sổ thời gian di chuyển
    SlidingWindow,
    
    /// Fixed window - giới hạn số lượng request trong một cửa sổ thời gian cố định
    FixedWindow,
}

/// Cấu hình giới hạn API
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Loại giới hạn
    pub limit_type: RateLimitType,
    
    /// Số lượng request tối đa trong một khoảng thời gian
    pub max_requests: u32,
    
    /// Khoảng thời gian (ms)
    pub time_window_ms: u64,
    
    /// Tốc độ nạp lại token (tokens/giây) - chỉ dùng cho TokenBucket
    pub refill_rate: f64,
    
    /// Độ ưu tiên của request
    pub priority_levels: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            limit_type: RateLimitType::TokenBucket,
            max_requests: 100,
            time_window_ms: 60000, // 1 phút
            refill_rate: 1.0,      // 1 token/giây
            priority_levels: false,
        }
    }
}

/// Độ ưu tiên của request
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequestPriority {
    /// Độ ưu tiên thấp
    Low,
    
    /// Độ ưu tiên trung bình
    Medium,
    
    /// Độ ưu tiên cao
    High,
    
    /// Độ ưu tiên rất cao
    Critical,
}

/// Token bucket rate limiter
struct TokenBucket {
    /// Số token hiện tại
    tokens: f64,
    
    /// Số token tối đa
    capacity: f64,
    
    /// Tốc độ nạp lại token (tokens/ms)
    refill_rate_ms: f64,
    
    /// Thời điểm cập nhật token lần cuối
    last_refill: Instant,
}

impl TokenBucket {
    /// Tạo token bucket mới
    fn new(capacity: u32, refill_rate_per_second: f64) -> Self {
        Self {
            tokens: capacity as f64,
            capacity: capacity as f64,
            refill_rate_ms: refill_rate_per_second / 1000.0,
            last_refill: Instant::now(),
        }
    }
    
    /// Cập nhật số token hiện tại
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_millis() as f64;
        self.last_refill = now;
        
        let new_tokens = elapsed * self.refill_rate_ms;
        self.tokens = (self.tokens + new_tokens).min(self.capacity);
    }
    
    /// Lấy token
    fn take(&mut self, count: u32) -> bool {
        self.refill();
        
        if self.tokens >= count as f64 {
            self.tokens -= count as f64;
            true
        } else {
            false
        }
    }
    
    /// Tính thời gian chờ để có đủ token
    fn wait_time_ms(&mut self, count: u32) -> u64 {
        self.refill();
        
        if self.tokens >= count as f64 {
            return 0;
        }
        
        let needed_tokens = count as f64 - self.tokens;
        let wait_time = needed_tokens / self.refill_rate_ms;
        
        wait_time.ceil() as u64
    }
}

/// Sliding window rate limiter
struct SlidingWindow {
    /// Danh sách timestamp của các request gần đây
    requests: Vec<Instant>,
    
    /// Số request tối đa trong một khoảng thời gian
    max_requests: usize,
    
    /// Khoảng thời gian (ms)
    time_window_ms: u64,
}

impl SlidingWindow {
    /// Tạo sliding window mới
    fn new(max_requests: u32, time_window_ms: u64) -> Self {
        Self {
            requests: Vec::with_capacity(max_requests as usize),
            max_requests: max_requests as usize,
            time_window_ms,
        }
    }
    
    /// Làm sạch các request cũ
    fn cleanup(&mut self) {
        let now = Instant::now();
        let window = Duration::from_millis(self.time_window_ms);
        
        self.requests.retain(|&time| now.duration_since(time) < window);
    }
    
    /// Kiểm tra xem có thể thực hiện request không
    fn allow_request(&mut self) -> bool {
        self.cleanup();
        
        if self.requests.len() < self.max_requests {
            self.requests.push(Instant::now());
            true
        } else {
            false
        }
    }
    
    /// Tính thời gian chờ để có thể thực hiện request
    fn wait_time_ms(&mut self) -> u64 {
        self.cleanup();
        
        if self.requests.len() < self.max_requests {
            return 0;
        }
        
        // Tính thời gian chờ đến khi request cũ nhất hết hạn
        let now = Instant::now();
        let oldest = self.requests[0];
        let window = Duration::from_millis(self.time_window_ms);
        let elapsed = now.duration_since(oldest);
        
        if elapsed >= window {
            0
        } else {
            (window - elapsed).as_millis() as u64
        }
    }
}

/// Fixed window rate limiter
struct FixedWindow {
    /// Số request đã thực hiện trong cửa sổ hiện tại
    count: u32,
    
    /// Số request tối đa trong một khoảng thời gian
    max_requests: u32,
    
    /// Thời điểm bắt đầu cửa sổ hiện tại
    window_start: Instant,
    
    /// Khoảng thời gian (ms)
    time_window_ms: u64,
}

impl FixedWindow {
    /// Tạo fixed window mới
    fn new(max_requests: u32, time_window_ms: u64) -> Self {
        Self {
            count: 0,
            max_requests,
            window_start: Instant::now(),
            time_window_ms,
        }
    }
    
    /// Kiểm tra và cập nhật cửa sổ nếu cần
    fn check_window(&mut self) {
        let now = Instant::now();
        let window = Duration::from_millis(self.time_window_ms);
        
        if now.duration_since(self.window_start) >= window {
            self.count = 0;
            self.window_start = now;
        }
    }
    
    /// Kiểm tra xem có thể thực hiện request không
    fn allow_request(&mut self) -> bool {
        self.check_window();
        
        if self.count < self.max_requests {
            self.count += 1;
            true
        } else {
            false
        }
    }
    
    /// Tính thời gian chờ để có thể thực hiện request
    fn wait_time_ms(&mut self) -> u64 {
        self.check_window();
        
        if self.count < self.max_requests {
            return 0;
        }
        
        // Tính thời gian chờ đến khi cửa sổ hiện tại kết thúc
        let now = Instant::now();
        let window = Duration::from_millis(self.time_window_ms);
        let elapsed = now.duration_since(self.window_start);
        
        if elapsed >= window {
            0
        } else {
            (window - elapsed).as_millis() as u64
        }
    }
}

/// Trait cơ bản cho RateLimiter, tương thích với dyn
pub trait RateLimiter: Send + Sync + 'static {
    /// Lấy một RateLimiterImpl cụ thể từ trait chung
    /// 
    /// Đây là phương thức không async để đảm bảo trait tương thích với dyn
    fn as_rate_limiter_impl(&self) -> &dyn RateLimiterImpl;
}

/// Trait cơ bản cho RateLimiterImpl (không có generic parameters)
/// Định nghĩa các phương thức cơ bản, tương thích với dyn
#[async_trait]
pub trait RateLimiterImpl: Send + Sync + 'static {
    /// Acquire a permit to make a request
    async fn acquire(&self, priority: Option<RequestPriority>) -> Result<()>;
    
    /// Try to acquire a permit without waiting
    async fn try_acquire(&self, priority: Option<RequestPriority>) -> bool;
    
    /// Get the wait time in milliseconds
    async fn wait_time_ms(&self, priority: Option<RequestPriority>) -> u64;
}

/// Trait mở rộng cho RateLimiterImpl chứa các phương thức generic
/// Không tương thích trực tiếp với dyn, nhưng cung cấp tiện ích
#[async_trait]
pub trait RateLimiterImplExt: RateLimiterImpl {
    /// Execute a function with rate limiting
    async fn with_rate_limit<F, Fut, T>(&self, f: F, priority: Option<RequestPriority>) -> Result<T>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T>> + Send,
        T: Send;
}

/// Implement trait mở rộng cho tất cả các loại RateLimiterImpl
#[async_trait]
impl<R: RateLimiterImpl + ?Sized> RateLimiterImplExt for R {
    async fn with_rate_limit<F, Fut, T>(&self, f: F, priority: Option<RequestPriority>) -> Result<T>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T>> + Send,
        T: Send,
    {
        // Acquire a permit
        self.acquire(priority).await?;
        
        // Execute the function
        f().await
    }
}

/// Rate limiter implementation
pub struct RateLimiterImpl {
    /// Tên rate limiter
    name: String,
    
    /// Loại giới hạn
    limit_type: RateLimitType,
    
    /// Token bucket rate limiter
    token_bucket: Option<Mutex<TokenBucket>>,
    
    /// Sliding window rate limiter
    sliding_window: Option<Mutex<SlidingWindow>>,
    
    /// Fixed window rate limiter
    fixed_window: Option<Mutex<FixedWindow>>,
    
    /// Cấu hình
    config: RateLimitConfig,
    
    /// Semaphore cho các request độ ưu tiên cao
    high_priority_semaphore: Option<Semaphore>,
    
    /// Semaphore cho các request độ ưu tiên trung bình
    medium_priority_semaphore: Option<Semaphore>,
    
    /// Semaphore cho các request độ ưu tiên thấp
    low_priority_semaphore: Option<Semaphore>,
}

impl RateLimiterImpl {
    /// Tạo rate limiter mới
    pub fn new(name: &str, config: RateLimitConfig) -> Self {
        let token_bucket = if config.limit_type == RateLimitType::TokenBucket {
            Some(Mutex::new(TokenBucket::new(config.max_requests, config.refill_rate)))
        } else {
            None
        };
        
        let sliding_window = if config.limit_type == RateLimitType::SlidingWindow {
            Some(Mutex::new(SlidingWindow::new(config.max_requests, config.time_window_ms)))
        } else {
            None
        };
        
        let fixed_window = if config.limit_type == RateLimitType::FixedWindow {
            Some(Mutex::new(FixedWindow::new(config.max_requests, config.time_window_ms)))
        } else {
            None
        };
        
        // Tạo semaphore cho các mức độ ưu tiên khác nhau nếu cần
        let (high_priority_semaphore, medium_priority_semaphore, low_priority_semaphore) = 
            if config.priority_levels {
                // Phân bổ permits: 50% cho high, 30% cho medium, 20% cho low
                let high_permits = (config.max_requests as f64 * 0.5).ceil() as usize;
                let medium_permits = (config.max_requests as f64 * 0.3).ceil() as usize;
                let low_permits = (config.max_requests as f64 * 0.2).ceil() as usize;
                
                (
                    Some(Semaphore::new(high_permits)),
                    Some(Semaphore::new(medium_permits)),
                    Some(Semaphore::new(low_permits)),
                )
            } else {
                (None, None, None)
            };
        
        Self {
            name: name.to_string(),
            limit_type: config.limit_type,
            token_bucket,
            sliding_window,
            fixed_window,
            config,
            high_priority_semaphore,
            medium_priority_semaphore,
            low_priority_semaphore,
        }
    }
}

#[async_trait]
impl RateLimiter for RateLimiterImpl {
    /// Lấy một RateLimiterImpl cụ thể từ trait chung
    /// 
    /// Đây là phương thức không async để đảm bảo trait tương thích với dyn
    fn as_rate_limiter_impl(&self) -> &dyn RateLimiterImpl {
        self
    }
}

#[async_trait]
impl RateLimiterImpl for RateLimiterImpl {
    /// Acquire a permit to make a request
    async fn acquire(&self, priority: Option<RequestPriority>) -> Result<()> {
        // Xử lý theo độ ưu tiên nếu được bật
        if self.config.priority_levels {
            let semaphore = match priority.unwrap_or(RequestPriority::Medium) {
                RequestPriority::Critical | RequestPriority::High => 
                    self.high_priority_semaphore.as_ref().unwrap(),
                RequestPriority::Medium => 
                    self.medium_priority_semaphore.as_ref().unwrap(),
                RequestPriority::Low => 
                    self.low_priority_semaphore.as_ref().unwrap(),
            };
            
            // Chờ semaphore
            let _permit = semaphore.acquire().await.map_err(|e| 
                anyhow!("Failed to acquire semaphore for {}: {}", self.name, e)
            )?;
        }
        
        // Xử lý theo loại rate limiter
        match self.limit_type {
            RateLimitType::TokenBucket => {
                if let Some(bucket) = &self.token_bucket {
                    let mut bucket = bucket.lock().await;
                    let wait_time = bucket.wait_time_ms(1);
                    
                    if wait_time > 0 {
                        debug!("Rate limited for {}: waiting {}ms", self.name, wait_time);
                        sleep(Duration::from_millis(wait_time)).await;
                    }
                    
                    bucket.take(1);
                }
            },
            RateLimitType::SlidingWindow => {
                if let Some(window) = &self.sliding_window {
                    let mut window = window.lock().await;
                    let wait_time = window.wait_time_ms();
                    
                    if wait_time > 0 {
                        debug!("Rate limited for {}: waiting {}ms", self.name, wait_time);
                        sleep(Duration::from_millis(wait_time)).await;
                    }
                    
                    window.allow_request();
                }
            },
            RateLimitType::FixedWindow => {
                if let Some(window) = &self.fixed_window {
                    let mut window = window.lock().await;
                    let wait_time = window.wait_time_ms();
                    
                    if wait_time > 0 {
                        debug!("Rate limited for {}: waiting {}ms", self.name, wait_time);
                        sleep(Duration::from_millis(wait_time)).await;
                    }
                    
                    window.allow_request();
                }
            },
        }
        
        Ok(())
    }
    
    /// Try to acquire a permit without waiting
    async fn try_acquire(&self, priority: Option<RequestPriority>) -> bool {
        // Xử lý theo độ ưu tiên nếu được bật
        if self.config.priority_levels {
            let semaphore = match priority.unwrap_or(RequestPriority::Medium) {
                RequestPriority::Critical | RequestPriority::High => 
                    self.high_priority_semaphore.as_ref().unwrap(),
                RequestPriority::Medium => 
                    self.medium_priority_semaphore.as_ref().unwrap(),
                RequestPriority::Low => 
                    self.low_priority_semaphore.as_ref().unwrap(),
            };
            
            // Thử lấy semaphore
            if semaphore.try_acquire().is_err() {
                return false;
            }
        }
        
        // Xử lý theo loại rate limiter
        match self.limit_type {
            RateLimitType::TokenBucket => {
                if let Some(bucket) = &self.token_bucket {
                    let mut bucket = bucket.lock().await;
                    bucket.take(1)
                } else {
                    true
                }
            },
            RateLimitType::SlidingWindow => {
                if let Some(window) = &self.sliding_window {
                    let mut window = window.lock().await;
                    window.allow_request()
                } else {
                    true
                }
            },
            RateLimitType::FixedWindow => {
                if let Some(window) = &self.fixed_window {
                    let mut window = window.lock().await;
                    window.allow_request()
                } else {
                    true
                }
            },
        }
    }
    
    /// Get the wait time in milliseconds
    async fn wait_time_ms(&self, priority: Option<RequestPriority>) -> u64 {
        // Xử lý theo loại rate limiter
        match self.limit_type {
            RateLimitType::TokenBucket => {
                if let Some(bucket) = &self.token_bucket {
                    let mut bucket = bucket.lock().await;
                    bucket.wait_time_ms(1)
                } else {
                    0
                }
            },
            RateLimitType::SlidingWindow => {
                if let Some(window) = &self.sliding_window {
                    let mut window = window.lock().await;
                    window.wait_time_ms()
                } else {
                    0
                }
            },
            RateLimitType::FixedWindow => {
                if let Some(window) = &self.fixed_window {
                    let mut window = window.lock().await;
                    window.wait_time_ms()
                } else {
                    0
                }
            },
        }
    }
}

/// Hàm helper để thực hiện rate limiting cho dyn RateLimiter
pub async fn with_rate_limit<F, Fut, T>(
    limiter: &dyn RateLimiter, 
    f: F, 
    priority: Option<RequestPriority>
) -> Result<T>
where
    F: FnOnce() -> Fut + Send,
    Fut: Future<Output = Result<T>> + Send,
    T: Send,
{
    // Lấy implementation
    let impl_limiter = limiter.as_rate_limiter_impl();
    
    // Acquire permit
    impl_limiter.acquire(priority).await?;
    
    // Thực hiện hàm
    f().await
}

/// API Rate Limiter Manager
#[derive(Debug)]
pub struct ApiRateLimiterManager {
    /// Các rate limiter đã đăng ký
    limiters: RwLock<HashMap<String, Arc<dyn RateLimiter>>>,
    
    /// Cấu hình mặc định cho các exchange phổ biến
    default_configs: HashMap<String, RateLimitConfig>,
    
    /// Timeout cho các operation (ms)
    timeout_ms: u64,
}

impl Default for ApiRateLimiterManager {
    fn default() -> Self {
        let mut default_configs = HashMap::new();
        
        // Thiết lập cấu hình mặc định cho các exchange phổ biến
        // Binance - 1200 requests/minute
        default_configs.insert("binance".to_string(), RateLimitConfig {
            limit_type: RateLimitType::TokenBucket,
            max_requests: 1200,
            time_window_ms: 60000,
            refill_rate: 20.0,
            priority_levels: true,
        });
        
        // Coinbase - 10 requests/second
        default_configs.insert("coinbase".to_string(), RateLimitConfig {
            limit_type: RateLimitType::TokenBucket,
            max_requests: 10,
            time_window_ms: 1000,
            refill_rate: 10.0,
            priority_levels: true,
        });
        
        // Kucoin - 30 requests/3 seconds
        default_configs.insert("kucoin".to_string(), RateLimitConfig {
            limit_type: RateLimitType::SlidingWindow,
            max_requests: 30,
            time_window_ms: 3000,
            refill_rate: 10.0,
            priority_levels: true,
        });
        
        // Etherscan - 5 requests/second
        default_configs.insert("etherscan".to_string(), RateLimitConfig {
            limit_type: RateLimitType::FixedWindow,
            max_requests: 5,
            time_window_ms: 1000,
            refill_rate: 5.0,
            priority_levels: false,
        });
        
        // CoinGecko - 50 requests/minute
        default_configs.insert("coingecko".to_string(), RateLimitConfig {
            limit_type: RateLimitType::TokenBucket,
            max_requests: 50,
            time_window_ms: 60000,
            refill_rate: 0.83,
            priority_levels: false,
        });
        
        // Alchemy RPC - 330 requests/second
        default_configs.insert("alchemy".to_string(), RateLimitConfig {
            limit_type: RateLimitType::TokenBucket,
            max_requests: 330,
            time_window_ms: 1000,
            refill_rate: 330.0,
            priority_levels: true,
        });
        
        Self {
            limiters: RwLock::new(HashMap::new()),
            default_configs,
            timeout_ms: 1000, // 1 giây timeout mặc định
        }
    }
}

impl ApiRateLimiterManager {
    /// Tạo manager mới
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Tạo manager mới với timeout tùy chỉnh
    pub fn with_timeout(timeout_ms: u64) -> Self {
        let mut manager = Self::default();
        manager.timeout_ms = timeout_ms;
        manager
    }
    
    /// Lấy rate limiter từ read lock với timeout
    async fn get_limiter_with_timeout(&self, name: &str) -> Result<Option<Arc<dyn RateLimiter>>, anyhow::Error> {
        use anyhow::Context;
        use tokio::time::timeout;
        
        // Sử dụng timeout để tránh deadlock khi chờ đợi read lock
        let timeout_duration = Duration::from_millis(self.timeout_ms);
        match timeout(timeout_duration, self.limiters.read()).await {
            Ok(guard) => {
                // Lấy limiter từ map
                let result = guard.get(name).cloned();
                // Guard tự động drop sau khi ra khỏi scope
                Ok(result)
            },
            Err(_) => {
                // Timeout khi chờ read lock
                warn!("Timeout khi chờ read lock trong get_limiter_with_timeout cho {}", name);
                Err(anyhow!("Timeout khi truy cập rate limiter cho {}", name))
            }
        }
    }
    
    /// Đăng ký rate limiter mới với timeout và cơ chế locking an toàn hơn
    pub async fn register_limiter(&self, name: &str, config: Option<RateLimitConfig>) -> Result<Arc<dyn RateLimiter>, anyhow::Error> {
        use anyhow::Context;
        use tokio::time::timeout;
        
        // Kiểm tra xem đã có limiter chưa để tránh lấy write lock không cần thiết
        if let Ok(Some(limiter)) = self.get_limiter_with_timeout(name).await {
            return Ok(limiter);
        }
        
        // Limiter chưa tồn tại, cần tạo mới và lấy write lock
        // Sử dụng timeout để tránh deadlock khi chờ đợi write lock
        let timeout_duration = Duration::from_millis(self.timeout_ms);
        let mut limiters = match timeout(timeout_duration, self.limiters.write()).await {
            Ok(guard) => guard,
            Err(_) => {
                // Timeout khi chờ write lock
                warn!("Timeout khi chờ write lock trong register_limiter cho {}", name);
                return Err(anyhow!("Timeout khi đăng ký rate limiter cho {}", name));
            }
        };
        
        // Kiểm tra lại xem có limiter không (có thể đã được tạo bởi thread khác)
        if let Some(limiter) = limiters.get(name) {
            return Ok(limiter.clone());
        }
        
        // Sử dụng cấu hình mặc định nếu có
        let config = config.unwrap_or_else(|| {
            self.default_configs.get(name)
                .cloned()
                .unwrap_or_default()
        });
        
        // Tạo limiter mới
        let limiter = Arc::new(RateLimiterImpl::new(name, config));
        
        // Lưu vào map
        limiters.insert(name.to_string(), limiter.clone());
        
        // Trả về limiter mới
        Ok(limiter)
    }
    
    /// Lấy rate limiter đã đăng ký với xử lý timeout
    pub async fn get_limiter(&self, name: &str) -> Result<Option<Arc<dyn RateLimiter>>, anyhow::Error> {
        self.get_limiter_with_timeout(name).await
    }
    
    /// Lấy hoặc tạo rate limiter với xử lý lỗi và timeout
    pub async fn get_or_create_limiter(&self, name: &str, config: Option<RateLimitConfig>) -> Result<Arc<dyn RateLimiter>, anyhow::Error> {
        use anyhow::Context;
        
        // Thử lấy limiter từ cache
        if let Ok(Some(limiter)) = self.get_limiter_with_timeout(name).await {
            return Ok(limiter);
        }
        
        // Limiter chưa tồn tại, đăng ký mới
        match self.register_limiter(name, config).await {
            Ok(limiter) => Ok(limiter),
            Err(e) => {
                // Nếu không thể đăng ký, tạo một limiter tạm thời không lưu vào cache
                warn!("Không thể đăng ký rate limiter cho {}, tạo limiter tạm thời: {}", name, e);
                let config = config.unwrap_or_else(|| {
                    self.default_configs.get(name)
                        .cloned()
                        .unwrap_or_default()
                });
                
                // Tạo limiter mới nhưng không lưu vào map để tránh race condition
                let temp_limiter = Arc::new(RateLimiterImpl::new(name, config));
                Ok(temp_limiter)
            }
        }
    }
    
    /// Thực hiện API call với rate limiting và xử lý lỗi
    pub async fn api_call<F, Fut, T>(
        &self,
        api_name: &str,
        f: F,
        priority: Option<RequestPriority>,
        config: Option<RateLimitConfig>,
    ) -> Result<T, anyhow::Error>
    where
        F: FnOnce() -> Fut + Send,
        Fut: std::future::Future<Output = Result<T, anyhow::Error>> + Send,
    {
        use anyhow::Context;
        
        // Lấy limiter từ cache hoặc tạo mới
        let limiter = match self.get_or_create_limiter(api_name, config).await {
            Ok(limiter) => limiter,
            Err(e) => {
                // Nếu không thể lấy limiter, thử thực thi API call mà không có rate limiting
                warn!("Không thể lấy rate limiter cho {}, thực thi không giới hạn: {}", api_name, e);
                return f().await.with_context(|| format!("API call cho {} không được rate limit", api_name));
            }
        };
        
        // Thực hiện API call với rate limiting
        with_rate_limit(limiter, f, priority).await
            .with_context(|| format!("API call cho {} với rate limiting", api_name))
    }
}

/// Singleton instance
pub static GLOBAL_API_RATE_LIMITER: Lazy<Arc<ApiRateLimiterManager>> = 
    Lazy::new(|| Arc::new(ApiRateLimiterManager::new()));

/// Lấy API rate limiter manager global
pub fn get_api_rate_limiter() -> Arc<ApiRateLimiterManager> {
    GLOBAL_API_RATE_LIMITER.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    
    #[tokio::test]
    async fn test_token_bucket() {
        let mut bucket = TokenBucket::new(10, 1.0); // 10 tokens, 1 token/second
        
        // Lấy 5 token, còn lại 5
        assert!(bucket.take(5));
        assert_eq!(bucket.tokens, 5.0);
        
        // Lấy 6 token, không đủ
        assert!(!bucket.take(6));
        assert_eq!(bucket.tokens, 5.0);
        
        // Lấy 5 token, còn lại 0
        assert!(bucket.take(5));
        assert_eq!(bucket.tokens, 0.0);
        
        // Đợi 5 giây để nạp lại 5 token
        bucket.last_refill = Instant::now() - Duration::from_secs(5);
        bucket.refill();
        assert!(bucket.tokens >= 5.0);
    }
    
    #[tokio::test]
    async fn test_sliding_window() {
        let mut window = SlidingWindow::new(5, 1000); // 5 requests/second
        
        // 5 request đầu tiên được chấp nhận
        for _ in 0..5 {
            assert!(window.allow_request());
        }
        
        // Request thứ 6 bị từ chối
        assert!(!window.allow_request());
        
        // Đợi quá thời gian window
        window.requests[0] = Instant::now() - Duration::from_millis(1001);
        
        // Request tiếp theo được chấp nhận vì request cũ đã hết hạn
        assert!(window.allow_request());
    }
    
    #[tokio::test]
    async fn test_fixed_window() {
        let mut window = FixedWindow::new(5, 1000); // 5 requests/second
        
        // 5 request đầu tiên được chấp nhận
        for _ in 0..5 {
            assert!(window.allow_request());
        }
        
        // Request thứ 6 bị từ chối
        assert!(!window.allow_request());
        
        // Đợi quá thời gian window
        window.window_start = Instant::now() - Duration::from_millis(1001);
        window.check_window();
        
        // Tất cả request trong cửa sổ mới đều được chấp nhận
        for _ in 0..5 {
            assert!(window.allow_request());
        }
    }
    
    #[tokio::test]
    async fn test_rate_limiter_impl() {
        let config = RateLimitConfig {
            limit_type: RateLimitType::TokenBucket,
            max_requests: 5,
            time_window_ms: 1000,
            refill_rate: 5.0,
            priority_levels: false,
        };
        
        let limiter = RateLimiterImpl::new("test", config);
        
        // 5 request đầu tiên được chấp nhận ngay lập tức
        for _ in 0..5 {
            assert!(limiter.try_acquire(None).await);
        }
        
        // Request thứ 6 bị từ chối nếu thử ngay lập tức
        assert!(!limiter.try_acquire(None).await);
        
        // Nhưng có thể acquire nếu chờ
        limiter.acquire(None).await.unwrap();
    }
    
    #[tokio::test]
    async fn test_api_rate_limiter_manager() {
        let manager = ApiRateLimiterManager::new();
        
        // Đăng ký limiter
        let limiter = manager.register_limiter("test_api", None).await;
        
        // Thực hiện API call
        let result = manager.api_call("test_api", || async { Ok(42) }, None, None).await;
        assert_eq!(result.unwrap(), 42);
        
        // Thử thực hiện nhiều API call
        let mut results = Vec::new();
        for _ in 0..10 {
            let result = manager.api_call("test_api", || async { Ok(42) }, None, None).await;
            results.push(result.unwrap());
        }
        
        assert_eq!(results.len(), 10);
        assert_eq!(results[0], 42);
    }
} 