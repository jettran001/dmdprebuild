//! Configuration Loader
//!
//! Module này cung cấp các chức năng tải cấu hình từ nhiều nguồn khác nhau.
//! Hợp nhất từ các ConfigLoader trước đây trong network/core/config.rs và network/infra/config_loader.rs.

use std::path::Path;
use std::fs;
use std::io::Read;
use std::env;
use serde::de::DeserializeOwned;
use tracing::{info, debug, error};

use crate::config::{NetworkConfig, ConfigError, RateLimitPath, RateLimitIdentifier, RateLimitStorage};
use crate::config::AuthConfig;
use crate::config::InputValidationConfig;

/// Các định dạng file cấu hình được hỗ trợ
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConfigFormat {
    /// JSON format
    Json,
    /// TOML format
    Toml,
    /// YAML format
    Yaml,
}

impl ConfigFormat {
    /// Detect format from file extension
    pub fn from_extension(path: &str) -> ConfigFormat {
        if path.ends_with(".json") {
            ConfigFormat::Json
        } else if path.ends_with(".toml") {
            ConfigFormat::Toml
        } else if path.ends_with(".yaml") || path.ends_with(".yml") {
            ConfigFormat::Yaml
        } else {
            // Default to JSON
            ConfigFormat::Json
        }
    }
}

/// Tùy chọn cho ConfigLoader
#[derive(Debug, Clone)]
pub struct ConfigLoaderOptions {
    /// Loads environment variables
    pub load_env: bool,
    /// Environment variable prefix
    pub env_prefix: Option<String>,
    /// Config format
    pub format: ConfigFormat,
    /// Default config file path
    pub default_path: Option<String>,
}

impl Default for ConfigLoaderOptions {
    fn default() -> Self {
        Self {
            load_env: true,
            env_prefix: Some("NETWORK_".to_string()),
            format: ConfigFormat::Json,
            default_path: Some("config/network.json".to_string()),
        }
    }
}

/// ConfigLoader tải cấu hình từ nhiều nguồn
pub struct ConfigLoader {
    options: ConfigLoaderOptions,
}

impl Default for ConfigLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigLoader {
    /// Tạo một ConfigLoader mới với tùy chọn mặc định
    pub fn new() -> Self {
        Self {
            options: ConfigLoaderOptions::default(),
        }
    }
    
    /// Tạo một ConfigLoader mới với tùy chọn tùy chỉnh
    pub fn with_options(options: ConfigLoaderOptions) -> Self {
        Self {
            options,
        }
    }
    
    /// Đọc cấu hình từ file
    pub fn load_from_file<P: AsRef<Path>>(&self, path: P) -> Result<NetworkConfig, ConfigError> {
        let path_ref = path.as_ref();
        debug!("Loading configuration from file: {:?}", path_ref);
        
        if !path_ref.exists() {
            error!("Configuration file not found: {:?}", path_ref);
            return Err(ConfigError::FileNotFound(path_ref.to_string_lossy().to_string()));
        }
        
        let mut file = fs::File::open(path_ref)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        
        let format = ConfigFormat::from_extension(&path_ref.to_string_lossy());
        let config = self.parse_content(&content, format)?;
        
        info!("Configuration loaded from file: {:?}", path_ref);
        Ok(config)
    }
    
    /// Đọc cấu hình từ biến môi trường
    pub fn load_from_env(&self) -> Result<NetworkConfig, ConfigError> {
        debug!("Loading configuration from environment variables");
        
        let mut config = NetworkConfig::default();
        if self.options.load_env {
            self.override_from_env(&mut config)?;
        }
        
        info!("Configuration loaded from environment variables");
        Ok(config)
    }
    
    /// Đọc cấu hình từ file hoặc tạo mặc định nếu không tồn tại
    pub fn load_or_create_default<P: AsRef<Path>>(&self, path: P) -> Result<NetworkConfig, ConfigError> {
        let path_ref = path.as_ref();
        
        if path_ref.exists() {
            self.load_from_file(path_ref)
        } else {
            info!("Configuration file not found, creating default: {:?}", path_ref);
            let config = NetworkConfig::default();
            self.save_to_file(&config, path_ref)?;
            Ok(config)
        }
    }
    
    /// Lưu cấu hình vào file
    pub fn save_to_file<P: AsRef<Path>>(&self, config: &NetworkConfig, path: P) -> Result<(), ConfigError> {
        let path_ref = path.as_ref();
        debug!("Saving configuration to file: {:?}", path_ref);
        
        // Ensure directory exists
        if let Some(parent) = path_ref.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        
        let format = ConfigFormat::from_extension(&path_ref.to_string_lossy());
        let content = match format {
            ConfigFormat::Json => match serde_json::to_string_pretty(config) {
                Ok(content) => content,
                Err(e) => return Err(ConfigError::from(e)),
            },
            ConfigFormat::Toml => match toml::to_string(config) {
                Ok(content) => content,
                Err(e) => return Err(ConfigError::ParseError(format!("TOML serialization error: {}", e))),
            },
            ConfigFormat::Yaml => match serde_yaml::to_string(config) {
                Ok(content) => content,
                Err(e) => return Err(ConfigError::ParseError(format!("YAML serialization error: {}", e))),
            },
        };
        
        match fs::write(path_ref, content) {
            Ok(_) => {
                info!("Configuration saved to file: {:?}", path_ref);
                Ok(())
            },
            Err(e) => Err(ConfigError::from(e)),
        }
    }
    
    /// Phân tích nội dung cấu hình theo định dạng
    fn parse_content<T: DeserializeOwned>(&self, content: &str, format: ConfigFormat) -> Result<T, ConfigError> {
        match format {
            ConfigFormat::Json => {
                serde_json::from_str(content).map_err(|e| ConfigError::ParseError(format!("JSON parse error: {}", e)))
            },
            ConfigFormat::Toml => {
                toml::from_str(content).map_err(|e| ConfigError::ParseError(format!("TOML parse error: {}", e)))
            },
            ConfigFormat::Yaml => {
                serde_yaml::from_str(content).map_err(|e| ConfigError::ParseError(format!("YAML parse error: {}", e)))
            },
        }
    }
    
    /// Ghi đè cấu hình từ biến môi trường
    fn override_from_env(&self, config: &mut NetworkConfig) -> Result<(), ConfigError> {
        let prefix = self.options.env_prefix.as_deref().unwrap_or("");
        
        // Core config
        if let Ok(node_id) = env::var(format!("{}NODE_ID", prefix)) {
            config.core.node_id = node_id;
        }
        
        if let Ok(network_id) = env::var(format!("{}NETWORK_ID", prefix)) {
            config.core.network_id = network_id;
        }
        
        if let Ok(listen_address) = env::var(format!("{}LISTEN_ADDRESS", prefix)) {
            config.core.listen_address = listen_address;
        }
        
        if let Ok(bootstrap_nodes) = env::var(format!("{}BOOTSTRAP_NODES", prefix)) {
            config.core.bootstrap_nodes = bootstrap_nodes.split(',').map(|s| s.trim().to_string()).collect();
        }
        
        if let Ok(max_peers) = env::var(format!("{}MAX_PEERS", prefix)) {
            if let Ok(value) = max_peers.parse::<usize>() {
                config.core.max_peers = value;
            }
        }
        
        // Plugin configs
        if let Ok(redis_url) = env::var(format!("{}REDIS_URL", prefix)) {
            config.plugins.redis.url = redis_url;
        }
        
        if let Ok(redis_pool_size) = env::var(format!("{}REDIS_POOL_SIZE", prefix)) {
            if let Ok(value) = redis_pool_size.parse::<u32>() {
                config.plugins.redis.pool_size = value;
            }
        }
        
        if let Ok(webrtc_stun) = env::var(format!("{}WEBRTC_STUN_SERVERS", prefix)) {
            config.plugins.webrtc.stun_servers = webrtc_stun.split(',').map(|s| s.trim().to_string()).collect();
        }
        
        if let Ok(ipfs_url) = env::var(format!("{}IPFS_URL", prefix)) {
            config.plugins.ipfs.url = ipfs_url;
        }
        
        // Security config
        if let Ok(tls_cert) = env::var(format!("{}TLS_CERT_PATH", prefix)) {
            config.security.tls_cert_path = Some(tls_cert);
        }
        
        if let Ok(tls_key) = env::var(format!("{}TLS_KEY_PATH", prefix)) {
            config.security.tls_key_path = Some(tls_key);
        }
        
        if let Ok(enable_auth) = env::var(format!("{}ENABLE_AUTH", prefix)) {
            config.security.enable_auth = self.parse_bool(&enable_auth)?;
        }
        
        // Logging config
        if let Ok(log_level) = env::var(format!("{}LOG_LEVEL", prefix)) {
            config.logging.level = log_level;
        }
        
        if let Ok(log_format) = env::var(format!("{}LOG_FORMAT", prefix)) {
            config.logging.format = log_format;
        }
        
        if let Ok(log_file) = env::var(format!("{}LOG_FILE", prefix)) {
            config.logging.log_file = Some(log_file);
        }
        
        Ok(())
    }
    
    /// Parse boolean value from string
    fn parse_bool(&self, value: &str) -> Result<bool, ConfigError> {
        match value.to_lowercase().as_str() {
            "true" | "yes" | "1" | "on" | "enabled" => Ok(true),
            "false" | "no" | "0" | "off" | "disabled" => Ok(false),
            _ => Err(ConfigError::InvalidValue(format!("Cannot parse as boolean: {}", value))),
        }
    }
    
    /// Tải cấu hình từ các file YAML trong thư mục security/configs
    pub fn load_security_configs(&self) -> Result<NetworkConfig, ConfigError> {
        debug!("Loading security configurations from YAML files");
        
        let mut config = NetworkConfig::default();
        
        // Tải auth.yaml
        let auth_path = Path::new("network/security/configs/auth.yaml");
        if auth_path.exists() {
            debug!("Loading auth config from: {:?}", auth_path);
            let content = fs::read_to_string(auth_path)
                .map_err(|e| ConfigError::IoError(format!("Failed to read auth config: {}", e)))?;
            
            let auth_config: serde_yaml::Value = serde_yaml::from_str(&content)
                .map_err(|e| ConfigError::ParseError(format!("YAML parse error in auth config: {}", e)))?;
            
            if let Some(auth) = auth_config.get("auth") {
                let auth_struct: AuthConfig = serde_yaml::from_value(auth.clone())
                    .map_err(|e| ConfigError::ParseError(format!("Failed to parse auth config: {}", e)))?;
                config.security.auth = Some(auth_struct);
            }
        }
        
        // Tải input_validation.yaml
        let validation_path = Path::new("network/security/configs/input_validation.yaml");
        if validation_path.exists() {
            debug!("Loading input validation config from: {:?}", validation_path);
            let content = fs::read_to_string(validation_path)
                .map_err(|e| ConfigError::IoError(format!("Failed to read input validation config: {}", e)))?;
            
            let validation_config: serde_yaml::Value = serde_yaml::from_str(&content)
                .map_err(|e| ConfigError::ParseError(format!("YAML parse error in input validation config: {}", e)))?;
            
            if let Some(validation) = validation_config.get("input_validation") {
                let validation_struct: InputValidationConfig = serde_yaml::from_value(validation.clone())
                    .map_err(|e| ConfigError::ParseError(format!("Failed to parse input validation config: {}", e)))?;
                config.security.input_validation = Some(validation_struct);
            }
        }
        
        // Tải rate_limiter.yaml
        let rate_limiter_path = Path::new("network/security/configs/rate_limiter.yaml");
        if rate_limiter_path.exists() {
            debug!("Loading rate limiter config from: {:?}", rate_limiter_path);
            let content = fs::read_to_string(rate_limiter_path)
                .map_err(|e| ConfigError::IoError(format!("Failed to read rate limiter config: {}", e)))?;
            
            let rate_limiter_config: serde_yaml::Value = serde_yaml::from_str(&content)
                .map_err(|e| ConfigError::ParseError(format!("YAML parse error in rate limiter config: {}", e)))?;
            
            if let Some(rate_limiter) = rate_limiter_config.get("rate_limiter") {
                let mut rate_limit = config.security.rate_limit;
                
                // Xử lý cấu hình cơ bản
                if let Some(enabled) = rate_limiter.get("enabled") {
                    if let Some(enabled_bool) = enabled.as_bool() {
                        rate_limit.enabled = enabled_bool;
                    }
                }
                
                if let Some(algorithm) = rate_limiter.get("default_algorithm") {
                    if let Some(algorithm_str) = algorithm.as_str() {
                        rate_limit.default_algorithm = Some(algorithm_str.to_string());
                    }
                }
                
                if let Some(limit) = rate_limiter.get("default_limit") {
                    if let Some(limit_int) = limit.as_u64() {
                        rate_limit.max_requests = limit_int as usize;
                    }
                }
                
                if let Some(window) = rate_limiter.get("default_window") {
                    if let Some(window_int) = window.as_u64() {
                        rate_limit.window_seconds = window_int;
                    }
                }
                
                if let Some(action) = rate_limiter.get("default_action") {
                    if let Some(action_str) = action.as_str() {
                        rate_limit.default_action = Some(action_str.to_string());
                    }
                }
                
                if let Some(code) = rate_limiter.get("reject_status_code") {
                    if let Some(code_int) = code.as_u64() {
                        rate_limit.reject_status_code = Some(code_int as u16);
                    }
                }
                
                if let Some(message) = rate_limiter.get("reject_message") {
                    if let Some(message_str) = message.as_str() {
                        rate_limit.reject_message = Some(message_str.to_string());
                    }
                }
                
                // Xử lý headers
                if let Some(include_headers) = rate_limiter.get("include_headers") {
                    if let Some(include_bool) = include_headers.as_bool() {
                        rate_limit.include_headers = Some(include_bool);
                    }
                }
                
                if let Some(remaining_header) = rate_limiter.get("remaining_header") {
                    if let Some(header_str) = remaining_header.as_str() {
                        rate_limit.remaining_header = Some(header_str.to_string());
                    }
                }
                
                if let Some(limit_header) = rate_limiter.get("limit_header") {
                    if let Some(header_str) = limit_header.as_str() {
                        rate_limit.limit_header = Some(header_str.to_string());
                    }
                }
                
                if let Some(reset_header) = rate_limiter.get("reset_header") {
                    if let Some(header_str) = reset_header.as_str() {
                        rate_limit.reset_header = Some(header_str.to_string());
                    }
                }
                
                // Xử lý paths
                if let Some(paths) = rate_limiter.get("paths") {
                    if let Some(paths_array) = paths.as_sequence() {
                        let mut path_configs = Vec::new();
                        
                        for path in paths_array {
                            if let (Some(path_str), Some(algorithm), Some(limit), Some(window), Some(action)) = (
                                path.get("path").and_then(|p| p.as_str()),
                                path.get("algorithm").and_then(|a| a.as_str()),
                                path.get("limit").and_then(|l| l.as_u64()),
                                path.get("window").and_then(|w| w.as_u64()),
                                path.get("action").and_then(|a| a.as_str())
                            ) {
                                path_configs.push(RateLimitPath {
                                    path: path_str.to_string(),
                                    algorithm: algorithm.to_string(),
                                    limit: limit as u32,
                                    window,
                                    action: action.to_string(),
                                });
                            }
                        }
                        
                        if !path_configs.is_empty() {
                            rate_limit.paths = Some(path_configs);
                        }
                    }
                }
                
                // Xử lý identifier
                if let Some(identifier) = rate_limiter.get("identifier") {
                    if let (
                        Some(use_ip), 
                        Some(use_api_key), 
                        Some(use_user_id), 
                        Some(trusted_proxy_ips),
                        Some(header_for_real_ip)
                    ) = (
                        identifier.get("use_ip").and_then(|v| v.as_bool()),
                        identifier.get("use_api_key").and_then(|v| v.as_bool()),
                        identifier.get("use_user_id").and_then(|v| v.as_bool()),
                        identifier.get("trusted_proxy_ips").and_then(|v| v.as_sequence()),
                        identifier.get("header_for_real_ip").and_then(|v| v.as_str())
                    ) {
                        let mut proxy_ips = Vec::new();
                        for ip in trusted_proxy_ips {
                            if let Some(ip_str) = ip.as_str() {
                                proxy_ips.push(ip_str.to_string());
                            }
                        }
                        
                        rate_limit.identifier = Some(RateLimitIdentifier {
                            use_ip,
                            use_api_key,
                            use_user_id,
                            trusted_proxy_ips: proxy_ips,
                            header_for_real_ip: header_for_real_ip.to_string(),
                        });
                    }
                }
                
                // Xử lý storage
                if let Some(storage) = rate_limiter.get("storage") {
                    if let (
                        Some(type_str), 
                        Some(cleanup_interval)
                    ) = (
                        storage.get("type").and_then(|v| v.as_str()),
                        storage.get("cleanup_interval").and_then(|v| v.as_u64())
                    ) {
                        let redis_url = storage.get("redis_url").and_then(|v| v.as_str()).map(|s| s.to_string());
                        let redis_prefix = storage.get("redis_prefix").and_then(|v| v.as_str()).map(|s| s.to_string());
                        
                        rate_limit.storage = Some(RateLimitStorage {
                            type_: type_str.to_string(),
                            cleanup_interval,
                            redis_url,
                            redis_prefix,
                        });
                    }
                }
                
                config.security.rate_limit = rate_limit;
            }
        }
        
        info!("Security configurations loaded from YAML files");
        Ok(config)
    }
    
    /// Tải cấu hình từ tất cả các nguồn (file, env, yaml)
    pub fn load_all(&self, config_path: &str) -> Result<NetworkConfig, ConfigError> {
        debug!("Loading configuration from all sources");
        
        // Tải từ file chính
        let mut config = if Path::new(config_path).exists() {
            self.load_from_file(config_path)?
        } else {
            info!("Main config file not found, using default");
            NetworkConfig::default()
        };
        
        // Tải từ biến môi trường
        if self.options.load_env {
            // Ghi đè các giá trị đã được cấu hình từ env
            self.override_from_env(&mut config)?;
        }
        
        // Tải từ các file YAML bảo mật
        let security_config = self.load_security_configs()?;
        
        // Ghi đè cấu hình bảo mật với ưu tiên cho file trong YAML
        if security_config.security.auth.is_some() {
            info!("Applying auth configuration from YAML");
            config.security.auth = security_config.security.auth;
            // Tự động bật xác thực nếu có cấu hình từ file YAML
            config.security.enable_auth = true;
        }
        
        if security_config.security.input_validation.is_some() {
            info!("Applying input validation configuration from YAML");
            config.security.input_validation = security_config.security.input_validation;
        }
        
        // Ghi đè rate limit từ file YAML nếu có
        if let Some(paths) = &security_config.security.rate_limit.paths {
            if !paths.is_empty() {
                info!("Applying rate limit configuration from YAML");
                config.security.rate_limit.paths = Some(paths.clone());
                // Bật rate limit khi có cấu hình đường dẫn
                config.security.rate_limit.enabled = true;
            }
        }
        
        if security_config.security.rate_limit.identifier.is_some() {
            config.security.rate_limit.identifier = security_config.security.rate_limit.identifier.clone();
        }
        
        if security_config.security.rate_limit.storage.is_some() {
            config.security.rate_limit.storage = security_config.security.rate_limit.storage.clone();
        }
        
        // Cấu hình thông báo rate limit
        if let Some(message) = &security_config.security.rate_limit.reject_message {
            if !message.is_empty() {
                config.security.rate_limit.reject_message = Some(message.clone());
            }
        }
        
        // Cấu hình thuật toán rate limit
        if let Some(algorithm) = &security_config.security.rate_limit.default_algorithm {
            if !algorithm.is_empty() {
                config.security.rate_limit.default_algorithm = Some(algorithm.clone());
            }
        }
        
        // Xử lý các cấu hình header
        if let Some(include_headers) = security_config.security.rate_limit.include_headers {
            config.security.rate_limit.include_headers = Some(include_headers);
        }
        
        if let Some(header) = &security_config.security.rate_limit.remaining_header {
            if !header.is_empty() {
                config.security.rate_limit.remaining_header = Some(header.clone());
            }
        }
        
        if let Some(header) = &security_config.security.rate_limit.limit_header {
            if !header.is_empty() {
                config.security.rate_limit.limit_header = Some(header.clone());
            }
        }
        
        if let Some(header) = &security_config.security.rate_limit.reset_header {
            if !header.is_empty() {
                config.security.rate_limit.reset_header = Some(header.clone());
            }
        }
        
        // Xác thực cấu hình
        config.validate()?;
        
        info!("Configuration loaded from all sources");
        Ok(config)
    }
} 