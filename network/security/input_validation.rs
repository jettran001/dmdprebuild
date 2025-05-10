//! # Input Validation
//! 
//! Module này cung cấp hệ thống validation và sanitization cho input từ người dùng.
//! Module này là module tập trung xử lý validation trong domain network, thay thế cho
//! các module input_validator.rs và validation.rs trước đây.
//!
//! ## Cách sử dụng:
//! 
//! ```rust
//! use crate::security::input_validation::{InputValidator, ValidationResult};
//! use crate::security::input_validation::string;
//! 
//! let mut validator = InputValidator::new();
//! validator.add_string_validator(
//!     FieldValidator::new("username")
//!         .add_rule(string::min_length(3))
//!         .add_rule(string::max_length(50))
//!         .add_rule(string::pattern(r"^[a-zA-Z0-9_]+$"))
//!         .required(true)
//! );
//! 
//! let data = [("username", "john_doe")];
//! let result = validator.validate(&data);
//! ```
//!
//! ## [HỢP NHẤT TRÙNG LẶP - 2024-09-01]
//! 
//! Module này hiện đã hợp nhất đầy đủ tất cả tính năng từ:
//! 
//! 1. `input_validator.rs`: Bao gồm các validator cho string, number, boolean, collection, 
//!    cùng các service và các kiểu dữ liệu hỗ trợ
//! 
//! 2. `validation.rs`: Bao gồm trait `Validate`, các validator cho email, URL, IP, 
//!    cùng các utility function và mẫu implementation
//!
//! Các file trên đã được loại bỏ, và tất cả code sử dụng chúng nên import từ module này.
//!
//! ## [ĐỒNG NHẤT ERROR - 2024-09-04]
//!
//! Lưu ý về việc xử lý lỗi:
//!
//! - `ValidationError` được định nghĩa trong module này để xử lý cụ thể các lỗi validation
//! - Có một implementation `From<ValidationError>` cho `NetworkError` trong `network/errors.rs`
//! - Khi cần trả về lỗi validation từ các API public của domain, nên convert sang
//!   `NetworkError::ValidationError`
//!
//! Ví dụ:
//! ```rust
//! use crate::errors::NetworkError;
//! use crate::security::input_validation::ValidationError;
//!
//! fn validate_input(input: &str) -> Result<(), NetworkError> {
//!     // Xử lý validation
//!     if input.is_empty() {
//!         return Err(ValidationError::Required("input".to_string()).into());
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## Features chính:
//! 
//! - Validation dựa trên field
//! - Rules engine linh hoạt
//! - Sanitization đầu vào
//! - Format validators cho các kiểu thông dụng (email, URL, etc.)
//! - Security validators (XSS, SQLi, etc.)
//! - Customizable error messages
//! - Trait `Validate` để thực hiện validation cho các struct
//! - InputValidationService quản lý đăng ký và sử dụng nhiều validator

use regex::Regex;
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use std::fmt::{self, Display};
use std::net::IpAddr;
use lazy_static::lazy_static;
use serde::{Serialize, Deserialize};
use std::marker::PhantomData;
use std::str::FromStr;
use std::any::Any;
use tracing::debug;

/// Version của các pattern validation, để dễ dàng theo dõi khi cập nhật
pub const PATTERN_VERSION: &str = "2024.09.01.1";

/// Patterns cho các định dạng phổ biến
pub const FORMAT_PATTERNS: &[(&str, &str)] = &[
    ("email", r"^[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$"),
    ("url", r"^(https?|ftp)://[^\s/$.?#].[^\s]*$"),
    ("alphanumeric", r"^[a-zA-Z0-9]+$"),
    ("numeric", r"^[0-9]+$"),
    ("alpha", r"^[a-zA-Z]+$"),
    ("date", r"^\d{4}-\d{2}-\d{2}$"),
    ("time", r"^\d{2}:\d{2}(:\d{2})?$"),
    ("datetime", r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2})?(\.\d+)?(Z|[+-]\d{2}:\d{2})?$"),
    ("ipv4", r"^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$"),
    ("ipv6", r"^([0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}|([0-9a-fA-F]{1,4}:){1,7}:|([0-9a-fA-F]{1,4}:){1,6}:[0-9a-fA-F]{1,4}|([0-9a-fA-F]{1,4}:){1,5}(:[0-9a-fA-F]{1,4}){1,2}|([0-9a-fA-F]{1,4}:){1,4}(:[0-9a-fA-F]{1,4}){1,3}|([0-9a-fA-F]{1,4}:){1,3}(:[0-9a-fA-F]{1,4}){1,4}|([0-9a-fA-F]{1,4}:){1,2}(:[0-9a-fA-F]{1,4}){1,5}|[0-9a-fA-F]{1,4}:((:[0-9a-fA-F]{1,4}){1,6})|:((:[0-9a-fA-F]{1,4}){1,7}|:)|fe80:(:[0-9a-fA-F]{0,4}){0,4}%[0-9a-zA-Z]{1,}|::(ffff(:0{1,4}){0,1}:){0,1}((25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])\.){3,3}(25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])|([0-9a-fA-F]{1,4}:){1,4}:((25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])\.){3,3}(25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])$"),
    ("uuid", r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"),
    ("hex", r"^[0-9a-fA-F]+$"),
    ("base64", r"^[A-Za-z0-9+/]*={0,2}$"),
];

/// Các patterns để phát hiện XSS
pub const XSS_PATTERNS: &[&str] = &[
    r#"<script[^>]*>.*?</script>"#,
    r#"javascript:"#,
    r#"on\w+\s*="#,
    r#"<img[^>]*src\s*=\s*['\"]?data:"#,
    r#"<iframe[^>]*>"#,
    r#"<object[^>]*>"#,
    r#"<embed[^>]*>"#,
    r#"<base[^>]*>"#,
    r#"<applet[^>]*>"#,
    r#"<form[^>]*>"#,
    r#"<svg[^>]*>.*?on\w+\s*="#,
    r#"expression\s*\("#,
    r#"url\s*\("#,
    r#"alert\s*\("#,
    r#"confirm\s*\("#,
    r#"prompt\s*\("#,
    r#"eval\s*\("#,
    r#"setTimeout\s*\("#,
    r#"setInterval\s*\("#,
    r#"document\.cookie"#,
    r#"document\.domain"#,
];

/// Các patterns để phát hiện SQL Injection
pub const SQLI_PATTERNS: &[&str] = &[
    r#"(?i)\bSELECT\b.+\bFROM\b"#,
    r#"(?i)\bINSERT\b.+\bINTO\b"#,
    r#"(?i)\bUPDATE\b.+\bSET\b"#,
    r#"(?i)\bDELETE\b.+\bFROM\b"#,
    r#"(?i)\bDROP\b.+\bTABLE\b"#,
    r#"(?i)\bALTER\b.+\bTABLE\b"#,
    r#"(?i)\bCREATE\b.+\bTABLE\b"#,
    r#"(?i)\bUNION\b.+\bSELECT\b"#,
    r#"(?i)(\b|')OR(\b|')\s*1\s*=\s*1"#,
    r#"(?i)--"#,
    r#"(?i)/\*.*?\*/"#,
    r#"(?i)SLEEP\s*\("# 
];

/// Các patterns phát hiện command injection
pub const CMDI_PATTERNS: &[&str] = &[
    r#"[\n\r]"#,
    r#"[;`&|]"#,
    r#"\$\("#,
    r#"\b(?:bash|sh|ksh|csh|tcsh|zsh)\b"#,
    r#"\b(?:cmd\.exe|powershell\.exe|pwsh\.exe)\b"#,
    r#"\b(?:cat|ls|dir|rm|cp|mv|chmod|chown|wget|curl|nc|ncat|netcat)\b"#,
];

/// Các patterns phát hiện path traversal
pub const PATH_TRAVERSAL_PATTERNS: &[&str] = &[
    r#"(?:\.\.[\\/]){1,}"#,
    r#"%2e%2e[\\/]"#,
    r#"\.\.%2f"#,
    r#"%252e%252e%255c"#,
    r#"(?:\/|\\)(?:etc|var|usr|home|root|windows|system|boot)(?:\/|\\)"#,
];

/// Các lỗi validation
#[derive(Error, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationError {
    /// Lỗi khi field không thỏa mãn pattern
    #[error("Giá trị cần phải thỏa mãn pattern: {0}")]
    Pattern(String),
    
    /// Lỗi khi field có độ dài không hợp lệ
    #[error("Độ dài phải nằm trong khoảng: {min} - {max}")]
    Length { min: usize, max: usize },
    
    /// Lỗi khi giá trị nằm ngoài khoảng cho phép
    #[error("Giá trị phải nằm trong khoảng: {min} - {max}")]
    Range { min: String, max: String },
    
    /// Lỗi khi field bắt buộc không được cung cấp hoặc rỗng
    #[error("Field '{0}' là bắt buộc")]
    Required(String),
    
    /// Lỗi khi giá trị không thuộc danh sách cho phép
    #[error("Giá trị phải là một trong: {0}")]
    OneOf(String),
    
    /// Lỗi khi field chứa ký tự không hợp lệ
    #[error("Field '{0}' chứa ký tự không hợp lệ")]
    InvalidChars(String),
    
    /// Lỗi khi field không đúng định dạng
    #[error("Field '{0}' không đúng định dạng: {1}")]
    Format(String, String),
    
    /// Lỗi khi sanitizing input
    #[error("Lỗi khi sanitize field '{0}': {1}")]
    SanitizationError(String, String),
    
    /// Lỗi khi field con không hợp lệ
    #[error("Field con '{0}' không hợp lệ: {1}")]
    NestedField(String, String),
    
    /// Lỗi khi field chứa nội dung không an toàn
    #[error("Field '{0}' chứa nội dung không an toàn: {1}")]
    Security(String, String),
    
    /// Lỗi khi field không đúng kiểu
    #[error("Field '{0}' không đúng kiểu: {1}")]
    Type(String, String),
    
    /// Lỗi tùy chỉnh
    #[error("{0}")]
    Custom(String),
}

/// Kết quả của validator
pub type ValidationResult = Result<(), ValidationError>;

/// Kết quả của nhiều validators
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidationErrors {
    /// Có lỗi validation hay không
    pub has_errors: bool,
    
    /// Validation có hợp lệ hay không
    pub is_valid: bool,
    
    /// Danh sách các lỗi theo field
    pub errors: HashMap<String, Vec<ValidationError>>,
}

impl ValidationErrors {
    /// Tạo mới ValidationErrors
    pub fn new() -> Self {
        Self {
            has_errors: false,
            is_valid: true,
            errors: HashMap::new(),
        }
    }
    
    /// Thêm lỗi cho field
    pub fn add(&mut self, field: &str, error: ValidationError) {
        self.has_errors = true;
        self.is_valid = false;
        self.errors.entry(field.to_string()).or_insert_with(Vec::new).push(error);
    }
    
    /// Kiểm tra xem có lỗi không
    pub fn has_errors(&self) -> bool {
        self.has_errors
    }
    
    /// Kiểm tra xem validation có hợp lệ không
    pub fn is_valid(&self) -> bool {
        self.is_valid
    }
    
    /// Lấy lỗi cho một field
    pub fn get_errors(&self, field: &str) -> Option<&Vec<ValidationError>> {
        self.errors.get(field)
    }
    
    /// Lấy danh sách các field có lỗi
    pub fn get_fields_with_errors(&self) -> Vec<&String> {
        self.errors.keys().collect()
    }
    
    /// Merge nhiều ValidationErrors
    pub fn merge(&mut self, other: ValidationErrors) {
        self.has_errors = self.has_errors || other.has_errors;
        self.is_valid = self.is_valid && other.is_valid;
            
            for (field, errors) in other.errors {
            for error in errors {
                self.add(&field, error);
            }
        }
    }
    
    /// Format lỗi thành string
    pub fn format_errors(&self) -> String {
        if !self.has_errors {
            return "Validation passed".to_string();
        }
        
        let mut result = format!("Validation failed with {} errors:\n", self.errors.len());
            
        for (field, errors) in &self.errors {
            for error in errors {
                result.push_str(&format!("  - {}: {}\n", field, error));
            }
        }
        
        result
    }
}

/// Field validator cho các kiểu dữ liệu cụ thể
pub struct FieldValidator<'a, T> {
    name: &'a str,
    rules: Vec<Box<dyn Fn(&T) -> ValidationResult + Send + Sync + 'a>>,
    required: bool,
    _marker: PhantomData<T>,
}

// Custom implementation Debug thay vì derive
impl<'a, T: 'a> std::fmt::Debug for FieldValidator<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FieldValidator")
            .field("name", &self.name)
            .field("rules", &format!("[{} rule functions]", self.rules.len()))
            .field("required", &self.required)
            .finish()
    }
}

// Không thể implement Clone cho Fn trait, nhưng có thể tạo một phương thức clone_box
impl<'a, T: 'a> FieldValidator<'a, T> {
    pub fn clone_box(&self) -> FieldValidator<'a, T> 
    where T: 'a {
        // Vì không thể clone Box<dyn Fn>, chúng ta chỉ có thể tạo validator mới
        let mut result = FieldValidator::new(self.name);
        result.required = self.required;
        result
    }
}

impl<'a, T> FieldValidator<'a, T> {
    /// Tạo validator mới cho field
    pub fn new(name: &'a str) -> Self {
        Self {
            name,
            rules: Vec::new(),
            required: false,
            _marker: PhantomData,
        }
    }
    
    /// Thêm rule cho validator
    pub fn add_rule<F>(mut self, rule: F) -> Self 
    where
        F: Fn(&T) -> ValidationResult + Send + Sync + 'a
    {
        self.rules.push(Box::new(rule));
        self
    }
    
    /// Đặt field là bắt buộc
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
    
    /// Kiểm tra giá trị có hợp lệ không
    pub fn validate(&self, value: Option<&T>) -> ValidationResult {
        if let Some(val) = value {
            // Áp dụng tất cả các rules
            for rule in &self.rules {
                rule(val)?;
            }
            Ok(())
        } else if self.required {
            // Báo lỗi nếu field bắt buộc nhưng không có giá trị
            Err(ValidationError::Required(self.name.to_string()))
        } else {
            // Field không bắt buộc và không có giá trị -> hợp lệ
            Ok(())
        }
    }
}

/// Input validator chính
pub struct InputValidator<'a> {
    string_validators: HashMap<&'a str, FieldValidator<'a, String>>,
    number_validators: HashMap<&'a str, FieldValidator<'a, i64>>,
    ip_validators: HashMap<&'a str, FieldValidator<'a, IpAddr>>,
    bool_validators: HashMap<&'a str, FieldValidator<'a, bool>>,
    custom_validators: Vec<Box<dyn Fn(&[(&str, &str)]) -> ValidationResult + Send + Sync + 'a>>,
}

impl<'a> InputValidator<'a> {
    /// Tạo mới input validator
    pub fn new() -> Self {
        Self {
            string_validators: HashMap::new(),
            number_validators: HashMap::new(),
            ip_validators: HashMap::new(),
            bool_validators: HashMap::new(),
            custom_validators: Vec::new(),
        }
    }
    
    /// Thêm validator cho string field
    pub fn add_string_validator(&mut self, validator: FieldValidator<'a, String>) {
        self.string_validators.insert(validator.name, validator);
    }
    
    /// Thêm validator cho number field
    pub fn add_number_validator(&mut self, validator: FieldValidator<'a, i64>) {
        self.number_validators.insert(validator.name, validator);
    }
    
    /// Thêm validator cho IP field
    pub fn add_ip_validator(&mut self, validator: FieldValidator<'a, IpAddr>) {
        self.ip_validators.insert(validator.name, validator);
    }
    
    /// Thêm validator cho boolean field
    pub fn add_bool_validator(&mut self, validator: FieldValidator<'a, bool>) {
        self.bool_validators.insert(validator.name, validator);
    }
    
    /// Thêm custom validator xử lý nhiều field cùng lúc
    pub fn add_custom_validator<F>(&mut self, validator: F)
    where
        F: Fn(&[(&str, &str)]) -> ValidationResult + Send + Sync + 'a
    {
        self.custom_validators.push(Box::new(validator));
    }
    
    /// Validate các field
    pub fn validate(&self, data: &[(&str, &str)]) -> ValidationErrors {
        let mut errors = ValidationErrors::new();
        
        // Tạo hashmap từ input data để dễ truy cập
        let data_map: HashMap<&str, &str> = data.iter().cloned().collect();
        
        // Validate các string field
        for (name, validator) in &self.string_validators {
            let value = data_map.get(name).map(|&v| v.to_string());
            if let Err(err) = validator.validate(value.as_ref()) {
                errors.add(name, err);
            }
        }
        
        // Validate các number field
        for (name, validator) in &self.number_validators {
            let value = data_map.get(name).and_then(|&v| v.parse::<i64>().ok());
            if let Err(err) = validator.validate(value.as_ref()) {
                errors.add(name, err);
            }
        }
        
        // Validate các IP field
        for (name, validator) in &self.ip_validators {
            let value = data_map.get(name).and_then(|&v| v.parse::<IpAddr>().ok());
            if let Err(err) = validator.validate(value.as_ref()) {
                errors.add(name, err);
            }
        }
        
        // Validate các boolean field
        for (name, validator) in &self.bool_validators {
            let value = data_map.get(name).and_then(|&v| v.parse::<bool>().ok());
            if let Err(err) = validator.validate(value.as_ref()) {
                errors.add(name, err);
            }
        }
        
        // Áp dụng các custom validators
        for validator in &self.custom_validators {
            if let Err(err) = validator(data) {
                errors.add("_custom", err);
            }
        }
        
        errors
    }
    
    /// Kiểm tra nội dung có an toàn hay không
    pub fn check_security(&self, content: &str) -> ValidationResult {
        // Kiểm tra XSS
        for pattern in XSS_PATTERNS {
            if Regex::new(pattern).unwrap_or_else(|_| Regex::new(r"^$").unwrap()).is_match(content) {
                return Err(ValidationError::Security("content".to_string(), "Phát hiện XSS".to_string()));
            }
        }
        
        // Kiểm tra SQLi
        for pattern in SQLI_PATTERNS {
            if Regex::new(pattern).unwrap_or_else(|_| Regex::new(r"^$").unwrap()).is_match(content) {
                return Err(ValidationError::Security("content".to_string(), "Phát hiện SQL Injection".to_string()));
            }
        }
        
        // Kiểm tra Command Injection
        for pattern in CMDI_PATTERNS {
            if Regex::new(pattern).unwrap_or_else(|_| Regex::new(r"^$").unwrap()).is_match(content) {
                return Err(ValidationError::Security("content".to_string(), "Phát hiện Command Injection".to_string()));
            }
        }
        
        // Kiểm tra Path Traversal
        for pattern in PATH_TRAVERSAL_PATTERNS {
            if Regex::new(pattern).unwrap_or_else(|_| Regex::new(r"^$").unwrap()).is_match(content) {
                return Err(ValidationError::Security("content".to_string(), "Phát hiện Path Traversal".to_string()));
            }
        }
        
        Ok(())
    }
}

impl<'a> Default for InputValidator<'a> {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait cho sanitizer
pub trait Sanitizer {
    /// Sanitize input
    fn sanitize(&self, input: &str) -> Result<String, ValidationError>;
}

/// Module chứa các validator cho string
pub mod string {
    use super::*;
    
    /// Kiểm tra độ dài tối thiểu
    pub fn min_length(min: usize) -> impl Fn(&String) -> ValidationResult {
        move |value: &String| {
            if value.len() >= min {
                Ok(())
            } else {
                Err(ValidationError::Length {
                    min,
                    max: usize::MAX,
                })
            }
        }
    }
    
    /// Kiểm tra độ dài tối đa
    pub fn max_length(max: usize) -> impl Fn(&String) -> ValidationResult {
        move |value: &String| {
            if value.len() <= max {
                Ok(())
            } else {
                Err(ValidationError::Length {
                    min: 0,
                    max,
                })
            }
        }
    }
    
    /// Kiểm tra độ dài trong khoảng
    pub fn length_between(min: usize, max: usize) -> impl Fn(&String) -> ValidationResult {
        move |value: &String| {
            if value.len() >= min && value.len() <= max {
                Ok(())
            } else {
                Err(ValidationError::Length {
                    min,
                    max,
                })
            }
        }
    }
    
    /// Kiểm tra pattern
    pub fn pattern(pattern: &'static str) -> impl Fn(&String) -> ValidationResult {
        move |value: &String| {
            let re = Regex::new(pattern)
                .map_err(|_| ValidationError::Pattern(pattern.to_string()))?;
                
            if re.is_match(value) {
                Ok(())
            } else {
                Err(ValidationError::Pattern(pattern.to_string()))
            }
        }
    }
    
    /// Kiểm tra format
    pub fn format(format_name: &'static str) -> impl Fn(&String) -> ValidationResult {
        move |value: &String| {
            if let Some(&(_, pattern)) = FORMAT_PATTERNS.iter().find(|&&(name, _)| name == format_name) {
                let re = Regex::new(pattern)
                    .map_err(|_| ValidationError::Format(String::new(), format_name.to_string()))?;
                    
                if re.is_match(value) {
                    Ok(())
                } else {
                    Err(ValidationError::Format(String::new(), format_name.to_string()))
                }
            } else {
                Err(ValidationError::Format(String::new(), format_name.to_string()))
            }
        }
    }
    
    /// Kiểm tra email
    pub fn email() -> impl Fn(&String) -> ValidationResult {
        format("email")
    }
    
    /// Kiểm tra URL
    pub fn url() -> impl Fn(&String) -> ValidationResult {
        format("url")
    }
    
    /// Kiểm tra alphanumeric
    pub fn alphanumeric() -> impl Fn(&String) -> ValidationResult {
        format("alphanumeric")
    }
    
    /// Kiểm tra numeric
    pub fn numeric() -> impl Fn(&String) -> ValidationResult {
        format("numeric")
    }
    
    /// Kiểm tra alpha
    pub fn alpha() -> impl Fn(&String) -> ValidationResult {
        format("alpha")
    }
    
    /// Kiểm tra là một trong các giá trị cho phép
    pub fn one_of(values: &'static [&'static str]) -> impl Fn(&String) -> ValidationResult {
        move |value: &String| {
            if values.contains(&value.as_str()) {
                Ok(())
            } else {
                Err(ValidationError::OneOf(values.join(", ")))
            }
        }
    }
}

/// Module chứa các validator cho number
pub mod number {
    use super::*;
    
    /// Kiểm tra giá trị tối thiểu
    pub fn min(min: i64) -> impl Fn(&i64) -> ValidationResult {
        move |value: &i64| {
            if *value >= min {
                Ok(())
            } else {
                Err(ValidationError::Range {
                    min: min.to_string(),
                    max: "no limit".to_string(),
                })
            }
        }
    }
    
    /// Kiểm tra giá trị tối đa
    pub fn max(max: i64) -> impl Fn(&i64) -> ValidationResult {
        move |value: &i64| {
            if *value <= max {
                Ok(())
            } else {
                Err(ValidationError::Range {
                    min: "no limit".to_string(),
                    max: max.to_string(),
                })
            }
        }
    }
    
    /// Kiểm tra giá trị trong khoảng
    pub fn between(min: i64, max: i64) -> impl Fn(&i64) -> ValidationResult {
        move |value: &i64| {
            if *value >= min && *value <= max {
                Ok(())
            } else {
                Err(ValidationError::Range {
                    min: min.to_string(),
                    max: max.to_string(),
                })
            }
        }
    }
    
    /// Kiểm tra là một trong các giá trị cho phép
    pub fn one_of(values: &'static [i64]) -> impl Fn(&i64) -> ValidationResult {
        move |value: &i64| {
            if values.contains(value) {
                Ok(())
            } else {
                Err(ValidationError::OneOf(values.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ")))
            }
        }
    }
}

/// Module chứa các security validators
pub mod security {
    use super::*;
    
    /// Kiểm tra XSS
    pub fn no_xss() -> impl Fn(&String) -> ValidationResult {
        move |value: &String| {
            for pattern in XSS_PATTERNS {
                if Regex::new(pattern).unwrap_or_else(|_| Regex::new(r"^$").unwrap()).is_match(value) {
                    return Err(ValidationError::Security(String::new(), "Phát hiện XSS".to_string()));
                }
            }
            Ok(())
        }
    }
    
    /// Kiểm tra SQL Injection
    pub fn no_sqli() -> impl Fn(&String) -> ValidationResult {
        move |value: &String| {
            for pattern in SQLI_PATTERNS {
                if Regex::new(pattern).unwrap_or_else(|_| Regex::new(r"^$").unwrap()).is_match(value) {
                    return Err(ValidationError::Security(String::new(), "Phát hiện SQL Injection".to_string()));
                }
            }
            Ok(())
        }
    }
    
    /// Kiểm tra Command Injection
    pub fn no_cmdi() -> impl Fn(&String) -> ValidationResult {
        move |value: &String| {
            for pattern in CMDI_PATTERNS {
                if Regex::new(pattern).unwrap_or_else(|_| Regex::new(r"^$").unwrap()).is_match(value) {
                    return Err(ValidationError::Security(String::new(), "Phát hiện Command Injection".to_string()));
                }
            }
            Ok(())
        }
    }
    
    /// Kiểm tra Path Traversal
    pub fn no_path_traversal() -> impl Fn(&String) -> ValidationResult {
        move |value: &String| {
            for pattern in PATH_TRAVERSAL_PATTERNS {
                if Regex::new(pattern).unwrap_or_else(|_| Regex::new(r"^$").unwrap()).is_match(value) {
                    return Err(ValidationError::Security(String::new(), "Phát hiện Path Traversal".to_string()));
                }
            }
            Ok(())
        }
    }
    
    /// Kiểm tra tất cả các security threats
    pub fn all_security_checks() -> impl Fn(&String) -> ValidationResult {
        move |value: &String| {
            no_xss()(value)?;
            no_sqli()(value)?;
            no_cmdi()(value)?;
            no_path_traversal()(value)?;
            Ok(())
        }
    }
    
    /// Kiểm tra XSS trong input
    /// 
    /// # Arguments
    /// * `input` - Chuỗi cần kiểm tra
    /// * `field_name` - Tên trường (để hiển thị trong lỗi)
    /// 
    /// # Returns
    /// * `Result<(), String>` - Ok nếu không có XSS, Err với thông báo lỗi nếu phát hiện XSS
    pub fn check_xss(input: &str, field_name: &str) -> Result<(), String> {
        for pattern in XSS_PATTERNS {
            if Regex::new(pattern).unwrap_or_else(|_| Regex::new(r"^$").unwrap()).is_match(input) {
                return Err(format!("XSS detected in {}: {}", field_name, pattern));
            }
        }
        Ok(())
    }
    
    /// Kiểm tra SQL Injection trong input
    /// 
    /// # Arguments
    /// * `input` - Chuỗi cần kiểm tra
    /// * `field_name` - Tên trường (để hiển thị trong lỗi)
    /// 
    /// # Returns
    /// * `Result<(), String>` - Ok nếu không có SQL Injection, Err với thông báo lỗi nếu phát hiện SQL Injection
    pub fn check_sql_injection(input: &str, field_name: &str) -> Result<(), String> {
        for pattern in SQLI_PATTERNS {
            if Regex::new(pattern).unwrap_or_else(|_| Regex::new(r"^$").unwrap()).is_match(input) {
                return Err(format!("SQL Injection detected in {}: {}", field_name, pattern));
            }
        }
        Ok(())
    }
    
    /// Sanitize HTML input
    pub fn sanitize_html(input: &str) -> String {
        super::sanitize_html(input)
    }
}

/// Hàm utility để sanitize HTML input
pub fn sanitize_html(input: &str) -> String {
    // Phiên bản đơn giản, chỉ loại bỏ các tag nguy hiểm
    let sanitized = input
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
        .replace("vbscript:", "");
        
    sanitized
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

/// Định dạng để validate
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Format {
    Email,
    URL,
    IP,
    UUID,
    Date,
    Time,
    DateTime,
    Alpha,
    Alphanumeric,
    Numeric,
    Integer,
    Float,
    Boolean,
    Base64,
    Hex,
    CreditCard,
    Phone,
    ZipCode,
    #[serde(with = "serde_custom_format")]
    Custom(&'static str, &'static str),
}

/// Helper module để serialize và deserialize Format::Custom
mod serde_custom_format {
    use super::Format;
    use serde::{Deserialize, Deserializer, Serializer, Serialize};
    use serde::de::Visitor;
    use std::fmt;

    pub fn serialize<S>(name: &&'static str, pattern: &&'static str, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        (name, pattern).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<(&'static str, &'static str), D::Error>
    where
        D: Deserializer<'de>,
    {
        struct CustomVisitor;

        impl<'de> Visitor<'de> for CustomVisitor {
            type Value = (&'static str, &'static str);

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a tuple with two &'static str")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let name = seq.next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
                let pattern = seq.next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(1, &self))?;
                
                Ok((name, pattern))
            }
        }

        deserializer.deserialize_seq(CustomVisitor)
    }
}

/// Trait cho các validator
pub trait Validator<T> {
    /// Validate giá trị
    fn validate(&self, value: &T) -> ValidationResult;
    
    /// Mô tả validator
    fn describe(&self) -> String;
}

/// Validator cho string
pub struct StringValidator {
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub pattern: Option<Regex>,
    pub format: Option<Format>,
    pub allowed_values: Option<Vec<String>>,
    pub required: bool,
    pub trim: bool,
    pub no_special_chars: bool,
}

impl Default for StringValidator {
    fn default() -> Self {
        Self {
            min_length: None,
            max_length: None,
            pattern: None,
            format: None,
            allowed_values: None,
            required: false,
            trim: true,
            no_special_chars: false,
        }
    }
}

impl StringValidator {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn min_length(mut self, min: usize) -> Self {
        self.min_length = Some(min);
        self
    }
    
    pub fn max_length(mut self, max: usize) -> Self {
        self.max_length = Some(max);
        self
    }
    
    pub fn pattern(mut self, pattern: Regex) -> Self {
        self.pattern = Some(pattern);
        self
    }
    
    pub fn pattern_str(mut self, pattern: &str) -> Self {
        self.pattern = Some(Regex::new(pattern).expect("Invalid regex pattern"));
        self
    }
    
    pub fn format(mut self, format: Format) -> Self {
        self.format = Some(format);
        self
    }
    
    pub fn allowed_values(mut self, values: Vec<String>) -> Self {
        self.allowed_values = Some(values);
        self
    }
    
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
    
    pub fn trim(mut self, trim: bool) -> Self {
        self.trim = trim;
        self
    }
    
    pub fn no_special_chars(mut self, no_special_chars: bool) -> Self {
        self.no_special_chars = no_special_chars;
        self
    }
}

impl Validator<String> for StringValidator {
    fn validate(&self, value: &String) -> ValidationResult {
        let value = if self.trim { value.trim() } else { value };
        
        if value.is_empty() {
            if self.required {
                return Err(ValidationError::Required("string".to_string()));
            }
            return Ok(());
        }
        
        if let Some(min) = self.min_length {
            if value.len() < min {
                return Err(ValidationError::Length { min, max: usize::MAX });
            }
        }
        
        if let Some(max) = self.max_length {
            if value.len() > max {
                return Err(ValidationError::Length { min: 0, max });
            }
        }
        
        if let Some(pattern) = &self.pattern {
            if !pattern.is_match(value) {
                return Err(ValidationError::Pattern(pattern.to_string()));
            }
        }
        
        if let Some(format) = &self.format {
            if !validate_format(value, *format) {
                return Err(ValidationError::Format("string".to_string(), format_to_string(*format)));
            }
        }
        
        if let Some(allowed) = &self.allowed_values {
            if !allowed.contains(&value.to_string()) {
                return Err(ValidationError::OneOf(allowed.join(", ")));
            }
        }
        
        if self.no_special_chars && !value.chars().all(|c| c.is_alphanumeric()) {
            return Err(ValidationError::InvalidChars("string".to_string()));
        }
        
        Ok(())
    }
    
    fn describe(&self) -> String {
        let mut desc = vec!["String validator".to_string()];
        
        if self.required {
            desc.push("required".to_string());
        }
        
        if let Some(min) = self.min_length {
            desc.push(format!("min length: {}", min));
        }
        
        if let Some(max) = self.max_length {
            desc.push(format!("max length: {}", max));
        }
        
        if let Some(pattern) = &self.pattern {
            desc.push(format!("pattern: {}", pattern));
        }
        
        if let Some(format) = &self.format {
            desc.push(format!("format: {}", format_to_string(*format)));
        }
        
        if let Some(allowed) = &self.allowed_values {
            desc.push(format!("allowed values: {}", allowed.join(", ")));
        }
        
        if self.trim {
            desc.push("trimmed".to_string());
        }
        
        if self.no_special_chars {
            desc.push("no special chars".to_string());
        }
        
        desc.join(", ")
    }
}

impl Sanitizer for StringValidator {
    fn sanitize(&self, input: &str) -> Result<String, ValidationError> {
        let mut result = if self.trim { input.trim().to_string() } else { input.to_string() };
        
        if self.no_special_chars {
            result = result.chars().filter(|c| c.is_alphanumeric()).collect();
        }
        
        Ok(result)
    }
}

/// Validator cho số
pub struct NumberValidator<T: PartialOrd + ToString + Copy> {
    pub min: Option<T>,
    pub max: Option<T>,
    pub allowed_values: Option<Vec<T>>,
    pub required: bool,
}

impl<T: PartialOrd + ToString + Copy> Default for NumberValidator<T> {
    fn default() -> Self {
        Self {
            min: None,
            max: None,
            allowed_values: None,
            required: false,
        }
    }
}

impl<T: PartialOrd + ToString + Copy> NumberValidator<T> {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn min(mut self, min: T) -> Self {
        self.min = Some(min);
        self
    }
    
    pub fn max(mut self, max: T) -> Self {
        self.max = Some(max);
        self
    }
    
    pub fn allowed_values(mut self, values: Vec<T>) -> Self {
        self.allowed_values = Some(values);
        self
    }
    
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
}

impl<T: PartialOrd + ToString + Copy> Validator<T> for NumberValidator<T> {
    fn validate(&self, value: &T) -> ValidationResult {
        if let Some(min) = self.min {
            if *value < min {
                return Err(ValidationError::Range {
                    min: min.to_string(),
                    max: self.max.map(|m| m.to_string()).unwrap_or_else(|| "∞".to_string()),
                });
            }
        }
        
        if let Some(max) = self.max {
            if *value > max {
                return Err(ValidationError::Range {
                    min: self.min.map(|m| m.to_string()).unwrap_or_else(|| "-∞".to_string()),
                    max: max.to_string(),
                });
            }
        }
        
        if let Some(allowed) = &self.allowed_values {
            if !allowed.contains(value) {
                return Err(ValidationError::OneOf(
                    allowed.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ")
                ));
            }
        }
        
        Ok(())
    }
    
    fn describe(&self) -> String {
        let mut desc = vec!["Number validator".to_string()];
        
        if self.required {
            desc.push("required".to_string());
        }
        
        if let Some(min) = self.min {
            desc.push(format!("min: {}", min.to_string()));
        }
        
        if let Some(max) = self.max {
            desc.push(format!("max: {}", max.to_string()));
        }
        
        if let Some(allowed) = &self.allowed_values {
            desc.push(format!("allowed values: {}", 
                allowed.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ")
            ));
        }
        
        desc.join(", ")
    }
}

impl<T: PartialOrd + ToString + Copy> Sanitizer for NumberValidator<T> {
    fn sanitize(&self, input: &str) -> Result<String, ValidationError> {
        // Giữ nguyên input vì số không cần sanitize
        Ok(input.to_string())
    }
}

/// Validator cho boolean
pub struct BooleanValidator {
    pub required: bool,
}

impl Default for BooleanValidator {
    fn default() -> Self {
        Self {
            required: false,
        }
    }
}

impl BooleanValidator {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
}

impl Validator<bool> for BooleanValidator {
    fn validate(&self, _value: &bool) -> ValidationResult {
        Ok(())
    }
    
    fn describe(&self) -> String {
        let mut desc = vec!["Boolean validator".to_string()];
        
        if self.required {
            desc.push("required".to_string());
        }
        
        desc.join(", ")
    }
}

impl Sanitizer for BooleanValidator {
    fn sanitize(&self, input: &str) -> Result<String, ValidationError> {
        // Giữ nguyên input vì boolean không cần sanitize
        Ok(input.to_string())
    }
}

/// Validator cho collection
pub struct CollectionValidator<T, V>
where
    V: Validator<T>,
{
    pub min_items: Option<usize>,
    pub max_items: Option<usize>,
    pub unique_items: bool,
    pub required: bool,
    pub item_validator: V,
    _phantom: PhantomData<T>,
}

impl<T, V> CollectionValidator<T, V>
where
    V: Validator<T>,
{
    pub fn new(item_validator: V) -> Self {
        Self {
            min_items: None,
            max_items: None,
            unique_items: false,
            required: false,
            item_validator,
            _phantom: PhantomData,
        }
    }
    
    pub fn min_items(mut self, min: usize) -> Self {
        self.min_items = Some(min);
        self
    }
    
    pub fn max_items(mut self, max: usize) -> Self {
        self.max_items = Some(max);
        self
    }
    
    pub fn unique_items(mut self, unique: bool) -> Self {
        self.unique_items = unique;
        self
    }
    
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
}

impl<T, V> Validator<Vec<T>> for CollectionValidator<T, V>
where
    T: PartialEq + Clone + Eq + std::hash::Hash,
    V: Validator<T>,
{
    fn validate(&self, value: &Vec<T>) -> ValidationResult {
        if value.is_empty() {
            if self.required {
                return Err(ValidationError::Required("collection".to_string()));
            }
            return Ok(());
        }
        
        if let Some(min) = self.min_items {
            if value.len() < min {
                return Err(ValidationError::Length { min, max: usize::MAX });
            }
        }
        
        if let Some(max) = self.max_items {
            if value.len() > max {
                return Err(ValidationError::Length { min: 0, max });
            }
        }
        
        if self.unique_items {
            let mut seen = HashSet::new();
            for item in value {
                if !seen.insert(item) {
                    return Err(ValidationError::Custom("Duplicate items are not allowed".to_string()));
                }
            }
        }
        
        for item in value {
            self.item_validator.validate(item)?;
        }
        
        Ok(())
    }
    
    fn describe(&self) -> String {
        let mut desc = vec!["Collection validator".to_string()];
        
        if self.required {
            desc.push("required".to_string());
        }
        
        if let Some(min) = self.min_items {
            desc.push(format!("min items: {}", min));
        }
        
        if let Some(max) = self.max_items {
            desc.push(format!("max items: {}", max));
        }
        
        if self.unique_items {
            desc.push("unique items".to_string());
        }
        
        desc.push(format!("item validator: {}", self.item_validator.describe()));
        
        desc.join(", ")
    }
}

/// Chuyển đổi Format thành string
pub fn format_to_string(format: Format) -> String {
    match format {
        Format::Email => "email".to_string(),
        Format::URL => "URL".to_string(),
        Format::IP => "IP address".to_string(),
        Format::UUID => "UUID".to_string(),
        Format::Date => "date (YYYY-MM-DD)".to_string(),
        Format::Time => "time (HH:MM:SS)".to_string(),
        Format::DateTime => "datetime (YYYY-MM-DDT HH:MM:SS)".to_string(),
        Format::Alpha => "alphabetic characters".to_string(),
        Format::Alphanumeric => "alphanumeric characters".to_string(),
        Format::Numeric => "numeric characters".to_string(),
        Format::Integer => "integer".to_string(),
        Format::Float => "float".to_string(),
        Format::Boolean => "boolean".to_string(),
        Format::Base64 => "Base64".to_string(),
        Format::Hex => "hexadecimal".to_string(),
        Format::CreditCard => "credit card number".to_string(),
        Format::Phone => "phone number".to_string(),
        Format::ZipCode => "ZIP code".to_string(),
        Format::Custom(name, _) => name.to_string(),
    }
}

/// Kiểm tra giá trị theo định dạng
pub fn validate_format(value: &str, format: Format) -> bool {
    match format {
        Format::Email => {
            let regex = Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$").unwrap();
            regex.is_match(value)
        },
        Format::URL => {
            let regex = Regex::new(r"^(https?|ftp)://[^\s/$.?#].[^\s]*$").unwrap();
            regex.is_match(value)
        },
        Format::IP => value.parse::<std::net::IpAddr>().is_ok(),
        Format::UUID => {
            let regex = Regex::new(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$").unwrap();
            regex.is_match(value)
        },
        Format::Date => {
            let regex = Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap();
            regex.is_match(value)
        },
        Format::Time => {
            let regex = Regex::new(r"^\d{2}:\d{2}(:\d{2})?$").unwrap();
            regex.is_match(value)
        },
        Format::DateTime => {
            let regex = Regex::new(r"^\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}(:\d{2})?(\.\d+)?(Z|[+-]\d{2}:\d{2})?$").unwrap();
            regex.is_match(value)
        },
        Format::Alpha => {
            let regex = Regex::new(r"^[a-zA-Z]+$").unwrap();
            regex.is_match(value)
        },
        Format::Alphanumeric => {
            let regex = Regex::new(r"^[a-zA-Z0-9]+$").unwrap();
            regex.is_match(value)
        },
        Format::Numeric => {
            let regex = Regex::new(r"^[0-9]+$").unwrap();
            regex.is_match(value)
        },
        Format::Integer => {
            let regex = Regex::new(r"^-?[0-9]+$").unwrap();
            regex.is_match(value)
        },
        Format::Float => {
            let regex = Regex::new(r"^-?[0-9]+(\.[0-9]+)?$").unwrap();
            regex.is_match(value)
        },
        Format::Boolean => {
            let regex = Regex::new(r"^(true|false|0|1)$").unwrap();
            regex.is_match(value)
        },
        Format::Base64 => {
            let regex = Regex::new(r"^[a-zA-Z0-9+/]+={0,2}$").unwrap();
            regex.is_match(value)
        },
        Format::Hex => {
            let regex = Regex::new(r"^[0-9a-fA-F]+$").unwrap();
            regex.is_match(value)
        },
        Format::CreditCard => {
            let regex = Regex::new(r"^[0-9]{13,19}$").unwrap();
            regex.is_match(value)
        },
        Format::Phone => {
            let regex = Regex::new(r"^\+?[0-9]{10,15}$").unwrap();
            regex.is_match(value)
        },
        Format::ZipCode => {
            let regex = Regex::new(r"^\d{5}(-\d{4})?$").unwrap();
            regex.is_match(value)
        },
        Format::Custom(_, pattern) => {
            let regex = Regex::new(pattern).unwrap();
            regex.is_match(value)
        },
    }
}

/// Service quản lý input validation
pub struct InputValidationService {
    validators: HashMap<String, Box<dyn Validator<String> + Send + Sync>>,
}

impl InputValidationService {
    /// Tạo mới một InputValidationService
    pub fn new() -> Self {
        Self {
            validators: HashMap::new(),
        }
    }
    
    /// Đăng ký validator cho một field
    pub fn register_validator<V>(&mut self, field: &str, validator: V)
    where
        V: Validator<String> + Send + Sync + 'static,
    {
        self.validators.insert(field.to_string(), Box::new(validator));
    }
    
    /// Validate dữ liệu đầu vào
    pub fn validate(&self, input: &HashMap<String, String>) -> ValidationErrors {
        let mut results = ValidationErrors::new();
        
        for (field, value) in input {
            if let Some(validator) = self.validators.get(field) {
                if let Err(error) = validator.validate(value) {
                    results.add(field, error);
                }
            } else {
                debug!("Không tìm thấy validator cho field: {}", field);
            }
        }
        
        results
    }
    
    /// Sanitize dữ liệu đầu vào
    pub fn sanitize(&self, input: HashMap<String, String>) -> Result<HashMap<String, String>, ValidationError> {
        let mut sanitized = HashMap::new();
        
        for (field, value) in input {
            if let Some(validator) = self.validators.get(&field) {
                match self.sanitize_string(validator.as_ref(), value) {
                    Ok(sanitized_value) => {
                        sanitized.insert(field, sanitized_value);
                    },
                    Err(e) => {
                        return Err(ValidationError::SanitizationError(field, e.to_string()));
                    }
                }
            } else {
                sanitized.insert(field, self.basic_sanitize(value));
            }
        }
        
        Ok(sanitized)
    }
    
    /// Sanitize một string dựa trên validator
    fn sanitize_string(&self, validator: &dyn Validator<String>, value: String) -> Result<String, ValidationError> {
        if let Some(sanitizer) = validator.as_any().downcast_ref::<StringValidator>() {
            sanitizer.sanitize(value)
        } else {
            Ok(self.basic_sanitize(value))
        }
    }
    
    /// Sanitize cơ bản một string
    fn basic_sanitize(&self, value: String) -> String {
        // Đây chỉ là một sanitize đơn giản, trong thực tế cần một sanitizer mạnh hơn
        value
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
    }
}

impl Default for InputValidationService {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait extension cho các validator để hỗ trợ runtime type checking
trait ValidatorExt: Validator<String> {
    fn as_any(&self) -> &dyn std::any::Any;
}

impl<T: Validator<String> + 'static> ValidatorExt for T {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Trait cho việc validation dữ liệu
/// 
/// Trait này được chuyển từ module validation.rs để chuẩn hóa cách validation
/// trong toàn bộ ứng dụng. Implement trait này cho các struct cần validation.
pub trait Validate {
    /// Validate dữ liệu và trả về Result với ValidationErrors nếu có lỗi
    fn validate(&self) -> Result<(), ValidationErrors>;
}

/// Mẫu implementation của trait Validate
/// 
/// Struct này minh họa cách sử dụng trait Validate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRegistration {
    pub username: String,
    pub email: String,
    pub password: String,
    pub age: Option<i32>,
    pub website: Option<String>,
}

impl Validate for UserRegistration {
    fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        
        // Validate username
        let username_validator = string::min_length(3);
        if let Err(e) = username_validator(&self.username) {
            errors.add("username", e);
        }
        
        let username_max_validator = string::max_length(30);
        if let Err(e) = username_max_validator(&self.username) {
            errors.add("username", e);
        }
        
        let pattern_validator = string::pattern(r"^[a-zA-Z0-9_]+$");
        if let Err(e) = pattern_validator(&self.username) {
            errors.add("username", e);
        }
        
        // Validate email
        let email_validator = string::email();
        if let Err(e) = email_validator(&self.email) {
            errors.add("email", e);
        }
        
        // Validate password
        let password_min_validator = string::min_length(8);
        if let Err(e) = password_min_validator(&self.password) {
            errors.add("password", e);
        }
        
        let password_max_validator = string::max_length(100);
        if let Err(e) = password_max_validator(&self.password) {
            errors.add("password", e);
        }
        
        // Validate age if provided
        if let Some(age) = self.age {
            let age_validator = number::between(18, 120);
            if let Err(e) = age_validator(&(age as i64)) {
                errors.add("age", e);
            }
        }
        
        // Validate website if provided
        if let Some(ref website) = self.website {
            if !website.is_empty() {
                let url_validator = string::url();
                if let Err(e) = url_validator(&website.to_string()) {
                    errors.add("website", e);
                }
                
                let security_validator = security::no_xss();
                if let Err(e) = security_validator(&website.to_string()) {
                    errors.add("website", e);
                }
            }
        }
        
        if errors.has_errors() {
            Err(errors)
        } else {
            Ok(())
        }
    }
} 