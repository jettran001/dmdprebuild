//! # Service Traits
//! 
//! Tập hợp các trait chuẩn cho tất cả các service trong domain network.
//! Module này là nơi tập trung định nghĩa các interface (trait) cho tất cả các service
//! để đảm bảo tính nhất quán và tránh trùng lặp code giữa các module.
//!
//! ## Cách sử dụng
//!
//! ```
//! // Import các trait cần thiết
//! use crate::infra::service_traits::{RedisService, IpfsService, WasmService, WebrtcService};
//!
//! // Implement trait cho struct cụ thể
//! #[async_trait::async_trait]
//! impl RedisService for MyRedisImplementation {
//!     // Implement các method theo yêu cầu của trait
//! }
//! ```

use async_trait::async_trait;
use thiserror::Error;
use tokio::time::Duration;
use serde::{Serialize, Deserialize};

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum ServiceError {
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    #[error("Timeout error: {0}")]
    TimeoutError(String),
    
    #[error("Authentication error: {0}")]
    AuthError(String),
    
    #[error("Resource not found: {0}")]
    NotFoundError(String),
    
    #[error("Validation error: {0}")]
    ValidationError(String),
    
    #[error("Rate limit exceeded: {0}")]
    RateLimitError(String),
    
    #[error("Internal error: {0}")]
    InternalError(String),
    
    #[error("Session error: {0}")]
    SessionError(String),
    
    #[error("Data channel error: {0}")]
    DataChannelError(String),
    
    #[error("Signal error: {0}")]
    SignalError(String),

    #[error("Security error: {0}")]
    SecurityError(String),
}

// Timeout constants
const DEFAULT_TIMEOUT_MS: u64 = 5000; // 5 seconds default timeout

/// Trait cho Redis service
#[async_trait]
pub trait RedisService: Send + Sync + 'static {
    /// Connect to Redis
    async fn connect(&self, url: &str) -> Result<(), ServiceError>;
    /// Connect to Redis with authentication
    async fn connect_with_auth(&self, url: &str, username: &Option<String>, password: &Option<String>) -> Result<(), ServiceError>;
    /// Set a key-value pair
    async fn set(&self, key: &str, value: &str) -> Result<(), ServiceError>;
    /// Get a value by key
    async fn get(&self, key: &str) -> Result<Option<String>, ServiceError>;
    /// Health check for this service
    async fn health_check(&self) -> Result<bool, ServiceError>;
    /// Ping Redis server with timeout (health check with custom timeout)
    async fn ping_with_timeout(&self, timeout: Duration) -> Result<bool, ServiceError>;
    /// Ping Redis server (basic health check)
    async fn ping(&self) -> Result<(), ServiceError>;
    /// Reconnect to Redis server
    async fn reconnect(&self) -> Result<(), ServiceError>;
}

/// Trait cho IPFS service
#[async_trait]
pub trait IpfsService: Send + Sync + 'static {
    /// Connect to IPFS
    async fn connect(&self, url: &str) -> Result<(), ServiceError>;
    /// Add data to IPFS
    async fn add(&self, data: &[u8]) -> Result<String, ServiceError>;
    /// Get data from IPFS by CID
    async fn get(&self, cid: &str) -> Result<Vec<u8>, ServiceError>;
    /// Pin a CID to ensure data persistence
    async fn pin(&self, cid: &str) -> Result<(), ServiceError>;
    /// Health check for this service
    async fn health_check(&self) -> Result<bool, ServiceError>;
}

/// Trait cho WebRTC service
#[async_trait]
pub trait WebRtcService: Send + Sync + 'static {
    /// Initialize WebRTC
    async fn init(&self) -> Result<(), ServiceError>;
    /// Create a new peer connection
    async fn create_peer_connection(&self, config: &WebRtcConfig) -> Result<String, ServiceError>;
    /// Add ICE candidate
    async fn add_ice_candidate(&self, connection_id: &str, candidate: &str) -> Result<(), ServiceError>;
    /// Send data through a peer connection
    async fn send_data(&self, connection_id: &str, data: &[u8]) -> Result<(), ServiceError>;
    /// Health check for this service
    async fn health_check(&self) -> Result<bool, ServiceError>;
    /// Close a specific connection
    async fn close_connection(&self, connection_id: &str) -> Result<(), ServiceError>;
    /// Close all connections
    async fn close_all_connections(&self) -> Result<usize, ServiceError>;
}

/// Trait cho Wasm service
#[async_trait]
pub trait WasmService: Send + Sync + 'static {
    /// Initialize the Wasm runtime
    async fn init(&self) -> Result<(), ServiceError>;
    /// Load a Wasm module from bytes
    async fn load_module(&self, bytes: &[u8]) -> Result<String, ServiceError>;
    /// Execute a function in a loaded module
    async fn execute_function(&self, module_id: &str, function_name: &str, params: &[u8]) -> Result<Vec<u8>, ServiceError>;
    /// Unload a module from memory
    async fn unload_module(&self, module_id: &str) -> Result<(), ServiceError>;
    /// Health check for this service
    async fn health_check(&self) -> Result<bool, ServiceError>;
}

/// Trait cho Kafka messaging service
#[async_trait]
pub trait MessagingKafkaService: Send + Sync + 'static {
    /// Connect to Kafka
    async fn connect(&self, brokers: &[String]) -> Result<(), ServiceError>;
    /// Send a message to a Kafka topic
    async fn send(&self, topic: &str, message: &[u8]) -> Result<(), ServiceError>;
    /// Subscribe to a Kafka topic
    async fn subscribe(&self, topic: &str, callback: Box<dyn Fn(&[u8]) + Send + Sync>) -> Result<String, ServiceError>;
    /// Unsubscribe from a Kafka topic
    async fn unsubscribe(&self, subscription_id: &str) -> Result<(), ServiceError>;
    /// Health check for this service
    async fn health_check(&self) -> Result<bool, ServiceError>;
}

/// Trait cho MQTT messaging service
#[async_trait]
pub trait MessagingMqttService: Send + Sync + 'static {
    /// Connect to MQTT broker
    async fn connect(&self, url: &str) -> Result<(), ServiceError>;
    /// Connect to MQTT broker with authentication
    async fn connect_with_auth(&self, url: &str, username: &str, password: &str) -> Result<(), ServiceError>;
    /// Publish a message to an MQTT topic
    async fn publish(&self, topic: &str, message: &[u8], qos: u8) -> Result<(), ServiceError>;
    /// Subscribe to an MQTT topic
    async fn subscribe(&self, topic: &str, qos: u8, callback: Box<dyn Fn(&str, &[u8]) + Send + Sync>) -> Result<String, ServiceError>;
    /// Unsubscribe from an MQTT topic
    async fn unsubscribe(&self, subscription_id: &str) -> Result<(), ServiceError>;
    /// Health check for this service
    async fn health_check(&self) -> Result<bool, ServiceError>;
}

/// Trait cho Execution adapter
#[async_trait]
pub trait ExecutionAdapter: Send + Sync + 'static {
    /// Initialize execution adapter
    async fn init(&self) -> Result<(), ServiceError>;
    /// Execute a command
    async fn execute(&self, command: &str, params: &[String]) -> Result<String, ServiceError>;
    /// Check status of execution
    async fn check_status(&self, execution_id: &str) -> Result<String, ServiceError>;
    /// Cancel execution
    async fn cancel(&self, execution_id: &str) -> Result<(), ServiceError>;
    /// Health check for this adapter
    async fn health_check(&self) -> Result<bool, ServiceError>;
}

/// Trait cho AI adapter
#[async_trait]
pub trait AiAdapter: Send + Sync + 'static {
    /// Initialize AI adapter
    async fn init(&self) -> Result<(), ServiceError>;
    /// Run inference with AI model
    async fn run_inference(&self, model: &str, input: &[u8]) -> Result<Vec<u8>, ServiceError>;
    /// Check GPU availability
    async fn check_gpu(&self) -> Result<bool, ServiceError>;
    /// Get model information
    async fn get_model_info(&self, model: &str) -> Result<String, ServiceError>;
    /// Health check for this adapter
    async fn health_check(&self) -> Result<bool, ServiceError>;
}

/// Trait cho Master node service
#[async_trait]
pub trait MasterNodeService: Send + Sync + 'static {
    /// Initialize master node
    async fn init(&self) -> Result<(), ServiceError>;
    /// Register a slave node
    async fn register_slave(&self, node_id: &str, node_info: &str) -> Result<(), ServiceError>;
    /// Unregister a slave node
    async fn unregister_slave(&self, node_id: &str) -> Result<(), ServiceError>;
    /// Get all registered slave nodes
    async fn get_slaves(&self) -> Result<Vec<String>, ServiceError>;
    /// Send command to a slave node
    async fn send_command(&self, node_id: &str, command: &str) -> Result<String, ServiceError>;
    /// Health check for this service
    async fn health_check(&self) -> Result<bool, ServiceError>;
}

/// Trait cho Slave node service
#[async_trait]
pub trait SlaveNodeService: Send + Sync + 'static {
    /// Initialize slave node
    async fn init(&self) -> Result<(), ServiceError>;
    /// Register with master node
    async fn register_with_master(&self, master_url: &str) -> Result<(), ServiceError>;
    /// Unregister from master node
    async fn unregister_from_master(&self) -> Result<(), ServiceError>;
    /// Execute command received from master
    async fn execute_command(&self, command: &str) -> Result<String, ServiceError>;
    /// Report status to master
    async fn report_status(&self) -> Result<(), ServiceError>;
    /// Health check for this service
    async fn health_check(&self) -> Result<bool, ServiceError>;
}

/// Trait cho Scheduler service
#[async_trait]
pub trait SchedulerService: Send + Sync + 'static {
    /// Initialize scheduler
    async fn init(&self) -> Result<(), ServiceError>;
    /// Schedule a task
    async fn schedule_task(&self, task_type: &str, task_data: &str, priority: u8) -> Result<String, ServiceError>;
    /// Cancel a scheduled task
    async fn cancel_task(&self, task_id: &str) -> Result<(), ServiceError>;
    /// Get task status
    async fn get_task_status(&self, task_id: &str) -> Result<String, ServiceError>;
    /// List all scheduled tasks
    async fn list_tasks(&self) -> Result<Vec<String>, ServiceError>;
    /// Health check for this service
    async fn health_check(&self) -> Result<bool, ServiceError>;
}

/// Trait cho Discovery service
#[async_trait]
pub trait DiscoveryService: Send + Sync + 'static {
    /// Initialize discovery service
    async fn init(&self) -> Result<(), ServiceError>;
    /// Register a node
    async fn register_node(&self, node_id: &str, node_info: &str) -> Result<(), ServiceError>;
    /// Unregister a node
    async fn unregister_node(&self, node_id: &str) -> Result<(), ServiceError>;
    /// Discover nodes with specific criteria
    async fn discover_nodes(&self, criteria: &str) -> Result<Vec<String>, ServiceError>;
    /// Check if a node is alive
    async fn is_node_alive(&self, node_id: &str) -> Result<bool, ServiceError>;
    /// Health check for this service
    async fn health_check(&self) -> Result<bool, ServiceError>;
}

/// Configuration for WebRTC
#[derive(Debug, Clone)]
pub struct WebRtcConfig {
    /// ICE servers
    pub ice_servers: Vec<String>,
    /// STUN servers
    pub stun_servers: Vec<String>,
    /// TURN servers
    pub turn_servers: Vec<String>,
    /// Use DTLS (true/false)
    pub use_dtls: bool,
    /// Timeout in milliseconds
    pub timeout_ms: u64,
    /// Max packet size in bytes
    pub max_packet_size: usize,
    /// Maximum number of concurrent connections to prevent socket exhaustion
    pub max_concurrent_connections: usize,
}

impl Default for WebRtcConfig {
    fn default() -> Self {
        Self {
            ice_servers: vec![],
            stun_servers: vec![],
            turn_servers: vec![],
            use_dtls: true,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_packet_size: 1024 * 1024, // 1MB default max packet size
            max_concurrent_connections: 100, // 100 default max concurrent connections
        }
    }
} 