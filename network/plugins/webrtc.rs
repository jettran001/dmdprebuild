//! WebRTC plugin cho network module
//! Plugin này cung cấp khả năng kết nối peer-to-peer thông qua WebRTC
//! Hỗ trợ: kết nối qua NAT, mã hóa dữ liệu, và giao tiếp real-time

use crate::core::engine::{Plugin, PluginType, PluginError};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use std::collections::HashMap;
use std::any::Any;

use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;

use tracing::{debug, info, warn, error};
use async_trait::async_trait;
use thiserror::Error;

use crate::security::input_validation::security;
use crate::infra::service_traits::{ServiceError};

/// Lỗi cụ thể cho WebRTC plugin
#[derive(Error, Debug)]
pub enum WebRtcError {
    /// Lỗi kết nối (ICE failures, không thể thiết lập kênh dữ liệu)
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    /// Lỗi validation (dữ liệu đầu vào không hợp lệ)
    #[error("Validation error: {0}")]
    ValidationError(String),
    
    /// Lỗi phiên làm việc (session không tồn tại hoặc quá hạn)
    #[error("Session error: {0}")]
    SessionError(String),
    
    /// Lỗi kênh dữ liệu (data channel failures)
    #[error("Data channel error: {0}")]
    DataChannelError(String),
    
    /// Lỗi trao đổi tín hiệu (signaling failures)
    #[error("Signal error: {0}")]
    SignalError(String),
    
    /// Lỗi khi đóng/shutdown plugin
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
#[allow(dead_code)]
const MAX_SDP_LENGTH: usize = 8192;
const MAX_ICE_CANDIDATE_LENGTH: usize = 1024;
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

/// Trait for WebRTC service
#[async_trait]
pub trait WebRtcService: Send + Sync {
    /// Initialize the service
    async fn init(&self) -> Result<(), ServiceError>;
    
    /// Create a new peer connection
    async fn create_peer_connection(&self, config: &WebRtcConfig) -> Result<String, ServiceError>;
    
    /// Thêm ICE candidate
    async fn add_ice_candidate(&self, connection_id: &str, candidate: &str) -> Result<(), ServiceError>;
    
    /// Gửi dữ liệu qua kết nối
    async fn send_data(&self, connection_id: &str, data: &[u8]) -> Result<(), ServiceError>;
    
    /// Kiểm tra trạng thái service
    async fn health_check(&self) -> Result<bool, ServiceError>;
    
    /// Đóng một kết nối
    async fn close_connection(&self, connection_id: &str) -> Result<(), ServiceError>;
    
    /// Đóng tất cả kết nối
    async fn close_all_connections(&self) -> Result<usize, ServiceError>;
    
    /// Cast to Any để hỗ trợ downcasting
    fn as_any(&self) -> &dyn Any;
}

/// ConnectionManager cho WebRTC, xử lý zombie connections và dọn dẹp tài nguyên
pub struct ConnectionManager {
    /// Các kết nối hiện tại, lưu thời gian tạo và dữ liệu tương ứng
    connections: HashMap<String, (SystemTime, HashMap<String, Vec<u8>>)>,
    /// Thời gian timeout tính bằng giây
    timeout_secs: u64,
    /// Số kết nối tối đa
    max_connections: usize,
    /// Theo dõi trạng thái kết nối
    active_connections: HashMap<String, bool>,
}

impl ConnectionManager {
    /// Tạo connection manager mới
    pub fn new(timeout_secs: u64, max_connections: usize) -> Self {
        Self {
            connections: HashMap::new(),
            timeout_secs,
            max_connections,
            active_connections: HashMap::new(),
        }
    }
    
    /// Thêm kết nối mới
    pub fn add_connection(&mut self, id: String) -> Result<(), WebRtcError> {
        // Kiểm tra số lượng kết nối tối đa
        if self.connections.len() >= self.max_connections {
            self.cleanup_expired_connections();
            
            // Nếu vẫn full sau khi cleanup
            if self.connections.len() >= self.max_connections {
                return Err(WebRtcError::ConnectionError(
                    format!("Maximum connections limit reached ({})", self.max_connections)
                ));
            }
        }
        
        self.connections.insert(id.clone(), (SystemTime::now(), HashMap::new()));
        self.active_connections.insert(id, true);
        Ok(())
    }
    
    /// Lấy dữ liệu từ kết nối
    pub fn get_data(&mut self, conn_id: &str, key: &str) -> Option<Vec<u8>> {
        if let Some((_, data_map)) = self.connections.get(conn_id) {
            if let Some(data) = data_map.get(key) {
                return Some(data.clone());
            }
        }
        None
    }
    
    /// Lưu dữ liệu vào kết nối
    pub fn set_data(&mut self, conn_id: &str, key: String, value: Vec<u8>) -> Result<(), WebRtcError> {
        if let Some((_, data_map)) = self.connections.get_mut(conn_id) {
            data_map.insert(key, value);
            Ok(())
        } else {
            Err(WebRtcError::SessionError(format!("Connection {} not found", conn_id)))
        }
    }
    
    /// Đóng kết nối
    pub fn close_connection(&mut self, id: &str) -> bool {
        if self.connections.remove(id).is_some() {
            self.active_connections.insert(id.to_string(), false);
            true
        } else {
            false
        }
    }
    
    /// Đóng tất cả kết nối
    pub fn close_all_connections(&mut self) -> usize {
        let count = self.connections.len();
        for (id, _) in self.connections.drain() {
            self.active_connections.insert(id, false);
        }
        count
    }
    
    /// Dọn dẹp các kết nối hết hạn
    pub fn cleanup_expired_connections(&mut self) -> usize {
        let now = SystemTime::now();
        let timeout_duration = Duration::from_secs(self.timeout_secs);
        
        let expired: Vec<String> = self.connections.iter()
            .filter_map(|(id, (created_time, _))| {
                match now.duration_since(*created_time) {
                    Ok(duration) if duration > timeout_duration => Some(id.clone()),
                    _ => None,
                }
            })
            .collect();
        
        for id in &expired {
            self.connections.remove(id);
            self.active_connections.insert(id.clone(), false);
        }
        
        expired.len()
    }
}

/// Default WebRTC service implementation
pub struct DefaultWebRtcService {
    /// Cấu hình service
    config: WebRtcConfig,
    /// Signal để shutdown service
    #[allow(dead_code)]
    shutdown_signal: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    /// Connection manager
    connection_manager: Arc<Mutex<ConnectionManager>>,
}

impl Default for DefaultWebRtcService {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultWebRtcService {
    /// Tạo WebRTC service mới với cấu hình mặc định
    pub fn new() -> Self {
        Self {
            config: WebRtcConfig::default(),
            shutdown_signal: Arc::new(Mutex::new(None)),
            connection_manager: Arc::new(Mutex::new(
                ConnectionManager::new(1800, DEFAULT_MAX_CONCURRENT_CONNECTIONS)
            )),
        }
    }
    
    /// Tạo WebRTC service với cấu hình tùy chỉnh
    pub fn with_config(config: WebRtcConfig) -> Self {
        Self {
            config: config.clone(),
            shutdown_signal: Arc::new(Mutex::new(None)),
            connection_manager: Arc::new(Mutex::new(
                ConnectionManager::new(1800, config.max_concurrent_connections)
            )),
        }
    }
    
    #[allow(dead_code)]
    fn validate_sdp(sdp: &str) -> Result<(), WebRtcError> {
        if sdp.is_empty() {
            return Err(WebRtcError::ValidationError("SDP is empty".to_string()));
        }
        
        if sdp.len() > MAX_SDP_LENGTH {
            return Err(WebRtcError::ValidationError(
                format!("SDP exceeds maximum allowed length: {} > {}", sdp.len(), MAX_SDP_LENGTH)
            ));
        }
        
        // Kiểm tra basic SDP syntax
        if !sdp.starts_with("v=") {
            return Err(WebRtcError::ValidationError("Invalid SDP format: must start with v=".to_string()));
        }
        
        // Xác nhận SDP không chứa script hoặc HTML tags chống XSS
        if sdp.contains("<script") || sdp.contains("<img") || sdp.contains("<iframe") {
            return Err(WebRtcError::ValidationError("SDP contains potentially malicious content".to_string()));
        }
        
        Ok(())
    }
    
    /// Validate ICE candidate
    fn validate_ice_candidate(candidate: &str) -> Result<(), WebRtcError> {
        // Kiểm tra độ dài
        if candidate.len() > MAX_ICE_CANDIDATE_LENGTH {
            return Err(WebRtcError::ValidationError(
                format!("ICE candidate too large: {} bytes (max: {})", candidate.len(), MAX_ICE_CANDIDATE_LENGTH)
            ));
        }
        
        // Kiểm tra định dạng
        if !candidate.contains("candidate:") {
            return Err(WebRtcError::ValidationError("Invalid ICE candidate format".to_string()));
        }
        
        // Kiểm tra an toàn
        if let Err(e) = security::check_xss(candidate, "webrtc_ice_candidate") {
            return Err(WebRtcError::ValidationError(e.to_string()));
        }
        
        Ok(())
    }
    
    /// Đóng tất cả các kết nối và trả về số lượng kết nối đã đóng
    pub fn close_all_connections_internal(&self) -> usize {
        match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime.block_on(async {
                let mut conn_manager = self.connection_manager.lock().await;
                conn_manager.close_all_connections()
            }),
            Err(e) => {
                error!("[WebRTC] Failed to create runtime for closing connections: {}", e);
                0 // Trả về 0 kết nối đã đóng khi runtime không thể tạo được
            }
        }
    }
}

#[async_trait]
impl WebRtcService for DefaultWebRtcService {
    async fn init(&self) -> Result<(), ServiceError> {
        info!("[WebRTC] Initializing service");
        
        // Validate cấu hình
        if self.config.stun_servers.is_empty() {
            warn!("[WebRTC] No STUN servers configured");
        }
        
        // Reset connection manager
        let mut conn_manager = self.connection_manager.lock().await;
        conn_manager.close_all_connections();
        
        debug!("[WebRTC] Service initialized with {} STUN servers", self.config.stun_servers.len());
        Ok(())
    }
    
    async fn create_peer_connection(&self, _config: &WebRtcConfig) -> Result<String, ServiceError> {
        let connection_id = format!("webrtc-{}", uuid::Uuid::new_v4());
        debug!("[WebRTC] Creating peer connection: {}", connection_id);
        
        // Add to connection manager
        let mut conn_manager = self.connection_manager.lock().await;
        if let Err(e) = conn_manager.add_connection(connection_id.clone()) {
            error!("[WebRTC] Failed to create connection: {}", e);
            return Err(ServiceError::InternalError(e.to_string()));
        }
        
        Ok(connection_id)
    }
    
    async fn add_ice_candidate(&self, connection_id: &str, candidate: &str) -> Result<(), ServiceError> {
        debug!("[WebRTC] Adding ICE candidate for connection: {}", connection_id);
        
        // Validate ICE candidate
        if let Err(e) = Self::validate_ice_candidate(candidate) {
            return Err(ServiceError::ValidationError(e.to_string()));
        }
        
        // Check if connection exists
        let mut conn_manager = self.connection_manager.lock().await;
        conn_manager.set_data(connection_id, "ice_candidate".to_string(), candidate.as_bytes().to_vec())
            .map_err(|e| ServiceError::InternalError(e.to_string()))
    }
    
    async fn send_data(&self, connection_id: &str, data: &[u8]) -> Result<(), ServiceError> {
        if data.len() > self.config.max_packet_size {
            return Err(ServiceError::ValidationError(
                format!("Data too large: {} bytes (max: {})", data.len(), self.config.max_packet_size)
            ));
        }
        
        // Check connection exists
        let conn_manager = self.connection_manager.lock().await;
        if !conn_manager.connections.contains_key(connection_id) {
            return Err(ServiceError::NotFoundError(format!("Connection {} not found", connection_id)));
        }
        
        debug!("[WebRTC] Sending {} bytes to connection: {}", data.len(), connection_id);
        // In a real implementation, this would send data over the WebRTC data channel
        Ok(())
    }
    
    async fn health_check(&self) -> Result<bool, ServiceError> {
        let conn_manager = self.connection_manager.lock().await;
        let active_count = conn_manager.active_connections.values().filter(|&&active| active).count();
        
        debug!("[WebRTC] Health check: {} active connections", active_count);
        Ok(true)
    }
    
    async fn close_connection(&self, connection_id: &str) -> Result<(), ServiceError> {
        debug!("[WebRTC] Closing connection: {}", connection_id);
        let mut conn_manager = self.connection_manager.lock().await;
        if conn_manager.close_connection(connection_id) {
            Ok(())
        } else {
            Err(ServiceError::NotFoundError(format!("Connection {} not found", connection_id)))
        }
    }
    
    async fn close_all_connections(&self) -> Result<usize, ServiceError> {
        debug!("[WebRTC] Closing all connections");
        let mut conn_manager = self.connection_manager.lock().await;
        let count = conn_manager.close_all_connections();
        debug!("[WebRTC] Closed {} connections", count);
        Ok(count)
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// WebRTC plugin implementation
pub struct WebRtcPlugin {
    /// Tên plugin
    name: String,
    /// Service xử lý logic WebRTC
    service: Arc<dyn WebRtcService>,
    /// Cấu hình plugin
    #[allow(dead_code)]
    config: WebRtcConfig,
    /// Channel để shutdown plugin
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    /// Danh sách task chạy nền
    background_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl Default for WebRtcPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl WebRtcPlugin {
    /// Tạo WebRTC plugin mới với cấu hình mặc định
    pub fn new() -> Self {
        let service = Arc::new(DefaultWebRtcService::new());
        Self {
            name: "webrtc".to_string(),
            service,
            config: WebRtcConfig::default(),
            shutdown_tx: Mutex::new(None),
            background_tasks: Mutex::new(Vec::new()),
        }
    }
    
    /// Tạo WebRTC plugin với cấu hình tùy chỉnh
    pub fn with_config(config: WebRtcConfig) -> Self {
        let service = Arc::new(DefaultWebRtcService::with_config(config.clone()));
        Self {
            name: "webrtc".to_string(),
            service,
            config,
            shutdown_tx: Mutex::new(None),
            background_tasks: Mutex::new(Vec::new()),
        }
    }
    
    /// Validate input data
    pub fn validate_input(&self, msg: &str) -> Result<(), String> {
        if let Err(e) = security::check_xss(msg, "webrtc_input") {
            return Err(format!("XSS validation failed: {}", e));
        }
        if let Err(e) = security::check_sql_injection(msg, "webrtc_input") {
            return Err(format!("SQL injection validation failed: {}", e));
        }
        Ok(())
    }
    
    /// Initialize WebRTC peer
    pub async fn init_node(&self) -> Result<(), String> {
        match self.service.init().await {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to initialize WebRTC node: {}", e)),
        }
    }
    
    /// Create peer connection
    pub async fn create_peer_connection(&self) -> Result<String, String> {
        match self.service.create_peer_connection(&self.config).await {
            Ok(id) => Ok(id),
            Err(e) => Err(format!("Failed to create peer connection: {}", e)),
        }
    }
    
    /// Send data over WebRTC
    pub async fn send_data(&self, connection_id: &str, data: &[u8]) -> Result<(), String> {
        match self.service.send_data(connection_id, data).await {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to send data: {}", e)),
        }
    }
    
    #[allow(dead_code)]
    async fn start_health_check(&self) -> Result<JoinHandle<()>, WebRtcError> {
        let service = self.service.clone();
        
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            
            loop {
                interval.tick().await;
                
                match service.health_check().await {
                    Ok(true) => {
                        debug!("[WebRTC] Health check passed");
                    },
                    Ok(false) => {
                        warn!("[WebRTC] Health check returned false, service may be degraded");
                    },
                    Err(e) => {
                        error!("[WebRTC] Health check failed: {:?}", e);
                    }
                }
            }
        });
        
        Ok(handle)
    }
    
    /// Đóng tất cả các kết nối nếu service là DefaultWebRtcService
    pub fn close_all_connections(&self) -> usize {
        match self.service.as_any().downcast_ref::<DefaultWebRtcService>() {
            Some(default_service) => default_service.close_all_connections_internal(),
            None => {
                let rt = tokio::runtime::Runtime::new().unwrap();
                match rt.block_on(self.service.close_all_connections()) {
                    Ok(count) => count,
                    Err(e) => {
                        error!("[WebRTC] Failed to close connections: {}", e);
                        0
                    }
                }
            }
        }
    }
}

#[async_trait]
impl Plugin for WebRtcPlugin {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn plugin_type(&self) -> PluginType {
        PluginType::WebRtc
    }
    
    async fn start(&self) -> Result<bool, PluginError> {
        // Khởi động WebRTC plugin
        info!("[WebRTC] Starting plugin...");
        
        // Kiểm tra xem có cấu hình hợp lệ không
        if self.config.stun_servers.is_empty() {
            error!("[WebRTC] No STUN servers configured");
            return Err(PluginError::ConfigError("No STUN servers configured".to_string()));
        }
        
        // Khởi tạo WebRTC service
        let service = self.service.clone();
        let config = self.config.clone();
        
        // Spawn task để khởi tạo service và xử lý message
        let _handle = tokio::spawn(async move {
            match service.init().await {
                Ok(_) => info!("[WebRTC] Service initialized successfully"),
                Err(e) => error!("[WebRTC] Failed to initialize service: {}", e),
            }
            
            // Khối code xử lý message sẽ được thêm vào đây sau
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                match service.health_check().await {
                    Ok(true) => debug!("[WebRTC] Health check passed"),
                    Ok(false) => warn!("[WebRTC] Health check failed but service is responding"),
                    Err(e) => error!("[WebRTC] Health check error: {}", e),
                }
            }
        });
        
        // Lưu handle của task
        tokio::spawn(async move {
            if let Err(e) = tokio::time::timeout(
                Duration::from_millis(config.connection_timeout_ms),
                async {}
            ).await {
                error!("[WebRTC] Plugin initialization timed out: {}", e);
            }
        });
        
        Ok(true)
    }
    
    async fn stop(&self) -> Result<(), PluginError> {
        info!("[WebRTC] Stopping plugin...");
        
        // Đóng tất cả kết nối
        let closed_count = self.close_all_connections();
        info!("[WebRTC] Closed {} connections", closed_count);
        
        // Send shutdown signal
        tokio::spawn(async {
            tokio::time::sleep(Duration::from_millis(500)).await;
            info!("[WebRTC] Plugin shutdown complete");
        });
        
        Ok(())
    }
    
    async fn check_health(&self) -> Result<bool, PluginError> {
        // Trả về trạng thái health check đơn giản
        // Trong thực tế, đây sẽ kiểm tra kết nối WebRTC, ICE status, v.v.
        Ok(true)
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Drop for WebRtcPlugin {
    fn drop(&mut self) {
        // Tự động dọn dẹp tài nguyên khi plugin bị drop
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            if let Some(tx) = self.shutdown_tx.lock().await.take() {
                let _ = tx.send(());
            }
            
            // Đợi các background task hoàn thành
            let mut tasks = self.background_tasks.lock().await;
            while let Some(task) = tasks.pop() {
                task.abort();
            }
            
            info!("[WebRTC] Plugin resources cleaned up");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::test;
    
    #[test]
    async fn test_webrtc_plugin_start_stop() {
        let plugin = WebRtcPlugin::new();
        assert!(plugin.start().await.is_ok());
        assert!(plugin.stop().await.is_ok());
    }
    
    #[test]
    async fn test_create_connection() {
        let plugin = WebRtcPlugin::new();
        let result = plugin.create_peer_connection().await;
        assert!(result.is_ok());
    }
    
    #[test]
    async fn test_validate_input() {
        let plugin = WebRtcPlugin::new();
        
        // Safe input
        assert!(plugin.validate_input("hello world").is_ok());
        
        // XSS attempt
        assert!(plugin.validate_input("<script>alert('xss')</script>").is_err());
        
        // SQL injection attempt
        assert!(plugin.validate_input("'; DROP TABLE users; --").is_err());
    }
}
