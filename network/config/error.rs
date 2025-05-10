//! Configuration Error Types
//!
//! Module này định nghĩa các loại lỗi liên quan đến cấu hình.

use std::fmt;
use std::error::Error as StdError;
use std::io;
use serde::{Serialize, Deserialize};

/// Các lỗi liên quan đến cấu hình
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfigError {
    /// Lỗi IO khi đọc/ghi file
    IoError(String),
    
    /// Lỗi phân tích cú pháp
    ParseError(String),
    
    /// Trường bắt buộc bị thiếu
    MissingField(String),
    
    /// Giá trị không hợp lệ
    InvalidValue(String),
    
    /// Cấu hình không hợp lệ
    InvalidConfig(String),
    
    /// Lỗi chuyển đổi kiểu
    ConversionError(String),
    
    /// Xung đột giữa các cấu hình
    ConfigConflict(String),
    
    /// File cấu hình không tồn tại
    FileNotFound(String),
    
    /// Lỗi xác thực cấu hình
    ValidationError(String),
    
    /// Lỗi không xác định
    UnknownError(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::IoError(msg) => write!(f, "IO error: {}", msg),
            ConfigError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            ConfigError::MissingField(field) => write!(f, "Missing required field: {}", field),
            ConfigError::InvalidValue(msg) => write!(f, "Invalid value: {}", msg),
            ConfigError::InvalidConfig(msg) => write!(f, "Invalid configuration: {}", msg),
            ConfigError::ConversionError(msg) => write!(f, "Type conversion error: {}", msg),
            ConfigError::ConfigConflict(msg) => write!(f, "Configuration conflict: {}", msg),
            ConfigError::FileNotFound(path) => write!(f, "Configuration file not found: {}", path),
            ConfigError::ValidationError(msg) => write!(f, "Configuration validation error: {}", msg),
            ConfigError::UnknownError(msg) => write!(f, "Unknown configuration error: {}", msg),
        }
    }
}

impl StdError for ConfigError {}

impl From<io::Error> for ConfigError {
    fn from(error: io::Error) -> Self {
        ConfigError::IoError(error.to_string())
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(error: serde_json::Error) -> Self {
        ConfigError::ParseError(error.to_string())
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(error: toml::de::Error) -> Self {
        ConfigError::ParseError(error.to_string())
    }
}

impl From<std::env::VarError> for ConfigError {
    fn from(error: std::env::VarError) -> Self {
        match error {
            std::env::VarError::NotPresent => ConfigError::MissingField("Environment variable not found".to_string()),
            std::env::VarError::NotUnicode(_) => ConfigError::InvalidValue("Environment variable contains invalid Unicode".to_string()),
        }
    }
}

impl From<&str> for ConfigError {
    fn from(error: &str) -> Self {
        ConfigError::UnknownError(error.to_string())
    }
}

impl From<String> for ConfigError {
    fn from(error: String) -> Self {
        ConfigError::UnknownError(error)
    }
} 