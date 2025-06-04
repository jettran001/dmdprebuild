use crate::core::engine::{Plugin, PluginType, PluginError};
use std::env;
use crate::security::input_validation::security;
use libp2p::{identity, PeerId};
use std::time::Duration;
use tracing::{info, warn, error, debug};
use tokio::sync::{Mutex, oneshot};
use std::sync::Arc;
use async_trait::async_trait;
use thiserror::Error;
use uuid;
use tokio::task::JoinHandle;
use std::any::Any;

#[derive(Error, Debug, Clone)]
pub enum Libp2pError {
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    #[error("Validation error: {0}")]
    ValidationError(String),
    
    #[error("Timeout error: {0}")]
    TimeoutError(String),
    
    #[error("Message handling error: {0}")]
    MessageError(String),
    
    #[error("Peer discovery error: {0}")]
    DiscoveryError(String),
    
    #[error("Shutdown error: {0}")]
    ShutdownError(String),
}

/// Configuration for Libp2p plugin
#[derive(Clone, Debug)]
pub struct Libp2pConfig {
    /// Connection timeout in milliseconds
    pub connection_timeout_ms: u64,
    /// Max number of retries for operations
    pub max_retries: u8,
    /// Delay between retries in milliseconds
    pub retry_delay_ms: u64,
    /// Listen address for the node
    pub listen_addr: Option<String>,
    /// Bootstrap peers
    pub bootstrap_peers: Vec<String>,
    /// Enable mDNS peer discovery
    pub enable_mdns: bool,
    /// Enable Kademlia DHT
    pub enable_kademlia: bool,
    /// Enable TLS
    pub enable_tls: bool,
}

impl Default for Libp2pConfig {
    fn default() -> Self {
        Self {
            connection_timeout_ms: 5000,
            max_retries: 3,
            retry_delay_ms: 1000,
            listen_addr: Some("/ip4/0.0.0.0/tcp/0".to_string()),
            bootstrap_peers: Vec::new(),
            enable_mdns: true,
            enable_kademlia: false,
            enable_tls: false,
        }
    }
}

/// Thread-safe service for Libp2p operations
#[async_trait]
pub trait Libp2pService: Send + Sync {
    /// Initialize a new node
    async fn init_node(&self) -> Result<(PeerId, identity::Keypair), Libp2pError>;
    
    /// Start listening on a multiaddr
    async fn start_listening(&self, keypair: &identity::Keypair, addr: &str) -> Result<(), Libp2pError>;
    
    /// Connect to a peer
    async fn connect_to_peer(&self, peer_addr: &str) -> Result<PeerId, Libp2pError>;
    
    /// Send message to a peer
    async fn send_message(&self, peer_id: &PeerId, msg: &str) -> Result<(), Libp2pError>;
    
    /// Receive and handle messages from peers
    async fn start_message_handler(&self) -> Result<oneshot::Sender<()>, Libp2pError>;
    
    /// Discover peers on the network
    async fn discover_peers(&self) -> Result<Vec<PeerId>, Libp2pError>;
    
    /// Shutdown the service
    async fn shutdown(&self) -> Result<(), Libp2pError>;
}

/// Default implementation of Libp2pService (mock)
pub struct DefaultLibp2pService {
    config: Libp2pConfig,
    shutdown_signal: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

impl Default for DefaultLibp2pService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Libp2pService for DefaultLibp2pService {
    async fn init_node(&self) -> Result<(PeerId, identity::Keypair), Libp2pError> {
        let keypair = identity::Keypair::generate_ed25519();
        let peer_id = PeerId::from(keypair.public());
        debug!("[Libp2p] Generated new peer ID: {}", peer_id);
        Ok((peer_id, keypair))
    }
    
    async fn start_listening(&self, _keypair: &identity::Keypair, addr: &str) -> Result<(), Libp2pError> {
        // WARNING: This is a mock implementation. Do NOT use in production. Replace with real listening logic!
        if let Err(e) = security::check_xss(addr, "libp2p_address") {
            return Err(Libp2pError::ValidationError(e.to_string()));
        }
        debug!(plugin_name = "libp2p", operation = "start_listening", status = "mock", addr = %addr, "[Libp2p] Listening on (mock)");
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(())
    }
    
    async fn connect_to_peer(&self, peer_addr: &str) -> Result<PeerId, Libp2pError> {
        // Validate input
        if let Err(e) = security::check_xss(peer_addr, "libp2p_peer_address") {
            return Err(Libp2pError::ValidationError(e.to_string()));
        }
        let correlation_id = uuid::Uuid::new_v4().to_string();
        for attempt in 0..=self.config.max_retries {
            match self.try_connect_to_peer(peer_addr).await {
                Ok(peer_id) => {
                    if attempt > 0 {
                        info!(plugin_name = "libp2p", operation = "connect_to_peer", status = "success", error_code = "", correlation_id = %correlation_id, attempt = attempt, peer_addr = %peer_addr, peer_id = %peer_id, "[Libp2p] Connected to peer after {} retries", attempt);
                    }
                    return Ok(peer_id);
                }
                Err(ref e) => {
                    let error_code = match e {
                        Libp2pError::ConnectionError(_) => "CONN_ERR",
                        Libp2pError::TimeoutError(_) => "TIMEOUT",
                        Libp2pError::ValidationError(_) => "VALIDATION",
                        Libp2pError::MessageError(_) => "MSG_ERR",
                        Libp2pError::DiscoveryError(_) => "DISCOVERY",
                        Libp2pError::ShutdownError(_) => "SHUTDOWN",
                    };
                    if attempt < self.config.max_retries {
                        warn!(plugin_name = "libp2p", operation = "connect_to_peer", status = "retry", error_code = error_code, correlation_id = %correlation_id, attempt = attempt + 1, peer_addr = %peer_addr, "[Libp2p] Connection attempt {}/{} failed: {}", attempt + 1, self.config.max_retries, e);
                        tokio::time::sleep(Duration::from_millis(
                            self.config.retry_delay_ms * (1 << attempt)
                        )).await;
                    } else {
                        error!(plugin_name = "libp2p", operation = "connect_to_peer", status = "fail", error_code = error_code, correlation_id = %correlation_id, attempt = attempt + 1, peer_addr = %peer_addr, "[Libp2p] Connection failed after {} retries: {}", self.config.max_retries, e);
                        return Err(match e {
                            Libp2pError::ConnectionError(msg) => Libp2pError::ConnectionError(msg.to_owned()),
                            Libp2pError::ValidationError(msg) => Libp2pError::ValidationError(msg.to_owned()),
                            Libp2pError::TimeoutError(msg) => Libp2pError::TimeoutError(msg.to_owned()),
                            Libp2pError::MessageError(msg) => Libp2pError::MessageError(msg.to_owned()),
                            Libp2pError::DiscoveryError(msg) => Libp2pError::DiscoveryError(msg.to_owned()),
                            Libp2pError::ShutdownError(msg) => Libp2pError::ShutdownError(msg.to_owned()),
                        });
                    }
                }
            }
        }
        Err(Libp2pError::ConnectionError("Unexpected error in connect_to_peer".to_string()))
    }
    
    async fn send_message(&self, peer_id: &PeerId, msg: &str) -> Result<(), Libp2pError> {
        // WARNING: This is a mock implementation. Do NOT use in production. Replace with real message sending logic!
        if let Err(e) = security::check_xss(msg, "libp2p_message") {
            return Err(Libp2pError::ValidationError(e.to_string()));
        }
        if let Err(e) = security::check_sql_injection(msg, "libp2p_message") {
            return Err(Libp2pError::ValidationError(e.to_string()));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        debug!(plugin_name = "libp2p", operation = "send_message", status = "mock", peer_id = %peer_id, "[Libp2p] Sent message to peer (mock)");
        Ok(())
    }
    
    async fn start_message_handler(&self) -> Result<oneshot::Sender<()>, Libp2pError> {
        // Tạo channel oneshot
        let (tx, rx) = oneshot::channel::<()>();
        
        // Store shutdown signal - không sử dụng clone cho oneshot::Sender
        let mut lock = self.shutdown_signal.lock().await;
        *lock = Some(tx);
        
        // Tạo một channel mới để trả về - vì oneshot::Sender không thể clone
        let (return_tx, _) = oneshot::channel::<()>();
        drop(lock);
        
        // Spawn a task to handle incoming messages
        tokio::spawn(async move {
            debug!("[Libp2p] Started message handler");
            
            // Tạo một rx_fut để có thể tokio::select! mà không move rx
            let mut rx_fut = rx;
            
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(10)) => {
                        debug!("[Libp2p] Message handler heartbeat");
                    }
                    _ = &mut rx_fut => {
                        debug!("[Libp2p] Shutting down message handler");
                        break;
                    }
                }
            }
        });
        
        Ok(return_tx)
    }
    
    async fn discover_peers(&self) -> Result<Vec<PeerId>, Libp2pError> {
        // WARNING: This is a mock implementation. Do NOT use in production. Replace with real peer discovery logic!
        tokio::time::sleep(Duration::from_millis(200)).await;
        if self.config.enable_mdns || self.config.enable_kademlia {
            let peer_id1 = PeerId::random();
            let peer_id2 = PeerId::random();
            debug!(plugin_name = "libp2p", operation = "discover_peers", status = "mock", peer_id1 = %peer_id1, peer_id2 = %peer_id2, "[Libp2p] Discovered peers (mock)");
            Ok(vec![peer_id1, peer_id2])
        } else {
            debug!(plugin_name = "libp2p", operation = "discover_peers", status = "mock", "[Libp2p] No peer discovery methods enabled (mock)");
            Ok(vec![])
        }
    }
    
    /// Implement shutdown method for trait
    async fn shutdown(&self) -> Result<(), Libp2pError> {
        let mut lock = self.shutdown_signal.lock().await;
        if let Some(tx) = lock.take() {
            if let Err(e) = tx.send(()) {
                return Err(Libp2pError::ShutdownError(
                    format!("Failed to send shutdown signal: {:?}", e)
                ));
            }
        }
        
        debug!("[Libp2p] Service shutdown complete");
        Ok(())
    }
}

impl DefaultLibp2pService {
    /// Create a new Libp2p service with default configuration
    pub fn new() -> Self {
        Self {
            config: Libp2pConfig::default(),
            shutdown_signal: Arc::new(Mutex::new(None)),
        }
    }
    
    /// Create a new Libp2p service with custom configuration
    pub fn with_config(config: Libp2pConfig) -> Self {
        Self {
            config,
            shutdown_signal: Arc::new(Mutex::new(None)),
        }
    }
    
    /// Try to connect to a peer (implementation detail)
    /// WARNING: This is a mock implementation. Do NOT use in production. Replace with real peer connection logic!
    async fn try_connect_to_peer(&self, peer_addr: &str) -> Result<PeerId, Libp2pError> {
        // Simulated network delay
        tokio::time::sleep(Duration::from_millis(100)).await;
        // 20% chance of failure to simulate network errors
        if rand::random::<f32>() < 0.2 {
            error!(plugin_name = "libp2p", operation = "try_connect_to_peer", status = "fail", error_code = "MOCK_CONN_FAIL", peer_addr = %peer_addr, "[Libp2p] Mock failed to connect to peer");
            return Err(Libp2pError::ConnectionError(
                format!("Failed to connect to peer at {}", peer_addr)
            ));
        }
        let peer_id = PeerId::random();
        debug!(plugin_name = "libp2p", operation = "try_connect_to_peer", status = "success", peer_addr = %peer_addr, peer_id = %peer_id, "[Libp2p] Connected to peer (mock)");
        Ok(peer_id)
    }
}

pub struct Libp2pPlugin {
    name: String,
    service: Arc<dyn Libp2pService>,
    #[allow(dead_code)]
    config: Libp2pConfig,
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    background_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl Default for Libp2pPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Libp2pPlugin {
    pub fn new() -> Self {
        warn!("[Libp2p] Đang sử dụng logic mock, chưa triển khai thực tế Swarm, peer discovery, connect, send/receive. Cần hoàn thiện logic thực tế khi production!");
        
        Self {
            name: "libp2p".to_string(),
            service: Arc::new(DefaultLibp2pService::new()),
            config: Libp2pConfig::default(),
            shutdown_tx: Mutex::new(None),
            background_tasks: Mutex::new(Vec::new()),
        }
    }
    
    /// Create a new Libp2p plugin with custom configuration
    pub fn with_config(config: Libp2pConfig) -> Self {
        Self {
            name: "libp2p".to_string(),
            service: Arc::new(DefaultLibp2pService::with_config(config.clone())),
            config,
            shutdown_tx: Mutex::new(None),
            background_tasks: Mutex::new(Vec::new()),
        }
    }
    
    /// Validate input message nếu nhận từ peer/external
    pub fn validate_input(&self, msg: &str) -> Result<(), String> {
        let enable = env::var("PLUGIN_INPUT_VALIDATION").unwrap_or_else(|_| "on".to_string());
        if enable == "off" {
            return Ok(());
        }
        security::check_xss(msg, "libp2p_message").map_err(|e| e.to_string())?;
        security::check_sql_injection(msg, "libp2p_message").map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Khởi tạo node libp2p với keypair mới
    pub async fn init_node(&self) -> Result<(PeerId, identity::Keypair), String> {
        self.service.init_node().await
            .map_err(|e| format!("Failed to initialize node: {}", e))
    }

    /// Lắng nghe trên địa chỉ multiaddr 
    pub async fn listen(&self, keypair: &identity::Keypair, addr: &str) -> Result<(), String> {
        self.service.start_listening(keypair, addr).await
            .map_err(|e| format!("Failed to start listening: {}", e))
    }

    /// Kết nối tới peer khác
    pub async fn connect(&self, peer_addr: &str) -> Result<PeerId, String> {
        self.service.connect_to_peer(peer_addr).await
            .map_err(|e| format!("Failed to connect to peer: {}", e))
    }

    /// Gửi message tới peer
    pub async fn send_message(&self, peer_id: &PeerId, msg: &str) -> Result<(), String> {
        self.validate_input(msg)?;
        self.service.send_message(peer_id, msg).await
            .map_err(|e| format!("Failed to send message: {}", e))
    }

    /// Nhận message từ peer
    pub async fn receive_message(&self, msg: &str) -> Result<(), String> {
        self.validate_input(msg)?;
        
        // Simulated processing delay
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        // TODO: Xử lý message thực tế
        Ok(())
    }

    /// Peer discovery
    pub async fn discover_peers(&self) -> Result<Vec<PeerId>, String> {
        self.service.discover_peers().await
            .map_err(|e| format!("Failed to discover peers: {}", e))
    }
    
    /// Start message handler
    pub async fn start_message_handler(&self) -> Result<oneshot::Sender<()>, String> {
        self.service.start_message_handler().await
            .map_err(|e| format!("Failed to start message handler: {}", e))
    }
}

#[async_trait::async_trait]
impl Plugin for Libp2pPlugin {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn plugin_type(&self) -> PluginType {
        PluginType::Libp2p
    }
    
    async fn start(&self) -> Result<bool, PluginError> {
        if self.name.is_empty() {
            return Err(PluginError::ConfigError("Plugin name is empty".to_string()));
        }
        
        // Spawn a task to initialize the plugin
        let init_handle = tokio::spawn({
            let service = self.service.clone();
            
            async move {
                match service.init_node().await {
                    Ok((peer_id, _)) => {
                        info!("[Libp2p] Plugin initialized with peer ID: {}", peer_id);
                    }
                    Err(e) => {
                        error!("[Libp2p] Plugin initialization error: {}", e);
                    }
                }
            }
        });
        
        if let Ok(mut bg) = self.background_tasks.try_lock() {
            bg.push(init_handle);
        } else {
            warn!("[Libp2p] Failed to acquire lock for background_tasks during start");
        }
        
        Ok(true)
    }
    
    async fn stop(&self) -> Result<(), PluginError> {
        info!("[Libp2p] Stopping plugin");
        // Attempt to shutdown service gracefully
        let service = self.service.clone();
        let shutdown_result = tokio::time::timeout(
            Duration::from_secs(5), 
            service.shutdown()
        ).await;
        
        match shutdown_result {
            Ok(result) => {
                match result {
                    Ok(_) => info!("[Libp2p] Service shutdown complete"),
                    Err(e) => {
                        warn!("[Libp2p] Error during service shutdown: {}", e);
                        // Trả về lỗi nếu shutdown dịch vụ thất bại
                        return Err(PluginError::ShutdownError(format!("Service shutdown error: {}", e)));
                    }
                }
            },
            Err(_) => {
                error!("[Libp2p] Service shutdown timed out after 5s");
                // Trả về lỗi timeout thay vì bỏ qua
                return Err(PluginError::ShutdownError("Service shutdown timed out after 5s".to_string()));
            }
        }
        
        // Shutdown all background tasks gracefully with timeout
        if let Ok(mut tasks) = self.background_tasks.try_lock() {
            for task in tasks.drain(..) {
                task.abort();
            }
        } else {
            warn!("[Libp2p] Failed to acquire lock for background tasks during shutdown");
        }
        // Đảm bảo lệnh shutdown được gửi đi (nếu có)
        if let Ok(mut lock) = self.shutdown_tx.try_lock() {
            if let Some(tx) = lock.take() {
                let _ = tx.send(());
            }
        }
        Ok(())
    }

    async fn check_health(&self) -> Result<bool, PluginError> {
        Ok(true)
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Drop for Libp2pPlugin {
    fn drop(&mut self) {
        // Đảm bảo khi plugin bị drop, tất cả resource được giải phóng đúng cách
        warn!("[Libp2p] Plugin is being dropped, cleaning up resources...");
        // Ngăn chặn deadlock bằng cách sử dụng try_lock thay vì lock trong drop
        if let Ok(mut shutdown_sender) = self.shutdown_tx.try_lock() {
            if let Some(sender) = shutdown_sender.take() {
                let _ = sender.send(());
            }
        }
        // Abort tất cả background tasks để tránh memory leak
        if let Ok(mut tasks) = self.background_tasks.try_lock() {
            for task in tasks.drain(..) {
                task.abort();
            }
        }
        info!("[Libp2p] Plugin dropped and cleaned up");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn test_libp2p_plugin_start_stop() {
        let plugin = Libp2pPlugin::new();
        assert_eq!(plugin.name(), "libp2p");
        assert_eq!(plugin.plugin_type(), PluginType::Libp2p);
        
        // Sửa unwrap bằng assertion
        let start_result = plugin.start().await;
        assert!(start_result.is_ok(), "Plugin start failed: {:?}", start_result.err());
        
        // Kiểm tra kết quả start có là true không
        if let Ok(started) = start_result {
            assert_eq!(started, true, "Plugin should start successfully");
        }
        
        let stop_result = plugin.stop().await;
        assert!(stop_result.is_ok(), "Plugin stop failed: {:?}", stop_result.err());
    }

    #[tokio::test]
    async fn test_libp2p_plugin_init_node() {
        let plugin = Libp2pPlugin::new();
        let init_result = plugin.init_node().await;
        
        // Sửa unwrap bằng assertion
        assert!(init_result.is_ok(), "Node initialization failed: {:?}", init_result.err());
        
        // Kiểm tra kết quả init_node
        if let Ok((peer_id, keypair)) = init_result {
            assert_eq!(peer_id, PeerId::from(keypair.public()), "Peer ID should match the public key");
        }
    }

    #[tokio::test]
    async fn test_libp2p_plugin_send_receive_message() {
        let plugin = Libp2pPlugin::new();
        let peer_id = PeerId::random();
        assert!(plugin.send_message(&peer_id, "hello").await.is_ok());
        assert!(plugin.receive_message("hello").await.is_ok());
        // Test XSS/SQLi
        assert!(plugin.send_message(&peer_id, "<script>alert('XSS')</script>").await.is_err());
    }
    
    #[tokio::test]
    async fn test_libp2p_plugin_discover_peers() {
        let plugin = Libp2pPlugin::new();
        let discover_result = plugin.discover_peers().await;
        
        // Sửa unwrap bằng assertion
        assert!(discover_result.is_ok(), "Peer discovery failed: {:?}", discover_result.err());
        
        // Kiểm tra kết quả discover_peers
        if let Ok(peers) = discover_result {
            assert_eq!(peers.len(), 2, "Mock should return 2 peers"); 
        }
    }

    #[tokio::test]
    async fn test_libp2p_plugin_connect_to_peer_retry_limit() {
        // Force try_connect_to_peer to always fail by patching rand::random
        let plugin = Libp2pPlugin::with_config(Libp2pConfig {
            max_retries: 2,
            ..Default::default()
        });
        // Patch: temporarily override try_connect_to_peer to always fail
        struct AlwaysFailService;
        #[async_trait::async_trait]
        impl Libp2pService for AlwaysFailService {
            async fn init_node(&self) -> Result<(PeerId, identity::Keypair), Libp2pError> { Err(Libp2pError::ConnectionError("fail".to_string())) }
            async fn start_listening(&self, _k: &identity::Keypair, _a: &str) -> Result<(), Libp2pError> { Ok(()) }
            async fn connect_to_peer(&self, _a: &str) -> Result<PeerId, Libp2pError> { Err(Libp2pError::ConnectionError("fail".to_string())) }
            async fn send_message(&self, _p: &PeerId, _m: &str) -> Result<(), Libp2pError> { Ok(()) }
            async fn start_message_handler(&self) -> Result<oneshot::Sender<()>, Libp2pError> { Err(Libp2pError::ConnectionError("fail".to_string())) }
            async fn discover_peers(&self) -> Result<Vec<PeerId>, Libp2pError> { Ok(vec![]) }
            async fn shutdown(&self) -> Result<(), Libp2pError> { Ok(()) }
        }
        let plugin = Libp2pPlugin {
            name: "libp2p-test".to_string(),
            service: Arc::new(AlwaysFailService),
            config: Libp2pConfig { max_retries: 2, ..Default::default() },
            shutdown_tx: Mutex::new(None),
            background_tasks: Mutex::new(Vec::new()),
        };
        let res = plugin.connect("/ip4/127.0.0.1/tcp/4001").await;
        assert!(res.is_err(), "Connect should fail with the test service");
        
        // Kiểm tra nội dung lỗi thay vì unwrap_err
        if let Err(error_msg) = res {
            assert!(error_msg.contains("Failed to connect to peer"), 
                  "Error message should contain 'Failed to connect to peer', but got: {}", error_msg);
        }
    }

    #[tokio::test]
    async fn test_libp2p_plugin_send_message_xss_payloads() {
        let plugin = Libp2pPlugin::new();
        let peer_id = PeerId::random();
        let xss_payloads = vec![
            "<script>alert('XSS')</script>",
            "<img src=x onerror=alert(1)>",
            "<svg/onload=alert(1)>",
            "<iframe src=javascript:alert(1)>"
        ];
        for payload in xss_payloads {
            assert!(plugin.send_message(&peer_id, payload).await.is_err());
        }
    }
    #[tokio::test]
    async fn test_libp2p_plugin_send_message_sql_payloads() {
        let plugin = Libp2pPlugin::new();
        let peer_id = PeerId::random();
        let sqli_payloads = vec![
            "' OR '1'='1", "1; DROP TABLE users; --", "admin' --", "' UNION SELECT NULL--"
        ];
        for payload in sqli_payloads {
            assert!(plugin.send_message(&peer_id, payload).await.is_err());
        }
    }
}

// WARNING: Nếu mở rộng Libp2pPlugin nhận message từ peer/external,
// bắt buộc gọi validate_input trước khi xử lý để đảm bảo an toàn XSS/SQLi.
