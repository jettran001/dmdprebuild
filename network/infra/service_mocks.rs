//! # Service Mocks
//! 
//! Tập hợp các mock implementation chuẩn cho tất cả các trait service trong domain network.
//! Module này là nơi tập trung các implementation giả lập cho các service,
//! sử dụng các macro để giảm code trùng lặp và nhất quán hóa logic mock.
//!
//! ## Cách sử dụng
//!
//! ```
//! // Import các mock implementation cần thiết
//! use crate::infra::service_mocks::{DefaultRedisService, DefaultIpfsService};
//! use crate::infra::service_traits::{RedisService, IpfsService};
//!
//! // Sử dụng implementation mock
//! let redis_service = DefaultRedisService::new();
//! redis_service.connect("redis://localhost:6379").expect("Failed to connect");
//! ```

use crate::infra::service_traits::{WebrtcService, ServiceError, RedisService, IpfsService, WasmService, MessagingKafkaService, MessagingMqttService, ExecutionAdapter, AiAdapter, MasterNodeService, SlaveNodeService, SchedulerService, DiscoveryService};
use crate::config::types::{RedisConfig, IpfsConfig, WasmConfig};
use crate::infra::service_traits::WebRtcConfig;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use tracing::{info, warn, error};
use async_trait::async_trait;

/// Macro để tạo implementation cơ bản cho các method của một trait
#[macro_export]
macro_rules! basic_mock_method {
    ($service_type:expr, $method_name:expr) => {
        info!("[{}] {}", $service_type, $method_name);
        Ok(())
    };
    
    ($service_type:expr, $method_name:expr, $return_type:ty) => {
        info!("[{}] {}", $service_type, $method_name);
        Ok(Default::default())
    };
}

/// Default implementation của RedisService
pub struct DefaultRedisService {
    /// Status of connection
    pub is_connected: bool,
    /// Mock Redis storage
    pub storage: Arc<Mutex<HashMap<String, String>>>,
}

impl DefaultRedisService {
    /// Create a new DefaultRedisService
    pub fn new() -> Self {
        Self {
            is_connected: false,
            storage: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl RedisService for DefaultRedisService {
    async fn connect(&self, url: &str) -> Result<(), ServiceError> {
        info!("[DefaultRedisService] Connecting to Redis at {}", url);
        Ok(())
    }
    
    async fn connect_with_auth(&self, url: &str, username: &Option<String>, password: &Option<String>) -> Result<(), ServiceError> {
        info!("[DefaultRedisService] Connecting to Redis at {} with auth", url);
        if let Some(username) = username {
            info!("[DefaultRedisService] Using username: {}", username);
        }
        if let Some(_) = password {
            info!("[DefaultRedisService] Using password: [REDACTED]");
        }
        Ok(())
    }
    
    async fn set(&self, key: &str, value: &str) -> Result<(), ServiceError> {
        info!("[DefaultRedisService] Set key: {} value: {}", key, value);
        
        match self.storage.lock() {
            Ok(mut storage) => {
                storage.insert(key.to_string(), value.to_string());
                Ok(())
            },
            Err(_) => Err(ServiceError::InternalError("Failed to acquire storage lock during set operation".to_string())),
        }
    }
    
    async fn get(&self, key: &str) -> Result<Option<String>, ServiceError> {
        info!("[DefaultRedisService] Get key: {}", key);
        
        match self.storage.lock() {
            Ok(storage) => {
                Ok(storage.get(key).cloned())
            },
            Err(_) => Err(ServiceError::InternalError("Failed to acquire storage lock during get operation".to_string())),
        }
    }
    
    async fn health_check(&self) -> Result<bool, ServiceError> {
        Ok(true)
    }
    
    async fn ping_with_timeout(&self, timeout: std::time::Duration) -> Result<bool, ServiceError> {
        info!("[DefaultRedisService] Ping with timeout: {:?}", timeout);
        Ok(true)
    }
    
    async fn ping(&self) -> Result<(), ServiceError> {
        info!("[DefaultRedisService] Ping Redis server (basic health check)");
        Ok(())
    }
}

/// Default implementation của IpfsService
pub struct DefaultIpfsService {
    /// Status of connection
    pub is_connected: bool,
    /// Mock IPFS storage (CID -> Data)
    pub storage: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl DefaultIpfsService {
    /// Create a new DefaultIpfsService
    pub fn new() -> Self {
        Self {
            is_connected: false,
            storage: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    /// Generate a fake CID for mock purposes
    fn generate_fake_cid(&self) -> String {
        // Simple mock CID generator 
        let random_part = uuid::Uuid::new_v4().to_string();
        format!("Qm{}", &random_part[..20])
    }
}

#[async_trait::async_trait]
impl IpfsService for DefaultIpfsService {
    async fn connect(&self, url: &str) -> Result<(), ServiceError> {
        info!("[DefaultIpfsService] Connecting to IPFS at {}", url);
        Ok(())
    }
    async fn add(&self, data: &[u8]) -> Result<String, ServiceError> {
        info!("[DefaultIpfsService] Adding data to IPFS, size: {} bytes", data.len());
        
        // Tạo CID giả và lưu vào storage mock
        let cid = self.generate_fake_cid();
        
        match self.storage.lock() {
            Ok(mut storage) => {
                storage.insert(cid.clone(), data.to_vec());
                Ok(cid)
            },
            Err(_) => Err(ServiceError::InternalError("Failed to acquire storage lock during add operation".to_string())),
        }
    }
    async fn get(&self, cid: &str) -> Result<Vec<u8>, ServiceError> {
        info!("[DefaultIpfsService] Getting data from IPFS, CID: {}", cid);
        
        // Lấy từ storage mock
        match self.storage.lock() {
            Ok(storage) => {
                if let Some(data) = storage.get(cid) {
                    Ok(data.clone())
                } else {
                    Err(ServiceError::NotFoundError(format!("CID not found: {}", cid)))
                }
            },
            Err(_) => Err(ServiceError::InternalError("Failed to acquire storage lock during get operation".to_string())),
        }
    }
    async fn pin(&self, cid: &str) -> Result<(), ServiceError> {
        info!("[DefaultIpfsService] Pinning CID: {}", cid);
        
        // Kiểm tra CID có tồn tại không
        match self.storage.lock() {
            Ok(storage) => {
                if storage.contains_key(cid) {
                    Ok(())
                } else {
                    Err(ServiceError::NotFoundError(format!("CID not found: {}", cid)))
                }
            },
            Err(_) => Err(ServiceError::InternalError("Failed to acquire storage lock during pin operation".to_string())),
        }
    }
    async fn health_check(&self) -> Result<bool, ServiceError> {
        Ok(true)
    }
}

/// Default implementation của WebrtcService
pub struct DefaultWebrtcService {
    /// Status of WebRTC initialization
    pub is_initialized: bool,
    /// Mock peer connections (ID -> Config)
    pub connections: Arc<Mutex<HashMap<String, WebRtcConfig>>>,
}

impl DefaultWebrtcService {
    /// Create a new DefaultWebrtcService
    pub fn new() -> Self {
        Self {
            is_initialized: false,
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl WebrtcService for DefaultWebrtcService {
    async fn init(&self) -> Result<(), ServiceError> {
        info!("[DefaultWebrtcService] Initializing WebRTC");
        Ok(())
    }
    
    async fn create_peer_connection(&self, config: &WebRtcConfig) -> Result<String, ServiceError> {
        info!("[DefaultWebrtcService] Creating peer connection");
        
        // Tạo ID kết nối và lưu config vào connections
        let connection_id = uuid::Uuid::new_v4().to_string();
        
        match self.connections.lock() {
            Ok(mut connections) => {
                connections.insert(connection_id.clone(), config.clone());
                Ok(connection_id)
            },
            Err(_) => Err(ServiceError::InternalError("Failed to acquire connections lock during create_peer_connection".to_string())),
        }
    }
    
    async fn add_ice_candidate(&self, connection_id: &str, candidate: &str) -> Result<(), ServiceError> {
        info!("[DefaultWebrtcService] Adding ICE candidate for connection: {}", connection_id);
        
        match self.connections.lock() {
            Ok(connections) => {
                if connections.contains_key(connection_id) {
                    Ok(())
                } else {
                    Err(ServiceError::NotFoundError(format!("Connection ID not found: {}", connection_id)))
                }
            },
            Err(_) => Err(ServiceError::InternalError("Failed to acquire connections lock during add_ice_candidate".to_string())),
        }
    }
    
    async fn send_data(&self, connection_id: &str, data: &[u8]) -> Result<(), ServiceError> {
        info!("[DefaultWebrtcService] Sending data through connection: {}, size: {} bytes", connection_id, data.len());
        
        match self.connections.lock() {
            Ok(connections) => {
                if connections.contains_key(connection_id) {
                    Ok(())
                } else {
                    Err(ServiceError::NotFoundError(format!("Connection ID not found: {}", connection_id)))
                }
            },
            Err(_) => Err(ServiceError::InternalError("Failed to acquire connections lock during send_data".to_string())),
        }
    }
    
    async fn health_check(&self) -> Result<bool, ServiceError> {
        Ok(true)
    }
    
    async fn close_connection(&self, connection_id: &str) -> Result<(), ServiceError> {
        info!("[DefaultWebrtcService] Closing connection: {}", connection_id);
        
        match self.connections.lock() {
            Ok(mut connections) => {
                if connections.remove(connection_id).is_some() {
                    Ok(())
                } else {
                    Err(ServiceError::NotFoundError(format!("Connection ID not found: {}", connection_id)))
                }
            },
            Err(_) => Err(ServiceError::InternalError("Failed to acquire connections lock during close_connection".to_string())),
        }
    }
    
    async fn close_all_connections(&self) -> Result<usize, ServiceError> {
        info!("[DefaultWebrtcService] Closing all connections");
        
        match self.connections.lock() {
            Ok(mut connections) => {
                let count = connections.len();
                connections.clear();
                Ok(count)
            },
            Err(_) => Err(ServiceError::InternalError("Failed to acquire connections lock during close_all_connections".to_string())),
        }
    }
}

/// Default implementation của WasmService
pub struct DefaultWasmService {
    /// Status of Wasm runtime initialization
    pub is_initialized: bool,
    /// Mock loaded modules (ID -> Size)
    pub modules: Arc<Mutex<HashMap<String, usize>>>,
}

impl DefaultWasmService {
    /// Create a new DefaultWasmService
    pub fn new() -> Self {
        Self {
            is_initialized: false,
            modules: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl WasmService for DefaultWasmService {
    async fn init(&self) -> Result<(), ServiceError> {
        info!("[DefaultWasmService] Initializing Wasm runtime");
        Ok(())
    }
    
    async fn load_module(&self, bytes: &[u8]) -> Result<String, ServiceError> {
        info!("[DefaultWasmService] Loading Wasm module, size: {} bytes", bytes.len());
        
        let module_id = uuid::Uuid::new_v4().to_string();
        
        match self.modules.lock() {
            Ok(mut modules) => {
                modules.insert(module_id.clone(), bytes.len());
                Ok(module_id)
            },
            Err(_) => Err(ServiceError::InternalError("Failed to acquire modules lock during load_module".to_string())),
        }
    }
    
    async fn execute_function(&self, module_id: &str, function_name: &str, params: &[u8]) -> Result<Vec<u8>, ServiceError> {
        info!("[DefaultWasmService] Executing function '{}' in module: {}, params size: {} bytes", 
            function_name, module_id, params.len());
        
        match self.modules.lock() {
            Ok(modules) => {
                if modules.contains_key(module_id) {
                    // Mock: Return params as result for now
                    Ok(params.to_vec())
                } else {
                    Err(ServiceError::NotFoundError(format!("Module ID not found: {}", module_id)))
                }
            },
            Err(_) => Err(ServiceError::InternalError("Failed to acquire modules lock during execute_function".to_string())),
        }
    }
    
    async fn unload_module(&self, module_id: &str) -> Result<(), ServiceError> {
        info!("[DefaultWasmService] Unloading module: {}", module_id);
        
        match self.modules.lock() {
            Ok(mut modules) => {
                if modules.remove(module_id).is_some() {
                    Ok(())
                } else {
                    Err(ServiceError::NotFoundError(format!("Module ID not found: {}", module_id)))
                }
            },
            Err(_) => Err(ServiceError::InternalError("Failed to acquire modules lock during unload_module".to_string())),
        }
    }
    
    async fn health_check(&self) -> Result<bool, ServiceError> {
        Ok(true)
    }
}

/// Default implementation của MessagingKafkaService
pub struct DefaultMessagingKafkaService {
    /// Status of connection
    pub is_connected: bool,
    /// Mock subscriptions (ID -> (Topic, Callback count))
    pub subscriptions: Arc<Mutex<HashMap<String, (String, usize)>>>,
}

impl DefaultMessagingKafkaService {
    /// Create a new DefaultMessagingKafkaService
    pub fn new() -> Self {
        Self {
            is_connected: false,
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl MessagingKafkaService for DefaultMessagingKafkaService {
    async fn connect(&self, _brokers: &[String]) -> Result<(), ServiceError> { Ok(()) }
    async fn send(&self, _topic: &str, _message: &[u8]) -> Result<(), ServiceError> { Ok(()) }
    async fn subscribe(&self, _topic: &str, _callback: Box<dyn Fn(&[u8]) + Send + Sync>) -> Result<String, ServiceError> { Ok("mock_sub_id".to_string()) }
    async fn unsubscribe(&self, _subscription_id: &str) -> Result<(), ServiceError> { Ok(()) }
    async fn health_check(&self) -> Result<bool, ServiceError> { Ok(true) }
}

/// Default implementation của MessagingMqttService
pub struct DefaultMessagingMqttService {
    /// Status of connection
    pub is_connected: bool,
    /// Mock subscriptions (ID -> (Topic, QoS, Callback count))
    pub subscriptions: Arc<Mutex<HashMap<String, (String, u8, usize)>>>,
}

impl DefaultMessagingMqttService {
    /// Create a new DefaultMessagingMqttService
    pub fn new() -> Self {
        Self {
            is_connected: false,
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl MessagingMqttService for DefaultMessagingMqttService {
    async fn connect(&self, _url: &str) -> Result<(), ServiceError> { Ok(()) }
    async fn connect_with_auth(&self, _url: &str, _username: &str, _password: &str) -> Result<(), ServiceError> { Ok(()) }
    async fn publish(&self, _topic: &str, _message: &[u8], _qos: u8) -> Result<(), ServiceError> { Ok(()) }
    async fn subscribe(&self, _topic: &str, _qos: u8, _callback: Box<dyn Fn(&str, &[u8]) + Send + Sync>) -> Result<String, ServiceError> { Ok("mock_sub_id".to_string()) }
    async fn unsubscribe(&self, _subscription_id: &str) -> Result<(), ServiceError> { Ok(()) }
    async fn health_check(&self) -> Result<bool, ServiceError> { Ok(true) }
}

/// Default implementation của ExecutionAdapter
pub struct DefaultExecutionAdapter;

#[async_trait::async_trait]
impl ExecutionAdapter for DefaultExecutionAdapter {
    async fn init(&self) -> Result<(), ServiceError> { Ok(()) }
    async fn execute(&self, _command: &str, _params: &[String]) -> Result<String, ServiceError> { Ok("mock_result".to_string()) }
    async fn check_status(&self, _execution_id: &str) -> Result<String, ServiceError> { Ok("mock_status".to_string()) }
    async fn cancel(&self, _execution_id: &str) -> Result<(), ServiceError> { Ok(()) }
    async fn health_check(&self) -> Result<bool, ServiceError> { Ok(true) }
}

/// Default implementation của AiAdapter
pub struct DefaultAiAdapter;

#[async_trait::async_trait]
impl AiAdapter for DefaultAiAdapter {
    async fn init(&self) -> Result<(), ServiceError> { Ok(()) }
    async fn run_inference(&self, _model: &str, _input: &[u8]) -> Result<Vec<u8>, ServiceError> { Ok(vec![]) }
    async fn check_gpu(&self) -> Result<bool, ServiceError> { Ok(true) }
    async fn get_model_info(&self, _model: &str) -> Result<String, ServiceError> { Ok("mock_info".to_string()) }
    async fn health_check(&self) -> Result<bool, ServiceError> { Ok(true) }
} 