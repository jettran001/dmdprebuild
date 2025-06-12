//! Middleware cho các service và API
//!
//! Module này cung cấp các middleware chuẩn hóa, bao gồm xác thực (auth),
//! logging, rate limiting và error handling để đảm bảo tính nhất quán.

pub mod auth;

/// Re-export các middleware chính
pub use auth::AuthMiddleware;

/// Enum mô tả các loại middleware
pub enum MiddlewareType {
    /// Authentication middleware
    Auth,
    
    /// Logging middleware
    Logging,
    
    /// Rate limiting middleware
    RateLimit,
    
    /// Error handling middleware
    ErrorHandling,
    
    /// Caching middleware
    Caching,
}

/// Configuration cho middleware
pub struct MiddlewareConfig {
    /// Danh sách middleware được kích hoạt
    pub enabled: Vec<MiddlewareType>,
    
    /// Cấu hình cụ thể cho từng middleware
    pub config: std::collections::HashMap<String, serde_json::Value>,
}

/// Factory function để tạo middleware chain
pub fn create_middleware_chain(config: MiddlewareConfig) -> MiddlewareChain {
    MiddlewareChain::new(config)
}

/// Middleware chain quản lý các middleware
pub struct MiddlewareChain {
    /// Cấu hình
    _config: MiddlewareConfig,
}

impl MiddlewareChain {
    /// Tạo mới middleware chain
    pub fn new(_config: MiddlewareConfig) -> Self {
        Self { _config }
    }
}

/// Trait định nghĩa interface chung cho tất cả middleware
#[async_trait::async_trait]
pub trait Middleware: Send + Sync + 'static {
    /// Xử lý request
    async fn process_request(&self, request: &mut Request) -> Result<(), MiddlewareError>;
    
    /// Xử lý response
    async fn process_response(&self, response: &mut Response) -> Result<(), MiddlewareError>;
}

/// Request structure
pub struct Request {
    /// URI
    pub uri: String,
    
    /// Method
    pub method: String,
    
    /// Headers
    pub headers: std::collections::HashMap<String, String>,
    
    /// Body
    pub body: Vec<u8>,
}

/// Response structure
pub struct Response {
    /// Status code
    pub status: u16,
    
    /// Headers
    pub headers: std::collections::HashMap<String, String>,
    
    /// Body
    pub body: Vec<u8>,
}

/// Middleware error
#[derive(Debug, thiserror::Error)]
pub enum MiddlewareError {
    /// Lỗi xác thực
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),
    
    /// Lỗi ủy quyền
    #[error("Authorization failed: {0}")]
    AuthorizationFailed(String),
    
    /// Rate limit exceeded
    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(String),
    
    /// Lỗi khác
    #[error("Middleware error: {0}")]
    Other(String),
} 