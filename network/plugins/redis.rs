use crate::core::engine::{Plugin, PluginType, PluginError};
use crate::infra::service_traits::RedisService;
use crate::config::types::RedisConfig;
use std::sync::Arc;
use std::time::Duration;
use std::env;
use std::collections::HashMap;
use crate::security::input_validation::security;
use crate::security::api_validation::{ApiValidator, ApiValidationRule, FieldRule};
use tracing::{warn, error, info};
use tokio::time::sleep as async_sleep;
use crate::security::rate_limiter::{RateLimiter, RateLimitAlgorithm, RateLimitAction, PathConfig};
use std::any::Any;
use async_trait::async_trait;

/// Redis plugin implementation
/// Plugin wrapper for RedisService trait
/// 
/// # Hướng dẫn cấu hình nâng cao
/// 
/// - Để sử dụng các tính năng bảo mật (TLS, ACL, password rotation), truyền các trường tương ứng trong RedisConfig khi khởi tạo plugin.
/// - Để tối ưu hiệu suất, cấu hình pool_size, min_pool_size, max_idle_connections, eviction_policy, replication_mode, persistence_mode.
/// - Để bật monitoring, cấu hình enable_exporter, log_rotation_days, metrics_history_days.
/// 
/// Ví dụ:
/// ```rust
/// use crate::config::types::RedisConfig;
/// let config = RedisConfig {
///     url: "rediss://secure-redis:6379".to_string(),
///     enable_tls: true,
///     tls_cert_path: Some("/etc/ssl/certs/redis.crt".to_string()),
///     tls_key_path: Some("/etc/ssl/private/redis.key".to_string()),
///     acl_file: Some("/etc/redis/users.acl".to_string()),
///     pool_size: 20,
///     min_pool_size: Some(5),
///     max_idle_connections: Some(10),
///     eviction_policy: Some("allkeys-lru".to_string()),
///     enable_exporter: true,
///     ..Default::default()
/// };
/// let plugin = RedisPlugin::new_with_config(service, config);
/// ```
pub struct RedisPlugin {
    /// Plugin name
    name: String,
    /// RedisService implementation
    service: Arc<dyn RedisService>,
    /// Redis configuration
    config: RedisConfig,
    /// API validator
    validator: ApiValidator,
    /// Optional background task shutdown
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl RedisPlugin {
    /// Create a new RedisPlugin with a service implementation and url (legacy, không khuyến khích)
    pub fn new(service: Arc<dyn RedisService>, url: String) -> Self {
        let config = RedisConfig {
            url,
            ..RedisConfig::default()
        };
        Self::new_with_config(service, config)
    }

    /// Create a new RedisPlugin with full RedisConfig (khuyến khích)
    pub fn new_with_config(service: Arc<dyn RedisService>, config: RedisConfig) -> Self {
        let mut validator = ApiValidator::new();
        // Thêm rules cho Redis endpoints
        let mut field_rules = HashMap::new();
        field_rules.insert(
            "key".to_string(),
            FieldRule {
                field_type: "string".to_string(),
                required: true,
                min_length: Some(1),
                max_length: Some(100),
                pattern: Some(r"^[a-zA-Z0-9_:]+$".to_string()),
                sanitize: true,
            },
        );
        field_rules.insert(
            "value".to_string(),
            FieldRule {
                field_type: "string".to_string(),
                required: true,
                min_length: Some(1),
                max_length: Some(1024 * 1024), // 1MB
                pattern: None,
                sanitize: true,
            },
        );
        // Rule cho endpoint set
        validator.add_rule(ApiValidationRule {
            path: "/redis/set".to_string(),
            method: "POST".to_string(),
            required_fields: vec!["key".to_string(), "value".to_string()],
            field_rules: field_rules.clone(),
        });
        // Rule cho endpoint get
        validator.add_rule(ApiValidationRule {
            path: "/redis/get".to_string(),
            method: "GET".to_string(),
            required_fields: vec!["key".to_string()],
            field_rules: HashMap::from([("key".to_string(), match field_rules.get("key").cloned() {
                Some(rule) => rule,
                None => {
                    error!("[RedisPlugin] Missing FieldRule for 'key', using default");
                    FieldRule {
                        field_type: "string".to_string(),
                        required: true,
                        min_length: Some(1),
                        max_length: Some(100),
                        pattern: Some(r"^[a-zA-Z0-9_:]+$".to_string()),
                        sanitize: true,
                    }
                }
            })]),
        });
        // Thêm rule cho endpoint del
        validator.add_rule(ApiValidationRule {
            path: "/redis/del".to_string(),
            method: "DELETE".to_string(),
            required_fields: vec!["key".to_string()],
            field_rules: HashMap::from([("key".to_string(), match field_rules.get("key").cloned() {
                Some(rule) => rule,
                None => {
                    error!("[RedisPlugin] Missing FieldRule for 'key', using default");
                    FieldRule {
                        field_type: "string".to_string(),
                        required: true,
                        min_length: Some(1),
                        max_length: Some(100),
                        pattern: Some(r"^[a-zA-Z0-9_:]+$".to_string()),
                        sanitize: true,
                    }
                }
            })]),
        });
        
        // Khởi tạo rate limiter nếu chưa có
        let rate_limiter = Arc::new(RateLimiter::new());
        // Đăng ký các endpoint với rate limiter
        let endpoints = vec!["/redis/set", "/redis/get", "/redis/del"];
        for endpoint in endpoints {
            let config = PathConfig {
                max_requests: 100, // Mặc định 100 request/phút
                time_window: 60, // 60 giây (1 phút)
                algorithm: RateLimitAlgorithm::TokenBucket,
                action: RateLimitAction::Reject,
            };
            match tokio::runtime::Handle::current().block_on(rate_limiter.register_path(endpoint, config)) {
                Ok(_) => info!("[RedisPlugin] Rate limiter registered for endpoint: {}", endpoint),
                Err(e) => warn!("[RedisPlugin] Failed to register rate limiter for endpoint {}: {}", endpoint, e),
            }
        }
        
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
        let service_clone = service.clone();
        let url = config.url.clone();
        // Spawn background health check & reconnect task
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {
                        // Health check
                        match service_clone.ping().await {
                            Ok(_) => info!("[RedisPlugin] Health check OK: {}", url),
                            Err(e) => {
                                warn!("[RedisPlugin] Health check failed: {}. Attempting reconnect...", e);
                                let _ = service_clone.reconnect().await;
                            }
                        }
                        // Log resource usage (memory, fd count nếu có thể)
                        #[cfg(target_os = "linux")]
                        if let Ok(meminfo) = std::fs::read_to_string("/proc/self/status") {
                            for line in meminfo.lines() {
                                if line.starts_with("VmRSS") || line.starts_with("VmSize") {
                                    info!("[RedisPlugin][Resource] {}", line);
                                }
                            }
                        }
                        #[cfg(target_os = "linux")]
                        if let Ok(fds) = std::fs::read_dir("/proc/self/fd") {
                            let count = fds.count();
                            info!("[RedisPlugin][Resource] Open file descriptors: {}", count);
                        }
                    }
                    _ = &mut shutdown_rx => {
                        info!("[RedisPlugin] Background health task shutting down");
                        break;
                    }
                }
            }
        });
        let plugin = Self {
            name: "redis".to_string(),
            service,
            config,
            validator,
            shutdown_tx: Some(shutdown_tx),
        };
        // Log cấu hình nâng cao khi khởi tạo
        if plugin.config.enable_tls {
            info!("[RedisPlugin] TLS/SSL enabled. Cert: {:?}, Key: {:?}", plugin.config.tls_cert_path, plugin.config.tls_key_path);
        }
        if plugin.config.acl_file.is_some() {
            info!("[RedisPlugin] ACL file: {:?}", plugin.config.acl_file);
        }
        if plugin.config.enable_exporter {
            info!("[RedisPlugin] Prometheus exporter enabled");
        }
        if let Some(policy) = &plugin.config.eviction_policy {
            info!("[RedisPlugin] Eviction policy: {}", policy);
        }
        if let Some(mode) = &plugin.config.replication_mode {
            info!("[RedisPlugin] Replication mode: {}", mode);
        }
        if let Some(mode) = &plugin.config.persistence_mode {
            info!("[RedisPlugin] Persistence mode: {}", mode);
        }
        plugin
    }
    
    /// Set authentication credentials
    pub fn with_auth(mut self, username: Option<String>, password: Option<String>) -> Self {
        self.config.username = username;
        self.config.password = password;
        self
    }
    
    /// Set retry configuration
    pub fn with_retry_config(mut self, max_retries: u8, retry_delay_ms: u64) -> Self {
        self.config.max_retries = max_retries;
        self.config.retry_delay_ms = retry_delay_ms;
        self
    }

    /// Validate input key/value nếu nhận từ external source (API, user, ...)
    /// 
    /// # Deprecated
    /// 
    /// Hàm này sẽ bị xóa trong phiên bản tương lai. Thay vào đó, hãy sử dụng ApiValidator
    /// và phương thức validate_and_sanitize_field.
    /// 
    /// Ví dụ:
    /// ```
    /// let validator = ApiValidator::new();
    /// validator.validate_and_sanitize_field(key, "key", &key_rule);
    /// ```
    #[deprecated(note = "Use ApiValidator or Validator trait instead. This method will be removed in the next major version.")]
    pub fn validate_input(&self, key: &str, value: &str) -> Result<(), String> {
        let enable = env::var("PLUGIN_INPUT_VALIDATION").unwrap_or_else(|_| "on".to_string());
        if enable == "off" {
            return Ok(());
        }
        security::check_xss(key, "redis_key").map_err(|e| e.to_string())?;
        security::check_sql_injection(key, "redis_key").map_err(|e| e.to_string())?;
        security::check_xss(value, "redis_value").map_err(|e| e.to_string())?;
        security::check_sql_injection(value, "redis_value").map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Validate input key/value/url/username/password nếu nhận từ external source (API, user, ...)
    /// 
    /// # Deprecated
    /// 
    /// Hàm này sẽ bị xóa trong phiên bản tương lai. Thay vào đó, hãy sử dụng ApiValidator
    /// và các rule validation cụ thể cho từng trường.
    /// 
    /// Ví dụ:
    /// ```
    /// let validator = ApiValidator::new();
    /// validator.add_rule(ApiValidationRule {
    ///     path: "/path".to_string(),
    ///     method: "POST".to_string(),
    ///     required_fields: vec!["key".to_string(), "value".to_string()],
    ///     field_rules: field_rules,
    /// });
    /// validator.validate_request(method, path, &params);
    /// ```
    #[deprecated(note = "Use ApiValidator or Validator trait instead. This method will be removed in the next major version.")]
    pub fn validate_input_all(&self, key: &str, value: &str, url: &str, username: &Option<String>, password: &Option<String>) -> Result<(), String> {
        // Thay thế gọi validate_input bằng gọi trực tiếp các hàm check
        security::check_xss(key, "redis_key").map_err(|e| e.to_string())?;
        security::check_sql_injection(key, "redis_key").map_err(|e| e.to_string())?;
        security::check_xss(value, "redis_value").map_err(|e| e.to_string())?;
        security::check_sql_injection(value, "redis_value").map_err(|e| e.to_string())?;
        
        // Kiểm tra url, username và password như trước
        crate::security::input_validation::security::check_xss(url, "redis_url").map_err(|e| e.to_string())?;
        crate::security::input_validation::security::check_sql_injection(url, "redis_url").map_err(|e| e.to_string())?;
        if let Some(u) = username {
            crate::security::input_validation::security::check_xss(u, "redis_username").map_err(|e| e.to_string())?;
            crate::security::input_validation::security::check_sql_injection(u, "redis_username").map_err(|e| e.to_string())?;
        }
        if let Some(p) = password {
            crate::security::input_validation::security::check_xss(p, "redis_password").map_err(|e| e.to_string())?;
            crate::security::input_validation::security::check_sql_injection(p, "redis_password").map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Set key-value với validate input và timeout (mock)
    pub async fn set_with_validate(&self, key: &str, value: &str) -> Result<(), String> {
        // Lấy field rules từ validator
        let key_rule = FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: Some(1),
            max_length: Some(100),
            pattern: Some(r"^[a-zA-Z0-9_:]+$".to_string()),
            sanitize: true,
        };
        
        let value_rule = FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: Some(1),
            max_length: Some(1024 * 1024), // 1MB
            pattern: None,
            sanitize: true,
        };
        
        // Validate và sanitize
        let sanitized_key = self.validator.validate_and_sanitize_field(key, "key", &key_rule)?;
        let sanitized_value = self.validator.validate_and_sanitize_field(value, "value", &value_rule)?;
        
        // Giả lập timeout mạng
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        // Thực hiện set thông qua service
        match self.service.set(&sanitized_key, &sanitized_value).await {
            Ok(_) => {
                info!("Redis set: {} = {}", sanitized_key, sanitized_value);
                Ok(())
            },
            Err(e) => {
                error!("Redis set failed: {}", e);
                Err(format!("Redis operation failed: {}", e))
            }
        }
    }
    
    /// Xử lý request từ endpoint API
    pub async fn handle_request(&self, method: &str, path: &str, params: &HashMap<String, String>) -> Result<String, String> {
        // Kiểm tra đầu vào với ApiValidator
        let validation_result = self.validator.validate_request(method, path, params);
        if validation_result.has_errors() {
            error!("[RedisPlugin] API validation failed for {} {}: {}", method, path, validation_result.format_errors());
            return Err(format!("API validation failed: {}", validation_result.format_errors()));
        }
        
        // Sanitize params để bảo vệ khỏi tấn công
        let sanitized_params: HashMap<String, String> = params.iter()
            .map(|(k, v)| {
                let sanitized = security::sanitize_html(v);
                (k.clone(), sanitized)
            })
            .collect();
        
        // Xử lý các endpoint cụ thể
        match (method, path) {
            ("POST", "/redis/set") => {
                // Kiểm tra tham số
                let key = match sanitized_params.get("key") {
                    Some(k) => k,
                    None => {
                        error!("Missing 'key' in params");
                        return Err("Missing 'key' field".to_string());
                    }
                };
                
                let value = match sanitized_params.get("value") {
                    Some(v) => v,
                    None => {
                        error!("Missing 'value' in params");
                        return Err("Missing 'value' field".to_string());
                    }
                };
                
                // Sử dụng validation chuẩn thay vì hàm deprecated
                let key_rule = FieldRule {
                    field_type: "string".to_string(),
                    required: true,
                    min_length: Some(1),
                    max_length: Some(100),
                    pattern: Some(r"^[a-zA-Z0-9_:]+$".to_string()),
                    sanitize: true,
                };
                
                let value_rule = FieldRule {
                    field_type: "string".to_string(),
                    required: true,
                    min_length: Some(1),
                    max_length: Some(1024 * 1024), // 1MB
                    pattern: None,
                    sanitize: true,
                };
                
                // Validate thêm một lần nữa và sanitize
                let sanitized_key = match self.validator.validate_and_sanitize_field(key, "key", &key_rule) {
                    Ok(k) => k,
                    Err(e) => return Err(format!("Key validation failed: {}", e))
                };
                
                let sanitized_value = match self.validator.validate_and_sanitize_field(value, "value", &value_rule) {
                    Ok(v) => v,
                    Err(e) => return Err(format!("Value validation failed: {}", e))
                };
                
                // Thực hiện set thông qua service
                match self.service.set(&sanitized_key, &sanitized_value).await {
                    Ok(_) => {
                        info!("Redis set: {} = {}", sanitized_key, sanitized_value);
                        Ok("Success".to_string())
                    },
                    Err(e) => {
                        error!("Redis set failed: {}", e);
                        Err(format!("Redis operation failed: {}", e))
                    }
                }
            },
            ("GET", "/redis/get") => {
                // Kiểm tra tham số
                let key = match sanitized_params.get("key") {
                    Some(k) => k,
                    None => {
                        error!("Missing 'key' in params");
                        return Err("Missing 'key' field".to_string());
                    }
                };
                
                // Validate key theo cùng quy tắc
                let key_rule = FieldRule {
                    field_type: "string".to_string(),
                    required: true,
                    min_length: Some(1),
                    max_length: Some(100),
                    pattern: Some(r"^[a-zA-Z0-9_:]+$".to_string()),
                    sanitize: true,
                };
                
                // Validate và sanitize
                let sanitized_key = match self.validator.validate_and_sanitize_field(key, "key", &key_rule) {
                    Ok(k) => k,
                    Err(e) => return Err(format!("Key validation failed: {}", e))
                };
                
                // Thực hiện get thông qua service
                match self.service.get(&sanitized_key).await {
                    Ok(result) => {
                        match result {
                            Some(value) => {
                                info!("Redis get: {} = {}", sanitized_key, value);
                                Ok(value)
                            },
                            None => {
                                info!("Redis get: {} = nil", sanitized_key);
                                Ok("nil".to_string())
                            }
                        }
                    },
                    Err(e) => {
                        error!("Redis get failed: {}", e);
                        Err(format!("Redis operation failed: {}", e))
                    }
                }
            },
            _ => {
                error!("Unsupported method/path: {} {}", method, path);
                Err(format!("Unsupported method/path: {} {}", method, path))
            }
        }
    }
    
    /// Retry helper for async operations with exponential backoff and detailed logging
    async fn retry_async<T, Fut, F, E>(&self, op_name: &str, mut op: F, max_retries: usize, base_delay_ms: u64) -> Result<T, String>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>> + Send,
        E: std::fmt::Display,
    {
        for attempt in 0..=max_retries {
            match op().await {
                Ok(val) => {
                    if attempt > 0 {
                        info!("[RedisPlugin] {} succeeded after {} retries", op_name, attempt);
                    }
                    return Ok(val);
                },
                Err(e) => {
                    if attempt < max_retries {
                        let delay = base_delay_ms * (1 << attempt).min(32); // exponential backoff, max 32x
                        warn!("[RedisPlugin] {} failed (attempt {}/{}): {}. Retrying in {}ms", op_name, attempt + 1, max_retries, e, delay);
                        async_sleep(Duration::from_millis(delay)).await;
                    } else {
                        error!("[RedisPlugin] {} failed after {} retries: {}", op_name, max_retries, e);
                        return Err(format!("{} failed after {} retries: {}", op_name, max_retries, e));
                    }
                }
            }
        }
        Err(format!("Unexpected end of retry for {}", op_name))
    }

    pub async fn set_with_retry(&self, key: &str, value: &str) -> Result<(), String> {
        let max_retries = self.config.max_retries as usize;
        let retry_delay_ms = self.config.retry_delay_ms;
        self.retry_async("set", || self.service.set(key, value), max_retries, retry_delay_ms).await
    }

    pub async fn get_with_retry(&self, key: &str) -> Result<Option<String>, String> {
        let max_retries = self.config.max_retries as usize;
        let retry_delay_ms = self.config.retry_delay_ms;
        self.retry_async("get", || self.service.get(key), max_retries, retry_delay_ms).await
    }
}

#[async_trait]
impl Plugin for RedisPlugin {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn plugin_type(&self) -> PluginType {
        PluginType::Storage
    }
    
    async fn start(&self) -> Result<bool, PluginError> {
        info!("[RedisPlugin] Starting...");
        
        // Connect to Redis
        match self.service.connect_with_auth(&self.config.url, &self.config.username, &self.config.password).await {
            Ok(_) => {
                info!("[RedisPlugin] Successfully connected to Redis at {}", self.config.url);
                Ok(true)
            },
            Err(e) => {
                error!("[RedisPlugin] Failed to connect to Redis: {}", e);
                Err(PluginError::StartFailed)
            }
        }
    }
    
    async fn stop(&self) -> Result<(), PluginError> {
        info!("[RedisPlugin] Stopping...");
        // Không cần cleanup đặc biệt - Redis connection sẽ tự drop khi RedisPlugin bị drop
        Ok(())
    }

    async fn check_health(&self) -> Result<bool, PluginError> {
        // Kiểm tra kết nối thực tế tới Redis
        match self.service.ping().await {
            Ok(_) => Ok(true),
            Err(e) => Err(PluginError::Other(format!("Redis health check failed: {}", e))),
        }
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Drop for RedisPlugin {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::service_mocks::DefaultRedisService;
    
    #[test]
    fn test_validate_with_api_validator_safe() {
        let redis_service = Arc::new(DefaultRedisService::new());
        let plugin = RedisPlugin::new(redis_service, "redis://localhost:6379".to_string());
        
        // Tạo ApiValidator và thêm các rule
        let mut validator = ApiValidator::new();
        
        // Rule cho key
        let mut key_rule = FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: Some(1),
            max_length: Some(100),
            pattern: Some(r"^[a-zA-Z0-9_:]+$".to_string()),
            sanitize: true,
        };
        
        // Rule cho value
        let mut value_rule = FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: Some(1),
            max_length: Some(1024 * 1024),
            pattern: None,
            sanitize: true,
        };
        
        let mut field_rules = HashMap::new();
        field_rules.insert("key".to_string(), key_rule);
        field_rules.insert("value".to_string(), value_rule);
        
        validator.add_rule(ApiValidationRule {
            path: "/redis/test".to_string(),
            method: "POST".to_string(),
            required_fields: vec!["key".to_string(), "value".to_string()],
            field_rules,
        });
        
        // Tạo params
        let mut params = HashMap::new();
        params.insert("key".to_string(), "valid_key".to_string());
        params.insert("value".to_string(), "valid_value".to_string());
        
        // Kiểm tra kết quả validation
        assert!(!validator.validate_request("POST", "/redis/test", &params).has_errors());
    }
    
    #[test]
    fn test_validate_with_api_validator_xss() {
        let redis_service = Arc::new(DefaultRedisService::new());
        let plugin = RedisPlugin::new(redis_service, "redis://localhost:6379".to_string());
        
        // Tạo ApiValidator và thêm các rule
        let mut validator = ApiValidator::new();
        
        // Rule cho key và value
        let key_rule = FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: Some(1),
            max_length: Some(100),
            pattern: Some(r"^[a-zA-Z0-9_:]+$".to_string()),
            sanitize: true,
        };
        
        let value_rule = FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: Some(1),
            max_length: Some(1024 * 1024),
            pattern: None,
            sanitize: true,
        };
        
        let mut field_rules = HashMap::new();
        field_rules.insert("key".to_string(), key_rule);
        field_rules.insert("value".to_string(), value_rule);
        
        validator.add_rule(ApiValidationRule {
            path: "/redis/test".to_string(),
            method: "POST".to_string(),
            required_fields: vec!["key".to_string(), "value".to_string()],
            field_rules,
        });
        
        // Tạo params với XSS payload
        let mut params = HashMap::new();
        params.insert("key".to_string(), "valid_key".to_string());
        params.insert("value".to_string(), "<script>alert(1)</script>".to_string());
        
        // XSS payload nên bị bắt bởi validator
        assert!(validator.validate_request("POST", "/redis/test", &params).has_errors());
    }
    
    #[test]
    fn test_validate_with_api_validator_sql() {
        let redis_service = Arc::new(DefaultRedisService::new());
        let plugin = RedisPlugin::new(redis_service, "redis://localhost:6379".to_string());
        
        // Tạo ApiValidator và thêm các rule
        let mut validator = ApiValidator::new();
        
        // Rule cho key và value
        let key_rule = FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: Some(1),
            max_length: Some(100),
            pattern: Some(r"^[a-zA-Z0-9_:]+$".to_string()),
            sanitize: true,
        };
        
        let value_rule = FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: Some(1),
            max_length: Some(1024 * 1024),
            pattern: None,
            sanitize: true,
        };
        
        let mut field_rules = HashMap::new();
        field_rules.insert("key".to_string(), key_rule);
        field_rules.insert("value".to_string(), value_rule);
        
        validator.add_rule(ApiValidationRule {
            path: "/redis/test".to_string(),
            method: "POST".to_string(),
            required_fields: vec!["key".to_string(), "value".to_string()],
            field_rules,
        });
        
        // Tạo params với SQL injection payload
        let mut params = HashMap::new();
        params.insert("key".to_string(), "valid_key".to_string());
        params.insert("value".to_string(), "value'; DROP TABLE users; --".to_string());
        
        // SQL injection payload nên bị bắt bởi validator
        assert!(validator.validate_request("POST", "/redis/test", &params).has_errors());
    }
    
    #[tokio::test]
    async fn test_api_validator_request() {
        let redis_service = Arc::new(DefaultRedisService::new());
        let plugin = RedisPlugin::new(redis_service, "redis://localhost:6379".to_string());
        
        let mut params = HashMap::new();
        params.insert("key".to_string(), "valid_key".to_string());
        params.insert("value".to_string(), "valid_value".to_string());
        
        let result = plugin.handle_request("POST", "/redis/set", &params).await;
        assert!(result.is_ok(), "Expected successful API validation but got error: {:?}", result.err());
    }
    
    #[test]
    fn test_validate_with_api_validator_xss_payloads() {
        let redis_service = Arc::new(DefaultRedisService::new());
        let plugin = RedisPlugin::new(redis_service, "redis://localhost:6379".to_string());
        
        // Tạo ApiValidator và thêm các rule
        let mut validator = ApiValidator::new();
        
        // Rule cho key và value
        let key_rule = FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: Some(1),
            max_length: Some(100),
            pattern: Some(r"^[a-zA-Z0-9_:]+$".to_string()),
            sanitize: true,
        };
        
        let value_rule = FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: Some(1),
            max_length: Some(1024 * 1024),
            pattern: None,
            sanitize: true,
        };
        
        let mut field_rules = HashMap::new();
        field_rules.insert("key".to_string(), key_rule);
        field_rules.insert("value".to_string(), value_rule);
        
        validator.add_rule(ApiValidationRule {
            path: "/redis/test".to_string(),
            method: "POST".to_string(),
            required_fields: vec!["key".to_string(), "value".to_string()],
            field_rules,
        });
        
        // Các payload XSS phổ biến
        let xss_payloads = vec![
            "<script>alert('XSS')</script>",
            "<img src=x onerror=alert(1)>",
            "<svg/onload=alert(1)>",
            "<iframe src=javascript:alert(1)>",
        ];
        
        for payload in xss_payloads {
            let mut params = HashMap::new();
            params.insert("key".to_string(), "valid_key".to_string());
            params.insert("value".to_string(), payload.to_string());
            
            // XSS payload nên bị bắt bởi validator
            assert!(validator.validate_request("POST", "/redis/test", &params).has_errors());
        }
    }
    
    #[test]
    fn test_validate_with_api_validator_sql_payloads() {
        let redis_service = Arc::new(DefaultRedisService::new());
        let plugin = RedisPlugin::new(redis_service, "redis://localhost:6379".to_string());
        
        // Tạo ApiValidator và thêm các rule
        let mut validator = ApiValidator::new();
        
        // Rule cho key và value
        let key_rule = FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: Some(1),
            max_length: Some(100),
            pattern: Some(r"^[a-zA-Z0-9_:]+$".to_string()),
            sanitize: true,
        };
        
        let value_rule = FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: Some(1),
            max_length: Some(1024 * 1024),
            pattern: None,
            sanitize: true,
        };
        
        let mut field_rules = HashMap::new();
        field_rules.insert("key".to_string(), key_rule);
        field_rules.insert("value".to_string(), value_rule);
        
        validator.add_rule(ApiValidationRule {
            path: "/redis/test".to_string(),
            method: "POST".to_string(),
            required_fields: vec!["key".to_string(), "value".to_string()],
            field_rules,
        });
        
        // Các payload SQLi phổ biến
        let sqli_payloads = vec![
            "' OR '1'='1", "1; DROP TABLE users; --", "admin' --", "' UNION SELECT NULL--"
        ];
        
        for payload in sqli_payloads {
            let mut params = HashMap::new();
            params.insert("key".to_string(), "valid_key".to_string());
            params.insert("value".to_string(), payload.to_string());
            
            // SQL injection payload nên bị bắt bởi validator
            assert!(validator.validate_request("POST", "/redis/test", &params).has_errors());
        }
    }
}

// WARNING: Nếu mở rộng RedisPlugin nhận input từ external (API, user command),
// bắt buộc sử dụng ApiValidator và các phương thức validate_and_sanitize_field để đảm bảo an toàn XSS/SQLi.
// KHÔNG sử dụng các phương thức validate_input và validate_input_all đã bị đánh dấu deprecated.
