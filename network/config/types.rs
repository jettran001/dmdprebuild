//! Configuration Types
//!
//! Module này cung cấp các type cấu hình cơ bản cho các plugin và dịch vụ.
//! Module này thay thế network/infra/config_types.rs.

use serde::{Serialize, Deserialize};
use tracing::{warn, error, debug};
use crate::config::error::ConfigError;

/// Redis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    pub url: String,
    pub enable_tls: bool,
    pub tls_cert_path: Option<String>,
    pub tls_key_path: Option<String>,
    pub acl_file: Option<String>,
    pub pool_size: u32,
    pub min_pool_size: Option<u32>,
    pub max_idle_connections: Option<u32>,
    pub eviction_policy: Option<String>,
    pub enable_exporter: bool,
    pub max_retries: u8,
    pub retry_delay_ms: u64,
    pub username: Option<String>,
    pub password: Option<String>,
    pub replication_mode: Option<String>,
    pub persistence_mode: Option<String>,
}

impl RedisConfig {
    /// Validates Redis configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        debug!("[RedisConfig] Validating Redis configuration");
        
        if self.url.is_empty() {
            error!("[RedisConfig] Redis URL is empty");
            return Err(ConfigError::MissingField("url".to_string()));
        }
        
        if self.enable_tls {
            debug!("[RedisConfig] TLS is enabled, checking certificates");
            if self.tls_cert_path.is_none() {
                warn!("[RedisConfig] TLS is enabled but no certificate path provided");
                return Err(ConfigError::MissingField("tls_cert_path".to_string()));
            }
            
            if self.tls_key_path.is_none() {
                warn!("[RedisConfig] TLS is enabled but no key path provided");
                return Err(ConfigError::MissingField("tls_key_path".to_string()));
            }
        }
        
        if self.pool_size == 0 {
            warn!("[RedisConfig] Pool size is 0, will use default value of 10");
        }
        
        debug!("[RedisConfig] Redis configuration is valid");
        Ok(())
    }
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            enable_tls: false,
            tls_cert_path: None,
            tls_key_path: None,
            acl_file: None,
            pool_size: 10,
            min_pool_size: None,
            max_idle_connections: None,
            eviction_policy: None,
            enable_exporter: false,
            max_retries: 3,
            retry_delay_ms: 500,
            username: None,
            password: None,
            replication_mode: None,
            persistence_mode: None,
        }
    }
}

/// IPFS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpfsConfig {
    pub url: String,
    pub enable_tls: bool,
    pub private_network: bool,
    pub swarm_key_path: Option<String>,
    pub allowlist: Option<Vec<String>>,
    pub pinning_strategy: Option<String>,
    pub gc_interval_secs: Option<u64>,
    pub repo_size_limit_mb: Option<u64>,
    pub dht_mode: Option<String>,
    pub bootstrap_nodes: Option<Vec<String>>,
    pub datastore_backend: Option<String>,
    pub content_routing: Option<String>,
    pub enable_exporter: bool,
    pub gateway: String,
    pub run_local_node: bool,
}

impl IpfsConfig {
    /// Validates IPFS configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        debug!("[IpfsConfig] Validating IPFS configuration");
        
        if self.url.is_empty() {
            error!("[IpfsConfig] IPFS URL is empty");
            return Err(ConfigError::MissingField("url".to_string()));
        }
        
        if self.private_network && self.swarm_key_path.is_none() {
            warn!("[IpfsConfig] Private network is enabled but no swarm key path provided");
            return Err(ConfigError::MissingField("swarm_key_path".to_string()));
        }
        
        if let Some(pinning) = &self.pinning_strategy {
            if !["direct", "recursive", "none"].contains(&pinning.as_str()) {
                warn!("[IpfsConfig] Invalid pinning strategy: {}", pinning);
                return Err(ConfigError::InvalidValue(format!("Invalid pinning strategy: {}", pinning)));
            }
        }
        
        if let Some(dht) = &self.dht_mode {
            if !["client", "server", "auto"].contains(&dht.as_str()) {
                warn!("[IpfsConfig] Invalid DHT mode: {}", dht);
                return Err(ConfigError::InvalidValue(format!("Invalid DHT mode: {}", dht)));
            }
        }
        
        debug!("[IpfsConfig] IPFS configuration is valid");
        Ok(())
    }
}

impl Default for IpfsConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            enable_tls: false,
            private_network: false,
            swarm_key_path: None,
            allowlist: None,
            pinning_strategy: Some("recursive".to_string()),
            gc_interval_secs: Some(3600),
            repo_size_limit_mb: Some(10240),
            dht_mode: Some("auto".to_string()),
            bootstrap_nodes: None,
            datastore_backend: None,
            content_routing: None,
            enable_exporter: false,
            gateway: String::new(),
            run_local_node: false,
        }
    }
}

/// WASM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmConfig {
    pub modules_dir: String,
    pub memory_limit: u32,
    pub execution_timeout: u64,
}

impl WasmConfig {
    /// Validates WASM configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        debug!("[WasmConfig] Validating WASM configuration");
        
        if self.modules_dir.is_empty() {
            error!("[WasmConfig] WASM modules directory is empty");
            return Err(ConfigError::MissingField("modules_dir".to_string()));
        }
        
        if self.memory_limit == 0 {
            warn!("[WasmConfig] Memory limit is 0, this may cause issues with WASM execution");
        }
        
        if self.execution_timeout == 0 {
            warn!("[WasmConfig] Execution timeout is 0, WASM modules will run without time limits");
        }
        
        debug!("[WasmConfig] WASM configuration is valid");
        Ok(())
    }
}

impl Default for WasmConfig {
    fn default() -> Self {
        Self {
            modules_dir: "modules".to_string(),
            memory_limit: 1024,
            execution_timeout: 60000,
        }
    }
}

/// WebRTC configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebrtcConfig {
    pub stun_servers: Vec<String>,
    pub turn_servers: Vec<String>,
    pub credentials: Option<String>,
    pub enable_data_channels: bool,
    pub max_packet_size: usize,
    pub max_concurrent_connections: usize,
}

impl WebrtcConfig {
    /// Validates WebRTC configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        debug!("[WebrtcConfig] Validating WebRTC configuration");
        
        if self.stun_servers.is_empty() && self.turn_servers.is_empty() {
            warn!("[WebrtcConfig] No STUN or TURN servers provided, NAT traversal may fail");
        }
        
        if !self.turn_servers.is_empty() && self.credentials.is_none() {
            warn!("[WebrtcConfig] TURN servers provided but no credentials, authentication will fail");
            return Err(ConfigError::MissingField("credentials".to_string()));
        }
        
        for server in &self.stun_servers {
            if !server.starts_with("stun:") && !server.starts_with("stuns:") {
                warn!("[WebrtcConfig] Invalid STUN server URL format: {}", server);
                return Err(ConfigError::InvalidValue(format!("Invalid STUN server URL format: {}", server)));
            }
        }
        
        for server in &self.turn_servers {
            if !server.starts_with("turn:") && !server.starts_with("turns:") {
                warn!("[WebrtcConfig] Invalid TURN server URL format: {}", server);
                return Err(ConfigError::InvalidValue(format!("Invalid TURN server URL format: {}", server)));
            }
        }
        
        if self.max_packet_size == 0 {
            warn!("[WebrtcConfig] Max packet size is 0, this may cause issues with WebRTC");
        }
        
        if self.max_concurrent_connections == 0 {
            warn!("[WebrtcConfig] Max concurrent connections is 0, this will prevent WebRTC connections");
        }
        
        debug!("[WebrtcConfig] WebRTC configuration is valid");
        Ok(())
    }
}

impl Default for WebrtcConfig {
    fn default() -> Self {
        Self {
            stun_servers: vec!["stun:stun.l.google.com:19302".to_string()],
            turn_servers: vec![],
            credentials: None,
            enable_data_channels: true,
            max_packet_size: 16384,
            max_concurrent_connections: 100,
        }
    }
}

/// Libp2p configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Libp2pConfig {
    pub port: u16,
    pub bootstrap_nodes: Vec<String>,
    pub protocol_version: String,
    pub dht_enabled: bool,
}

impl Libp2pConfig {
    /// Validates Libp2p configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        debug!("[Libp2pConfig] Validating Libp2p configuration");
        
        if self.port == 0 {
            warn!("[Libp2pConfig] Port is 0, a random port will be assigned");
        }
        
        if self.protocol_version.is_empty() {
            error!("[Libp2pConfig] Protocol version is empty");
            return Err(ConfigError::MissingField("protocol_version".to_string()));
        }
        
        debug!("[Libp2pConfig] Libp2p configuration is valid");
        Ok(())
    }
}

impl Default for Libp2pConfig {
    fn default() -> Self {
        Self {
            port: 0,
            bootstrap_nodes: vec![],
            protocol_version: "1.0.0".to_string(),
            dht_enabled: true,
        }
    }
}

/// gRPC configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrpcConfig {
    pub port: u16,
    pub max_message_size: usize,
    pub keep_alive_interval: u64,
}

impl GrpcConfig {
    /// Validates gRPC configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        debug!("[GrpcConfig] Validating gRPC configuration");
        
        if self.port == 0 {
            warn!("[GrpcConfig] Port is 0, a random port will be assigned");
        }
        
        if self.max_message_size == 0 {
            warn!("[GrpcConfig] Max message size is 0, this may cause issues with gRPC");
        }
        
        debug!("[GrpcConfig] gRPC configuration is valid");
        Ok(())
    }
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            port: 50051,
            max_message_size: 4 * 1024 * 1024, // 4MB
            keep_alive_interval: 60,
        }
    }
} 