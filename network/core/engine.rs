// src/core/engine.rs
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn, error};
use crate::config::LoggingConfig;
use tracing_subscriber::fmt;
use tracing_subscriber::EnvFilter;
use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{RwLock, Mutex};
use serde::{Serialize, Deserialize};
use uuid::Uuid;
use std::any::Any;

use crate::config::{NetworkConfig, core::NetworkCoreConfig};

pub enum EngineError {
    PluginStartFailed(String),
    PluginNameEmpty,
    LockError(String),
    InternalError(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::PluginStartFailed(msg) => write!(f, "Failed to start plugin: {}", msg),
            EngineError::PluginNameEmpty => write!(f, "Plugin name cannot be empty"),
            EngineError::LockError(msg) => write!(f, "Lock acquisition failed: {}", msg),
            EngineError::InternalError(msg) => write!(f, "Internal engine error: {}", msg),
        }
    }
}

/// Represents the status of a plugin
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PluginStatus {
    Active,
    Inactive,
    Failed,
    Unknown,
}

/// NetworkEngine trait định nghĩa các chức năng cốt lõi của node
#[async_trait]
pub trait NetworkEngine: Send + Sync {
    /// Initialize the engine
    async fn init(&self, config: &NetworkCoreConfig) -> Result<()>;
    
    /// Start the engine
    async fn start(&self) -> Result<()>;
    
    /// Stop the engine
    async fn stop(&self) -> Result<()>;
    
    /// Register a plugin
    async fn register_plugin(&self, plugin: Box<dyn Plugin>) -> Result<PluginId>;
    
    /// Get plugin by ID
    async fn get_plugin(&self, id: PluginId) -> Option<Box<dyn Plugin>>;
    
    /// Get all plugins
    async fn get_all_plugins(&self) -> Vec<Box<dyn Plugin>>;
    
    /// Remove a plugin
    async fn remove_plugin(&self, id: PluginId) -> Result<()>;

    async fn get_all_plugin_statuses(&self) -> Result<std::collections::HashMap<crate::core::engine::PluginType, crate::core::engine::PluginStatus>, EngineError>;

    async fn get_metrics(&self) -> Result<String, EngineError>;

    async fn list_plugins(&self) -> Result<Vec<crate::core::engine::PluginType>, EngineError>;

    async fn check_plugin_health(&self, plugin_type: &crate::core::engine::PluginType) -> Result<bool, EngineError>;

    async fn unregister_plugin(&self, plugin_type: &crate::core::engine::PluginType) -> Result<(), EngineError>;
}

/// NetworkMessage định nghĩa format message giữa các node
#[derive(Debug, Clone)]
pub struct NetworkMessage {
    pub from: String,
    pub to: String,
    pub message_type: MessageType,
    pub payload: Vec<u8>,
    pub timestamp: u64,
}

/// MessageType định nghĩa các loại message
#[derive(Debug, Clone)]
pub enum MessageType {
    Heartbeat,
    Data,
    Control,
    Discovery,
    Sync,
}

/// NetworkPlugin trait cho các plugin
#[async_trait]
pub trait NetworkPlugin {
    /// Tên plugin
    fn name(&self) -> &str;
    
    /// Khởi tạo plugin
    async fn init(&self) -> Result<()>;
    
    /// Xử lý message
    async fn handle_message(&self, message: &NetworkMessage) -> Result<()>;
}

/// Plugin trait cơ bản cho tất cả plugin trong NetworkEngine
pub trait Plugin: Send + Sync {
    /// Tên plugin
    fn name(&self) -> &str;
    
    /// Loại plugin
    fn plugin_type(&self) -> PluginType;
    
    /// Khởi động plugin
    fn start(&self) -> Result<bool, PluginError>;
    
    /// Dừng plugin
    fn stop(&self) -> Result<(), PluginError>;
    
    /// Kiểm tra sức khỏe plugin
    /// Returns Ok(true) nếu plugin đang hoạt động tốt, Ok(false) hoặc Err nếu có vấn đề
    fn check_health(&self) -> Result<bool, PluginError> {
        // Implement mặc định trả về Ok(true) để các plugin không cần implement lại nếu không cần
        Ok(true)
    }
    
    /// Convert plugin to &dyn Any để hỗ trợ downcasting
    fn as_any(&self) -> &dyn std::any::Any where Self: 'static {
        self
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum PluginType {
    Grpc,
    Wasm,
    WebRtc,
    WebSocket,
    Ipfs,
    Redis,
    Libp2p,
    Unknown,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("Plugin startup failed")]
    StartFailed,
    #[error("Plugin not initialized")]
    NotInitialized,
    #[error("Plugin configuration error: {0}")]
    ConfigError(String),
    #[error("Plugin internal error: {0}")]
    InternalError(String),
}

/// MockPlugin cho testing và bootstrap
pub struct MockPlugin {
    name: String,
}

impl MockPlugin {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl Plugin for MockPlugin {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn plugin_type(&self) -> PluginType {
        match self.name.as_str() {
            "redis" => PluginType::Redis,
            "ipfs" => PluginType::Ipfs,
            "wasm" => PluginType::Wasm,
            "webrtc" => PluginType::WebRtc,
            "ws" => PluginType::WebSocket,
            "grpc" => PluginType::Grpc,
            "libp2p" => PluginType::Libp2p,
            _ => PluginType::Unknown,
        }
    }
    
    fn start(&self) -> Result<bool, PluginError> {
        Ok(true)
    }
    
    fn stop(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

/// DefaultNetworkEngine implementation
pub struct DefaultNetworkEngine {
    config: Arc<RwLock<NetworkConfig>>,
    plugins: Arc<RwLock<Vec<Arc<dyn Plugin + Send + Sync>>>>,
    is_running: Arc<RwLock<bool>>,
    master_service: Arc<crate::node_manager::master::PartitionedMasterNodeService>,
    discovery_service: Arc<crate::node_manager::discovery::DefaultDiscoveryService>,
    scheduler_service: Arc<crate::node_manager::scheduler::DefaultSchedulerService>,
    plugin_status: Arc<RwLock<std::collections::HashMap<crate::core::engine::PluginType, crate::core::engine::PluginStatus>>>,
    plugin_last_update: Arc<RwLock<std::collections::HashMap<crate::core::engine::PluginType, std::time::Instant>>>,
}

impl DefaultNetworkEngine {
    pub fn new() -> Self {
        Self {
            config: Arc::new(RwLock::new(NetworkConfig::default())),
            plugins: Arc::new(RwLock::new(Vec::new())),
            is_running: Arc::new(RwLock::new(false)),
            master_service: Arc::new(crate::node_manager::master::PartitionedMasterNodeService::new()),
            discovery_service: Arc::new(crate::node_manager::discovery::DefaultDiscoveryService::default()),
            scheduler_service: Arc::new(crate::node_manager::scheduler::DefaultSchedulerService::default()),
            plugin_status: Arc::new(RwLock::new(std::collections::HashMap::new())),
            plugin_last_update: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    pub fn init_logging() {
        let logging = LoggingConfig::load();
        if let Some(ref file_path) = logging.log_file {
            match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(file_path)
            {
                Ok(file) => {
                    let writer = move || match file.try_clone() {
                        Ok(cloned) => cloned,
                        Err(e) => {
                            eprintln!("Warning: Could not clone log file handle: {}", e);
                            // Fallback để tránh crash hệ thống khi không thể clone file
                            std::io::stderr()
                        }
                    };
                    tracing_subscriber::fmt()
                        .with_env_filter(EnvFilter::new(logging.level))
                        .with_ansi(logging.format == "ansi")
                        .with_writer(writer)
                        .init();
                }
                Err(e) => {
                    eprintln!("Warning: Could not open log file '{}': {}", file_path, e);
                    // Fallback to stderr if file cannot be opened
                    tracing_subscriber::fmt()
                        .with_env_filter(EnvFilter::new(logging.level))
                        .with_ansi(logging.format == "ansi")
                        .init();
                }
            }
        } else {
            tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::new(logging.level))
                .with_ansi(logging.format == "ansi")
                .init();
        }
    }
}

#[async_trait]
impl NetworkEngine for DefaultNetworkEngine {
    async fn init(&self, config: &NetworkCoreConfig) -> Result<()> {
        let mut config_guard = self.config.write().await;
        
        // Tạo cấu hình mới từ NetworkCoreConfig
        let mut new_config = NetworkConfig::default();
        new_config.core = config.clone();
        
        // Cập nhật cấu hình hiện tại
        *config_guard = new_config;
        
        info!("Network engine initialized with config: {:?}", config);
        Ok(())
    }
    
    async fn start(&self) -> Result<()> {
        let mut running_guard = self.is_running.write().await;
        if *running_guard {
            return Ok(());
        }
        
        *running_guard = true;
        info!("Network engine started");
        
        // Khởi tạo các plugin
        let plugins = self.plugins.read().await;
        for plugin in plugins.iter() {
            if let Err(e) = plugin.init().await {
                error!("Failed to initialize plugin {}: {}", plugin.name(), e);
            }
        }
        
        Ok(())
    }
    
    async fn stop(&self) -> Result<()> {
        // Đánh dấu engine đang dừng
        let mut running_guard = self.is_running.write().await;
        if !*running_guard {
            info!("Network engine already stopped");
            return Ok(());
        }
        
        info!("Network engine stopping gracefully...");
        *running_guard = false;
        
        // Dừng tất cả các plugin theo thứ tự ngược lại
        let plugins = self.plugins.read().await;
        
        // Dừng từng plugin và đợi hoàn thành với timeout
        let mut stop_tasks = Vec::new();
        
        for plugin in plugins.iter() {
            let plugin_clone = plugin.clone();
            let plugin_name = plugin.name().to_owned();
            
            // Tạo task dừng plugin với timeout
            let stop_task = tokio::spawn(async move {
                info!("Stopping plugin: {}", plugin_name);
                match tokio::time::timeout(
                    tokio::time::Duration::from_secs(10), // 10s timeout
                    async move {
                        if let Err(e) = plugin_clone.stop() {
                            error!("Error stopping plugin {}: {}", plugin_name, e);
                        } else {
                            info!("Plugin {} stopped successfully", plugin_name);
                        }
                    }
                ).await {
                    Ok(_) => {
                        info!("Plugin {} shutdown complete", plugin_name);
                    },
                    Err(_) => {
                        error!("Plugin {} shutdown timed out after 10s", plugin_name);
                    }
                }
            });
            
            stop_tasks.push(stop_task);
        }
        
        // Đợi tất cả các plugin dừng với timeout tổng thể
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(30), // 30s timeout tổng thể
            futures::future::join_all(stop_tasks)
        ).await {
            Ok(_) => {
                info!("All plugins stopped successfully");
            },
            Err(_) => {
                error!("Timed out waiting for all plugins to stop after 30s");
            }
        }
        
        // Dừng các service
        info!("Stopping master service...");
        if let Err(e) = self.master_service.shutdown().await {
            error!("Error stopping master service: {}", e);
        }
        
        info!("Stopping discovery service...");
        if let Err(e) = self.discovery_service.shutdown().await {
            error!("Error stopping discovery service: {}", e);
        }
        
        info!("Stopping scheduler service...");
        if let Err(e) = self.scheduler_service.shutdown().await {
            error!("Error stopping scheduler service: {}", e);
        }
        
        // Xóa trạng thái các plugin
        {
            let mut status = self.plugin_status.write().await;
            status.clear();
        }
        
        {
            let mut last_update = self.plugin_last_update.write().await;
            last_update.clear();
        }
        
        info!("Network engine stopped successfully");
        Ok(())
    }
    
    async fn register_plugin(&self, plugin: Box<dyn Plugin>) -> Result<PluginId> {
        let mut plugins_guard = self.plugins.write().await;
        plugins_guard.push(plugin);
        Ok(PluginId::new())
    }

    async fn get_plugin(&self, id: PluginId) -> Option<Box<dyn Plugin>> {
        let plugins = self.plugins.read().await;
        
        // Tìm plugin theo id, tạm thời trả về None vì chưa triển khai đầy đủ
        None 
    }

    async fn get_all_plugins(&self) -> Vec<Box<dyn Plugin>> {
        let plugins = self.plugins.read().await;
        
        // Tạm thời trả về vector rỗng vì chưa triển khai đầy đủ
        Vec::new() 
    }

    async fn remove_plugin(&self, id: PluginId) -> Result<()> {
        let mut plugins = self.plugins.write().await;
        
        // Tạm thời trả về Ok vì chưa triển khai đầy đủ
        Ok(())
    }

    async fn get_all_plugin_statuses(&self) -> Result<std::collections::HashMap<crate::core::engine::PluginType, crate::core::engine::PluginStatus>, EngineError> {
        let status = self.plugin_status.read().await;
        Ok(status.clone())
    }

    async fn get_metrics(&self) -> Result<String, EngineError> {
        // Sử dụng cách an toàn hơn để lấy read lock cho plugins
        let plugins = self.plugins.read().await;
        
        // Sử dụng cách an toàn để lấy plugin statuses
        let statuses = self.get_all_plugin_statuses().await?;
        
        let mut metrics = String::new();
        
        // Add plugin count metrics
        metrics.push_str("# HELP network_plugin_count Number of registered plugins\n");
        metrics.push_str("# TYPE network_plugin_count gauge\n");
        metrics.push_str(&format!("network_plugin_count {}\n\n", statuses.len()));
        
        // Add plugin status metrics
        metrics.push_str("# HELP network_plugin_status Status of plugins (1 = active, 0 = inactive)\n");
        metrics.push_str("# TYPE network_plugin_status gauge\n");
        
        for (plugin_type, status) in statuses {
            let status_value = match status {
                PluginStatus::Active => 1,
                _ => 0,
            };
            metrics.push_str(&format!("network_plugin_status{{plugin=\"{:?}\"}} {}\n", plugin_type, status_value));
        }
        
        // Add count by status metrics
        metrics.push_str("\n# HELP network_plugins_by_status Count of plugins by status\n");
        metrics.push_str("# TYPE network_plugins_by_status gauge\n");
        
        let mut active_count = 0;
        let mut inactive_count = 0;
        
        for (_, status) in &statuses {
            if *status == PluginStatus::Active {
                active_count += 1;
            } else {
                inactive_count += 1;
            }
        }
        
        metrics.push_str(&format!("network_plugins_by_status{{status=\"active\"}} {}\n", active_count));
        metrics.push_str(&format!("network_plugins_by_status{{status=\"inactive\"}} {}\n", inactive_count));
        
        // Add system metrics
        metrics.push_str("\n# HELP network_system_info System information\n");
        metrics.push_str("# TYPE network_system_info gauge\n");
        metrics.push_str("network_system_info{version=\"1.0.0\"} 1\n");
        
        // Add thread metrics
        let num_cpus = num_cpus::get();
        metrics.push_str("\n# HELP network_system_cpus Number of CPU cores\n");
        metrics.push_str("# TYPE network_system_cpus gauge\n");
        metrics.push_str(&format!("network_system_cpus {}\n", num_cpus));
        
        // Add memory usage (simple, using std)
        #[cfg(target_os = "linux")]
        {
            if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
                for line in meminfo.lines() {
                    if line.starts_with("MemTotal") || line.starts_with("MemFree") {
                        // Xử lý an toàn khi trích xuất tên của metric
                        let metric_name = match line.split(':').next() {
                            Some(name) => name,
                            None => "mem_unknown"
                        };
                        metrics.push_str(&format!("# HELP network_{}\n", metric_name));
                        metrics.push_str(&format!("# TYPE network_{} gauge\n", metric_name));
                        
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            let key = parts[0];
                            let value = parts[1];
                            metrics.push_str(&format!("network_{} {}\n", key, value));
                        }
                    }
                }
            }
        }
        
        // Expose object count for plugin registry
        metrics.push_str("\n# HELP network_plugin_registry_object_count Số lượng plugin trong registry\n");
        metrics.push_str("# TYPE network_plugin_registry_object_count gauge\n");
        metrics.push_str(&format!("network_plugin_registry_object_count {}\n", plugins.len()));
        // Expose object count for node_manager/master.rs (slave count)
        if let Ok(slave_count) = crate::node_manager::master::get_slave_count(&self.master_service).await {
            metrics.push_str("# HELP network_slave_node_count Số lượng slave node trong master\n");
            metrics.push_str("# TYPE network_slave_node_count gauge\n");
            metrics.push_str(&format!("network_slave_node_count {}\n", slave_count));
        }
        // Expose object count for node_manager/discovery.rs (discovered node count)
        if let Ok(discovery_count) = crate::node_manager::discovery::get_discovery_node_count(&self.discovery_service).await {
            metrics.push_str("# HELP network_discovery_node_count Số lượng node discovery\n");
            metrics.push_str("# TYPE network_discovery_node_count gauge\n");
            metrics.push_str(&format!("network_discovery_node_count {}\n", discovery_count));
        }
        // Expose object count for node_manager/scheduler.rs (task count)
        if let Ok(task_count) = crate::node_manager::scheduler::get_task_node_count(&self.scheduler_service).await {
            metrics.push_str("# HELP network_task_node_count Số lượng node có task\n");
            metrics.push_str("# TYPE network_task_node_count gauge\n");
            metrics.push_str(&format!("network_task_node_count {}\n", task_count));
        }
        
        Ok(metrics)
    }

    async fn list_plugins(&self) -> Result<Vec<crate::core::engine::PluginType>, EngineError> {
        let plugin_info = self.list_plugin_info().await?;
        let plugin_types: Vec<PluginType> = plugin_info.into_iter().map(|(ptype, _, _)| ptype).collect();
        Ok(plugin_types)
    }

    async fn check_plugin_health(&self, plugin_type: &crate::core::engine::PluginType) -> Result<bool, EngineError> {
        // Sử dụng thực hiện đúng cách thay vì sử dụng match với Ok/Err
        let plugins = self.plugins.read().await;
        
        let plugin = if let Some(p) = plugins.get(plugin_type) {
            p
        } else {
            return Err(EngineError::InternalError(format!("Plugin not found: {:?}", plugin_type)));
        };

        match plugin.check_health() {
            Ok(status) => Ok(status),
            Err(e) => {
                error!("Plugin health check failed for {:?}: {:?}", plugin_type, e);
                Err(EngineError::InternalError(format!("Plugin health check failed: {:?}", e)))
            }
        }
    }

    async fn unregister_plugin(&self, plugin_type: &crate::core::engine::PluginType) -> Result<(), EngineError> {
        let mut plugins = self.plugins.write().await;
        let mut status = self.plugin_status.write().await;
        
        if plugins.remove(plugin_type).is_some() {
            status.remove(plugin_type);
            info!("Plugin unregistered: {:?}", plugin_type);
            Ok(())
        } else {
            warn!("Attempted to unregister non-existent plugin: {:?}", plugin_type);
            Err(EngineError::PluginStartFailed(format!("Plugin {:?} not found", plugin_type)))
        }
    }
}

impl DefaultNetworkEngine {
    pub async fn handle_message(&self, message: NetworkMessage) -> Result<()> {
        let plugins = self.plugins.read().await;
        for plugin in plugins.iter() {
            // Bỏ downcast_ref từ dyn NetworkPlugin (không đúng cú pháp) 
            // Thay bằng kiểm tra manual qua plugin_type
            let is_network_plugin = match plugin.plugin_type() {
                PluginType::WebSocket | PluginType::LibP2P | PluginType::MQTT => true,
                _ => false
            };
            
            if is_network_plugin {
                if let Err(e) = plugin.as_any().downcast_ref::<Box<dyn NetworkPlugin>>().unwrap().handle_message(&message).await {
                    error!("Plugin {} failed to handle message: {}", plugin.name(), e);
                }
            }
        }
        Ok(())
    }

    pub async fn register_plugin(&self, plugin_type: PluginType, plugin: Arc<dyn Plugin + Send + Sync>) -> Result<(), EngineError> {
        if plugin.name().is_empty() {
            warn!("Attempted to register plugin with empty name: {:?}", plugin_type);
            return Err(EngineError::PluginNameEmpty);
        }
        let mut last_err = None;
        for attempt in 0..3 {
            match plugin.start() {
                Ok(true) => {
                    info!("Plugin started and registered: {} ({:?})", plugin.name(), plugin_type);
                    let mut plugins = self.plugins.write().await;
                    plugins.insert(plugin_type.clone(), plugin.clone());
                    
                    let mut status = self.plugin_status.write().await;
                    status.insert(plugin_type.clone(), PluginStatus::Active);
                    
                    let mut last_update = self.plugin_last_update.write().await;
                    last_update.insert(plugin_type, std::time::Instant::now());
                    return Ok(());
                },
                Ok(false) => {
                    warn!("Plugin started but reported failure: {} ({:?}) [attempt {}/3]", plugin.name(), plugin_type, attempt+1);
                    last_err = Some(EngineError::PluginStartFailed(plugin.name().to_string()));
                },
                Err(e) => {
                    error!("Plugin failed to start: {} ({:?}) [attempt {}/3]: {:?}", plugin.name(), plugin_type, attempt+1, e);
                    last_err = Some(EngineError::PluginStartFailed(plugin.name().to_string()));
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        // Nếu vẫn thất bại sau 3 lần
        let mut status = self.plugin_status.write().await;
        status.insert(plugin_type.clone(), PluginStatus::Failed);
        
        let mut last_update = self.plugin_last_update.write().await;
        last_update.insert(plugin_type, std::time::Instant::now());
        
        // Trả về lỗi sau khi đã thử nhiều lần
        match last_err {
            Some(err) => Err(err),
            None => Err(EngineError::PluginStartFailed("Unknown error".to_string()))
        }
    }

    pub async fn get_plugin(&self, plugin_type: &PluginType) -> Option<Arc<dyn Plugin + Send + Sync>> {
        let plugins = self.plugins.read().await;
        plugins.get(plugin_type).cloned()
    }

    /// Management interface: trả về thông tin chi tiết về tất cả plugin runtime
    pub async fn list_plugin_info(&self) -> Result<Vec<(PluginType, String, PluginStatus)>, EngineError> {
        // Sử dụng cách an toàn để lấy read lock cho plugins
        let plugins = self.plugins.read().await;
        
        // Sử dụng cách an toàn để lấy read lock cho statuses
        let statuses = self.plugin_status.read().await;
        
        let mut info = Vec::new();
        for (ptype, plugin) in plugins.iter() {
            let status = statuses.get(ptype).cloned().unwrap_or(PluginStatus::Unknown);
            info.push((ptype.clone(), plugin.name().to_string(), status));
        }
        Ok(info)
    }

    /// Get plugin count by status (for metrics)
    pub async fn get_plugin_count_by_status(&self) -> Result<HashMap<PluginStatus, usize>, EngineError> {
        let status = self.plugin_status.read().await;
        let mut counts = HashMap::new();
        
        // Initialize all status counts to 0
        counts.insert(PluginStatus::Active, 0);
        counts.insert(PluginStatus::Inactive, 0);
        counts.insert(PluginStatus::Failed, 0);
        counts.insert(PluginStatus::Unknown, 0);
        
        // Count plugins by status
        for status_value in status.values() {
            *counts.entry(status_value.clone()).or_insert(0) += 1;
        }
        
        Ok(counts)
    }

    pub async fn bootstrap(&self) -> Result<(), EngineError> {
        // Sử dụng MockPlugin được định nghĩa ở ngoài để tạo các plugin mẫu
        let redis = Arc::new(MockPlugin::new("redis"));
        let ipfs = Arc::new(MockPlugin::new("ipfs"));
        let wasm = Arc::new(MockPlugin::new("wasm"));
        self.register_plugin(PluginType::Redis, redis).await?;
        self.register_plugin(PluginType::Ipfs, ipfs).await?;
        self.register_plugin(PluginType::Wasm, wasm).await?;
        Ok(())
    }

    pub async fn service_discovery(&self) -> Result<Vec<PluginType>, EngineError> {
        Ok(self.list_plugins().await)
    }
}

/// # Hướng dẫn theo dõi memory leak & expose memory usage
/// - Định kỳ kiểm tra số lượng plugin trong registry, log cảnh báo nếu vượt ngưỡng.
/// - Sử dụng Prometheus/Grafana để theo dõi số lượng plugin, memory usage.
/// - Sử dụng valgrind, heaptrack, tokio-console để kiểm tra memory leak khi phát triển/CI/CD.

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct PluginId(uuid::Uuid);

impl PluginId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
    
    pub fn as_uuid(&self) -> &uuid::Uuid {
        &self.0
    }
    
    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Thay thế import từ module không tồn tại, sử dụng MockPlugin đã định nghĩa ở trên
    // use crate::core::types::{MockPlugin, PluginType, PluginError};
    use std::sync::Arc;

    #[test]
    fn test_plugin_registry() {
        let engine = NetworkEngine::new();
        let plugin = Arc::new(MockPlugin::new("redis"));
        
        // Sử dụng expect thay vì unwrap với thông báo lỗi rõ ràng
        engine.register_plugin(PluginType::Redis, plugin.clone())
            .expect("Failed to register Redis plugin");
        
        // Kiểm tra plugin đã đăng ký thành công
        let retrieved = engine.get_plugin(&PluginType::Redis);
        assert!(retrieved.is_some(), "Plugin Redis phải tồn tại trong registry");
        if let Some(retrieved_plugin) = retrieved {
            assert_eq!(retrieved_plugin.name(), "redis");
        }
        
        // Unregister plugin
        engine.unregister_plugin(&PluginType::Redis)
            .expect("Failed to unregister Redis plugin");
        
        // Kiểm tra plugin đã bị xóa khỏi registry
        let retrieved_after_unregister = engine.get_plugin(&PluginType::Redis);
        assert!(retrieved_after_unregister.is_none(), "Plugin Redis không còn tồn tại trong registry sau khi unregister");
    }

    #[test]
    fn test_register_plugin_empty_name() {
        struct EmptyNamePlugin;
        impl Plugin for EmptyNamePlugin {
            fn name(&self) -> &str { "" }
            fn plugin_type(&self) -> PluginType { PluginType::Unknown }
            fn start(&self) -> Result<bool, PluginError> { Ok(true) }
            fn stop(&self) -> Result<(), PluginError> { Ok(()) }
        }
        let engine = NetworkEngine::new();
        let plugin = Arc::new(EmptyNamePlugin);
        let result = engine.register_plugin(PluginType::Unknown, plugin);
        assert!(result.is_err());
    }

    #[test]
    fn test_register_plugin_start_failed() {
        struct FailStartPlugin;
        impl Plugin for FailStartPlugin {
            fn name(&self) -> &str { "fail" }
            fn plugin_type(&self) -> PluginType { PluginType::Unknown }
            fn start(&self) -> Result<bool, PluginError> { Err(PluginError::StartFailed) }
            fn stop(&self) -> Result<(), PluginError> { Ok(()) }
        }
        let engine = NetworkEngine::new();
        let plugin = Arc::new(FailStartPlugin);
        let result = engine.register_plugin(PluginType::Unknown, plugin);
        assert!(result.is_err());
    }
    
    #[tokio::test]
    async fn test_metrics_generation() {
        let engine = NetworkEngine::new();
        let redis = Arc::new(MockPlugin::new("redis"));
        let ipfs = Arc::new(MockPlugin::new("ipfs"));
        
        // Sử dụng match thay vì unwrap để kiểm tra rõ ràng với thông báo lỗi cụ thể
        match engine.register_plugin(PluginType::Redis, redis) {
            Ok(_) => {},
            Err(e) => panic!("Failed to register Redis plugin: {:?}", e)
        }
        
        match engine.register_plugin(PluginType::Ipfs, ipfs) {
            Ok(_) => {},
            Err(e) => panic!("Failed to register IPFS plugin: {:?}", e)
        }
        
        // Kiểm tra metrics
        let metrics = engine.get_metrics().await;
        assert!(metrics.is_ok(), "Failed to get metrics: {:?}", metrics.err());
        
        // Sử dụng match thay vì unwrap
        match metrics {
            Ok(metrics_str) => {
                assert!(metrics_str.contains("redis"), "Metrics should contain Redis information");
                assert!(metrics_str.contains("ipfs"), "Metrics should contain IPFS information");
            },
            Err(e) => panic!("Error getting metrics: {:?}", e)
        }
    }
}


