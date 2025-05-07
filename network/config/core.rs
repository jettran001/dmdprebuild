//! Core Configuration 
//!
//! Module này định nghĩa cấu hình cho network core engine.
//! Chuyển từ network/core/config.rs.

use serde::{Deserialize, Serialize};
use tracing::{debug, warn, error};
use crate::config::error::ConfigError;
use crate::core::types::{NetworkDiscovery, NetworkSecurity, NetworkProtocol, NetworkCapability};

/// NodeType định nghĩa loại node
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeType {
    DePIN,
    Master,
    Slave,
    Discovery,
    Scheduler,
}

/// NetworkCoreConfig: Cấu hình engine core
/// 
/// Đây là phiên bản hợp nhất và chuẩn hóa từ NetworkConfig trong module core
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkCoreConfig {
    /// ID của node
    pub node_id: String,
    
    /// Loại node
    pub node_type: NodeType,
    
    /// ID của network
    pub network_id: String,
    
    /// Địa chỉ lắng nghe
    pub listen_address: String,
    
    /// Địa chỉ máy chủ (IP hoặc hostname)
    pub host: String,
    
    /// Port để lắng nghe kết nối
    pub port: u16,
    
    /// Các node bootstrap
    pub bootstrap_nodes: Vec<String>,
    
    /// Số lượng peer tối đa
    pub max_peers: usize,
    
    /// Bật/tắt discovery
    pub enable_discovery: bool,
    
    /// Cấu hình bảo mật
    pub security: NetworkSecurity,
    
    /// Cấu hình discovery
    pub discovery: NetworkDiscovery,
    
    /// Các giao thức được hỗ trợ
    pub protocols: Vec<NetworkProtocol>,
    
    /// Các khả năng của node
    pub capabilities: Vec<NetworkCapability>,
    
    /// Bật/tắt metrics
    pub enable_metrics: bool,
    
    /// Timeout kết nối tính bằng giây
    pub connection_timeout: u64,
    
    /// Số lượng kết nối đồng thời tối đa
    pub max_connections: usize,
    
    /// Số lượng worker thread (0 = sử dụng mặc định của hệ thống)
    pub workers: usize,
    
    /// Bật/tắt chế độ debug
    pub debug: bool,
}

impl NetworkCoreConfig {
    /// Tạo mới một đối tượng NetworkCoreConfig mặc định
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Kiểm tra tính hợp lệ của cấu hình
    pub fn validate(&self) -> Result<(), ConfigError> {
        debug!("Validating core network configuration");
        
        if self.node_id.is_empty() {
            return Err(ConfigError::MissingField("node_id".to_string()));
        }
        
        if self.network_id.is_empty() {
            return Err(ConfigError::MissingField("network_id".to_string()));
        }
        
        if self.listen_address.is_empty() {
            return Err(ConfigError::MissingField("listen_address".to_string()));
        }
        
        if self.max_peers == 0 {
            warn!("max_peers is 0, this may cause connection issues");
        }
        
        if self.connection_timeout == 0 {
            warn!("connection_timeout is 0, this may cause timeout issues");
        }
        
        if self.protocols.is_empty() {
            warn!("No protocols configured, node may not be able to communicate");
        }
        
        debug!("Core network configuration is valid");
        Ok(())
    }
}

impl Default for NetworkCoreConfig {
    fn default() -> Self {
        Self {
            node_id: String::new(),
            node_type: NodeType::DePIN,
            network_id: "diamondchain".to_string(),
            listen_address: "/ip4/0.0.0.0/tcp/0".to_string(),
            host: "127.0.0.1".to_string(),
            port: 8080,
            bootstrap_nodes: vec![],
            max_peers: 50,
            enable_discovery: true,
            security: NetworkSecurity {
                enable_encryption: true,
                enable_authentication: true,
                enable_authorization: true,
                allowed_peers: vec![],
                blocked_peers: vec![],
            },
            discovery: NetworkDiscovery {
                enable_mdns: true,
                enable_kademlia: true,
                bootstrap_nodes: vec![],
                discovery_interval: std::time::Duration::from_secs(60),
                max_peers: 50,
            },
            protocols: vec![
                NetworkProtocol::LibP2P,
                NetworkProtocol::gRPC,
                NetworkProtocol::MQTT,
            ],
            capabilities: vec![
                NetworkCapability::Storage,
                NetworkCapability::Compute,
                NetworkCapability::Routing,
            ],
            enable_metrics: true,
            connection_timeout: 30,
            max_connections: 100,
            workers: 0,
            debug: false,
        }
    }
} 