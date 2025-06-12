//! Module cung cấp API validation cho hệ thống
//!
//! Module này đã hợp nhất các tính năng từ module `validation.rs` cũ
//! và cung cấp các chức năng chuẩn hóa cho việc validation API trong toàn bộ codebase.
//!
//! ## Features chính:
//!
//! - ApiValidator: validator chính cho API endpoints
//! - Hỗ trợ validation và sanitize API request
//! - Định nghĩa quy tắc validation cho từng field
//! - Kiểm tra và phòng chống các lỗ hổng bảo mật

use std::collections::HashMap;
use regex;
use crate::security::sanitize_html;
use crate::security::input_validation::security::{check_xss, check_sql_injection};
use serde::{Serialize, Deserialize};
use tracing::warn;

/// Cấu trúc lưu trữ lỗi validation đảm bảo định dạng chuẩn, dễ chuyển đổi thành JSON
/// hoặc các định dạng khác để trả về cho client.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ValidationErrors {
    errors: HashMap<String, Vec<String>>,
}

impl ValidationErrors {
    /// Tạo mới một đối tượng ValidationErrors rỗng
    pub fn new() -> Self {
        Self {
            errors: HashMap::<String, Vec<String>>::new()
        }
    }

    /// Thêm một lỗi cho một trường cụ thể
    /// 
    /// # Arguments
    /// * `field` - Tên trường bị lỗi
    /// * `message` - Thông báo lỗi
    pub fn add_error(&mut self, field: &str, message: &str) {
        self.errors
            .entry(field.to_string())
            .or_default()
            .push(message.to_string());
    }

    /// Kiểm tra xem có lỗi nào không
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Lấy danh sách các lỗi
    pub fn get_errors(&self) -> &HashMap<String, Vec<String>> {
        &self.errors
    }

    /// Format lỗi thành string
    pub fn format_errors(&self) -> String {
        if !self.has_errors() {
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

/// Quy tắc validation cho các trường trong request
#[derive(Debug, Clone)]
pub struct FieldRule {
    /// Kiểu dữ liệu của trường ("string", "number", "boolean", v.v.)
    pub field_type: String,
    
    /// Trường có bắt buộc hay không
    pub required: bool,
    
    /// Độ dài tối thiểu (cho string)
    pub min_length: Option<usize>,
    
    /// Độ dài tối đa (cho string)
    pub max_length: Option<usize>,
    
    /// Regex pattern để kiểm tra giá trị (cho string)
    pub pattern: Option<String>,
    
    /// Có làm sạch (sanitize) giá trị không
    pub sanitize: bool,
}

/// Quy tắc validation cho một API endpoint
#[derive(Debug)]
pub struct ApiValidationRule {
    /// Đường dẫn API (e.g., "/redis/set")
    pub path: String,
    
    /// HTTP method (GET, POST, v.v.)
    pub method: String,
    
    /// Danh sách các trường bắt buộc
    pub required_fields: Vec<String>,
    
    /// Quy tắc validation cho từng trường
    pub field_rules: HashMap<String, FieldRule>,
}

/// Validator chính cho API endpoints
pub struct ApiValidator {
    rules: HashMap<String, ApiValidationRule>,
}

impl ApiValidator {
    /// Tạo một validator mới
    pub fn new() -> Self {
        Self {
            rules: HashMap::<String, ApiValidationRule>::new()
        }
    }

    /// Thêm một quy tắc validation cho một API endpoint
    pub fn add_rule(&mut self, rule: ApiValidationRule) {
        let key = format!("{}_{}", rule.method, rule.path);
        self.rules.insert(key, rule);
    }

    /// Validate một request dựa trên method, path và params
    /// 
    /// # Arguments
    /// * `method` - HTTP method (GET, POST, etc.)
    /// * `path` - Đường dẫn API
    /// * `params` - Các tham số của request
    /// 
    /// # Returns
    /// * `ValidationErrors` - Danh sách các lỗi (nếu có)
    pub fn validate_request(&self, method: &str, path: &str, params: &HashMap<String, String>) -> ValidationErrors {
        let mut errors = ValidationErrors::new();
        let key = format!("{}_{}", method, path);
        
        if let Some(rule) = self.rules.get(&key) {
            // Kiểm tra các trường bắt buộc
            for field in &rule.required_fields {
                if !params.contains_key(field) {
                    errors.add_error(field, &format!("Trường {} là bắt buộc", field));
                }
            }

            // Kiểm tra từng trường theo quy tắc
            for (field, value) in params {
                if let Some(field_rule) = rule.field_rules.get(field) {
                    // Kiểm tra độ dài
                    if let Some(min_length) = field_rule.min_length {
                        if value.len() < min_length {
                            errors.add_error(field, &format!("Độ dài tối thiểu là {}", min_length));
                        }
                    }

                    if let Some(max_length) = field_rule.max_length {
                        if value.len() > max_length {
                            errors.add_error(field, &format!("Độ dài tối đa là {}", max_length));
                        }
                    }

                    // Kiểm tra regex pattern
                    if let Some(pattern) = &field_rule.pattern {
                        match regex::Regex::new(pattern) {
                            Ok(re) => {
                                if !re.is_match(value) {
                                    errors.add_error(field, &format!("Giá trị không khớp với pattern {}", pattern));
                                }
                            },
                            Err(e) => {
                                warn!("Invalid regex pattern {}: {}", pattern, e);
                                errors.add_error("validation", &format!("Lỗi pattern regex: {}", e));
                            }
                        }
                    }

                    // Kiểm tra XSS và SQL injection
                    if let Err(e) = check_xss(value, field) {
                        errors.add_error(field, &format!("XSS: {}", e));
                    }
                    if let Err(e) = check_sql_injection(value, field) {
                        errors.add_error(field, &format!("SQL Injection: {}", e));
                    }
                }
            }
        } else {
            errors.add_error("validation", &format!("Không tìm thấy quy tắc cho {}", key));
        }

        errors
    }

    /// Làm sạch (sanitize) các tham số của request
    /// 
    /// # Arguments
    /// * `method` - HTTP method
    /// * `path` - Đường dẫn API
    /// * `params` - Các tham số của request
    /// 
    /// # Returns
    /// * `HashMap<String, String>` - Các tham số đã được làm sạch
    pub fn sanitize_request(&self, method: &str, path: &str, params: HashMap<String, String>) -> HashMap<String, String> {
        let mut sanitized: HashMap<String, String> = HashMap::new();
        let key = format!("{}_{}", method, path);
        
        if let Some(rule) = self.rules.get(&key) {
            for (field, value) in params {
                if let Some(field_rule) = rule.field_rules.get(&field) {
                    if field_rule.sanitize {
                        let sanitized_value = sanitize_html(&value);
                        sanitized.insert(field, sanitized_value);
                    } else {
                        sanitized.insert(field, value);
                    }
                } else {
                    sanitized.insert(field, value);
                }
            }
        } else {
            // Nếu không tìm thấy quy tắc, trả về nguyên params
            sanitized = params;
        }

        sanitized
    }
    
    /// Kiểm tra và làm sạch một trường đơn lẻ
    /// 
    /// # Arguments
    /// * `value` - Giá trị cần kiểm tra
    /// * `field_name` - Tên trường
    /// * `rules` - Quy tắc validation cho trường
    /// 
    /// # Returns
    /// * `Result<String, String>` - Giá trị đã được làm sạch hoặc lỗi
    pub fn validate_and_sanitize_field(&self, value: &str, field_name: &str, rules: &FieldRule) -> Result<String, String> {
        // Kiểm tra độ dài
        if let Some(min_length) = rules.min_length {
            if value.len() < min_length {
                return Err(format!("Độ dài tối thiểu của {} là {}", field_name, min_length));
            }
        }

        if let Some(max_length) = rules.max_length {
            if value.len() > max_length {
                return Err(format!("Độ dài tối đa của {} là {}", field_name, max_length));
            }
        }

        // Kiểm tra pattern
        if let Some(pattern) = &rules.pattern {
            match regex::Regex::new(pattern) {
                Ok(re) => {
                    if !re.is_match(value) {
                        return Err(format!("{} không khớp với pattern {}", field_name, pattern));
                    }
                },
                Err(e) => {
                    return Err(format!("Lỗi pattern regex: {}", e));
                }
            }
        }

        // Kiểm tra XSS và SQL injection
        check_xss(value, field_name).map_err(|e| e.to_string())?;
        check_sql_injection(value, field_name).map_err(|e| e.to_string())?;

        // Làm sạch nếu cần
        if rules.sanitize {
            Ok(sanitize_html(value))
        } else {
            Ok(value.to_string())
        }
    }
}

impl Default for ApiValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_basic() {
        let mut validator = ApiValidator::new();

        let mut field_rules = HashMap::new();
        field_rules.insert(
            "key".to_string(),
            FieldRule {
                field_type: "string".to_string(),
                required: true,
                min_length: Some(1),
                max_length: Some(100),
                pattern: Some(r"^[a-zA-Z0-9_]+$".to_string()),
                sanitize: true,
            },
        );
        field_rules.insert(
            "value".to_string(),
            FieldRule {
                field_type: "string".to_string(),
                required: true,
                min_length: Some(1),
                max_length: Some(1000),
                pattern: None,
                sanitize: true,
            },
        );

        validator.add_rule(ApiValidationRule {
            path: "/redis/set".to_string(),
            method: "POST".to_string(),
            required_fields: vec!["key".to_string(), "value".to_string()],
            field_rules,
        });

        // Test valid request
        let mut params = HashMap::new();
        params.insert("key".to_string(), "test_key".to_string());
        params.insert("value".to_string(), "test_value".to_string());

        let errors = validator.validate_request("POST", "/redis/set", &params);
        assert!(!errors.has_errors());

        // Test invalid key (contains special character)
        let mut params = HashMap::new();
        params.insert("key".to_string(), "test-key!".to_string());
        params.insert("value".to_string(), "test_value".to_string());

        let errors = validator.validate_request("POST", "/redis/set", &params);
        assert!(errors.has_errors());

        // Test missing required field
        let mut params = HashMap::new();
        params.insert("key".to_string(), "test_key".to_string());

        let errors = validator.validate_request("POST", "/redis/set", &params);
        assert!(errors.has_errors());
    }

    #[test]
    fn test_sanitize_request() {
        let mut validator = ApiValidator::new();

        let mut field_rules = HashMap::new();
        field_rules.insert(
            "key".to_string(),
            FieldRule {
                field_type: "string".to_string(),
                required: true,
                min_length: Some(1),
                max_length: Some(100),
                pattern: Some(r"^[a-zA-Z0-9_]+$".to_string()),
                sanitize: true,
            },
        );
        field_rules.insert(
            "value".to_string(),
            FieldRule {
                field_type: "string".to_string(),
                required: true,
                min_length: Some(1),
                max_length: Some(1000),
                pattern: None,
                sanitize: true,
            },
        );

        validator.add_rule(ApiValidationRule {
            path: "/redis/set".to_string(),
            method: "POST".to_string(),
            required_fields: vec!["key".to_string(), "value".to_string()],
            field_rules,
        });

        // Test sanitize HTML
        let mut params = HashMap::new();
        params.insert("key".to_string(), "test_key".to_string());
        params.insert("value".to_string(), "<script>alert('XSS')</script>test_value".to_string());

        let sanitized = validator.sanitize_request("POST", "/redis/set", params);
        assert_eq!(sanitized.get("value").unwrap(), "&lt;script&gt;alert('XSS')&lt;/script&gt;test_value");
    }
    
    #[test]
    fn test_validate_and_sanitize_field() {
        let validator = ApiValidator::new();
        
        let rules = FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: Some(3),
            max_length: Some(10),
            pattern: Some(r"^[a-zA-Z0-9_]+$".to_string()),
            sanitize: true,
        };
        
        // Valid input
        let result = validator.validate_and_sanitize_field("test_123", "test_field", &rules);
        assert!(result.is_ok());
        
        // Too short
        let result = validator.validate_and_sanitize_field("ab", "test_field", &rules);
        assert!(result.is_err());
        
        // Too long
        let result = validator.validate_and_sanitize_field("abcdefghijk", "test_field", &rules);
        assert!(result.is_err());
        
        // Invalid pattern
        let result = validator.validate_and_sanitize_field("test-123!", "test_field", &rules);
        assert!(result.is_err());
        
        // XSS
        let result = validator.validate_and_sanitize_field("<script>alert(1)</script>", "test_field", &rules);
        assert!(result.is_err());
        
        // Sanitize HTML
        let rules = FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: None,
            max_length: None,
            pattern: None,
            sanitize: true,
        };
        
        let result = validator.validate_and_sanitize_field("<b>text</b>", "test_field", &rules);
        assert_eq!(result.unwrap(), "&lt;b&gt;text&lt;/b&gt;");
    }
}

/// Ví dụ minh họa cách sử dụng input_validation từ api_validation
/// 
/// Section này chứng minh cách module api_validation sử dụng các chức năng
/// đã hợp nhất từ input_validation (cũ validation.rs)
#[cfg(test)]
mod integration_examples {
    use super::*;
    use crate::security::input_validation::{
        InputValidator, FieldValidator, string, number, security as security_validators
    };

    #[test]
    fn test_integration_with_input_validation() {
        // Tạo API validator
        let mut api_validator = ApiValidator::new();
        
        // Tạo field rules
        let mut field_rules = HashMap::new();
        field_rules.insert(
            "username".to_string(),
            FieldRule {
                field_type: "string".to_string(),
                required: true,
                min_length: Some(3),
                max_length: Some(30),
                pattern: Some(r"^[a-zA-Z0-9_]+$".to_string()),
                sanitize: true,
            },
        );
        
        // Thêm endpoint rule
        api_validator.add_rule(ApiValidationRule {
            path: "/api/user".to_string(),
            method: "POST".to_string(),
            required_fields: vec!["username".to_string()],
            field_rules,
        });
        
        // Sử dụng InputValidator từ input_validation.rs
        let mut input_validator = InputValidator::new();
        input_validator.add_string_validator(
            FieldValidator::new("username")
                .add_rule(string::min_length(3))
                .add_rule(string::max_length(30))
                .add_rule(string::pattern(r"^[a-zA-Z0-9_]+$"))
                .add_rule(security_validators::no_xss())
                .required(true)
        );
        
        // Tạo test data
        let mut params = HashMap::new();
        params.insert("username".to_string(), "test_user".to_string());
        
        // Validate với API validator
        let api_result = api_validator.validate_request("POST", "/api/user", &params);
        assert!(!api_result.has_errors());
        
        // Validate với Input validator
        let data = [("username", "test_user")];
        let input_result = input_validator.validate(&data);
        assert!(!input_result.has_errors());
        
        // Sanitize với API validator
        let sanitized = api_validator.sanitize_request("POST", "/api/user", params);
        assert_eq!(sanitized.get("username").unwrap(), "test_user");
    }
    
    // Ví dụ sử dụng trait Validate từ input_validation
    #[test]
    fn test_using_validate_trait() {
        use crate::security::input_validation::UserRegistration;
        
        let user = UserRegistration {
            username: "test_user".to_string(),
            email: "test@example.com".to_string(),
            password: "password123".to_string(),
            age: Some(25),
            website: Some("https://example.com".to_string()),
        };
        
        // Sử dụng trait Validate từ input_validation
        let result = user.validate();
        assert!(result.is_ok());
    }
}

// =================================================================
// [HỢP NHẤT] Module endpoint_validators từ network/api_validator.rs
// =================================================================

/// Module cung cấp các validator đặc thù cho các endpoint API
pub mod endpoint_validators {
    use crate::security::input_validation::{InputValidator, FieldValidator};
    use crate::security::input_validation::string;
    use crate::errors::NetworkError;

    /// Validator cho endpoint /logs/{domain}
    pub struct LogDomainValidator {
        validator: InputValidator<'static>,
    }

    impl Default for LogDomainValidator {
        fn default() -> Self {
            let mut validator = InputValidator::new();
            
            validator.add_string_validator(
                FieldValidator::new("domain")
                    .add_rule(string::one_of(&["network", "wallet", "snipebot", "common"]))
            );
            
            Self { validator }
        }
    }

    impl LogDomainValidator {
        pub fn validate(&self, domain: &str) -> Result<(), NetworkError> {
            let fields = &[("domain", domain)];
            
            let errors = self.validator.validate(fields);
            if errors.has_errors() {
                return Err(NetworkError::ValidationError(errors.format_errors()));
            }
            
            Ok(())
        }
    }

    /// Validator cho endpoint /plugins/{plugin_type}
    pub struct PluginTypeValidator {
        validator: InputValidator<'static>,
    }

    impl Default for PluginTypeValidator {
        fn default() -> Self {
            let mut validator = InputValidator::new();
            
            // Chuẩn hóa danh sách PluginType hợp lệ từ core::types::PluginType
            let valid_plugin_types = &[
                "WebSocket", "WebRTC", "Libp2p", "GRPC", "MQTT", "Kafka", "Redis", "IPFS", "WASM", "Unknown"
            ];
            
            validator.add_string_validator(
                FieldValidator::new("plugin_type")
                    .add_rule(string::one_of(valid_plugin_types))
            );
            
            Self { validator }
        }
    }

    impl PluginTypeValidator {
        pub fn validate(&self, plugin_type: &str) -> Result<(), NetworkError> {
            let fields = &[("plugin_type", plugin_type)];
            
            let errors = self.validator.validate(fields);
            if errors.has_errors() {
                return Err(NetworkError::ValidationError(format!("Invalid plugin type: {}", errors.format_errors())));
            }
            
            Ok(())
        }
    }

    /// Validator cho endpoint /metrics (không cần tham số)
    #[derive(Default)]
    pub struct MetricsValidator {}

    impl MetricsValidator {
        pub fn validate(&self) -> Result<(), NetworkError> {
            // Không cần validate input
            Ok(())
        }
    }

    /// Validator cho endpoint /health (không cần tham số)
    #[derive(Default)]
    pub struct HealthValidator {}

    impl HealthValidator {
        pub fn validate(&self) -> Result<(), NetworkError> {
            // Không cần validate input
            Ok(())
        }
    }

    /// Tạo các validator mặc định cho toàn bộ API
    pub fn create_default_validators() -> (
        LogDomainValidator,
        PluginTypeValidator,
        MetricsValidator,
        HealthValidator
    ) {
        (
            LogDomainValidator::default(),
            PluginTypeValidator::default(),
            MetricsValidator::default(),
            HealthValidator::default()
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        
        #[test]
        fn test_log_domain_validator() {
            let validator = LogDomainValidator::default();
            
            // Valid domains
            assert!(validator.validate("network").is_ok());
            assert!(validator.validate("wallet").is_ok());
            assert!(validator.validate("snipebot").is_ok());
            assert!(validator.validate("common").is_ok());
            
            // Invalid domains
            assert!(validator.validate("invalid").is_err());
            assert!(validator.validate("").is_err());
            assert!(validator.validate("NETWORK").is_err()); // Case sensitive
        }
        
        #[test]
        fn test_plugin_type_validator() {
            let validator = PluginTypeValidator::default();
            
            // Valid plugin types
            assert!(validator.validate("WebSocket").is_ok());
            assert!(validator.validate("WebRTC").is_ok());
            assert!(validator.validate("Libp2p").is_ok());
            assert!(validator.validate("GRPC").is_ok());
            
            // Invalid plugin types
            assert!(validator.validate("Invalid").is_err());
            assert!(validator.validate("").is_err());
            assert!(validator.validate("websocket").is_err()); // Case sensitive
        }
    }
}

// Re-export các validators đặc thù để dễ sử dụng
pub use endpoint_validators::{
    LogDomainValidator, 
    PluginTypeValidator, 
    MetricsValidator, 
    HealthValidator, 
    create_default_validators
}; 