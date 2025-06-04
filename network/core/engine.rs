// src/core/engine.rs
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn, error};
use tracing_subscriber::EnvFilter;
use anyhow::Result;
use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::RwLock;
use std::any::Any;
use std::io::Write;

use crate::config::{NetworkConfig, core::NetworkCoreConfig};

/// Cấu hình logging cho hệ thống
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
    pub log_file: Option<String>,
}

impl LoggingConfig {
    pub fn load() -> Self {
        LoggingConfig {
            level: std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
            format: std::env::var("LOG_FORMAT").unwrap_or_else(|_| "ansi".to_string()),
            log_file: std::env::var("LOG_FILE").ok(),
        }
    }
}

#[derive(Debug, Clone, Error)]
pub enum EngineError {
    #[error("Failed to start plugin: {0}")]
    PluginStartFailed(String),
    #[error("Plugin name cannot be empty")]
    PluginNameEmpty,
    #[error("Lock acquisition failed: {0}")]
    LockError(String),
    #[error("Internal engine error: {0}")]
    InternalError(String),
}

/// Represents the status of a plugin
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PluginStatus {
    Active,
    Inactive,
    Failed,
    Unknown,
    Stopping,
    TimedOut,
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
    async fn register_plugin(&self, plugin_type: PluginType, plugin: Arc<dyn Plugin + Send + Sync>) -> Result<(), EngineError>;
    
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
    HeartbeatMessage,
    DataMessage,
    ControlMessage,
    DiscoveryMessage,
    SyncMessage,
}

/// NetworkPlugin trait cho các plugin
#[async_trait]
pub trait NetworkPlugin: Send + Sync {
    /// Tên plugin
    fn name(&self) -> &str;
    
    /// Khởi tạo plugin
    async fn init(&self) -> Result<()>;
    
    /// Xử lý message
    async fn handle_message(&self, message: &NetworkMessage) -> Result<()>;
    
    /// Kiểm tra trạng thái plugin
    async fn check_health(&self) -> Result<bool> {
        // Mặc định trả về OK
        Ok(true)
    }
    
    /// Dừng plugin
    async fn shutdown(&self) -> Result<()> {
        // Mặc định thực hiện noop
        Ok(())
    }
    
    /// Chuyển đổi plugin thành Any để hỗ trợ downcasting
    fn as_any(&self) -> &dyn Any;
}

/// Plugin trait cơ bản cho tất cả plugin trong NetworkEngine
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Tên plugin
    fn name(&self) -> &str;
    
    /// Loại plugin
    fn plugin_type(&self) -> PluginType;
    
    /// Khởi động plugin
    async fn start(&self) -> Result<bool, PluginError>;
    
    /// Dừng plugin
    async fn stop(&self) -> Result<(), PluginError>;
    
    /// Kiểm tra sức khỏe plugin
    /// Returns Ok(true) nếu plugin đang hoạt động tốt, Ok(false) hoặc Err nếu có vấn đề
    async fn check_health(&self) -> Result<bool, PluginError> {
        // Implement mặc định trả về Ok(true) để các plugin không cần implement lại nếu không cần
        Ok(true)
    }
    
    /// Convert plugin to &dyn Any để hỗ trợ downcasting
    /// Phải được triển khai cụ thể cho mỗi struct
    fn as_any(&self) -> &dyn Any;
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
    Storage,
    Unknown,
}

impl PluginType {
    /// Chuyển enum thành &str
    pub fn as_str(&self) -> &'static str {
        match self {
            PluginType::Grpc => "grpc",
            PluginType::Wasm => "wasm",
            PluginType::WebRtc => "webrtc",
            PluginType::WebSocket => "websocket",
            PluginType::Ipfs => "ipfs",
            PluginType::Redis => "redis", 
            PluginType::Libp2p => "libp2p",
            PluginType::Storage => "storage",
            PluginType::Unknown => "unknown",
        }
    }
}

impl std::str::FromStr for PluginType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "grpc" => Ok(PluginType::Grpc),
            "wasm" => Ok(PluginType::Wasm),
            "webrtc" => Ok(PluginType::WebRtc),
            "websocket" => Ok(PluginType::WebSocket),
            "ipfs" => Ok(PluginType::Ipfs),
            "redis" => Ok(PluginType::Redis),
            "libp2p" => Ok(PluginType::Libp2p),
            "storage" => Ok(PluginType::Storage),
            "unknown" => Ok(PluginType::Unknown),
            _ => Err(format!("Unknown plugin type: {}", s)),
        }
    }
}

impl std::fmt::Display for PluginType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
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
    #[error("Plugin shutdown error: {0}")]
    ShutdownError(String),
    #[error("Other plugin error: {0}")]
    Other(String),
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

#[async_trait]
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
    
    async fn start(&self) -> Result<bool, PluginError> {
        Ok(true)
    }
    
    async fn stop(&self) -> Result<(), PluginError> {
        Ok(())
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Thêm trait ServiceShutdown để có thể dừng các service
#[async_trait]
pub trait ServiceShutdown {
    async fn shutdown(&self) -> Result<()>;
}

/// Triển khai trait ServiceShutdown cho các service
#[async_trait]
impl ServiceShutdown for crate::node_manager::master::PartitionedMasterNodeService {
    async fn shutdown(&self) -> Result<()> {
        // Triển khai thực tế tùy thuộc vào service
        info!("Shutting down master service...");
        // TODO: Thực hiện dừng service thực tế
        Ok(())
    }
}

#[async_trait]
impl ServiceShutdown for crate::node_manager::discovery::DefaultDiscoveryService {
    async fn shutdown(&self) -> Result<()> {
        // Triển khai thực tế tùy thuộc vào service
        info!("Shutting down discovery service...");
        // TODO: Thực hiện dừng service thực tế
        Ok(())
    }
}

#[async_trait]
impl ServiceShutdown for crate::node_manager::scheduler::DefaultSchedulerService {
    async fn shutdown(&self) -> Result<()> {
        // Triển khai thực tế tùy thuộc vào service
        info!("Shutting down scheduler service...");
        // TODO: Thực hiện dừng service thực tế
        Ok(())
    }
}

/// Triển khai ServiceShutdown cho Arc<T>
#[async_trait]
impl<T: ServiceShutdown + Send + Sync + 'static> ServiceShutdown for Arc<T> {
    async fn shutdown(&self) -> Result<()> {
        (**self).shutdown().await
    }
}

/// DefaultNetworkEngine implementation
pub struct DefaultNetworkEngine {
    config: Arc<RwLock<NetworkConfig>>,
    plugins: Arc<RwLock<HashMap<PluginType, Arc<dyn Plugin + Send + Sync>>>>,
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
            plugins: Arc::new(RwLock::new(HashMap::new())),
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
        
        match &logging.log_file {
            Some(file_path) => {
                match std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(file_path)
                {
                    Ok(_) => {
                        // Sử dụng String::from để clone path vào trong closure
                        let file_path_owned = String::from(file_path);
                        let writer = move || -> Box<dyn Write + Send + 'static> {
                            match std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(&file_path_owned)
                            {
                                Ok(f) => Box::new(f),
                                Err(e) => {
                                    eprintln!("Warning: Could not open log file: {}", e);
                                    Box::new(std::io::stderr())
                                }
                            }
                        };
                        
                        tracing_subscriber::fmt()
                            .with_env_filter(EnvFilter::new(&logging.level))
                            .with_ansi(logging.format == "ansi")
                            .with_writer(writer)
                            .init();
                        
                        info!("Logging initialized with file: {}", file_path);
                    }
                    Err(e) => {
                        eprintln!("Warning: Could not open log file '{}': {}", file_path, e);
                        // Fallback to stderr if file cannot be opened
                        tracing_subscriber::fmt()
                            .with_env_filter(EnvFilter::new(&logging.level))
                            .with_ansi(logging.format == "ansi")
                            .init();
                        
                        info!("Fallback to stderr logging due to file error");
                    }
                }
            }
            None => {
                tracing_subscriber::fmt()
                    .with_env_filter(EnvFilter::new(&logging.level))
                    .with_ansi(logging.format == "ansi")
                    .init();
                
                info!("Logging initialized with stderr (no log file specified)");
            }
        }
    }
}

impl Default for DefaultNetworkEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NetworkEngine for DefaultNetworkEngine {
    async fn init(&self, config: &NetworkCoreConfig) -> Result<()> {
        // Không cần match, tokio::sync::RwLock không trả về Result mà trả về guard trực tiếp
        let mut config_guard = self.config.write().await;
        
        // Tạo cấu hình mới từ NetworkCoreConfig với cách khởi tạo đúng trong initializer
        let new_config = NetworkConfig {
            core: config.clone(),
            ..NetworkConfig::default()
        };
        
        // Cập nhật cấu hình hiện tại
        *config_guard = new_config;
        
        info!("Network engine initialized with config: {:?}", config);
        Ok(())
    }
    
    async fn start(&self) -> Result<()> {
        // Không cần match, tokio::sync::RwLock không trả về Result mà trả về guard trực tiếp
        let mut running_guard = self.is_running.write().await;
        
        if *running_guard {
            info!("Network engine already started");
            return Ok(());
        }
        
        *running_guard = true;
        info!("Network engine started");
        
        // Khởi tạo các plugin
        let plugins = self.plugins.read().await;
        
        let mut init_errors = Vec::new();
        
        for (plugin_type, plugin) in plugins.iter() {
            let plugin_name = plugin.name().to_owned();
            info!("Initializing plugin: {}", plugin_name);
            
            // Khởi tạo plugin
            match plugin.start().await {
                Ok(true) => {
                    info!("Plugin {} initialized successfully", plugin_name);
                    // Cập nhật status của plugin thành Active
                    let mut status = self.plugin_status.write().await;
                    status.insert(plugin_type.clone(), PluginStatus::Active);
                },
                Ok(false) => {
                    let err_msg = format!("Plugin {} failed initialization", plugin_name);
                    error!("{}", err_msg);
                    init_errors.push(err_msg);
                    
                    // Cập nhật status của plugin thành Failed
                    let mut status = self.plugin_status.write().await;
                    status.insert(plugin_type.clone(), PluginStatus::Failed);
                },
                Err(e) => {
                    let err_msg = format!("Plugin {} error during initialization: {:?}", plugin_name, e);
                    error!("{}", err_msg);
                    init_errors.push(err_msg);
                    
                    // Cập nhật status của plugin thành Failed
                    let mut status = self.plugin_status.write().await;
                    status.insert(plugin_type.clone(), PluginStatus::Failed);
                }
            }
        }
        
        if !init_errors.is_empty() {
            warn!("Some plugins failed to initialize: {:?}", init_errors);
        }
        
        Ok(())
    }
    
    async fn stop(&self) -> Result<()> {
        // Lấy write lock cho running state an toàn, không cần match
        let mut running = self.is_running.write().await;
        
        if !*running {
            info!("Network engine already stopped");
            return Ok(());
        }
        
        info!("Stopping network engine...");
        
        // Cập nhật trạng thái chạy
        *running = false;
        
        // Clone plugins để tránh deadlock giữa plugins và plugin_status
        let plugins_to_stop: Vec<(PluginType, Arc<dyn Plugin + Send + Sync>)> = {
            let plugins_guard = self.plugins.read().await;
            plugins_guard.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        };
        
        // Cập nhật trạng thái tất cả plugin
        {
            let mut status = self.plugin_status.write().await;
            
            for (plugin_type, _) in &plugins_to_stop {
                status.insert(plugin_type.clone(), PluginStatus::Stopping);
            }
        }
        
        // Shutdown các plugin
        let mut stop_errors = Vec::new();
        for (plugin_type, plugin) in plugins_to_stop {
            let plugin_name = plugin.name().to_owned();
            info!("Stopping plugin: {}", plugin_name);
            
            match plugin.stop().await {
            Ok(_) => {
                    info!("Plugin {} stopped successfully", plugin_name);
                    let mut status = self.plugin_status.write().await;
                    status.insert(plugin_type.clone(), PluginStatus::Inactive);
                },
                Err(e) => {
                    let err_msg = format!("Error stopping plugin {}: {:?}", plugin_name, e);
                    error!("{}", err_msg);
                    stop_errors.push(err_msg);
                    
                    let mut status = self.plugin_status.write().await;
                    status.insert(plugin_type.clone(), PluginStatus::Failed);
                }
            }
        }
        
        // Shutdown các service
        info!("Shutting down services...");
        self.master_service.shutdown().await?;
        self.discovery_service.shutdown().await?;
        self.scheduler_service.shutdown().await?;
        
        // Xóa các plugin đã dừng khỏi registry
        {
            let mut plugins = self.plugins.write().await;
            
            let plugin_types: Vec<PluginType> = plugins.keys().cloned().collect();
            for plugin_type in plugin_types {
                plugins.remove(&plugin_type);
                info!("Removed plugin {:?} from registry", plugin_type);
            }
        }
        
        if !stop_errors.is_empty() {
            warn!("Some plugins failed to stop cleanly: {:?}", stop_errors);
        }
        
        // Giữ lại trạng thái plugin cho metrics
        {
            let mut status = self.plugin_status.write().await;
            
            // Chỉ xoá các plugin không còn hoạt động
            status.retain(|_, state| {
                *state == PluginStatus::Active || *state == PluginStatus::Failed
            });
            info!("Plugin statuses updated after shutdown");
        }
        
        {
            let mut last_update = self.plugin_last_update.write().await;
            last_update.clear();
            info!("Plugin last update timestamps cleared");
        }
        
        info!("Network engine stopped successfully");
        Ok(())
    }
    
    async fn register_plugin(&self, plugin_type: PluginType, plugin: Arc<dyn Plugin + Send + Sync>) -> Result<(), EngineError> {
        if plugin.name().is_empty() {
            warn!("Attempted to register plugin with empty name: {:?}", plugin_type);
            return Err(EngineError::PluginNameEmpty);
        }
        
        let mut last_err = None;
        for attempt in 0..3 {
            match plugin.start().await {
                Ok(true) => {
                    info!("Plugin started and registered: {} ({:?})", plugin.name(), plugin_type);
                    
                    // Sử dụng scope độc lập cho mỗi lock để tránh deadlock
                    // Lấy lock cho plugins
                    {
                        let mut plugins = self.plugins.write().await;
                        plugins.insert(plugin_type.clone(), plugin.clone());
                    } // drop plugins lock ở đây
                    
                    // Lấy lock cho plugin_status riêng biệt
                    {
                        let mut status = self.plugin_status.write().await;
                        status.insert(plugin_type.clone(), PluginStatus::Active);
                    } // drop status lock ở đây
                    
                    // Lấy lock cho plugin_last_update riêng biệt
                    {
                        let mut last_update = self.plugin_last_update.write().await;
                        last_update.insert(plugin_type, std::time::Instant::now());
                    } // drop last_update lock ở đây
                    
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
            
            // Chờ một chút trước khi thử lại
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
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

    async fn get_plugin(&self, _id: PluginId) -> Option<Box<dyn Plugin>> {
        let _plugins = self.plugins.read().await;
        
        // Tìm plugin theo id, hiện tại trả về None vì chưa triển khai đầy đủ
        // TODO: Implement plugin lookup by ID
        warn!("get_plugin by ID is not fully implemented yet");
        None 
    }

    async fn get_all_plugins(&self) -> Vec<Box<dyn Plugin>> {
        let _plugins = self.plugins.read().await;
        
        // Tạm thời trả về vector rỗng vì chưa triển khai đầy đủ
        // TODO: Implement full plugin cloning/conversion
        warn!("get_all_plugins is not fully implemented yet");
        Vec::new() 
    }

    async fn remove_plugin(&self, _id: PluginId) -> Result<()> {
        let _plugins = self.plugins.write().await;
        
        // Tạm thời trả về Ok vì chưa triển khai đầy đủ
        Ok(())
    }

    async fn get_all_plugin_statuses(&self) -> Result<std::collections::HashMap<crate::core::engine::PluginType, crate::core::engine::PluginStatus>, EngineError> {
        let status = self.plugin_status.read().await;
        Ok(status.clone())
    }

    async fn get_metrics(&self) -> Result<String, EngineError> {
        // Lấy dữ liệu từ các shared state, nhưng không giữ locks quá lâu
        // Lấy dữ liệu từ plugins trước, trong scope riêng biệt
        let plugin_count;
        let plugin_types: Vec<PluginType>;
        
        {
            let plugins = self.plugins.read().await;
            plugin_count = plugins.len();
            plugin_types = plugins.keys().cloned().collect();
        } // drop plugins lock tại đây
        
        // Lấy dữ liệu từ plugin_status, trong scope riêng biệt
        let statuses_clone;
        
        {
            let statuses = self.plugin_status.read().await;
            statuses_clone = statuses.clone();
        } // drop statuses lock tại đây
        
        let mut metrics = String::new();
        
        // Add plugin count metrics
        metrics.push_str("# HELP network_plugin_count Number of registered plugins\n");
        metrics.push_str("# TYPE network_plugin_count gauge\n");
        metrics.push_str(&format!("network_plugin_count {}\n\n", plugin_count));
        
        // Add plugin status metrics
        metrics.push_str("# HELP network_plugin_status Status of plugins (1 = active, 0 = inactive)\n");
        metrics.push_str("# TYPE network_plugin_status gauge\n");
        
        // Lặp qua từng cặp plugin_type và status để tạo metrics
        for plugin_type in plugin_types {
            let status = statuses_clone.get(&plugin_type).unwrap_or(&PluginStatus::Unknown);
            let status_value = match status {
                PluginStatus::Active => 1,
                _ => 0,
            };
            metrics.push_str(&format!("network_plugin_status{{plugin=\"{:?}\"}} {}\n", plugin_type, status_value));
        }
        
        // Add count by status metrics
        metrics.push_str("\n# HELP network_plugins_by_status Count of plugins by status\n");
        metrics.push_str("# TYPE network_plugins_by_status gauge\n");
        
        // Đếm số lượng plugin theo trạng thái
        let mut active_count = 0;
        let mut inactive_count = 0;
        
        for status in statuses_clone.values() {
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
                        let metric_name = line.split(':').next().unwrap_or("mem_unknown");
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
        metrics.push_str(&format!("network_plugin_registry_object_count {}\n", plugin_count));
        
        // Thêm mock metrics cho các node
        metrics.push_str("# HELP network_slave_node_count Số lượng slave node trong master\n");
        metrics.push_str("# TYPE network_slave_node_count gauge\n");
        metrics.push_str(&format!("network_slave_node_count {}\n", 0)); // Mock: 0 nodes
        
        metrics.push_str("# HELP network_discovery_node_count Số lượng node discovery\n");
        metrics.push_str("# TYPE network_discovery_node_count gauge\n");
        metrics.push_str(&format!("network_discovery_node_count {}\n", 0)); // Mock: 0 nodes
        
        metrics.push_str("# HELP network_task_node_count Số lượng node có task\n");
        metrics.push_str("# TYPE network_task_node_count gauge\n");
        metrics.push_str(&format!("network_task_node_count {}\n", 0)); // Mock: 0 nodes
        
        Ok(metrics)
    }

    async fn list_plugins(&self) -> Result<Vec<crate::core::engine::PluginType>, EngineError> {
        match self.list_plugin_info().await {
            Ok(plugin_info) => {
        let plugin_types: Vec<PluginType> = plugin_info.into_iter().map(|(ptype, _, _)| ptype).collect();
        Ok(plugin_types)
            },
            Err(e) => Err(e)
        }
    }

    async fn check_plugin_health(&self, plugin_type: &crate::core::engine::PluginType) -> Result<bool, EngineError> {
        let plugins = self.plugins.read().await;
        
        match plugins.get(plugin_type) {
            Some(plugin) => {
                match plugin.check_health().await {
                    Ok(is_healthy) => Ok(is_healthy),
                    Err(e) => Err(EngineError::InternalError(format!("Plugin health check failed: {}", e)))
                }
            },
            None => Err(EngineError::InternalError(format!("Plugin {:?} not found", plugin_type)))
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
        
        for plugin in plugins.values() {
            // Kiểm tra type qua plugin_type
            let plugin_type = plugin.plugin_type();
            match plugin_type {
                // Các plugin type thường xử lý message
                PluginType::WebSocket | PluginType::Libp2p | PluginType::Grpc => {
                    // Mô phỏng xử lý message
                    info!("Plugin {} handling message from {}", plugin.name(), message.from);
                    // Trong thực tế cần cơ chế phức tạp hơn để gửi message đến plugin 
                }
                _ => {} // Các plugin khác không xử lý message
            }
        }
        Ok(())
    }

    pub async fn get_plugin(&self, plugin_type: &PluginType) -> Option<Arc<dyn Plugin + Send + Sync>> {
        self.plugins.read().await.get(plugin_type).cloned()
    }

    /// Management interface: trả về thông tin chi tiết về tất cả plugin runtime
    pub async fn list_plugin_info(&self) -> Result<Vec<(PluginType, String, PluginStatus)>, EngineError> {
        // Lấy read lock cho plugins và statuses
        let plugins = self.plugins.read().await;
        let statuses = self.plugin_status.read().await;
        
        let mut info = Vec::new();
        for (ptype, plugin) in plugins.iter() {
            let status = match statuses.get(ptype) {
                Some(status) => status.clone(),
                None => PluginStatus::Unknown
            };
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
        
        // Xử lý từng kết quả đăng ký riêng biệt
        self.register_plugin(PluginType::Redis, redis).await?;
        self.register_plugin(PluginType::Ipfs, ipfs).await?;
        self.register_plugin(PluginType::Wasm, wasm).await?;
        
        Ok(())
    }

    pub async fn service_discovery(&self) -> Result<Vec<PluginType>, EngineError> {
        // Implement service_discovery bằng cách lấy danh sách plugins từ registry
        let plugins = self.plugins.read().await;
        let plugin_types: Vec<PluginType> = plugins.keys().cloned().collect();
        Ok(plugin_types)
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
}

impl std::fmt::Display for PluginId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for PluginId {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrapper để wrap Box<dyn Plugin> trong Arc
#[allow(dead_code)]
struct BoxPlugin(Box<dyn Plugin>);

#[async_trait]
impl Plugin for BoxPlugin {
    fn name(&self) -> &str {
        self.0.name()
    }
    
    fn plugin_type(&self) -> PluginType {
        self.0.plugin_type()
    }
    
    async fn start(&self) -> Result<bool, PluginError> {
        self.0.start().await
    }
    
    async fn stop(&self) -> Result<(), PluginError> {
        self.0.stop().await
    }
    
    async fn check_health(&self) -> Result<bool, PluginError> {
        self.0.check_health().await
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Thay thế import từ module không tồn tại, sử dụng MockPlugin đã định nghĩa ở trên
    // use crate::core::types::{MockPlugin, PluginType, PluginError};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_plugin_registry() {
        // Sử dụng DefaultNetworkEngine thay vì trait
        let engine = DefaultNetworkEngine::new();
        let plugin = Arc::new(MockPlugin::new("redis"));
        
        // Sử dụng method DefaultNetworkEngine::register_plugin, không phải trait method
        let register_result = engine.register_plugin(PluginType::Redis, plugin.clone()).await;
        assert!(register_result.is_ok(), "Failed to register Redis plugin: {:?}", register_result.err());
        
        // Kiểm tra plugin đã đăng ký thành công
        let retrieved = engine.get_plugin(&PluginType::Redis).await;
        assert!(retrieved.is_some(), "Plugin Redis phải tồn tại trong registry");
        
        if let Some(retrieved_plugin) = retrieved {
            assert_eq!(retrieved_plugin.name(), "redis");
        }
        
        // Unregister plugin - sử dụng trait method
        let unregister_result = engine.unregister_plugin(&PluginType::Redis).await;
        assert!(unregister_result.is_ok(), "Failed to unregister Redis plugin: {:?}", unregister_result.err());
        
        // Kiểm tra plugin đã bị xóa khỏi registry
        let retrieved_after_unregister = engine.get_plugin(&PluginType::Redis).await;
        assert!(retrieved_after_unregister.is_none(), "Plugin Redis không còn tồn tại trong registry sau khi unregister");
    }

    #[tokio::test]
    async fn test_register_plugin_empty_name() {
        struct EmptyNamePlugin;
        
        #[async_trait]
        impl Plugin for EmptyNamePlugin {
            fn name(&self) -> &str { "" }
            fn plugin_type(&self) -> PluginType { PluginType::Unknown }
            async fn start(&self) -> Result<bool, PluginError> { Ok(true) }
            async fn stop(&self) -> Result<(), PluginError> { Ok(()) }
            fn as_any(&self) -> &dyn Any {
                self
            }
        }
        
        let engine = DefaultNetworkEngine::new();
        let plugin = Arc::new(EmptyNamePlugin);
        let result = engine.register_plugin(PluginType::Unknown, plugin).await;
        assert!(result.is_err(), "Empty plugin name should cause registration failure");
        if let Err(e) = result {
            assert!(matches!(e, EngineError::PluginNameEmpty), 
                   "Error should be EngineError::PluginNameEmpty, got: {:?}", e);
        }
    }

    #[tokio::test]
    async fn test_metrics_generation() {
        let engine = DefaultNetworkEngine::new();
        let redis = Arc::new(MockPlugin::new("redis"));
        let ipfs = Arc::new(MockPlugin::new("ipfs"));
        
        // Đăng ký plugin Redis
        let redis_result = engine.register_plugin(PluginType::Redis, redis).await;
        assert!(redis_result.is_ok(), "Failed to register Redis plugin: {:?}", redis_result.err());
        
        // Đăng ký plugin IPFS
        let ipfs_result = engine.register_plugin(PluginType::Ipfs, ipfs).await;
        assert!(ipfs_result.is_ok(), "Failed to register IPFS plugin: {:?}", ipfs_result.err());
        
        // Kiểm tra metrics
        let metrics = engine.get_metrics().await;
        assert!(metrics.is_ok(), "Failed to get metrics: {:?}", metrics.err());
        
        // Kiểm tra nội dung metrics
        if let Ok(metrics_str) = metrics {
            assert!(metrics_str.contains("network_plugin_count"), "Metrics should include plugin count");
            assert!(metrics_str.contains("network_plugin_status"), "Metrics should include plugin status");
        }
    }
}


