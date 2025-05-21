//! All locking in this file uses tokio::sync::{Mutex, RwLock} for async context, as required by .cursorrc. Do NOT use std::sync::* for any state/config.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};
use serde::{Serialize, Deserialize};
use tracing::{debug, error};

/// Các loại lỗi rate limit
#[derive(Error, Debug, Clone)]
pub enum RateLimitError {
    #[error("Đã vượt quá số lượt gọi: {0}")]
    LimitExceeded(String),
    #[error("Lỗi cấu hình: {0}")]
    ConfigurationError(String),
}

/// Các thuật toán rate limiting
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RateLimitAlgorithm {
    /// Giới hạn cố định trong một khoảng thời gian
    FixedWindow,
    /// Cửa sổ trượt (chính xác hơn)
    SlidingWindow,
    /// Token bucket
    TokenBucket,
    /// Leaky bucket
    LeakyBucket,
}

/// Các loại hành động khi rate limit bị vượt quá
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RateLimitAction {
    /// Từ chối yêu cầu
    Reject,
    /// Làm chậm yêu cầu
    Delay,
    /// Chỉ ghi log
    LogOnly,
}

/// Loại định danh cho rate limit
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RequestIdentifier {
    /// Định danh bằng API key
    ApiKey(String),
    /// Định danh bằng User ID
    UserId(String),
    /// Định danh bằng địa chỉ IP
    IpAddress(std::net::IpAddr),
    /// Định danh tùy chỉnh
    Custom(String),
}

/// Cấu hình cho một đường dẫn
#[derive(Debug, Clone)]
pub struct PathConfig {
    /// Số lượng yêu cầu tối đa
    pub max_requests: u32,
    /// Thời gian áp dụng (tính bằng giây)
    pub time_window: u64,
    /// Thuật toán được sử dụng
    pub algorithm: RateLimitAlgorithm,
    /// Hành động khi vượt quá giới hạn
    pub action: RateLimitAction,
}

impl Default for PathConfig {
    fn default() -> Self {
        Self {
            max_requests: 100,
            time_window: 60,
            algorithm: RateLimitAlgorithm::TokenBucket,
            action: RateLimitAction::Reject,
        }
    }
}

/// Cấu hình kiểm soát địa chỉ IP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IPAddressConfig {
    /// Danh sách IP bị chặn
    pub blocked_ips: Vec<String>,
    /// Danh sách IP được phép
    pub allowed_ips: Vec<String>,
    /// Số lượng yêu cầu tối đa cho mỗi IP
    pub max_requests_per_ip: u32,
    /// Thời gian áp dụng giới hạn IP (giây)
    pub block_duration: u64,
    /// Có kiểm tra danh sách đen toàn cục không
    pub check_global_blacklist: bool,
}

impl Default for IPAddressConfig {
    fn default() -> Self {
        Self {
            blocked_ips: Vec::new(),
            allowed_ips: Vec::new(),
            max_requests_per_ip: 1000,
            block_duration: 3600,
            check_global_blacklist: true,
        }
    }
}

/// Trạng thái rate limit cho một đường dẫn và identifier
#[derive(Debug, Clone)]
struct RateLimitState {
    /// Thời điểm bắt đầu cửa sổ hiện tại
    window_start: Instant,
    /// Số lượng yêu cầu trong cửa sổ hiện tại
    request_count: u32,
    /// Danh sách thời điểm của các yêu cầu (cho sliding window)
    request_timestamps: Vec<Instant>,
    /// Số lượng token hiện có (cho token bucket)
    available_tokens: u32,
    /// Thời điểm gần nhất token được thêm vào (cho token bucket)
    last_refill: Instant,
}

impl RateLimitState {
    fn new(max_tokens: u32) -> Self {
        let now = Instant::now();
        Self {
            window_start: now,
            request_count: 0,
            request_timestamps: Vec::new(),
            available_tokens: max_tokens,
            last_refill: now,
        }
    }
}

/// Kết quả kiểm tra rate limit
#[derive(Debug, Clone)]
pub struct RateLimitResult {
    /// Yêu cầu có bị giới hạn không
    pub is_limited: bool,
    /// Số yêu cầu còn lại
    pub remaining: u32,
    /// Thời gian reset (giây)
    pub reset_after: u64,
    /// Thông báo lỗi (nếu có)
    pub message: Option<String>,
}

/// RateLimiter chính
/// NOTE: All locks use tokio::sync for async context, as required by .cursorrc rules.
pub struct RateLimiter {
    /// Cấu hình các đường dẫn
    path_configs: RwLock<HashMap<String, PathConfig>>,
    /// Trạng thái rate limit
    states: Arc<Mutex<HashMap<(String, RequestIdentifier), RateLimitState>>>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    /// Tạo một rate limiter mới
    pub fn new() -> Self {
        Self {
            path_configs: RwLock::new(HashMap::new()),
            states: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Đăng ký cấu hình cho một đường dẫn
    pub async fn register_path(&self, path: &str, config: PathConfig) -> Result<(), RateLimitError> {
        if config.max_requests == 0 {
            return Err(RateLimitError::ConfigurationError(
                "Số lượng yêu cầu tối đa phải lớn hơn 0".to_string(),
            ));
        }
        if config.time_window == 0 {
            return Err(RateLimitError::ConfigurationError(
                "Thời gian cửa sổ phải lớn hơn 0".to_string(),
            ));
        }
        let mut configs = self.path_configs.write().await;
        configs.insert(path.to_string(), config);
        Ok(())
    }

    /// Kiểm tra giới hạn tốc độ
    pub async fn check_limit(
        &self,
        path: &str,
        identifier: RequestIdentifier,
    ) -> Result<RateLimitResult, RateLimitError> {
        let configs = self.path_configs.read().await;
        let config = configs.get(path).ok_or_else(|| {
            RateLimitError::ConfigurationError(format!("Không tìm thấy cấu hình cho đường dẫn: {}", path))
        })?;
        let mut states = self.states.lock().await;
        let key = (path.to_string(), identifier);
        // Tạo trạng thái mới nếu chưa tồn tại
        if !states.contains_key(&key) {
            states.insert(key.clone(), RateLimitState::new(config.max_requests));
        }
        let state = states.get_mut(&key).ok_or_else(|| RateLimitError::ConfigurationError("Không tìm thấy trạng thái rate limit cho key".to_string()))?;
        let now = Instant::now();
        match config.algorithm {
            RateLimitAlgorithm::FixedWindow => self.check_fixed_window(state, config, now),
            RateLimitAlgorithm::SlidingWindow => self.check_sliding_window(state, config, now),
            RateLimitAlgorithm::TokenBucket => self.check_token_bucket(state, config, now),
            RateLimitAlgorithm::LeakyBucket => self.check_leaky_bucket(state, config, now),
        }
    }
    
    /// Spawn a background task to clear stale states every 5 minutes.
    pub fn spawn_cleanup_task(self: &Arc<Self>) {
        let states = self.states.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                let mut map = states.lock().await;
                let now = Instant::now();
                let mut removed = 0;
                map.retain(|_, state| {
                    // Nếu không còn request nào hợp lệ trong 10 phút thì xóa
                    let last_activity = state.request_timestamps.last().copied().unwrap_or(state.window_start);
                    if now.duration_since(last_activity) >= std::time::Duration::from_secs(600) {
                        removed += 1;
                        false
                    } else {
                        true
                    }
                });
                debug!("[RateLimiter] Cleaned up {} stale rate limit states", removed);
            }
        });
    }

    /// Helper: Tạo RateLimitResult khi bị giới hạn
    fn make_limited_result(&self, config: &PathConfig, reset_after: u64, msg: Option<String>) -> Result<RateLimitResult, RateLimitError> {
        let result = RateLimitResult {
            is_limited: true,
            remaining: 0,
            reset_after,
            message: msg.clone().or_else(|| Some(format!("Đã vượt quá giới hạn {} yêu cầu trong {} giây", config.max_requests, config.time_window))),
        };
        if config.action == RateLimitAction::Reject {
            return Err(RateLimitError::LimitExceeded(result.message.clone().unwrap_or_else(|| "Rate limit exceeded".to_string())));
        }
        Ok(result)
    }

    /// Gom logic tính reset_after cho fixed/sliding window
    fn calc_reset_after(&self, config: &PathConfig, now: Instant, window_start: Instant) -> u64 {
        config.time_window - now.duration_since(window_start).as_secs().min(config.time_window)
    }

    /// Gom logic check_fixed_window
    fn check_fixed_window(
        &self,
        state: &mut RateLimitState,
        config: &PathConfig,
        now: Instant,
    ) -> Result<RateLimitResult, RateLimitError> {
        let window_duration = std::time::Duration::from_secs(config.time_window);
        if now.duration_since(state.window_start) >= window_duration {
            state.window_start = now;
            state.request_count = 0;
        }
        if state.request_count >= config.max_requests {
            let reset_after = self.calc_reset_after(config, now, state.window_start);
            return self.make_limited_result(config, reset_after, None);
        }
        state.request_count += 1;
        let reset_after = self.calc_reset_after(config, now, state.window_start);
        Ok(RateLimitResult {
            is_limited: false,
            remaining: config.max_requests - state.request_count,
            reset_after,
            message: None,
        })
    }
    
    /// Gom logic check_sliding_window
    fn check_sliding_window(
        &self,
        state: &mut RateLimitState,
        config: &PathConfig,
        now: Instant,
    ) -> Result<RateLimitResult, RateLimitError> {
        let window_duration = std::time::Duration::from_secs(config.time_window);
        state.request_timestamps.retain(|&time| now.duration_since(time) < window_duration);
        if state.request_timestamps.len() >= config.max_requests as usize {
            let oldest = state.request_timestamps.first().copied().unwrap_or(now);
            let reset_after = self.calc_reset_after(config, now, oldest);
            return self.make_limited_result(config, reset_after, None);
        }
        state.request_timestamps.push(now);
        let oldest = state.request_timestamps.first().copied().unwrap_or(now);
        let reset_after = self.calc_reset_after(config, now, oldest);
        Ok(RateLimitResult {
            is_limited: false,
            remaining: config.max_requests - state.request_timestamps.len() as u32,
            reset_after,
            message: None,
        })
    }
    
    /// Gom logic check_token_bucket
    fn check_token_bucket(
        &self,
        state: &mut RateLimitState,
        config: &PathConfig,
        now: Instant,
    ) -> Result<RateLimitResult, RateLimitError> {
        let elapsed = now.duration_since(state.last_refill);
        let refill_rate = config.max_requests as f64 / config.time_window as f64;
        let new_tokens = (elapsed.as_secs_f64() * refill_rate).floor() as u32;
        if new_tokens > 0 {
            state.available_tokens = (state.available_tokens + new_tokens).min(config.max_requests);
            state.last_refill = now;
        }
        if state.available_tokens == 0 {
            let time_to_next_token = (1.0 / refill_rate).ceil() as u64;
            return self.make_limited_result(config, time_to_next_token, None);
        }
        state.available_tokens -= 1;
        let tokens_needed = config.max_requests - state.available_tokens;
        let time_to_full = if state.available_tokens < config.max_requests {
            (tokens_needed as f64 / refill_rate).ceil() as u64
        } else { 0 };
        Ok(RateLimitResult {
            is_limited: false,
            remaining: state.available_tokens,
            reset_after: time_to_full,
            message: None,
        })
    }
    
    /// Gom logic check_leaky_bucket
    fn check_leaky_bucket(
        &self,
        state: &mut RateLimitState,
        config: &PathConfig,
        now: Instant,
    ) -> Result<RateLimitResult, RateLimitError> {
        let elapsed = now.duration_since(state.window_start);
        let leak_rate = config.max_requests as f64 / config.time_window as f64;
        let leaked_requests = (elapsed.as_secs_f64() * leak_rate).floor() as u32;
        if leaked_requests > 0 {
            state.request_count = state.request_count.saturating_sub(leaked_requests);
            state.window_start = now;
        }
        if state.request_count >= config.max_requests {
            let time_to_leak = (1.0 / leak_rate).ceil() as u64;
            return self.make_limited_result(config, time_to_leak, None);
        }
        state.request_count += 1;
        let time_to_empty = if state.request_count > 0 {
            (state.request_count as f64 / leak_rate).ceil() as u64
        } else { 0 };
        Ok(RateLimitResult {
            is_limited: false,
            remaining: config.max_requests - state.request_count,
            reset_after: time_to_empty,
            message: None,
        })
    }
    
    /// Reset giới hạn cho một đường dẫn và identifier
    pub async fn reset_limit(&self, path: &str, identifier: RequestIdentifier) -> Result<(), RateLimitError> {
        // Kiểm tra cấu hình đường dẫn
        let configs = self.path_configs.read().await;
        
        let config = configs.get(path).ok_or_else(|| {
            RateLimitError::ConfigurationError(format!("Không tìm thấy cấu hình cho đường dẫn: {}", path))
        })?;
        
        // Lấy lock cho states với timeout
        let lock_result = match tokio::time::timeout(Duration::from_secs(1), self.states.lock()).await {
            Ok(result) => result,
            Err(_) => return Err(RateLimitError::ConfigurationError("Lock timeout".to_string())),
        };
        
        let mut states = lock_result;
        
        let key = (path.to_string(), identifier);
        
        states.insert(key, RateLimitState::new(config.max_requests));
        Ok(())
    }
    
    /// Lấy thông tin giới hạn cho một đường dẫn và identifier
    pub async fn get_limit_info(
        &self,
        path: &str,
        identifier: RequestIdentifier,
    ) -> Result<RateLimitResult, RateLimitError> {
        self.check_limit(path, identifier).await
    }
}

/// Middleware cho các web framework
pub struct RateLimitMiddleware {
    limiter: Arc<RateLimiter>,
    extract_identifier: Box<dyn Fn(&str) -> RequestIdentifier + Send + Sync>,
}

impl RateLimitMiddleware {
    /// Tạo middleware mới
    pub fn new(
        limiter: Arc<RateLimiter>,
        extract_identifier: Box<dyn Fn(&str) -> RequestIdentifier + Send + Sync>,
    ) -> Self {
        Self {
            limiter,
            extract_identifier,
        }
    }
    
    /// Kiểm tra giới hạn tốc độ
    pub async fn check(&self, path: &str, context: &str) -> Result<RateLimitResult, RateLimitError> {
        let identifier = (self.extract_identifier)(context);
        self.limiter.check_limit(path, identifier).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    
    #[test]
    fn test_fixed_window() {
        let limiter = RateLimiter::new();
        
        // Đăng ký path
        tokio_test::block_on(async {
        limiter.register_path("/api/test", PathConfig {
            max_requests: 3,
            time_window: 10,
            algorithm: RateLimitAlgorithm::FixedWindow,
            action: RateLimitAction::Reject,
            }).await.expect("Đăng ký path cho rate limiter phải thành công");
        });
        
        let id = RequestIdentifier::IpAddress("127.0.0.1".parse().expect("IP phải hợp lệ"));
        
        // Thử 3 yêu cầu - phải thành công
        for _ in 0..3 {
            let result = tokio_test::block_on(async {
                limiter.check_limit("/api/test", id.clone()).await
            }).expect("Check limit phải thành công");
            assert!(!result.is_limited);
        }
        
        // Yêu cầu thứ 4 phải bị từ chối
        let result = tokio_test::block_on(async {
            limiter.check_limit("/api/test", id.clone()).await
        });
        assert!(result.is_err());
        
        // Reset và thử lại
        tokio_test::block_on(async {
            limiter.reset_limit("/api/test", id.clone()).await
        }).expect("Reset limit phải thành công");
        
        let result = tokio_test::block_on(async {
            limiter.check_limit("/api/test", id.clone()).await
        }).expect("Check limit sau reset phải thành công");
        assert!(!result.is_limited);
    }
    
    #[test]
    fn test_sliding_window() {
        let limiter = RateLimiter::new();
        
        // Đăng ký path
        tokio_test::block_on(async {
        limiter.register_path("/api/test", PathConfig {
            max_requests: 3,
            time_window: 2,
            algorithm: RateLimitAlgorithm::SlidingWindow,
            action: RateLimitAction::Reject,
            }).await.expect("Đăng ký path cho rate limiter phải thành công");
        });
        
        let id = RequestIdentifier::IpAddress("127.0.0.1".parse().expect("IP phải hợp lệ"));
        
        // Thử 3 yêu cầu - phải thành công
        for _ in 0..3 {
            let result = tokio_test::block_on(async {
                limiter.check_limit("/api/test", id.clone()).await
            }).expect("Check limit phải thành công");
            assert!(!result.is_limited);
        }
        
        // Yêu cầu thứ 4 phải bị từ chối
        let result = tokio_test::block_on(async {
            limiter.check_limit("/api/test", id.clone()).await
        });
        assert!(result.is_err());
        
        // Đợi cho window trượt
        thread::sleep(Duration::from_secs(2));
        
        // Yêu cầu mới phải thành công
        let result = tokio_test::block_on(async {
            limiter.check_limit("/api/test", id.clone()).await
        }).expect("Check limit sau window trượt phải thành công");
        assert!(!result.is_limited);
    }
    
    #[test]
    fn test_token_bucket() {
        let limiter = RateLimiter::new();
        
        // Đăng ký path
        tokio_test::block_on(async {
        limiter.register_path("/api/test", PathConfig {
            max_requests: 3,
            time_window: 1,
            algorithm: RateLimitAlgorithm::TokenBucket,
            action: RateLimitAction::Reject,
            }).await.expect("Đăng ký path cho rate limiter phải thành công");
        });
        
        let id = RequestIdentifier::IpAddress("127.0.0.1".parse().expect("IP phải hợp lệ"));
        
        // Thử 3 yêu cầu - phải thành công
        for _ in 0..3 {
            let result = tokio_test::block_on(async {
                limiter.check_limit("/api/test", id.clone()).await
            }).expect("Check limit phải thành công");
            assert!(!result.is_limited);
        }
        
        // Yêu cầu thứ 4 phải bị từ chối
        let result = tokio_test::block_on(async {
            limiter.check_limit("/api/test", id.clone()).await
        });
        assert!(result.is_err());
        
        // Đợi để nạp lại token
        thread::sleep(Duration::from_millis(400)); // ~1.2 token
        
        // Yêu cầu mới phải thành công (vì đã có ít nhất 1 token)
        let result = tokio_test::block_on(async {
            limiter.check_limit("/api/test", id.clone()).await
        }).expect("Check limit sau refill token phải thành công");
        assert!(!result.is_limited);
    }
} 