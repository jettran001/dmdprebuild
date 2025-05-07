use crate::core::engine::{Plugin, PluginType, PluginError};
use std::env;
use crate::security::input_validation::security;
use std::error::Error;
use std::time::Duration;
use tokio::sync::{Mutex, oneshot};
use std::sync::Arc;
use tracing::{debug, info, warn, error};
use async_trait::async_trait;
use thiserror::Error;
use rand;
use std::time::SystemTime;
use std::collections::HashMap;
use regex;
use crate::infra::service_traits::{WebrtcService as InfraWebrtcService, WebrtcConfig, ServiceError};

#[derive(Error, Debug)]
pub enum WebRtcError {
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    #[error("Validation error: {0}")]
    ValidationError(String),
    
    #[error("Session error: {0}")]
    SessionError(String),
    
    #[error("Data channel error: {0}")]
    DataChannelError(String),
    
    #[error("Signal error: {0}")]
    SignalError(String),
    
    #[error("Shutdown error: {0}")]
    ShutdownError(String),
}

// Thêm constants cho các giá trị mặc định thay vì hardcoded values
const DEFAULT_STUN_SERVER: &str = "stun:stun.l.google.com:19302";
const DEFAULT_CONNECTION_TIMEOUT_MS: u64 = 30000;
const DEFAULT_MAX_RETRIES: u8 = 3;
const DEFAULT_RETRY_DELAY_MS: u64 = 1000;
const DEFAULT_MAX_PACKET_SIZE: usize = 65536;
const DEFAULT_ENABLE_ENCRYPTION: bool = true;
const MAX_SDP_LENGTH: usize = 8192;
const MAX_ICE_CANDIDATE_LENGTH: usize = 1024;
// Add new constant for max connections
const DEFAULT_MAX_CONCURRENT_CONNECTIONS: usize = 100;

/// Configuration for WebRTC plugin
#[derive(Clone, Debug)]
pub struct WebRtcConfig {
    /// STUN servers for NAT traversal
    pub stun_servers: Vec<String>,
    /// TURN servers for fallback relay
    pub turn_servers: Vec<String>,
    /// Turn server username if authentication is required
    pub turn_username: Option<String>,
    /// Turn server credential if authentication is required
    pub turn_credential: Option<String>,
    /// Connection timeout in milliseconds
    pub connection_timeout_ms: u64,
    /// Max number of retries for operations
    pub max_retries: u8,
    /// Delay between retries in milliseconds
    pub retry_delay_ms: u64,
    /// Enable data encryption
    pub enable_encryption: bool,
    /// Max packet size in bytes
    pub max_packet_size: usize,
    /// Maximum number of concurrent connections to prevent socket exhaustion
    pub max_concurrent_connections: usize,
}

impl Default for WebRtcConfig {
    fn default() -> Self {
        Self {
            stun_servers: vec![DEFAULT_STUN_SERVER.to_string()],
            turn_servers: Vec::new(),
            turn_username: None,
            turn_credential: None,
            connection_timeout_ms: DEFAULT_CONNECTION_TIMEOUT_MS,
            max_retries: DEFAULT_MAX_RETRIES,
            retry_delay_ms: DEFAULT_RETRY_DELAY_MS,
            enable_encryption: DEFAULT_ENABLE_ENCRYPTION,
            max_packet_size: DEFAULT_MAX_PACKET_SIZE,
            max_concurrent_connections: DEFAULT_MAX_CONCURRENT_CONNECTIONS,
        }
    }
}

#[async_trait]
pub trait WebrtcService: Send + Sync + 'static {
    async fn init(&self) -> Result<(), ServiceError>;
    async fn create_peer_connection(&self, config: &WebrtcConfig) -> Result<String, ServiceError>;
    async fn add_ice_candidate(&self, connection_id: &str, candidate: &str) -> Result<(), ServiceError>;
    async fn send_data(&self, connection_id: &str, data: &[u8]) -> Result<(), ServiceError>;
    async fn health_check(&self) -> Result<bool, ServiceError>;
    async fn close_connection(&self, connection_id: &str) -> Result<(), ServiceError>;
    async fn close_all_connections(&self) -> Result<usize, ServiceError>;
}

/// ConnectionManager cho WebRTC, xử lý zombie connections và dọn dẹp tài nguyên
pub struct ConnectionManager {
    connections: HashMap<String, (SystemTime, HashMap<String, Vec<u8>>)>,
    timeout_secs: u64,
    max_connections: usize,
    // Thêm trường để đánh dấu các kết nối WebRTC thực đã đóng
    active_connections: HashMap<String, bool>,
}

impl ConnectionManager {
    /// Tạo Connection Manager mới với timeout cụ thể (giây)
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            connections: HashMap::new(),
            timeout_secs,
            max_connections: DEFAULT_MAX_CONCURRENT_CONNECTIONS,
            active_connections: HashMap::new(),
        }
    }
    
    /// Tạo Connection Manager với cấu hình tùy chỉnh
    pub fn with_config(timeout_secs: u64, max_connections: usize) -> Self {
        Self {
            connections: HashMap::new(),
            timeout_secs,
            max_connections,
            active_connections: HashMap::new(),
        }
    }
    
    /// Kiểm tra xem đã đạt giới hạn kết nối chưa
    pub fn is_connection_limit_reached(&self) -> bool {
        self.connections.len() >= self.max_connections
    }
    
    /// Cập nhật hoặc thêm connection mới
    pub fn update_connection(&mut self, connection_id: &str) -> Result<(), String> {
        // Kiểm tra giới hạn kết nối
        if !self.connections.contains_key(connection_id) && self.is_connection_limit_reached() {
            return Err(format!("Connection limit reached: Maximum {} connections allowed", 
                              self.max_connections));
        }
        
        let now = SystemTime::now();
        self.connections.entry(connection_id.to_string())
            .and_modify(|(last, _)| { *last = now })
            .or_insert((now, HashMap::new()));
        
        Ok(())
    }
    
    /// Kiểm tra xem connection có tồn tại và còn active không
    pub fn is_active(&self, connection_id: &str) -> bool {
        if let Some((last_active, _)) = self.connections.get(connection_id) {
            // Kiểm tra thời gian hoạt động cuối
            match last_active.elapsed() {
                Ok(elapsed) => elapsed.as_secs() <= self.timeout_secs,
                Err(_) => false, // System time error (hiếm gặp)
            }
        } else {
            false
        }
    }
    
    /// Lưu dữ liệu của connection 
    pub fn set_connection_data(&mut self, connection_id: &str, channel_id: &str, data: Vec<u8>) -> Result<(), String> {
        if let Some((_, data_map)) = self.connections.get_mut(connection_id) {
            // Update lại timestamp trước
            if let Some((timestamp, _)) = self.connections.get_mut(connection_id) {
                *timestamp = SystemTime::now();
            }
            
            // Lưu dữ liệu
            data_map.insert(channel_id.to_string(), data);
            Ok(())
        } else {
            Err(format!("Connection not found: {}", connection_id))
        }
    }
    
    /// Lấy dữ liệu của connection
    pub fn get_connection_data(&mut self, connection_id: &str, channel_id: &str) -> Option<Vec<u8>> {
        // Update lại timestamp trước để đánh dấu connection đang được sử dụng
        if let Some((timestamp, _)) = self.connections.get_mut(connection_id) {
            *timestamp = SystemTime::now();
        }
        
        // Lấy dữ liệu
        self.connections.get(connection_id)
            .and_then(|(_, data_map)| data_map.get(channel_id).cloned())
    }
    
    /// Kiểm tra và dọn dẹp các connection hết hạn (zombie connections)
    /// Trả về số lượng connection đã bị xóa
    pub fn cleanup(&mut self) -> usize {
        let before_count = self.connections.len();
        let now = SystemTime::now();
        
        // Chỉ sử dụng một lần mutable borrow để tránh lỗi "cannot borrow as mutable more than once"
        let mut expired_connections = Vec::new();
        
        // Xác định các connection đã expired
        for (conn_id, (last_active, _)) in &self.connections {
            match last_active.elapsed() {
                Ok(elapsed) if elapsed.as_secs() > self.timeout_secs => {
                    expired_connections.push(conn_id.clone());
                },
                Ok(_) => {}, // Còn active, không làm gì
                Err(_) => {  // System time error (hiếm gặp)
                    expired_connections.push(conn_id.clone());
                },
            }
        }
        
        // Xóa các connection đã expired
        for conn_id in &expired_connections {
            self.connections.remove(conn_id);
            // Đồng thời cập nhật active_connections map
            self.active_connections.remove(conn_id);
        }
        
        let removed = expired_connections.len();
        if removed > 0 {
            debug!("[WebRTC] Cleaned up {} zombie connections", removed);
        }
        
        removed
    }
    
    /// Xóa một connection cụ thể
    pub fn remove_connection(&mut self, connection_id: &str) -> bool {
        self.connections.remove(connection_id).is_some()
    }
    
    /// Trả về số lượng connections hiện tại
    pub fn connections_count(&self) -> usize {
        self.connections.len()
    }
    
    /// Trả về danh sách connection_id đang quản lý
    pub fn list_connections(&self) -> Vec<String> {
        self.connections.keys().cloned().collect()
    }
    
    /// Force đóng tất cả connections
    pub fn close_all(&mut self) -> usize {
        let count = self.connections.len();
        self.connections.clear();
        count
    }
    
    /// Set max connection limit
    pub fn set_max_connections(&mut self, max: usize) {
        self.max_connections = max;
    }
    
    /// Get max connection limit
    pub fn get_max_connections(&self) -> usize {
        self.max_connections
    }
    
    /// Đánh dấu kết nối là active
    pub fn mark_connection_active(&mut self, connection_id: &str) {
        self.active_connections.insert(connection_id.to_string(), true);
    }
    
    /// Đánh dấu kết nối là inactive (đã đóng)
    pub fn mark_connection_inactive(&mut self, connection_id: &str) {
        self.active_connections.insert(connection_id.to_string(), false);
    }
    
    /// Kiểm tra xem connection có đang active thực sự không (để đóng)
    pub fn is_connection_active(&self, connection_id: &str) -> bool {
        self.active_connections.get(connection_id).copied().unwrap_or(false)
    }
    
    /// Lấy danh sách các kết nối active
    pub fn get_active_connections(&self) -> Vec<String> {
        self.active_connections.iter()
            .filter(|(_, &active)| active)
            .map(|(id, _)| id.clone())
            .collect()
    }
}

/// DefaultWebRtcService implements WebRtcService with proper connection management to prevent memory leaks
pub struct DefaultWebRtcService {
    config: WebrtcConfig,
    // Sử dụng tokio::sync::Mutex cho async context để tránh blocking thread
    shutdown_signal: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    // Sử dụng tokio::sync::Mutex cho ConnectionManager để đảm bảo non-blocking trong async context
    connection_manager: Arc<Mutex<ConnectionManager>>,
}

impl DefaultWebRtcService {
    /// Create a new WebRTC service with default configuration
    pub fn new() -> Self {
        let service = Self {
            config: WebrtcConfig::default(),
            shutdown_signal: Arc::new(Mutex::new(None)),
            connection_manager: Arc::new(Mutex::new(ConnectionManager::new(3600))), // 1 hour timeout
        };
        
        // Spawn background cleanup task
        let conn_manager = service.connection_manager.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300)); // Run every 5 minutes
            
            loop {
                interval.tick().await;
                let mut manager = conn_manager.lock().await;
                let removed = manager.cleanup();
                if removed > 0 {
                    info!("[WebRTC] Cleaned up {} expired connections", removed);
                }
            }
        });
        
        service
    }
    
    /// Create a new WebRTC service with custom configuration
    pub fn with_config(config: WebrtcConfig) -> Self {
        let service = Self {
            config: config.clone(),
            shutdown_signal: Arc::new(Mutex::new(None)),
            connection_manager: Arc::new(Mutex::new(ConnectionManager::with_config(
                3600, // 1 hour timeout
                config.max_concurrent_connections
            ))),
        };
        
        // Spawn background cleanup task
        let conn_manager = service.connection_manager.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300)); // Run every 5 minutes
            
            loop {
                interval.tick().await;
                let mut manager = conn_manager.lock().await;
                let removed = manager.cleanup();
                if removed > 0 {
                    info!("[WebRTC] Cleaned up {} expired connections", removed);
                }
            }
        });
        
        service
    }
    
    /// Shutdown the service
    pub async fn shutdown(&self) -> Result<(), WebRtcError> {
        let mut lock = self.shutdown_signal.lock().await;
        if let Some(tx) = lock.take() {
            if let Err(e) = tx.send(()) {
                return Err(WebRtcError::ShutdownError(
                    format!("Failed to send shutdown signal: {:?}", e)
                ));
            }
        }
        
        debug!("[WebRTC] Service shutdown complete");
        Ok(())
    }
}

#[async_trait]
impl WebrtcService for DefaultWebRtcService {
    async fn init(&self) -> Result<(), ServiceError> {
        Ok(())
    }
    
    async fn create_peer_connection(&self, config: &WebrtcConfig) -> Result<String, ServiceError> {
        let connection_id = format!("conn-{}", uuid::Uuid::new_v4());
        
        // Connect to STUN/TURN servers
        debug!("Creating WebRTC peer connection with config: {:?}", config);
        
        // Check if connection limit is reached
        let mut conn_mgr = self.connection_manager.lock().await;
        if conn_mgr.is_connection_limit_reached() {
            error!("Maximum connections limit reached: {}", conn_mgr.get_max_connections());
            return Err(ServiceError::ConnectionError(
                format!("Connection limit reached: {}", conn_mgr.get_max_connections())
            ));
        }
        
        // Add the new connection to the manager
        if let Err(e) = conn_mgr.update_connection(&connection_id) {
            error!("Failed to register connection: {}", e);
            return Err(ServiceError::ConnectionError(e));
        }
        
        // Mark the connection as active
        conn_mgr.mark_connection_active(&connection_id);
        
        // Log successful connection
        info!("WebRTC peer connection created: {}", connection_id);
        
        Ok(connection_id)
    }
    
    async fn add_ice_candidate(&self, connection_id: &str, candidate: &str) -> Result<(), ServiceError> {
        // Security: Validate input to prevent XSS, SQLi, command injection, etc.
        if let Err(e) = security::check_xss(connection_id, "webrtc_connection_id") {
            return Err(ServiceError::ValidationError(e.to_string()));
        }
        
        if let Err(e) = security::check_xss(candidate, "webrtc_ice_candidate") {
            return Err(ServiceError::ValidationError(e.to_string()));
        }
        
        // Additional security: Validate ICE candidate format
        if let Err(e) = validate_ice_candidate(candidate) {
            return Err(ServiceError::ValidationError(e.to_string()));
        }
        
        // Update connection activity time
        let mut conn_mgr = self.connection_manager.lock().await;
        if !conn_mgr.is_active(connection_id) {
            return Err(ServiceError::SessionError(format!("Connection not active or expired: {}", connection_id)));
        }
        
        // Update the connection
        if let Err(e) = conn_mgr.update_connection(connection_id) {
            return Err(ServiceError::SessionError(e));
        }
        
        // Store candidate (mock - in real implementation would pass to WebRTC)
        conn_mgr.set_connection_data(connection_id, "ice_candidate", candidate.as_bytes().to_vec())
            .map_err(|e| ServiceError::SessionError(e))?;
        
        debug!("Added ICE candidate for connection {}", connection_id);
        Ok(())
    }
    
    async fn send_data(&self, connection_id: &str, data: &[u8]) -> Result<(), ServiceError> {
        // Security: Validate connection_id
        if let Err(e) = security::check_xss(connection_id, "webrtc_connection_id") {
            return Err(ServiceError::ValidationError(e.to_string()));
        }
        
        // Security: Check data size
        if data.len() > self.config.max_packet_size {
            return Err(ServiceError::DataChannelError(
                format!("Data size exceeds maximum allowed: {} > {}", 
                        data.len(), self.config.max_packet_size)
            ));
        }
        
        // Update connection activity time and check if active
        let mut conn_mgr = self.connection_manager.lock().await;
        if !conn_mgr.is_active(connection_id) {
            return Err(ServiceError::SessionError(
                format!("Connection not active or expired: {}", connection_id)
            ));
        }
        
        // Update the connection
        if let Err(e) = conn_mgr.update_connection(connection_id) {
            return Err(ServiceError::SessionError(e));
        }
        
        debug!("Sent {} bytes on connection {}", data.len(), connection_id);
        Ok(())
    }
    
    async fn health_check(&self) -> Result<bool, ServiceError> {
        Ok(true)
    }
    
    async fn close_connection(&self, connection_id: &str) -> Result<(), ServiceError> {
        // Validate connection_id
        if let Err(e) = security::check_xss(connection_id, "webrtc_connection_id") {
            return Err(ServiceError::ValidationError(e.to_string()));
        }
        
        // Lock connection manager and remove the connection
        let mut conn_mgr = self.connection_manager.lock().await;
        if !conn_mgr.is_active(connection_id) {
            // If already inactive, just log a warning
            warn!("Attempted to close already inactive connection: {}", connection_id);
            return Ok(());
        }
        
        // Mark connection as inactive first
        conn_mgr.mark_connection_inactive(connection_id);
        
        // Then remove it
        if !conn_mgr.remove_connection(connection_id) {
            warn!("Connection not found for removal: {}", connection_id);
            return Err(ServiceError::SessionError(
                format!("Connection not found: {}", connection_id)
            ));
        }
        
        info!("WebRTC connection closed: {}", connection_id);
        Ok(())
    }
    
    async fn close_all_connections(&self) -> Result<usize, ServiceError> {
        let mut conn_mgr = self.connection_manager.lock().await;
        let count = conn_mgr.close_all();
        
        info!("Closed all WebRTC connections: {}", count);
        Ok(count)
    }
}

pub struct WebRtcPlugin {
    name: String,
    service: Arc<dyn WebrtcService>,
    config: WebrtcConfig,
    // Sử dụng tokio::sync::Mutex để tránh blocking trong async context
    // Không sử dụng std::sync::Mutex vì sẽ gây blocking khi lock trong async functions
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    // Sử dụng tokio::sync::Mutex để tránh blocking trong async context
    background_tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl Drop for WebRtcPlugin {
    fn drop(&mut self) {
        // Đảm bảo tất cả kết nối được đóng khi plugin bị drop
        let service = self.service.clone();
        
        // Chạy task đóng tất cả các kết nối
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            // Đã có runtime, tạo blocking task
            rt.block_on(async {
                if let Err(e) = service.close_all_connections().await {
                    error!("[WebRTC] Error closing connections during shutdown: {}", e);
                }
            });
        } else {
            // Không có runtime, log warning
            warn!("[WebRTC] Cannot close connections: no runtime available");
        }
        
        // Đảm bảo tất cả background tasks được cleanup khi plugin bị drop
        // Sử dụng try_lock thay vì lock vì lock trong drop có thể gây deadlock nếu đang bị lock ở nơi khác
        if let Ok(mut tasks) = self.background_tasks.try_lock() {
            for task in tasks.drain(..) {
                task.abort();
            }
        }
        
        // Gửi shutdown signal
        // Sử dụng try_lock thay vì lock vì lock trong drop có thể gây deadlock nếu đang bị lock ở nơi khác
        if let Ok(mut shutdown_sender) = self.shutdown_tx.try_lock() {
            if let Some(sender) = shutdown_sender.take() {
                let _ = sender.send(());
            }
        }
        
        info!("[WebRTC] Plugin dropped and cleaned up");
    }
}

impl WebRtcPlugin {
    pub fn new() -> Self {
        warn!("[WebRTC] Chạy logic mock, chưa có triển khai thực tế! Bắt buộc update khi mang lên production.");
        
        Self {
            name: "webrtc".to_string(),
            service: Arc::new(DefaultWebRtcService::new()) as Arc<dyn WebrtcService>,
            config: WebrtcConfig::default(),
            shutdown_tx: Mutex::new(None),
            background_tasks: Mutex::new(Vec::new()),
        }
    }
    
    /// Create a new WebRTC plugin with custom configuration
    pub fn with_config(config: WebrtcConfig) -> Self {
        Self {
            name: "webrtc".to_string(),
            service: Arc::new(DefaultWebRtcService::with_config(config.clone())) as Arc<dyn WebrtcService>,
            config,
            shutdown_tx: Mutex::new(None),
            background_tasks: Mutex::new(Vec::new()),
        }
    }
    
    /// Validate input nếu nhận từ nguồn bên ngoài
    pub fn validate_input(&self, input: &str) -> Result<(), String> {
        let enable = env::var("PLUGIN_INPUT_VALIDATION").unwrap_or_else(|_| "on".to_string());
        if enable == "off" {
            return Ok(());
        }
        security::check_xss(input, "webrtc_input").map_err(|e| e.to_string())?;
        security::check_sql_injection(input, "webrtc_input").map_err(|e| e.to_string())?;
        Ok(())
    }
    
    /// Create a new peer connection
    pub async fn create_peer_connection(&self) -> Result<String, String> {
        self.service.create_peer_connection(&self.config).await
            .map_err(|e| format!("Failed to create peer connection: {}", e))
    }
    
    /// Create an offer to initiate connection
    pub async fn create_offer(&self, connection_id: &str) -> Result<String, String> {
        if let Err(e) = self.validate_input(connection_id) {
            return Err(format!("Invalid connection ID: {}", e));
        }
        
        self.service.create_peer_connection(&self.config).await
            .map_err(|e| format!("Failed to create peer connection: {}", e))
    }
    
    /// Set remote description (answer from peer)
    pub async fn set_remote_description(&self, connection_id: &str, sdp: &str) -> Result<(), String> {
        if let Err(e) = self.validate_input(connection_id) {
            return Err(format!("Invalid connection ID: {}", e));
        }
        if let Err(e) = self.validate_input(sdp) {
            return Err(format!("Invalid SDP: {}", e));
        }
        
        self.service.add_ice_candidate(connection_id, sdp).await
            .map_err(|e| format!("Failed to set remote description: {}", e))
    }
    
    /// Add ICE candidate
    pub async fn add_ice_candidate(&self, connection_id: &str, candidate: &str) -> Result<(), String> {
        if let Err(e) = self.validate_input(connection_id) {
            return Err(format!("Invalid connection ID: {}", e));
        }
        if let Err(e) = self.validate_input(candidate) {
            return Err(format!("Invalid ICE candidate: {}", e));
        }
        
        self.service.add_ice_candidate(connection_id, candidate).await
            .map_err(|e| format!("Failed to add ICE candidate: {}", e))
    }
    
    /// Create a data channel
    pub async fn create_data_channel(&self, connection_id: &str, label: &str) -> Result<String, String> {
        if let Err(e) = self.validate_input(connection_id) {
            return Err(format!("Invalid connection ID: {}", e));
        }
        if let Err(e) = self.validate_input(label) {
            return Err(format!("Invalid channel label: {}", e));
        }
        
        self.service.create_peer_connection(&self.config).await
            .map_err(|e| format!("Failed to create peer connection: {}", e))
    }
    
    /// Send data through a data channel
    pub async fn send_data(&self, connection_id: &str, data: &[u8]) -> Result<(), String> {
        if let Err(e) = self.validate_input(connection_id) {
            return Err(format!("Invalid connection ID: {}", e));
        }
        
        self.service.send_data(connection_id, data).await
            .map_err(|e| format!("Failed to send data: {}", e))
    }
    
    /// Start signal handler
    pub async fn start_signal_handler(&self) -> Result<oneshot::Sender<()>, String> {
        self.service.health_check().await
            .map_err(|e| format!("Failed to start signal handler: {}", e))
    }

    fn check_health(&self) -> Result<bool, PluginError> {
        Ok(true)
    }

    /// Đóng kết nối WebRTC
    pub async fn close_connection(&self, connection_id: &str) -> Result<(), String> {
        if let Err(e) = self.validate_input(connection_id) {
            return Err(format!("Invalid connection ID: {}", e));
        }
        
        self.service.close_connection(connection_id).await
            .map_err(|e| format!("Failed to close connection: {}", e))
    }
    
    /// Đóng tất cả các kết nối WebRTC
    pub async fn close_all_connections(&self) -> Result<usize, String> {
        self.service.close_all_connections().await
            .map_err(|e| format!("Failed to close all connections: {}", e))
    }
}

impl Plugin for WebRtcPlugin {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn plugin_type(&self) -> PluginType {
        PluginType::Webrtc
    }
    
    fn start(&self) -> Result<bool, PluginError> {
        if self.name.is_empty() {
            return Err(PluginError::Other("Plugin name is empty".to_string()));
        }
        // Logic khởi tạo plugin (nếu cần async, spawn task nội bộ)
        Ok(true)
    }
    
    fn stop(&self) -> Result<(), PluginError> {
        // Logic dừng plugin (nếu cần async, spawn task nội bộ)
        Ok(())
    }
    
    fn check_health(&self) -> Result<bool, PluginError> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_webrtc_plugin_start_stop() {
        let plugin = WebRtcPlugin::new();
        assert_eq!(plugin.name(), "webrtc");
        assert_eq!(plugin.plugin_type(), PluginType::Webrtc);
        assert_eq!(plugin.start().unwrap(), true);
        assert!(plugin.stop().is_ok());
    }
    
    #[tokio::test]
    async fn test_webrtc_plugin_create_peer_connection() {
        let plugin = WebRtcPlugin::new();
        let connection_id = plugin.create_peer_connection().await.unwrap();
        assert!(!connection_id.is_empty());
    }
    
    #[tokio::test]
    async fn test_webrtc_plugin_create_offer() {
        let plugin = WebRtcPlugin::new();
        let connection_id = plugin.create_peer_connection().await.unwrap();
        let offer = plugin.create_offer(&connection_id).await.unwrap();
        assert!(offer.starts_with("v=0"));
        assert!(offer.contains(&connection_id));
    }
    
    #[tokio::test]
    async fn test_webrtc_plugin_input_validation() {
        let plugin = WebRtcPlugin::new();
        // XSS attempt
        assert!(plugin.create_offer("<script>alert('XSS')</script>").await.is_err());
    }
    
    #[tokio::test]
    async fn test_webrtc_plugin_data_channel() {
        let plugin = WebRtcPlugin::new();
        let connection_id = plugin.create_peer_connection().await.unwrap();
        let channel_id = plugin.create_data_channel(&connection_id, "test").await.unwrap();
        assert!(!channel_id.is_empty());
        let data = b"Hello, WebRTC!";
        assert!(plugin.send_data(&connection_id, data).await.is_ok());
    }
    
    #[tokio::test]
    async fn test_webrtc_plugin_retry_limit() {
        // Patch: override service to always fail
        struct AlwaysFailService;
        #[async_trait::async_trait]
        impl WebrtcService for AlwaysFailService {
            async fn init(&self) -> Result<(), ServiceError> { Err(ServiceError::ConnectionError("fail".to_string())) }
            async fn create_peer_connection(&self, _c: &WebrtcConfig) -> Result<String, ServiceError> { Err(ServiceError::ConnectionError("fail".to_string())) }
            async fn add_ice_candidate(&self, _c: &str, _cand: &str) -> Result<(), ServiceError> { Err(ServiceError::SessionError("fail".to_string())) }
            async fn send_data(&self, _c: &str, _d: &[u8]) -> Result<(), ServiceError> { Err(ServiceError::DataChannelError("fail".to_string())) }
            async fn health_check(&self) -> Result<bool, ServiceError> { Err(ServiceError::SignalError("fail".to_string())) }
        }
        let plugin = WebRtcPlugin {
            name: "webrtc-test".to_string(),
            service: Arc::new(AlwaysFailService) as Arc<dyn WebrtcService>,
            config: WebrtcConfig { max_retries: 2, ..Default::default() },
            shutdown_tx: Mutex::new(None),
            background_tasks: Mutex::new(Vec::new()),
        };
        let res = plugin.create_peer_connection().await;
        assert!(res.is_err());
        let res = plugin.create_offer("conn").await;
        assert!(res.is_err());
        let res = plugin.set_remote_description("conn", "sdp").await;
        assert!(res.is_err());
        let res = plugin.add_ice_candidate("conn", "cand").await;
        assert!(res.is_err());
        let res = plugin.send_data("conn", b"data").await;
        assert!(res.is_err());
    }
    
    #[tokio::test]
    async fn test_webrtc_plugin_create_offer_xss_payloads() {
        let plugin = WebRtcPlugin::new();
        let xss_payloads = vec![
            "<script>alert('XSS')</script>",
            "<img src=x onerror=alert(1)>",
            "<svg/onload=alert(1)>",
            "<iframe src=javascript:alert(1)>"
        ];
        for payload in xss_payloads {
            assert!(plugin.create_offer(payload).await.is_err());
        }
    }
    
    #[tokio::test]
    async fn test_webrtc_plugin_create_offer_sql_payloads() {
        let plugin = WebRtcPlugin::new();
        let sqli_payloads = vec![
            "' OR '1'='1", "1; DROP TABLE users; --", "admin' --", "' UNION SELECT NULL--"
        ];
        for payload in sqli_payloads {
            assert!(plugin.create_offer(payload).await.is_err());
        }
    }
}

/// WARNING: Plugin WebRTC chỉ triển khai mock hiện tại.
/// Bắt buộc cập nhật triển khai thực với WebRTC API
/// trước khi deployment production!

/// Hàm trợ giúp để validate SDP
fn validate_sdp(sdp: &str) -> Result<(), WebRtcError> {
    // Kiểm tra độ dài SDP
    if sdp.is_empty() || sdp.len() > MAX_SDP_LENGTH {
        return Err(WebRtcError::ValidationError(format!(
            "Invalid SDP length: {}, must be between 1-{} characters",
            sdp.len(), MAX_SDP_LENGTH
        )));
    }
    
    // Kiểm tra định dạng SDP cơ bản
    if !sdp.starts_with("v=0") {
        return Err(WebRtcError::ValidationError(
            "Invalid SDP format: must start with 'v=0'".to_string()
        ));
    }
    
    // Kiểm tra SDP có chứa các thành phần cần thiết
    if !sdp.contains("\r\no=") || !sdp.contains("\r\ns=") || !sdp.contains("\r\nt=") {
        return Err(WebRtcError::ValidationError(
            "Invalid SDP format: missing required components".to_string()
        ));
    }
    
    // Kiểm tra định dạng chặt chẽ với regex cho các thành phần bắt buộc
    let required_patterns = [
        r"v=0\r\n",                                // Version
        r"o=.+ \d+ \d+ IN IP[46] .+\r\n",         // Origin
        r"s=.+\r\n",                              // Session Name
        r"t=\d+ \d+\r\n",                         // Timing
        r"m=(audio|video|application) \d+ .+\r\n" // Media Description
    ];
    
    for pattern in required_patterns {
        if let Err(_) = regex::Regex::new(pattern) {
            continue; // Skip if regex is invalid
        }
        
        if let Ok(regex) = regex::Regex::new(pattern) {
            if !regex.is_match(sdp) {
                return Err(WebRtcError::ValidationError(
                    format!("Invalid SDP format: missing required pattern {}", pattern)
                ));
            }
        }
    }
    
    // Kiểm tra format cụ thể cho security tokens trong a=fingerprint
    if sdp.contains("a=fingerprint") {
        if let Ok(fingerprint_regex) = regex::Regex::new(r"a=fingerprint:(sha-\d+|sha-\d+-\d+) ([0-9A-F]{2}:){15}[0-9A-F]{2}\r\n") {
            if !fingerprint_regex.is_match(sdp) {
                return Err(WebRtcError::ValidationError(
                    "Invalid SDP fingerprint format: must match standard cryptographic hash pattern".to_string()
                ));
            }
        }
    }
    
    // Kiểm tra encryption attributes
    if sdp.contains("a=crypto") {
        if let Ok(crypto_regex) = regex::Regex::new(r"a=crypto:\d+ (AES_CM_128_HMAC_SHA1_\d+|AES_256_CM_HMAC_SHA1_\d+) inline:.+\r\n") {
            if !crypto_regex.is_match(sdp) {
                return Err(WebRtcError::ValidationError(
                    "Invalid SDP crypto attribute format: must match standard encryption pattern".to_string()
                ));
            }
        }
    }
    
    // Kiểm tra nội dung không chứa script injection
    if sdp.contains("<script") || sdp.contains("javascript:") {
        return Err(WebRtcError::ValidationError(
            "Invalid SDP content: potential script injection detected".to_string()
        ));
    }
    
    // Kiểm tra Unicode attack vectors
    let dangerous_unicode = [
        '\u{202E}', // RIGHT-TO-LEFT OVERRIDE
        '\u{202D}', // LEFT-TO-RIGHT OVERRIDE
        '\u{200E}', // LEFT-TO-RIGHT MARK
        '\u{200F}', // RIGHT-TO-LEFT MARK
        '\u{2028}', // LINE SEPARATOR
        '\u{2029}', // PARAGRAPH SEPARATOR
        '\u{FEFF}', // ZERO WIDTH NO-BREAK SPACE
        '\u{200B}', // ZERO WIDTH SPACE
        '\u{200C}', // ZERO WIDTH NON-JOINER
        '\u{200D}', // ZERO WIDTH JOINER
    ];
    
    for unicode_char in dangerous_unicode {
        if sdp.contains(unicode_char) {
            return Err(WebRtcError::ValidationError(
                format!("Invalid SDP content: dangerous unicode character detected: U+{:04X}", unicode_char as u32)
            ));
        }
    }
    
    // Kiểm tra homoglyph attack (kí tự giống nhau nhưng khác unicode)
    let homoglyph_pairs = [
        ('a', 'а'), // 'a' Latin vs 'а' Cyrillic
        ('e', 'е'), // 'e' Latin vs 'е' Cyrillic
        ('o', 'о'), // 'o' Latin vs 'о' Cyrillic
        ('p', 'р'), // 'p' Latin vs 'р' Cyrillic
        ('c', 'с'), // 'c' Latin vs 'с' Cyrillic
    ];
    
    // Kiểm tra Latin trong SDP line key (v=, o=, s=, ...) nhưng Cyrillic trong value
    for (latin, cyrillic) in homoglyph_pairs {
        let pattern = format!("{}=", latin);
        let bad_pattern = format!("{}=", cyrillic);
        
        if sdp.contains(&bad_pattern) {
            return Err(WebRtcError::ValidationError(
                format!("Invalid SDP content: homoglyph attack detected (using {} instead of {})", cyrillic, latin)
            ));
        }
    }
    
    // Thêm kiểm tra null byte injection
    if sdp.contains('\0') {
        return Err(WebRtcError::ValidationError(
            "Invalid SDP content: null byte injection detected".to_string()
        ));
    }
    
    // Kiểm tra XSS qua attribute values
    let xss_patterns = [
        "=\"javascript:", "onload=", "onerror=", "onclick=", "onmouseover=",
        "<img", "<svg", "<iframe", "document.cookie", "eval(", "setTimeout(", 
        "setInterval(", "alert(", "confirm(", "prompt("
    ];
    
    for pattern in xss_patterns {
        if sdp.contains(pattern) {
            return Err(WebRtcError::ValidationError(
                format!("Invalid SDP content: potential XSS detected with pattern '{}'", pattern)
            ));
        }
    }
    
    // Kiểm tra SQL injection patterns
    let sql_patterns = [
        "'--", "'; ", "OR '1'='1", "DROP TABLE", "UNION SELECT", "EXEC(",
        "INSERT INTO", "DELETE FROM", "UPDATE ", "1=1", "admin'--",
        ";--", "/*", "*/", "WAITFOR DELAY", "SELECT @@version", "xp_cmdshell"
    ];
    
    for pattern in sql_patterns {
        if sdp.contains(pattern) {
            return Err(WebRtcError::ValidationError(
                format!("Invalid SDP content: potential SQL injection detected with pattern '{}'", pattern)
            ));
        }
    }
    
    // Kiểm tra OS command injection
    let cmd_patterns = [
        "; ls", "& dir", "| cat", "$(", "`", "&& ", "|| ", "; rm", "> /dev/null",
        "/etc/passwd", "; ping", "; wget", "; curl", "; nc", "; bash", "; sh",
        ">/dev/null", "2>&1"
    ];
    
    for pattern in cmd_patterns {
        if sdp.contains(pattern) {
            return Err(WebRtcError::ValidationError(
                format!("Invalid SDP content: potential command injection detected with pattern '{}'", pattern)
            ));
        }
    }
    
    Ok(())
}

/// Hàm trợ giúp để validate ICE candidate
fn validate_ice_candidate(candidate: &str) -> Result<(), WebRtcError> {
    // Kiểm tra độ dài
    if candidate.is_empty() || candidate.len() > MAX_ICE_CANDIDATE_LENGTH {
        return Err(WebRtcError::ValidationError(format!(
            "Invalid ICE candidate length: {}, must be between 1-{} characters",
            candidate.len(), MAX_ICE_CANDIDATE_LENGTH
        )));
    }
    
    // Kiểm tra định dạng ICE candidate cơ bản
    if !candidate.starts_with("a=candidate:") && !candidate.starts_with("candidate:") {
        return Err(WebRtcError::ValidationError(
            "Invalid ICE candidate format: must start with 'a=candidate:' or 'candidate:'".to_string()
        ));
    }
    
    // Kiểm tra nội dung không chứa script injection
    if candidate.contains("<script") || candidate.contains("javascript:") {
        return Err(WebRtcError::ValidationError(
            "Invalid ICE candidate content: potential script injection detected".to_string()
        ));
    }
    
    // Kiểm tra Unicode attack vectors
    if candidate.contains("\u{202E}") || candidate.contains("\u{202D}") {
        return Err(WebRtcError::ValidationError(
            "Invalid ICE candidate content: bidirectional unicode override detected".to_string()
        ));
    }
    
    // Thêm kiểm tra null byte injection
    if candidate.contains('\0') {
        return Err(WebRtcError::ValidationError(
            "Invalid ICE candidate content: null byte injection detected".to_string()
        ));
    }
    
    // Kiểm tra XSS qua attribute values
    if candidate.contains("=\"javascript:") || candidate.contains("onload=") || candidate.contains("onerror=") {
        return Err(WebRtcError::ValidationError(
            "Invalid ICE candidate content: potential attribute injection detected".to_string()
        ));
    }
    
    // Kiểm tra SQL injection patterns
    if candidate.contains("'--") || candidate.contains("'; ") || candidate.contains("OR '1'='1") {
        return Err(WebRtcError::ValidationError(
            "Invalid ICE candidate content: potential SQL injection detected".to_string()
        ));
    }
    
    Ok(())
}
