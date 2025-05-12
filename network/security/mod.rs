//! Module bảo mật (Security) cho hệ thống network
//!
//! Module này cung cấp các tính năng bảo mật như:
//! - Xác thực và ủy quyền (Authentication and Authorization)
//! - Giới hạn tốc độ truy cập (Rate limiting)
//! - Xác thực đầu vào (Input validation)
//! - Kiểm tra bảo mật (Security checks)
//! - Middleware bảo mật (Security middleware)
//!
//! ## Module chuẩn hóa sau quá trình hợp nhất
//!
//! - `auth_middleware.rs`: Xử lý xác thực và ủy quyền người dùng, hỗ trợ JWT, API key và simple token
//! - `rate_limiter.rs`: Giới hạn tốc độ truy cập các endpoint API dựa trên IP, user hoặc các tham số khác
//! - `input_validation.rs`: Xử lý validation đầu vào chung, hỗ trợ string, number, boolean, array
//! - `api_validation.rs`: Validation đặc biệt cho các API endpoint, tương tác với input_validation
//! - `jwt.rs`: Dịch vụ xử lý JWT token riêng biệt
//! - `token.rs`: Dịch vụ quản lý token đơn giản

pub mod auth_middleware;
pub mod rate_limiter;
pub mod input_validation;
pub mod api_validation;
pub mod jwt;
pub mod token;

// =====================================================================
// CHÚ Ý: CÁC MODULE ĐÃ BỊ TRÙNG LẶP ĐÃ ĐƯỢC HỢP NHẤT VÀ XÓA
// =====================================================================
// * auth.rs -> ĐÃ XÓA, mọi tính năng đã nằm trong auth_middleware.rs
// * validation.rs -> ĐÃ XÓA, mọi tính năng đã hợp nhất vào input_validation.rs
// * input_validator.rs -> ĐÃ XÓA, mọi tính năng đã hợp nhất vào input_validation.rs
// =====================================================================

// Standard export của validation module (hợp nhất từ cả 3 module)
pub use input_validation::{
    InputValidator, ValidationErrors, ValidationError, ValidationResult, 
    FieldValidator, Sanitizer, Validate, sanitize_html, escape_sql
};

// Export các hàm kiểm tra bảo mật từ module security con của input_validation
pub use input_validation::security::{check_xss, check_sql_injection, no_xss, no_sqli, no_cmdi, no_path_traversal, all_security_checks};

// Standard export của auth module
pub use auth_middleware::{AuthService, AuthError, UserRole, AuthInfo, AuthType, Auth, Claims};

// Standard export của api_validation module
pub use api_validation::{
    ApiValidator, ValidationErrors as ApiValidationErrors, FieldRule, ApiValidationRule,
    // Validators cho các endpoint cụ thể
    LogDomainValidator, PluginTypeValidator, MetricsValidator, HealthValidator, create_default_validators
};

// Các export khác
pub use rate_limiter::{RateLimiter, RateLimitError, RateLimitAlgorithm, RateLimitAction, RateLimitResult};
pub use jwt::{JwtService, JwtConfig};
pub use token::{TokenService, TokenConfig};

/// Cấu hình cho security module
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub auth: AuthConfig,
    pub rate_limit: RateLimitConfig,
    pub input_validation: InputValidationConfig,
    pub jwt: JwtConfig,
    pub token: TokenConfig,
}
    
/// Cấu hình cho auth
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub enabled: bool,
    pub jwt_secret: String,
    pub jwt_algorithm: String,
    pub jwt_expires_in_seconds: u64,
    pub api_key_enabled: bool,
    pub rsa_private_key_path: Option<String>,
    pub rsa_public_key_path: Option<String>,
}
    
/// Cấu hình cho rate limit
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub enabled: bool,
    pub default_limit: u32,
    pub default_window_seconds: u32,
    pub redis_url: Option<String>,
    pub in_memory_capacity: usize,
}
    
/// Cấu hình cho input validation
#[derive(Debug, Clone)]
pub struct InputValidationConfig {
    pub enabled: bool,
    pub max_body_size: usize,
    pub max_uri_length: usize,
    pub max_header_count: usize,
    pub max_header_size: usize,
    pub max_query_params: usize,
    pub allowed_content_types: Vec<String>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            auth: AuthConfig::default(),
            rate_limit: RateLimitConfig::default(),
            input_validation: InputValidationConfig::default(),
            jwt: JwtConfig::default(),
            token: TokenConfig::default(),
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            jwt_secret: "default_jwt_secret_for_development_only".to_string(),
            jwt_algorithm: "HS256".to_string(),
            jwt_expires_in_seconds: 3600,
            api_key_enabled: true,
            rsa_private_key_path: None,
            rsa_public_key_path: None,
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_limit: 100,
            default_window_seconds: 60,
            redis_url: None,
            in_memory_capacity: 10000,
        }
    }
}

impl Default for InputValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_body_size: 1024 * 1024, // 1MB
            max_uri_length: 8192,
            max_header_count: 50,
            max_header_size: 8192,
            max_query_params: 30,
            allowed_content_types: vec![
                "application/json".to_string(),
                "application/x-www-form-urlencoded".to_string(),
                "multipart/form-data".to_string(),
                "text/plain".to_string(),
            ],
        }
    }
} 