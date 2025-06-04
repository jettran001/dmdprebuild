// Copyright 2024 Diamond Chain Network
// Bảo mật - các cơ chế kiểm soát bảo mật, xác thực, phân quyền, và kiểm tra đầu vào

use std::collections::HashMap;
use serde::{Serialize, Deserialize};

pub mod auth_middleware;
pub mod input_validation;
pub mod api_validation;
pub mod rate_limiter;
pub mod jwt;
pub mod token;

// Standard export của validation module (hợp nhất từ cả 3 module)
pub use input_validation::{
    InputValidator, ValidationErrors, ValidationError, ValidationResult, 
    FieldValidator, Sanitizer, Validate
};

/// Hàm utility để sanitize HTML input
pub fn sanitize_html(input: &str) -> String {
    // Phiên bản đơn giản, chỉ loại bỏ các tag nguy hiểm
    input
        .replace("<script", "&lt;script")
        .replace("</script>", "&lt;/script&gt;")
        .replace("<iframe", "&lt;iframe")
        .replace("</iframe>", "&lt;/iframe&gt;")
        .replace("<object", "&lt;object")
        .replace("</object>", "&lt;/object&gt;")
        .replace("<embed", "&lt;embed")
        .replace("</embed>", "&lt;/embed&gt;")
        .replace("javascript:", "")
        .replace("data:", "")
        .replace("vbscript:", "")
}

/// Hàm utility để escape SQL input
pub fn escape_sql(input: &str) -> String {
    // Phiên bản đơn giản, chỉ escape các ký tự đặc biệt trong SQL
    input
        .replace("'", "''")
        .replace("\\", "\\\\")
        .replace("%", "\\%")
        .replace("_", "\\_")
}

/// Security flags
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SecurityFlags {
    /// Cho phép bypass auth trong dev mode
    #[serde(default)]
    pub allow_auth_bypass: bool,
    /// Cho phép ip nào được bypass rate limit
    #[serde(default)]
    pub rate_limit_bypass_ip: Vec<String>,
    /// Cho phép dev account
    #[serde(default)]
    pub allow_dev_accounts: bool,
    /// Yêu cầu 2FA
    #[serde(default)]
    pub require_2fa: bool,
}

/// Auth config
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AuthConfig {
    /// JWT secret key
    #[serde(default = "default_jwt_secret")]
    pub jwt_secret: String,
    /// JWT expiration time (seconds)
    #[serde(default = "default_jwt_expiration")]
    pub jwt_expiration: u64,
    /// API key expiration time (days)
    #[serde(default = "default_api_key_expiration")]
    pub api_key_expiration_days: u32,
    /// Simple token expiration time (hours)
    #[serde(default = "default_simple_token_expiration")]
    pub simple_token_expiration_hours: u32,
    /// Refresh token expiration time (days)
    #[serde(default = "default_refresh_token_expiration")]
    pub refresh_token_expiration_days: u32,
    /// Rate limit failed login attempts
    #[serde(default = "default_max_failed_login_attempts")]
    pub max_failed_login_attempts: u32,
    /// Rate limit window (minutes)
    #[serde(default = "default_failed_login_window")]
    pub failed_login_window_minutes: u32,
}

// Helper functions for default values in AuthConfig
fn default_jwt_secret() -> String { "change-me-in-production".to_string() }
fn default_jwt_expiration() -> u64 { 3600 * 24 } // 1 day
fn default_api_key_expiration() -> u32 { 30 }
fn default_simple_token_expiration() -> u32 { 24 }
fn default_refresh_token_expiration() -> u32 { 30 }
fn default_max_failed_login_attempts() -> u32 { 5 }
fn default_failed_login_window() -> u32 { 15 }

/// Input validation config
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct InputValidationConfig {
    /// Max field length
    #[serde(default = "default_max_field_length")]
    pub max_field_length: usize,
    /// Min password length
    #[serde(default = "default_min_password_length")]
    pub min_password_length: usize,
    /// Max password length
    #[serde(default = "default_max_password_length")]
    pub max_password_length: usize,
    /// Các input field bị blocked
    #[serde(default)]
    pub blocked_input: Vec<String>,
    /// Có check XSS không
    #[serde(default = "default_true")]
    pub enable_xss_protection: bool,
    /// Có check SQL injection không
    #[serde(default = "default_true")]
    pub enable_sqli_protection: bool,
    /// Có check command injection không
    #[serde(default = "default_true")]
    pub enable_cmdi_protection: bool,
    /// Có sanitize tất cả input không
    #[serde(default = "default_true")]
    pub sanitize_all_input: bool,
}

// Helper functions for default values in InputValidationConfig
fn default_max_field_length() -> usize { 1000 }
fn default_min_password_length() -> usize { 8 }
fn default_max_password_length() -> usize { 128 }
fn default_true() -> bool { true }

/// Rate limit config
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RateLimitConfig {
    /// Enable rate limiting
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Global rate limit (requests per minute)
    #[serde(default = "default_global_rate")]
    pub global_rate: u32,
    /// Endpoint specific rate limits
    #[serde(default)]
    pub endpoint_rates: HashMap<String, u32>,
    /// IP whitelist for rate limiting
    #[serde(default = "default_ip_whitelist")]
    pub ip_whitelist: Vec<String>,
    /// Block time after exceeding rate limit (minutes)
    #[serde(default = "default_block_time")]
    pub block_time_minutes: u32,
    /// Use custom headers for rate limit info
    #[serde(default = "default_true")]
    pub use_custom_headers: bool,
}

// Helper functions for default values in RateLimitConfig
fn default_global_rate() -> u32 { 60 }
fn default_ip_whitelist() -> Vec<String> { vec!["127.0.0.1".to_string()] }
fn default_block_time() -> u32 { 15 }

/// General security configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SecurityConfig {
    /// Cấu hình auth
    #[serde(default)]
    pub auth: AuthConfig,
    /// Cấu hình rate limit
    #[serde(default)]
    pub rate_limit: RateLimitConfig,
    /// Cấu hình validation
    #[serde(default)]
    pub validation: InputValidationConfig,
    /// Flags bảo mật
    #[serde(default)]
    pub security_flags: SecurityFlags,
} 