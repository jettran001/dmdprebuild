use crate::core::engine::{Plugin, PluginType, PluginError};
use std::env;
use crate::security::input_validation::security;
use log::{info, warn, error, debug};
use async_trait::async_trait;
use crate::infra::service_traits::{WasmService, ServiceError};
use std::any::Any;

pub struct WasmPlugin {
    name: String,
    config: WasmConfig,
}

pub struct WasmConfig {
    pub module_path: String,
    pub memory_limit_mb: usize,
    pub timeout_ms: u64,
}

impl Default for WasmConfig {
    fn default() -> Self {
        Self {
            module_path: String::new(),
            memory_limit_mb: 128,
            timeout_ms: 5000,
        }
    }
}

impl WasmPlugin {
    pub fn new(name: String, config: WasmConfig) -> Self {
        Self {
            name,
            config,
        }
    }
    
    pub fn with_memory_limit(mut self, limit_mb: usize) -> Self {
        self.config.memory_limit_mb = limit_mb;
        self
    }
    
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.config.timeout_ms = timeout_ms;
        self
    }
    
    pub fn load_module(&self) -> Result<(), PluginError> {
        // Implementation for loading WASM module
        info!("[{}] Loading WASM module from {}", self.name, self.config.module_path);
        // Placeholder for now
        Ok(())
    }
    /// Validate input data nếu nhận từ external (API, user, ...)
    #[deprecated(note = "Use ApiValidator or Validator trait instead")]
    /// [DEPRECATED] Không sử dụng hàm này trong code xử lý chính. Hãy dùng ApiValidator hoặc trait Validator chuẩn.
    pub fn validate_input(&self, data: &str) -> Result<(), String> {
        let enable = env::var("PLUGIN_INPUT_VALIDATION").unwrap_or_else(|_| "on".to_string());
        if enable == "off" {
            warn!("[WasmPlugin] Input validation disabled via environment variable. This is NOT recommended for production!");
            return Ok(());
        }
        debug!("[WasmPlugin] Validating input data");
        security::check_xss(data, "wasm_data").map_err(|e| e.to_string())?;
        security::check_sql_injection(data, "wasm_data").map_err(|e| e.to_string())?;
        Ok(())
    }
    /// Validate input cho API endpoint bằng ApiValidator
    pub fn validate_api_input(&self, validator: &crate::security::api_validation::ApiValidator, method: &str, path: &str, params: &std::collections::HashMap<String, String>) -> Result<(), String> {
        debug!("[WasmPlugin] Validating API input for method={}, path={}", method, path);
        let errors = validator.validate_request(method, path, params);
        if errors.has_errors() {
            error!("[WasmPlugin] API validation failed: {:?}", errors);
            return Err(format!("Validation failed: {:?}", errors));
        }
        Ok(())
    }
    pub fn health_check(&self) -> Result<bool, String> {
        // Mock: luôn trả về Ok(true)
        debug!("[WasmPlugin] Health check called");
        Ok(true)
    }
}

#[async_trait]
impl Plugin for WasmPlugin {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn plugin_type(&self) -> PluginType {
        PluginType::Wasm
    }
    
    async fn start(&self) -> Result<bool, PluginError> {
        info!("[{}] Starting WASM plugin", self.name);
        self.load_module()?;
        Ok(true)
    }
    
    async fn stop(&self) -> Result<(), PluginError> {
        info!("[{}] Stopping WASM plugin", self.name);
        // Implementation for cleanup
        Ok(())
    }
    
    async fn check_health(&self) -> Result<bool, PluginError> {
        // Placeholder health check
        Ok(true)
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}

// Thêm implementation cho DefaultWasmService để tuân thủ theo trait WasmService
pub struct DefaultWasmService {
    // Các trường cần thiết cho WasmService
    #[allow(dead_code)]
    initialized: bool,
    #[allow(dead_code)]
    modules: std::collections::HashMap<String, Vec<u8>>,
}

impl DefaultWasmService {
    pub fn new() -> Self {
        Self {
            initialized: false,
            modules: std::collections::HashMap::new(),
        }
    }
}

impl Default for DefaultWasmService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WasmService for DefaultWasmService {
    /// Initialize the Wasm runtime
    async fn init(&self) -> Result<(), ServiceError> {
        info!("[DefaultWasmService] Initializing Wasm runtime");
        // Mô phỏng khởi tạo runtime
        Ok(())
    }

    /// Load a Wasm module from bytes
    async fn load_module(&self, bytes: &[u8]) -> Result<String, ServiceError> {
        debug!("[DefaultWasmService] Loading Wasm module, size: {} bytes", bytes.len());
        // Mô phỏng tải module và trả về module ID
        let module_id = format!("module-{}", uuid::Uuid::new_v4());
        Ok(module_id)
    }

    /// Execute a function in a loaded module
    async fn execute_function(&self, module_id: &str, function_name: &str, params: &[u8]) -> Result<Vec<u8>, ServiceError> {
        debug!("[DefaultWasmService] Executing function '{}' in module '{}'", function_name, module_id);
        // Kiểm tra module tồn tại
        if !module_id.starts_with("module-") {
            return Err(ServiceError::NotFoundError(format!("Module not found: {}", module_id)));
        }
        
        // Mô phỏng thực thi hàm và trả về kết quả
        let result = format!("Result of executing {} with {} bytes of params", function_name, params.len())
            .into_bytes();
        Ok(result)
    }

    /// Unload a module from memory
    async fn unload_module(&self, module_id: &str) -> Result<(), ServiceError> {
        debug!("[DefaultWasmService] Unloading module '{}'", module_id);
        // Kiểm tra module tồn tại
        if !module_id.starts_with("module-") {
            return Err(ServiceError::NotFoundError(format!("Module not found: {}", module_id)));
        }
        
        // Mô phỏng dọn dẹp bộ nhớ
        Ok(())
    }

    /// Health check for this service
    async fn health_check(&self) -> Result<bool, ServiceError> {
        debug!("[DefaultWasmService] Health check");
        Ok(true)
    }
}

/// WARNING: Nếu mở rộng WasmPlugin nhận input từ external (API, user, ...),
/// bắt buộc gọi validate_input trước khi xử lý để đảm bảo an toàn XSS/SQLi.

/// Unit test cho validate_api_input
#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::api_validation::{ApiValidator, ApiValidationRule, FieldRule};
    use std::collections::HashMap;
    #[test]
    fn test_validate_api_input() {
        let plugin = WasmPlugin::new("wasm".to_string(), WasmConfig {
            module_path: String::new(),
            memory_limit_mb: 128,
            timeout_ms: 5000,
        });
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
            path: "/wasm/send".to_string(),
            method: "POST".to_string(),
            required_fields: vec!["data".to_string()],
            field_rules,
        });
        let mut params = HashMap::new();
        params.insert("data".to_string(), "valid_data".to_string());
        assert!(plugin.validate_api_input(&validator, "POST", "/wasm/send", &params).is_ok());
        params.insert("data".to_string(), "<script>bad</script>".to_string());
        assert!(plugin.validate_api_input(&validator, "POST", "/wasm/send", &params).is_err());
    }
    #[test]
    fn test_validate_api_input_xss_payloads() {
        let plugin = WasmPlugin::new("wasm".to_string(), WasmConfig {
            module_path: String::new(),
            memory_limit_mb: 128,
            timeout_ms: 5000,
        });
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
            path: "/wasm/send".to_string(),
            method: "POST".to_string(),
            required_fields: vec!["data".to_string()],
            field_rules,
        });
        let mut params = HashMap::new();
        let xss_payloads = vec![
            "<script>alert('XSS')</script>",
            "<img src=x onerror=alert(1)>",
            "<svg/onload=alert(1)>",
            "<iframe src=javascript:alert(1)>"
        ];
        for payload in xss_payloads {
            params.insert("data".to_string(), payload.to_string());
            assert!(plugin.validate_api_input(&validator, "POST", "/wasm/send", &params).is_err());
        }
    }
    #[test]
    fn test_validate_api_input_sql_payloads() {
        let plugin = WasmPlugin::new("wasm".to_string(), WasmConfig {
            module_path: String::new(),
            memory_limit_mb: 128,
            timeout_ms: 5000,
        });
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
            path: "/wasm/send".to_string(),
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
            assert!(plugin.validate_api_input(&validator, "POST", "/wasm/send", &params).is_err());
        }
    }
}

// The following code is for demonstration only. Move this into a function or test if needed.
#[allow(dead_code)]
fn spawn_resource_monitor() -> tokio::sync::oneshot::Sender<()> {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    
    // Tạo một hàm riêng biệt thay vì closure để đảm bảo Send
    async fn resource_monitor_task(mut shutdown_rx: tokio::sync::oneshot::Receiver<()>) {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {
                    #[cfg(target_os = "linux")]
                    check_memory_usage();
                    
                    #[cfg(target_os = "linux")]
                    check_file_descriptors();
                }
                _ = &mut shutdown_rx => {
                    info!("[WasmPlugin] Background resource task shutting down");
                    break;
                }
            }
        }
    }
    
    #[cfg(target_os = "linux")]
    fn check_memory_usage() {
        if let Ok(meminfo) = std::fs::read_to_string("/proc/self/status") {
            for line in meminfo.lines() {
                if line.starts_with("VmRSS") || line.starts_with("VmSize") {
                    info!("[WasmPlugin][Resource] {}", line);
                }
            }
        }
    }
    
    #[cfg(target_os = "linux")]
    fn check_file_descriptors() {
        if let Ok(fds) = std::fs::read_dir("/proc/self/fd") {
            let count = fds.count();
            info!("[WasmPlugin][Resource] Open file descriptors: {}", count);
        }
    }
    
    // Spawn task với từ khóa 'static để đảm bảo Send
    tokio::spawn(async move {
        resource_monitor_task(shutdown_rx).await;
    });
    
    shutdown_tx
}

