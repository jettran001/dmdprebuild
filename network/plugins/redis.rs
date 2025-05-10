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
        let mut config = RedisConfig::default();
        config.url = url;
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
            match rate_limiter.register_path(endpoint, config) {
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
        let mut plugin = Self {
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
    #[deprecated(note = "Use ApiValidator or Validator trait instead")]
    /// [DEPRECATED] Không sử dụng hàm này trong code xử lý chính. Hãy dùng ApiValidator hoặc trait Validator chuẩn.
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
    #[deprecated(note = "Use ApiValidator or Validator trait instead")]
    /// [DEPRECATED] Không sử dụng hàm này trong code xử lý chính. Hãy dùng ApiValidator hoặc trait Validator chuẩn.
    pub fn validate_input_all(&self, key: &str, value: &str, url: &str, username: &Option<String>, password: &Option<String>) -> Result<(), String> {
        self.validate_input(key, value)?;
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
    pub fn set_with_validate(&self, key: &str, value: &str) -> Result<(), String> {
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
        tokio::task::block_in_place(|| std::thread::sleep(Duration::from_millis(50)));
        
        // Thực hiện set thông qua service
        self.service.set(&sanitized_key, &sanitized_value)?;
        info!("Redis set: {} = {}", sanitized_key, sanitized_value);
        
        Ok(())
    }
    
    /// Xử lý request từ endpoint API
    pub fn handle_request(&self, method: &str, path: &str, params: &HashMap<String, String>) -> Result<String, String> {
        // Validate request
        let errors = self.validator.validate_request(method, path, params);
        if errors.has_errors() {
            error!("Redis validation failed: {:?}", errors);
            return Err(format!("Validation failed: {:?}", errors));
        }
        
        // Sanitize request
        let sanitized_params = self.validator.sanitize_request(method, path, params.clone());
        
        // Xử lý request dựa trên method và path
        match (method, path) {
            ("POST", "/redis/set") => {
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
                
                // Thực hiện set thông qua service - thay ? bằng match
                match self.service.set(key, value) {
                    Ok(_) => {
                        info!("Redis set: {} = {}", key, value);
                        Ok(format!("OK"))
                    },
                    Err(e) => {
                        error!("Redis set failed: {}", e);
                        Err(format!("Redis operation failed: {}", e))
                    }
                }
            },
            ("GET", "/redis/get") => {
                let key = match sanitized_params.get("key") {
                    Some(k) => k,
                    None => {
                        error!("Missing 'key' in params");
                        return Err("Missing 'key' field".to_string());
                    }
                };
                
                // Thực hiện get thông qua service - thay ? bằng match
                match self.service.get(key) {
                    Ok(result) => {
                        match result {
                            Some(value) => {
                                info!("Redis get: {} = {}", key, value);
                                Ok(value)
                            },
                            None => {
                                info!("Redis get: {} = nil", key);
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

impl Plugin for RedisPlugin {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn plugin_type(&self) -> PluginType {
        PluginType::Storage
    }
    
    fn start(&self) -> Result<bool, PluginError> {
        info!("[RedisPlugin] Starting...");
        
        // Connect to Redis
        match self.service.connect_with_auth(&self.config.url, &self.config.username, &self.config.password) {
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
    
    fn stop(&self) -> Result<(), PluginError> {
        info!("[RedisPlugin] Stopping...");
        // Không cần cleanup đặc biệt - Redis connection sẽ tự drop khi RedisPlugin bị drop
        Ok(())
    }

    fn check_health(&self) -> Result<bool, PluginError> {
        // Kiểm tra kết nối thực tế tới Redis
        match self.service.ping() {
            Ok(_) => Ok(true),
            Err(e) => Err(PluginError::Other(format!("Redis health check failed: {}", e))),
        }
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
    fn test_validate_input_all_safe() {
        let redis_service = Arc::new(DefaultRedisService::new());
        let plugin = RedisPlugin::new(redis_service, "redis://localhost:6379".to_string());
        assert!(plugin.validate_input_all("valid_key", "valid_value", "redis://localhost:6379", &Some("user".to_string()), &Some("pass".to_string())).is_ok());
    }
    
    #[test]
    fn test_validate_input_all_xss() {
        let redis_service = Arc::new(DefaultRedisService::new());
        let plugin = RedisPlugin::new(redis_service, "redis://localhost:6379".to_string());
        assert!(plugin.validate_input_all("valid_key", "<script>alert(1)</script>", "redis://localhost:6379", &Some("user".to_string()), &Some("pass".to_string())).is_err());
    }
    
    #[test]
    fn test_validate_input_all_sql() {
        let redis_service = Arc::new(DefaultRedisService::new());
        let plugin = RedisPlugin::new(redis_service, "redis://localhost:6379".to_string());
        assert!(plugin.validate_input_all("valid_key", "value'; DROP TABLE users; --", "redis://localhost:6379", &Some("user".to_string()), &Some("pass".to_string())).is_err());
    }
    
    #[test]
    fn test_api_validator_request() {
        let redis_service = Arc::new(DefaultRedisService::new());
        let plugin = RedisPlugin::new(redis_service, "redis://localhost:6379".to_string());
        
        let mut params = HashMap::new();
        params.insert("key".to_string(), "valid_key".to_string());
        params.insert("value".to_string(), "valid_value".to_string());
        
        assert!(plugin.handle_request("POST", "/redis/set", &params).is_ok());
    }
    
    #[test]
    fn test_validate_input_all_xss_payloads() {
        let redis_service = Arc::new(DefaultRedisService::new());
        let plugin = RedisPlugin::new(redis_service, "redis://localhost:6379".to_string());
        // Các payload XSS phổ biến
        let xss_payloads = vec![
            "<script>alert('XSS')</script>",
            "<img src=x onerror=alert(1)>",
            "<svg/onload=alert(1)>",
            "<iframe src=javascript:alert(1)>",
        ];
        for payload in xss_payloads {
            assert!(plugin.validate_input_all("valid_key", payload, "redis://localhost:6379", &Some("user".to_string()), &Some("pass".to_string())).is_err());
        }
    }
    #[test]
    fn test_validate_input_all_sql_payloads() {
        let redis_service = Arc::new(DefaultRedisService::new());
        let plugin = RedisPlugin::new(redis_service, "redis://localhost:6379".to_string());
        // Các payload SQLi phổ biến
        let sqli_payloads = vec![
            "' OR '1'='1", "1; DROP TABLE users; --", "admin' --", "' UNION SELECT NULL--"
        ];
        for payload in sqli_payloads {
            assert!(plugin.validate_input_all("valid_key", payload, "redis://localhost:6379", &Some("user".to_string()), &Some("pass".to_string())).is_err());
        }
    }
}

// WARNING: Nếu mở rộng RedisPlugin nhận input từ external (API, user command),
// bắt buộc sử dụng ApiValidator hoặc gọi validate_input trước khi xử lý key/value để đảm bảo an toàn XSS/SQLi.
