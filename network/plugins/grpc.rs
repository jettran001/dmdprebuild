use crate::core::engine::{Plugin, PluginType, PluginError};
use std::env;
use crate::security::input_validation::security;
use async_trait::async_trait;
use std::time::Duration;
use tokio::time::timeout;
use std::future::Future;
use tracing::{info, warn, error, debug};

/// Cấu hình cho GrpcPlugin
pub struct GrpcConfig {
    /// Timeout mặc định cho client call (milliseconds)
    pub default_timeout_ms: u64,
    /// Timeout tối đa cho bất kỳ client call nào (milliseconds)
    pub max_timeout_ms: u64,
    /// Số lượng retries tối đa cho gọi không timeout nhưng thất bại
    pub max_retries: u32,
    /// Delay giữa các lần retry (milliseconds)
    pub retry_delay_ms: u64,
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: 5000,  // 5 seconds
            max_timeout_ms: 30000,     // 30 seconds
            max_retries: 3,
            retry_delay_ms: 1000,      // 1 second
        }
    }
}

pub struct GrpcPlugin {
    name: String,
    config: GrpcConfig,
}

impl GrpcPlugin {
    pub fn new() -> Self {
        Self {
            name: "grpc".to_string(),
            config: GrpcConfig::default(),
        }
    }
    
    /// Tạo mới GrpcPlugin với config tùy chỉnh
    pub fn with_config(config: GrpcConfig) -> Self {
        Self {
            name: "grpc".to_string(),
            config,
        }
    }
    
    /// Validate input data nếu nhận từ external (API, user, ...)
    #[deprecated(note = "Use ApiValidator or Validator trait instead")]
    /// [DEPRECATED] Không sử dụng hàm này trong code xử lý chính. Hãy dùng ApiValidator hoặc trait Validator chuẩn.
    pub fn validate_input(&self, data: &str) -> Result<(), String> {
        let enable = env::var("PLUGIN_INPUT_VALIDATION").unwrap_or_else(|_| "on".to_string());
        if enable == "off" {
            return Ok(());
        }
        security::check_xss(data, "grpc_data").map_err(|e| e.to_string())?;
        security::check_sql_injection(data, "grpc_data").map_err(|e| e.to_string())?;
        Ok(())
    }
    
    /// Validate input cho API endpoint bằng ApiValidator
    pub fn validate_api_input(&self, validator: &crate::security::api_validation::ApiValidator, method: &str, path: &str, params: &std::collections::HashMap<String, String>) -> Result<(), String> {
        let errors = validator.validate_request(method, path, params);
        if errors.has_errors() {
            return Err(format!("Validation failed: {:?}", errors));
        }
        Ok(())
    }
    
    pub fn health_check(&self) -> Result<bool, String> {
        // Mock: luôn trả về Ok(true)
        Ok(true)
    }
    
    /// Thực hiện gRPC client call với timeout để tránh deadlock
    pub async fn call_with_timeout<F, T, E>(&self, 
                                          future: F, 
                                          timeout_ms: Option<u64>, 
                                          context: &str) -> Result<T, String> 
    where
        F: Future<Output = Result<T, E>>,
        E: std::fmt::Display,
    {
        // Sử dụng timeout chỉ định hoặc default timeout
        let timeout_duration = Duration::from_millis(
            timeout_ms
                .map(|t| t.min(self.config.max_timeout_ms)) // Không cho phép timeout lớn hơn max_timeout_ms
                .unwrap_or(self.config.default_timeout_ms)
        );
        
        // Thực hiện call với timeout
        match timeout(timeout_duration, future).await {
            Ok(result) => {
                match result {
                    Ok(value) => Ok(value),
                    Err(e) => {
                        error!("[GrpcPlugin] Call failed for {}: {}", context, e);
                        Err(format!("gRPC call error: {}", e))
                    }
                }
            },
            Err(_) => {
                error!("[GrpcPlugin] Call timed out after {}ms for {}", timeout_duration.as_millis(), context);
                Err(format!("gRPC call timed out after {}ms", timeout_duration.as_millis()))
            }
        }
    }
    
    /// Thực hiện gRPC client call với retry và timeout
    pub async fn call_with_retry_and_timeout<F, Fut, T, E>(&self, 
                                                       operation: F,
                                                       context: &str,
                                                       timeout_ms: Option<u64>,
                                                       retries: Option<u32>) -> Result<T, String> 
    where
        F: Fn() -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, E>> + Send,
        E: std::fmt::Display + Send + Sync,
        T: Send,
    {
        let max_retries = retries.unwrap_or(self.config.max_retries);
        let timeout_duration = timeout_ms.unwrap_or(self.config.default_timeout_ms);
        
        let mut attempts = 0;
        let mut last_error = None;
        
        while attempts <= max_retries {
            match self.call_with_timeout(operation(), Some(timeout_duration), context).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    attempts += 1;
                    last_error = Some(e.clone());
                    
                    if attempts <= max_retries {
                        warn!("[GrpcPlugin] Retry {}/{} for {}: {}", 
                             attempts, max_retries, context, e);
                        tokio::time::sleep(Duration::from_millis(self.config.retry_delay_ms)).await;
                    }
                }
            }
        }
        
        Err(last_error.unwrap_or_else(|| format!("Unknown error after {} retries", max_retries)))
    }
}

impl Plugin for GrpcPlugin {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn plugin_type(&self) -> PluginType {
        PluginType::Grpc
    }
    
    fn start(&self) -> Result<bool, PluginError> {
        // Mock: always return Ok(true)
        Ok(true)
    }
    
    fn stop(&self) -> Result<(), PluginError> {
        // Mock: do nothing
        Ok(())
    }
    
    fn check_health(&self) -> Result<bool, PluginError> {
        Ok(true)
    }
}

/// WARNING: Nếu mở rộng GrpcPlugin nhận input từ external (API, user, ...),
/// bắt buộc gọi validate_input trước khi xử lý để đảm bảo an toàn XSS/SQLi.

/// Unit test cho validate_api_input
#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::api_validation::{ApiValidator, ApiValidationRule, FieldRule};
    use std::collections::HashMap;
    
    #[test]
    fn test_validate_api_input() {
        let plugin = GrpcPlugin::new();
        let mut validator = ApiValidator::new();
        let mut field_rules = HashMap::new();
        field_rules.insert("data".to_string(), FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: Some(1),
            max_length: Some(100),
            pattern: None,
            sanitize: true,
        });
        validator.add_rule(ApiValidationRule {
            path: "/grpc/send".to_string(),
            method: "POST".to_string(),
            required_fields: vec!["data".to_string()],
            field_rules,
        });
        let mut params = HashMap::new();
        params.insert("data".to_string(), "valid_data".to_string());
        assert!(plugin.validate_api_input(&validator, "POST", "/grpc/send", &params).is_ok());
        params.insert("data".to_string(), "<script>bad</script>".to_string());
        assert!(plugin.validate_api_input(&validator, "POST", "/grpc/send", &params).is_err());
    }
    
    #[test]
    fn test_validate_api_input_xss_payloads() {
        let plugin = GrpcPlugin::new();
        let mut validator = ApiValidator::new();
        let mut field_rules = HashMap::new();
        field_rules.insert("data".to_string(), FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: Some(1),
            max_length: Some(100),
            pattern: None,
            sanitize: true,
        });
        validator.add_rule(ApiValidationRule {
            path: "/grpc/send".to_string(),
            method: "POST".to_string(),
            required_fields: vec!["data".to_string()],
            field_rules,
        });
        let mut params = HashMap::new();
        let xss_payloads = vec![
            "<script>alert('XSS')</script>",
            "<img src=x onerror=alert(1)>",
            "<svg/onload=alert(1)>",
            "<iframe src=javascript:alert(1)>",
        ];
        for payload in xss_payloads {
            params.insert("data".to_string(), payload.to_string());
            assert!(plugin.validate_api_input(&validator, "POST", "/grpc/send", &params).is_err());
        }
    }
    
    #[test]
    fn test_validate_api_input_sql_payloads() {
        let plugin = GrpcPlugin::new();
        let mut validator = ApiValidator::new();
        let mut field_rules = HashMap::new();
        field_rules.insert("data".to_string(), FieldRule {
            field_type: "string".to_string(),
            required: true,
            min_length: Some(1),
            max_length: Some(100),
            pattern: None,
            sanitize: true,
        });
        validator.add_rule(ApiValidationRule {
            path: "/grpc/send".to_string(),
            method: "POST".to_string(),
            required_fields: vec!["data".to_string()],
            field_rules,
        });
        let mut params = HashMap::new();
        let sqli_payloads = vec![
            "' OR '1'='1", "1; DROP TABLE users; --", "admin' --", "' UNION SELECT NULL--"
        ];
        for payload in sqli_payloads {
            params.insert("data".to_string(), payload.to_string());
            assert!(plugin.validate_api_input(&validator, "POST", "/grpc/send", &params).is_err());
        }
    }
    
    #[tokio::test]
    async fn test_call_with_timeout_success() {
        let plugin = GrpcPlugin::new();
        
        // Test with a fast operation that should succeed
        let result = plugin.call_with_timeout(
            async { 
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok::<_, String>("success") 
            },
            Some(100),
            "test_operation"
        ).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
    }
    
    #[tokio::test]
    async fn test_call_with_timeout_timeout() {
        let plugin = GrpcPlugin::new();
        
        // Test with a slow operation that should timeout
        let result = plugin.call_with_timeout(
            async { 
                tokio::time::sleep(Duration::from_millis(200)).await;
                Ok::<_, String>("success") 
            },
            Some(50),
            "test_operation_slow"
        ).await;
        
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("timed out"));
    }
    
    #[tokio::test]
    async fn test_call_with_retry_and_timeout() {
        let plugin = GrpcPlugin::with_config(GrpcConfig {
            retry_delay_ms: 10, // Fast retry for test
            ..Default::default()
        });
        
        // Operation that fails twice then succeeds
        let mut attempts = 0;
        let result = plugin.call_with_retry_and_timeout(
            || async {
                attempts += 1;
                if attempts <= 2 {
                    Err::<&str, _>("temporary failure")
                } else {
                    Ok("success")
                }
            },
            "test_retry_operation",
            Some(100),
            Some(3)
        ).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
        assert_eq!(attempts, 3); // Should have attempted 3 times
    }
}

// The following code is for demonstration only. Move this into a function or test if needed.
#[allow(dead_code)]
fn spawn_resource_monitor() {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    
    tokio::spawn(async move {
        let mut shutdown_rx = shutdown_rx;
        
        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {
                    #[cfg(target_os = "linux")]
                    if let Ok(meminfo) = std::fs::read_to_string("/proc/self/status") {
                        for line in meminfo.lines() {
                            if line.starts_with("VmRSS") || line.starts_with("VmSize") {
                                info!("[GrpcPlugin][Resource] {}", line);
                            }
                        }
                    }
                    #[cfg(target_os = "linux")]
                    if let Ok(fds) = std::fs::read_dir("/proc/self/fd") {
                        let count = fds.count();
                        info!("[GrpcPlugin][Resource] Open file descriptors: {}", count);
                    }
                }
                _ = &mut shutdown_rx => {
                    info!("[GrpcPlugin] Background resource task shutting down");
                    break;
                }
            }
        }
    });
}

// GrpcClient struct với shutdown capabilities
pub struct GrpcClient {
    shutdown_signal: Option<tokio::sync::oneshot::Sender<()>>,
}

impl GrpcClient {
    pub fn new() -> Self {
        Self {
            shutdown_signal: None,
        }
    }
    
    pub fn start_monitoring(&mut self) {
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        self.shutdown_signal = Some(shutdown_tx);
        
        tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;
            
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                        debug!("[GrpcClient] Still monitoring...");
                    }
                    _ = &mut shutdown_rx => {
                        info!("[GrpcClient] Monitoring task shutting down");
                        break;
                    }
                }
            }
        });
    }
    
    pub fn shutdown(&mut self) {
        if let Some(signal) = self.shutdown_signal.take() {
            let _ = signal.send(());
            debug!("[GrpcClient] Shutdown signal sent");
        }
    }
}

impl Drop for GrpcClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}
