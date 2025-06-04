//! # Network Error Management
//! 
//! Module này định nghĩa một enum lỗi tập trung cho toàn bộ domain network,
//! cùng với các trait để chuyển đổi từ các error enum cụ thể khác.
//! 
//! ## Cách sử dụng
//!
//! ```rust
//! use network::errors::NetworkError;
//! use network::errors::Result;
//!
//! fn some_function() -> Result<()> {
//!     // Có thể trả về bất kỳ lỗi nào
//!     Err(NetworkError::ValidationError("Invalid input".to_string()))
//! }
//! ```
//!
//! ## [HỢP NHẤT - 2024-09-03]
//! 
//! Module này đã hợp nhất các error type từ:
//! 
//! 1. `core/types.rs::NetworkError`: Các error cơ bản về kết nối, giao thức, auth, timeout
//! 2. `errors.rs::NetworkError`: Error tập trung cho domain network
//! 3. Các conversion từ error type cụ thể như AuthError, ValidationError, etc.
//!
//! ## [ĐỒNG NHẤT ERROR - 2024-09-10] 
//!
//! Đã cập nhật để đảm bảo tính nhất quán giữa các error types trong các module:
//!
//! 1. Đồng nhất error handling giữa `network/core/types.rs::PluginError`, `EventError` với `NetworkError` 
//! 2. Đảm bảo các specialized error types trong `network/security/input_validation.rs`, 
//!    `network/security/auth_middleware.rs` và `network/security/rate_limiter.rs` đều có 
//!    implementation `From` để convert sang `NetworkError`
//! 3. Mở rộng `NetworkError::status_code()` và `NetworkError::is_*()` để xử lý tất cả các loại lỗi
//!
//! Mọi module trong domain network đều nên sử dụng error type này thay vì định nghĩa riêng.

use std::io;
use serde::{Serialize, Deserialize};
use thiserror::Error;
use warp::reject::Reject;

use crate::core::engine::EngineError;
use crate::security::auth_middleware::AuthError;
use crate::security::rate_limiter::RateLimitError;
use crate::security::input_validation::ValidationError;
use crate::config::error::ConfigError;
use crate::infra::service_traits::ServiceError;
use crate::plugins::webrtc::WebRtcError;
use crate::plugins::libp2p::Libp2pError;
use crate::infra::redis_pool::PoolError;

/// Kết quả chung cho toàn bộ domain network
pub type Result<T> = std::result::Result<T, NetworkError>;

/// Enum lỗi tập trung cho toàn bộ domain network
#[derive(Debug, Error, Serialize, Deserialize)]
#[serde(tag = "error_type", content = "error_details")]
pub enum NetworkError {
    /// Lỗi xác thực
    #[error("Authentication error: {0}")]
    AuthError(String),
    
    /// Lỗi xác thực JWT
    #[error("JWT authentication error: {0}")]
    JwtError(String),

    /// Lỗi API Key
    #[error("API key error: {0}")]
    ApiKeyError(String),
    
    /// Lỗi cấu hình
    #[error("Configuration error: {0}")]
    ConfigError(String),
    
    /// Lỗi kết nối
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    /// Lỗi cơ sở dữ liệu
    #[error("Database error: {0}")]
    DatabaseError(String),
    
    /// Lỗi engine
    #[error("Engine error: {0}")]
    EngineError(String),
    
    /// Lỗi plugin
    #[error("Plugin error: {0}")]
    PluginError(String),
    
    /// Lỗi IO
    #[error("IO error: {0}")]
    IoError(String),
    
    /// Lỗi phân tích JSON
    #[error("JSON parsing error: {0}")]
    JsonError(String),
    
    /// Lỗi phân tích đầu vào
    #[error("Parsing error: {0}")]
    ParseError(String),
    
    /// Lỗi giới hạn tốc độ
    #[error("Rate limit error: {0}")]
    RateLimitError(String),
    
    /// Lỗi dịch vụ
    #[error("Service error: {0}")]
    ServiceError(String),
    
    /// Lỗi thời gian chờ
    #[error("Timeout error: {0}")]
    TimeoutError(String),
    
    /// Lỗi xác thực đầu vào
    #[error("Validation error: {0}")]
    ValidationError(String),
    
    /// Lỗi WebRTC
    #[error("WebRTC error: {0}")]
    WebRtcError(String),
    
    /// Lỗi Libp2p
    #[error("Libp2p error: {0}")]
    Libp2pError(String),
    
    /// Lỗi kết nối pool
    #[error("Connection pool error: {0}")]
    PoolError(String),
    
    /// Lỗi không tìm thấy
    #[error("Not found: {0}")]
    NotFoundError(String),
    
    /// Lỗi không có quyền
    #[error("Permission denied: {0}")]
    PermissionDeniedError(String),
    
    /// Lỗi giao thức
    #[error("Protocol error: {0}")]
    ProtocolError(String),
    
    /// Lỗi resource
    #[error("Resource error: {0}")]
    ResourceError(String),
    
    /// Lỗi không xác định
    #[error("Unknown error: {0}")]
    UnknownError(String),
    
    /// Lỗi sự kiện
    #[error("Event error: {0}")]
    EventError(String),
}

impl NetworkError {
    /// Tạo lỗi từ error message
    pub fn new<T: Into<String>>(message: T) -> Self {
        NetworkError::UnknownError(message.into())
    }
    
    /// Tạo lỗi service từ ServiceError enum
    pub fn from_service_error(error: ServiceError) -> Self {
        NetworkError::ServiceError(error.to_string())
    }
    
    /// Tạo lỗi emit cho event router
    pub fn emit_error(msg: impl Into<String>) -> Self {
        NetworkError::EventError(msg.into())
    }
    
    /// Tạo lỗi "đã đang chạy" cho service/router
    pub fn already_running() -> Self {
        NetworkError::EventError("Service is already running".to_string())
    }
    
    /// Tạo lỗi "chưa chạy" cho service/router
    pub fn not_running() -> Self {
        NetworkError::EventError("Service is not running".to_string())
    }
    
    pub fn is_auth_error(&self) -> bool {
        matches!(self, NetworkError::AuthError(_) | NetworkError::JwtError(_) | NetworkError::ApiKeyError(_))
    }
    
    /// Lỗi này có phải lỗi validation không
    pub fn is_validation_error(&self) -> bool {
        matches!(self, NetworkError::ValidationError(_))
    }
    
    /// Lỗi này có phải lỗi liên quan đến kết nối không
    pub fn is_connection_error(&self) -> bool {
        matches!(self, 
            NetworkError::ConnectionError(_) | 
            NetworkError::TimeoutError(_) | 
            NetworkError::WebRtcError(_) | 
            NetworkError::Libp2pError(_) |
            NetworkError::PoolError(_) |
            NetworkError::ProtocolError(_))
    }
    
    /// Lỗi này có phải lỗi sự kiện không
    pub fn is_event_error(&self) -> bool {
        matches!(self, NetworkError::EventError(_))
    }
    
    /// Lỗi này có thể retry không
    pub fn is_retryable(&self) -> bool {
        matches!(self, 
            NetworkError::ConnectionError(_) | 
            NetworkError::TimeoutError(_) |
            NetworkError::DatabaseError(_))
    }
    
    /// Mã lỗi HTTP tương ứng
    pub fn status_code(&self) -> u16 {
        match self {
            NetworkError::AuthError(_) | NetworkError::JwtError(_) | NetworkError::ApiKeyError(_) => 401,
            NetworkError::ValidationError(_) => 422,
            NetworkError::RateLimitError(_) => 429,
            NetworkError::NotFoundError(_) => 404,
            NetworkError::PermissionDeniedError(_) => 403,
            NetworkError::ResourceError(_) => 503, // Service Unavailable
            NetworkError::EventError(_) => 500,    // Internal Server Error cho event errors
            _ => 500,
        }
    }
    
    /// Log lỗi này
    pub fn log(&self) {
        use tracing::{error, warn};

        match self {
            NetworkError::AuthError(msg) | 
            NetworkError::JwtError(msg) |
            NetworkError::ApiKeyError(msg) => {
                warn!(error_type = "auth", message = %msg, "Auth error");
            },

            NetworkError::ValidationError(msg) => {
                warn!(error_type = "validation", message = %msg, "Validation error");
            },

            NetworkError::RateLimitError(msg) => {
                warn!(error_type = "rate_limit", message = %msg, "Rate limit error");
            },

            NetworkError::NotFoundError(msg) => {
                warn!(error_type = "not_found", message = %msg, "Not found error");
            },

            _ => {
                error!(error_type = ?self, message = %self, "Network error");
            }
        }
    }
}

// Triển khai From trait để convert từ các enum lỗi cụ thể khác
impl From<io::Error> for NetworkError {
    fn from(err: io::Error) -> Self {
        NetworkError::IoError(err.to_string())
    }
}

impl From<serde_json::Error> for NetworkError {
    fn from(err: serde_json::Error) -> Self {
        NetworkError::JsonError(err.to_string())
    }
}

impl From<AuthError> for NetworkError {
    fn from(err: AuthError) -> Self {
        NetworkError::AuthError(err.to_string())
    }
}

impl From<RateLimitError> for NetworkError {
    fn from(err: RateLimitError) -> Self {
        NetworkError::RateLimitError(err.to_string())
    }
}

impl From<ValidationError> for NetworkError {
    fn from(err: ValidationError) -> Self {
        NetworkError::ValidationError(err.to_string())
    }
}

impl From<ConfigError> for NetworkError {
    fn from(err: ConfigError) -> Self {
        NetworkError::ConfigError(err.to_string())
    }
}

impl From<EngineError> for NetworkError {
    fn from(err: EngineError) -> Self {
        NetworkError::EngineError(err.to_string())
    }
}

impl From<ServiceError> for NetworkError {
    fn from(err: ServiceError) -> Self {
        NetworkError::ServiceError(err.to_string())
    }
}

impl From<WebRtcError> for NetworkError {
    fn from(err: WebRtcError) -> Self {
        NetworkError::WebRtcError(err.to_string())
    }
}

impl From<Libp2pError> for NetworkError {
    fn from(err: Libp2pError) -> Self {
        NetworkError::Libp2pError(err.to_string())
    }
}

impl From<PoolError> for NetworkError {
    fn from(err: PoolError) -> Self {
        NetworkError::PoolError(err.to_string())
    }
}

impl From<anyhow::Error> for NetworkError {
    fn from(err: anyhow::Error) -> Self {
        NetworkError::UnknownError(err.to_string())
    }
}

// Implement Reject trait cho NetworkError để có thể sử dụng với warp::reject::custom
impl Reject for NetworkError {}

// Thêm implementation From<warp::Rejection> for NetworkError
impl From<warp::Rejection> for NetworkError {
    fn from(rejection: warp::Rejection) -> Self {
        // Thử extract NetworkError từ Rejection nếu đó là custom rejection
        if let Some(network_err) = rejection.find::<NetworkError>() {
            return network_err.clone();
        }
        
        // Xử lý các loại rejection chuẩn
        if rejection.is_not_found() {
            NetworkError::NotFoundError("Resource not found".to_string())
        } else if rejection.find::<warp::reject::MethodNotAllowed>().is_some() {
            NetworkError::ValidationError("Method not allowed".to_string())
        } else if rejection.find::<warp::reject::MissingHeader>().is_some() {
            NetworkError::ValidationError("Missing required header".to_string())
        } else if rejection.find::<warp::reject::InvalidQuery>().is_some() {
            NetworkError::ValidationError("Bad request: invalid query".to_string())
        } else if rejection.find::<warp::reject::InvalidHeader>().is_some() {
            NetworkError::ValidationError("Bad request: invalid header".to_string())
        } else if rejection.find::<warp::reject::PayloadTooLarge>().is_some() {
            NetworkError::ValidationError("Payload too large".to_string())
        } else if rejection.find::<warp::reject::UnsupportedMediaType>().is_some() {
            NetworkError::ValidationError("Unsupported media type".to_string())
        } else {
            NetworkError::UnknownError("Unknown rejection reason".to_string())
        }
    }
}

// Implement Clone cho NetworkError
impl Clone for NetworkError {
    fn clone(&self) -> Self {
        match self {
            Self::AuthError(s) => Self::AuthError(s.clone()),
            Self::JwtError(s) => Self::JwtError(s.clone()),
            Self::ApiKeyError(s) => Self::ApiKeyError(s.clone()),
            Self::ConfigError(s) => Self::ConfigError(s.clone()),
            Self::ConnectionError(s) => Self::ConnectionError(s.clone()),
            Self::DatabaseError(s) => Self::DatabaseError(s.clone()),
            Self::EngineError(s) => Self::EngineError(s.clone()),
            Self::PluginError(s) => Self::PluginError(s.clone()),
            Self::IoError(s) => Self::IoError(s.clone()),
            Self::JsonError(s) => Self::JsonError(s.clone()),
            Self::ParseError(s) => Self::ParseError(s.clone()),
            Self::RateLimitError(s) => Self::RateLimitError(s.clone()),
            Self::ServiceError(s) => Self::ServiceError(s.clone()),
            Self::TimeoutError(s) => Self::TimeoutError(s.clone()),
            Self::ValidationError(s) => Self::ValidationError(s.clone()),
            Self::WebRtcError(s) => Self::WebRtcError(s.clone()),
            Self::Libp2pError(s) => Self::Libp2pError(s.clone()),
            Self::PoolError(s) => Self::PoolError(s.clone()),
            Self::NotFoundError(s) => Self::NotFoundError(s.clone()),
            Self::PermissionDeniedError(s) => Self::PermissionDeniedError(s.clone()),
            Self::ProtocolError(s) => Self::ProtocolError(s.clone()),
            Self::ResourceError(s) => Self::ResourceError(s.clone()),
            Self::UnknownError(s) => Self::UnknownError(s.clone()),
            Self::EventError(s) => Self::EventError(s.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_error_types() {
        let auth_err = NetworkError::AuthError("Invalid token".to_string());
        assert!(auth_err.is_auth_error());
        assert!(!auth_err.is_validation_error());
        assert_eq!(auth_err.status_code(), 401);
        
        let validation_err = NetworkError::ValidationError("Invalid input".to_string());
        assert!(validation_err.is_validation_error());
        assert!(!validation_err.is_auth_error());
        assert_eq!(validation_err.status_code(), 422);
        
        let connection_err = NetworkError::ConnectionError("Connection refused".to_string());
        assert!(connection_err.is_connection_error());
        assert!(connection_err.is_retryable());
        assert_eq!(connection_err.status_code(), 500);
    }
    
    #[test]
    fn test_error_conversions() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "File not found");
        let network_err: NetworkError = io_err.into();
        
        match network_err {
            NetworkError::IoError(msg) => {
                assert!(msg.contains("File not found"));
            },
            other => {
                assert!(false, "Expected IoError variant but got {:?}", other);
            }
        }
    }
} 