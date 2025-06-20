//! Health Monitor - Module theo dõi sức khỏe mạng và hệ thống
//! Module này cung cấp các chức năng giám sát sức khỏe hệ thống, tự động phát hiện
//! và khắc phục các vấn đề mạng, kết nối blockchain, và tài nguyên hệ thống.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, Mutex, mpsc};
use tokio::time::{interval};
use anyhow::{Result, anyhow};
use tracing::{debug, error, info, warn};
use async_trait::async_trait;

use super::network_reliability::{
    NetworkStatus, 
    NetworkReliabilityManager,
    get_network_manager
};

/// Loại tài nguyên hệ thống cần giám sát
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemResourceType {
    /// CPU sử dụng
    CpuUsage,
    /// Bộ nhớ RAM sử dụng
    MemoryUsage,
    /// Dung lượng ổ đĩa còn trống
    DiskSpace,
    /// Số lượng file descriptors đang mở
    FileDescriptors,
    /// Số lượng kết nối mạng đang mở
    NetworkConnections,
}

/// Thông tin sức khỏe hệ thống
#[derive(Debug, Clone)]
pub struct SystemHealth {
    /// Tỷ lệ CPU sử dụng (0-100%)
    pub cpu_usage: f64,
    /// Tỷ lệ bộ nhớ RAM sử dụng (0-100%)
    pub memory_usage: f64,
    /// Dung lượng ổ đĩa còn trống (bytes)
    pub free_disk_space: u64,
    /// Số lượng file descriptors đang mở
    pub open_file_descriptors: u64,
    /// Số lượng kết nối mạng đang mở
    pub open_network_connections: u64,
    /// Thời gian hệ thống hoạt động (uptime) tính bằng giây
    pub uptime_seconds: u64,
    /// Thời điểm lấy thông tin
    pub timestamp: u64,
}

/// Mức độ nghiêm trọng của cảnh báo
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertSeverity {
    /// Thông tin (Information)
    Info,
    /// Cảnh báo (Warning)
    Warning,
    /// Lỗi (Error)
    Error,
    /// Nghiêm trọng (Critical)
    Critical,
}

/// Thông tin cảnh báo
#[derive(Debug, Clone)]
pub struct HealthAlert {
    /// Loại tài nguyên gây cảnh báo
    pub resource_type: String,
    /// Mô tả cảnh báo
    pub message: String,
    /// Mức độ nghiêm trọng
    pub severity: AlertSeverity,
    /// Thời điểm phát cảnh báo
    pub timestamp: u64,
    /// Giá trị ngưỡng
    pub threshold: f64,
    /// Giá trị thực tế
    pub actual_value: f64,
    /// Hành động đề xuất
    pub suggested_action: Option<String>,
}

/// Cấu hình giám sát sức khỏe
#[derive(Debug, Clone)]
pub struct HealthMonitorConfig {
    /// Khoảng thời gian kiểm tra sức khỏe (giây)
    pub check_interval_seconds: u64,
    /// Khoảng thời gian giữa các lần thử kết nối lại (ms)
    pub reconnect_interval_ms: u64,
    /// Số lần thử kết nối lại tối đa
    pub max_reconnect_attempts: usize,
    /// Ngưỡng CPU (%) để cảnh báo
    pub cpu_warning_threshold: f64,
    /// Ngưỡng CPU (%) để báo lỗi
    pub cpu_error_threshold: f64,
    /// Ngưỡng bộ nhớ (%) để cảnh báo
    pub memory_warning_threshold: f64,
    /// Ngưỡng bộ nhớ (%) để báo lỗi
    pub memory_error_threshold: f64,
    /// Ngưỡng ổ đĩa còn trống (MB) để cảnh báo
    pub disk_warning_threshold_mb: u64,
    /// Ngưỡng ổ đĩa còn trống (MB) để báo lỗi
    pub disk_error_threshold_mb: u64,
    /// Kích hoạt tự động thử kết nối lại
    pub enable_auto_reconnect: bool,
    /// Kích hoạt giám sát tài nguyên hệ thống
    pub enable_system_monitoring: bool,
    /// Kích hoạt giám sát kết nối mạng
    pub enable_network_monitoring: bool,
    /// Cảnh báo khi không thể kết nối lại sau số lần thử này
    pub alert_after_reconnect_attempts: usize,
}

impl Default for HealthMonitorConfig {
    fn default() -> Self {
        Self {
            check_interval_seconds: 60,
            reconnect_interval_ms: 5000,
            max_reconnect_attempts: 10,
            cpu_warning_threshold: 80.0,
            cpu_error_threshold: 95.0,
            memory_warning_threshold: 85.0,
            memory_error_threshold: 95.0,
            disk_warning_threshold_mb: 1000, // 1GB
            disk_error_threshold_mb: 500,    // 500MB
            enable_auto_reconnect: true,
            enable_system_monitoring: true,
            enable_network_monitoring: true,
            alert_after_reconnect_attempts: 3,
        }
    }
}

/// Trait định nghĩa API để nhận cảnh báo sức khỏe
#[async_trait]
pub trait HealthAlertHandler: Send + Sync {
    /// Xử lý cảnh báo sức khỏe
    async fn handle_alert(&self, alert: HealthAlert) -> Result<()>;
}

/// Health Monitor quản lý việc giám sát sức khỏe hệ thống
pub struct HealthMonitor {
    /// Cấu hình
    config: RwLock<HealthMonitorConfig>,
    
    /// Network reliability manager
    network_manager: Arc<NetworkReliabilityManager>,
    
    /// Trạng thái sức khỏe hệ thống
    system_health: RwLock<SystemHealth>,
    
    /// Trạng thái mạng cho từng service
    network_health: RwLock<HashMap<String, NetworkStatus>>,
    
    /// Lịch sử cảnh báo
    alert_history: RwLock<Vec<HealthAlert>>,
    
    /// Channel để gửi cảnh báo
    alert_sender: mpsc::Sender<HealthAlert>,
    
    /// Channel để nhận cảnh báo
    alert_receiver: Mutex<mpsc::Receiver<HealthAlert>>,
    
    /// Alert handlers
    alert_handlers: RwLock<Vec<Arc<dyn HealthAlertHandler>>>,
    
    /// Task handle cho health check
    health_check_task: RwLock<Option<tokio::task::JoinHandle<()>>>,
    
    /// Task handle cho alert processor
    alert_processor_task: RwLock<Option<tokio::task::JoinHandle<()>>>,
    
    /// Các service đang giám sát
    monitored_services: RwLock<HashMap<String, Arc<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>> + Send + Sync>>>,
    
    /// Số lần thử kết nối lại
    pub reconnect_attempts: RwLock<usize>,
}

impl HealthMonitor {
    /// Tạo health monitor mới
    pub fn new(config: HealthMonitorConfig) -> Self {
        // Tạo alert channel
        let (alert_sender, alert_receiver) = mpsc::channel(100);
        
        // Tạo hệ thống health mặc định
        let system_health = SystemHealth {
            cpu_usage: 0.0,
            memory_usage: 0.0,
            free_disk_space: 0,
            open_file_descriptors: 0,
            open_network_connections: 0,
            uptime_seconds: 0,
            timestamp: 0,
        };
        
        Self {
            config: RwLock::new(config),
            network_manager: get_network_manager(),
            system_health: RwLock::new(system_health),
            network_health: RwLock::new(HashMap::new()),
            alert_history: RwLock::new(Vec::new()),
            alert_sender,
            alert_receiver: Mutex::new(alert_receiver),
            alert_handlers: RwLock::new(Vec::new()),
            health_check_task: RwLock::new(None),
            alert_processor_task: RwLock::new(None),
            monitored_services: RwLock::new(HashMap::new()),
            reconnect_attempts: RwLock::new(0),
        }
    }
    
    /// Khởi động health monitor
    pub async fn start(self: Arc<Self>) -> Result<()> {
        // Khởi động task xử lý cảnh báo
        let alert_task = self.clone().start_alert_processor().await?;
        
        {
            let mut task_handle = self.alert_processor_task.write().await;
            *task_handle = Some(alert_task);
        }
        
        // Khởi động task kiểm tra sức khỏe
        if self.config.read().await.enable_system_monitoring || self.config.read().await.enable_network_monitoring {
            let health_task = self.clone().start_health_check().await?;
            
            {
                let mut task_handle = self.health_check_task.write().await;
                *task_handle = Some(health_task);
            }
        }
        
        // Thiết lập các service được giám sát cho auto-reconnect
        let config = self.config.read().await;
        if config.enable_auto_reconnect {
            self.setup_auto_reconnect().await?;
        }
        
        Ok(())
    }
    
    /// Dừng health monitor
    pub async fn stop(&self) -> Result<()> {
        // Dừng task kiểm tra sức khỏe
        {
            let mut task_handle = self.health_check_task.write().await;
            if let Some(handle) = task_handle.take() {
                handle.abort();
                debug!("Stopped health check task");
            }
        }
        
        // Dừng task xử lý cảnh báo
        {
            let mut task_handle = self.alert_processor_task.write().await;
            if let Some(handle) = task_handle.take() {
                handle.abort();
                debug!("Stopped alert processor task");
            }
        }
        
        Ok(())
    }
    
    /// Khởi động task xử lý cảnh báo
    async fn start_alert_processor(self: Arc<Self>) -> Result<tokio::task::JoinHandle<()>> {
        let task = tokio::spawn(async move {
            let mut receiver = self.alert_receiver.lock().await;
            
            while let Some(alert) = receiver.recv().await {
                // Lưu vào lịch sử
                {
                    let mut history = self.alert_history.write().await;
                    history.push(alert.clone());
                    
                    // Giới hạn số lượng alert trong lịch sử
                    if history.len() > 1000 {
                        history.remove(0);
                    }
                }
                
                // Gửi đến các handlers
                let handlers = self.alert_handlers.read().await;
                for handler in handlers.iter() {
                    if let Err(e) = handler.handle_alert(alert.clone()).await {
                        error!("Error handling health alert: {}", e);
                    }
                }
                
                // Log cảnh báo
                match alert.severity {
                    AlertSeverity::Info => info!("Health alert: {} - {}", alert.resource_type, alert.message),
                    AlertSeverity::Warning => warn!("Health alert: {} - {}", alert.resource_type, alert.message),
                    AlertSeverity::Error => error!("Health alert: {} - {}", alert.resource_type, alert.message),
                    AlertSeverity::Critical => error!("CRITICAL HEALTH ALERT: {} - {}", alert.resource_type, alert.message),
                }
            }
        });
        
        Ok(task)
    }
    
    /// Khởi động task kiểm tra sức khỏe
    async fn start_health_check(self: Arc<Self>) -> Result<tokio::task::JoinHandle<()>> {
        let config = self.config.read().await.clone();
        let check_interval = Duration::from_secs(config.check_interval_seconds);
        
        let task = tokio::spawn(async move {
            let mut interval_timer = interval(check_interval);
            
            loop {
                interval_timer.tick().await;
                
                // Kiểm tra sức khỏe hệ thống
                if self.config.read().await.enable_system_monitoring {
                    if let Err(e) = self.check_system_health().await {
                        error!("Error checking system health: {}", e);
                    }
                }
                
                // Kiểm tra kết nối mạng
                if self.config.read().await.enable_network_monitoring {
                    if let Err(e) = self.check_network_health().await {
                        error!("Error checking network health: {}", e);
                    }
                }
            }
        });
        
        Ok(task)
    }
    
    /// Thiết lập auto-reconnect cho các service
    async fn setup_auto_reconnect(&self) -> Result<()> {
        // Đăng ký health check cho RPC service
        let network_manager = self.network_manager.clone();
        
        // Đăng ký health check cho các service
        for (service_id, health_check_fn) in self.monitored_services.read().await.iter() {
            let service_id = service_id.clone();
            let health_check = health_check_fn.clone();
            
            network_manager.register_health_check(&service_id, move || {
                let health_check = health_check.clone();
                async move {
                    health_check().await
                }
            }).await?;
        }
        
        Ok(())
    }
    
    /// Kiểm tra sức khỏe hệ thống
    async fn check_system_health(&self) -> Result<()> {
        // Lấy thông tin sức khỏe hệ thống (thực tế sẽ lấy từ hệ thống)
        let system_health = SystemHealth {
            cpu_usage: get_system_cpu_usage().await?,
            memory_usage: get_system_memory_usage().await?,
            free_disk_space: get_free_disk_space().await?,
            open_file_descriptors: get_open_file_descriptors().await?,
            open_network_connections: get_open_network_connections().await?,
            uptime_seconds: get_system_uptime().await?,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        
        // Cập nhật thông tin
        {
            let mut current_health = self.system_health.write().await;
            *current_health = system_health.clone();
        }
        
        // Kiểm tra ngưỡng và tạo cảnh báo nếu cần
        let config = self.config.read().await;
        
        // Kiểm tra CPU
        if system_health.cpu_usage >= config.cpu_error_threshold {
            self.send_alert(
                "cpu_usage",
                format!("CPU usage is critically high: {:.1}%", system_health.cpu_usage),
                AlertSeverity::Error,
                config.cpu_error_threshold,
                system_health.cpu_usage,
                Some("Consider scaling up resources or optimizing performance".to_string()),
            ).await?;
        } else if system_health.cpu_usage >= config.cpu_warning_threshold {
            self.send_alert(
                "cpu_usage",
                format!("CPU usage is high: {:.1}%", system_health.cpu_usage),
                AlertSeverity::Warning,
                config.cpu_warning_threshold,
                system_health.cpu_usage,
                Some("Monitor system performance".to_string()),
            ).await?;
        }
        
        // Kiểm tra memory
        if system_health.memory_usage >= config.memory_error_threshold {
            self.send_alert(
                "memory_usage",
                format!("Memory usage is critically high: {:.1}%", system_health.memory_usage),
                AlertSeverity::Error,
                config.memory_error_threshold,
                system_health.memory_usage,
                Some("Consider increasing memory or finding memory leaks".to_string()),
            ).await?;
        } else if system_health.memory_usage >= config.memory_warning_threshold {
            self.send_alert(
                "memory_usage",
                format!("Memory usage is high: {:.1}%", system_health.memory_usage),
                AlertSeverity::Warning,
                config.memory_warning_threshold,
                system_health.memory_usage,
                Some("Monitor memory consumption".to_string()),
            ).await?;
        }
        
        // Kiểm tra disk space
        let free_disk_space_mb = system_health.free_disk_space / (1024 * 1024);
        if free_disk_space_mb <= config.disk_error_threshold_mb {
            self.send_alert(
                "disk_space",
                format!("Free disk space is critically low: {} MB", free_disk_space_mb),
                AlertSeverity::Error,
                config.disk_error_threshold_mb as f64,
                free_disk_space_mb as f64,
                Some("Free up disk space immediately".to_string()),
            ).await?;
        } else if free_disk_space_mb <= config.disk_warning_threshold_mb {
            self.send_alert(
                "disk_space",
                format!("Free disk space is low: {} MB", free_disk_space_mb),
                AlertSeverity::Warning,
                config.disk_warning_threshold_mb as f64,
                free_disk_space_mb as f64,
                Some("Consider cleaning up disk space".to_string()),
            ).await?;
        }
        
        Ok(())
    }
    
    /// Kiểm tra sức khỏe mạng
    async fn check_network_health(&self) -> Result<()> {
        let monitored_services = self.monitored_services.read().await;
        
        // Danh sách các service cần kiểm tra
        let services: Vec<(String, Arc<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>> + Send + Sync>)> = 
            monitored_services.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        
        // Kiểm tra từng service
        for (service_id, check_fn) in services {
            // Lấy trạng thái hiện tại
            let current_status = self.network_manager.get_network_status(&service_id).await;
            
            // Lưu trạng thái
            {
                let mut health = self.network_health.write().await;
                health.insert(service_id.clone(), current_status);
            }
            
            // Nếu service bị lỗi, thử kết nối lại
            if current_status == NetworkStatus::Failed {
                debug!("Service {} is in Failed state, attempting to reconnect", service_id);
                
                // Lấy số lần thử kết nối lại
                let reconnect_attempts = self.network_manager.get_reconnect_attempts(&service_id).await;
                
                // Kiểm tra xem đã vượt quá ngưỡng cảnh báo chưa
                let config = self.config.read().await;
                if reconnect_attempts >= config.alert_after_reconnect_attempts {
                    self.send_alert(
                        &format!("network_service_{}", service_id),
                        format!("Service {} is unavailable after {} reconnect attempts", service_id, reconnect_attempts),
                        AlertSeverity::Error,
                        config.alert_after_reconnect_attempts as f64,
                        reconnect_attempts as f64,
                        Some("Check network connectivity and service availability".to_string()),
                    ).await?;
                }
                
                // Thử kết nối lại nếu enable_auto_reconnect = true
                if config.enable_auto_reconnect {
                    let network_manager = self.network_manager.clone();
                    let check_fn_clone = check_fn.clone();
                    let service_id_clone = service_id.clone();
                    
                    tokio::spawn(async move {
                        let success = network_manager.try_reconnect(&service_id_clone, || check_fn_clone()).await;
                        if success {
                            debug!("Successfully reconnected to service {}", service_id_clone);
                        } else {
                            // Nếu không thành công, lên lịch thử lại
                            network_manager.clone().schedule_reconnect(
                                &service_id_clone,
                                move || check_fn_clone(),
                                Some(config.max_reconnect_attempts),
                            ).await;
                        }
                    });
                }
            }
        }
        
        Ok(())
    }
    
    /// Gửi cảnh báo
    async fn send_alert(
        &self,
        resource_type: &str,
        message: String,
        severity: AlertSeverity,
        threshold: f64,
        actual_value: f64,
        suggested_action: Option<String>,
    ) -> Result<()> {
        let alert = HealthAlert {
            resource_type: resource_type.to_string(),
            message,
            severity,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            threshold,
            actual_value,
            suggested_action,
        };
        
        self.alert_sender.send(alert).await.map_err(|e| anyhow!("Failed to send alert: {}", e))
    }
    
    /// Đăng ký service cần giám sát
    pub async fn register_service<F, Fut>(
        &self,
        service_id: &str,
        health_check_fn: F,
    ) -> Result<()>
    where
        F: Fn() -> Fut + Send + Sync + Clone + 'static,
        Fut: std::future::Future<Output = bool> + Send + 'static,
    {
        // Convert the health check function to the required type
        let boxed_fn = Arc::new(move || -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>> {
            Box::pin(health_check_fn())
        });
        
        // Register with monitored services
        {
            let mut services = self.monitored_services.write().await;
            services.insert(service_id.to_string(), boxed_fn.clone());
        }
        
        // Also register with network manager if using auto-reconnect
        let config = self.config.read().await;
        if config.enable_auto_reconnect {
            let nm = self.network_manager.clone();
            let service_id_owned = service_id.to_string();
            
            // Create a wrapper function that can be called by the network manager
            let check_wrapper = move || {
                let fn_clone = boxed_fn.clone();
                Box::pin(async move {
                    let fut = fn_clone();
                    fut.await
                }) as std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>>
            };
            
            nm.register_health_check(&service_id_owned, check_wrapper).await?;
        }
        
        info!("Registered service {} for health monitoring", service_id);
        Ok(())
    }
    
    /// Hủy đăng ký service
    pub async fn unregister_service(&self, service_id: &str) -> Result<()> {
        let mut services = self.monitored_services.write().await;
        services.remove(service_id);
        Ok(())
    }
    
    /// Đăng ký handler nhận cảnh báo
    pub async fn register_alert_handler<H>(&self, handler: H) -> Result<()>
    where
        H: HealthAlertHandler + 'static,
    {
        let mut handlers = self.alert_handlers.write().await;
        handlers.push(Arc::new(handler));
        Ok(())
    }
    
    /// Lấy thông tin sức khỏe hệ thống hiện tại
    pub async fn get_system_health(&self) -> SystemHealth {
        self.system_health.read().await.clone()
    }
    
    /// Lấy thông tin sức khỏe mạng hiện tại
    pub async fn get_network_health(&self) -> HashMap<String, NetworkStatus> {
        self.network_health.read().await.clone()
    }
    
    /// Lấy lịch sử cảnh báo
    pub async fn get_alert_history(&self, limit: Option<usize>) -> Vec<HealthAlert> {
        let history = self.alert_history.read().await;
        
        match limit {
            Some(n) => {
                let start = if history.len() > n {
                    history.len() - n
                } else {
                    0
                };
                history[start..].to_vec()
            },
            None => history.clone(),
        }
    }
    
    /// Cập nhật cấu hình health monitor
    pub async fn update_config(&self, new_config: HealthMonitorConfig) -> Result<()> {
        let mut config = self.config.write().await;
        *config = new_config;
        debug!("Health monitor configuration updated");
        Ok(())
    }
}

/// Khởi tạo singleton instance
pub static GLOBAL_HEALTH_MONITOR: once_cell::sync::OnceCell<Arc<HealthMonitor>> = once_cell::sync::OnceCell::new();

/// Lấy health monitor global
pub fn get_health_monitor() -> Arc<HealthMonitor> {
    GLOBAL_HEALTH_MONITOR.get_or_init(|| Arc::new(HealthMonitor::new(HealthMonitorConfig::default())))
}

/// Các hàm helper để lấy thông tin hệ thống. Trong ứng dụng thực tế, các hàm này sẽ dùng thư viện hệ thống
/// như sysinfo, psutil, hoặc system_info để lấy thông tin
///
/// Lấy thông tin CPU usage của hệ thống. Trong phiên bản production, cần sử dụng thư viện OS-specific
/// như sysinfo, psutil, hoặc system_info để lấy thông tin
async fn get_system_cpu_usage() -> Result<f64> {
    // Trả về giá trị giả lập cho mục đích development
    Ok(rand::random::<f64>() * 100.0)
}

async fn get_system_memory_usage() -> Result<f64> {
    // Giả lập lấy memory usage
    Ok(60.0 + (rand::random::<f64>() * 10.0))
}

async fn get_free_disk_space() -> Result<u64> {
    // Giả lập dung lượng ổ đĩa còn trống (10GB + random)
    Ok(10 * 1024 * 1024 * 1024 + (rand::random::<u64>() % (5 * 1024 * 1024 * 1024)))
}

async fn get_open_file_descriptors() -> Result<u64> {
    // Giả lập số lượng file descriptors
    Ok(500 + (rand::random::<u64>() % 500))
}

async fn get_open_network_connections() -> Result<u64> {
    // Giả lập số lượng kết nối mạng
    Ok(100 + (rand::random::<u64>() % 100))
}

async fn get_system_uptime() -> Result<u64> {
    // Giả lập uptime (hệ thống chạy được 7 ngày + random)
    Ok(7 * 24 * 60 * 60 + (rand::random::<u64>() % (24 * 60 * 60)))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    struct TestAlertHandler {
        alerts: Arc<RwLock<Vec<HealthAlert>>>,
    }
    
    #[async_trait]
    impl HealthAlertHandler for TestAlertHandler {
        async fn handle_alert(&self, alert: HealthAlert) -> Result<()> {
            let mut alerts = self.alerts.write().await;
            alerts.push(alert);
            Ok(())
        }
    }
    
    #[tokio::test]
    async fn test_health_monitor() {
        // Tạo health monitor
        let config = HealthMonitorConfig {
            check_interval_seconds: 1,
            reconnect_interval_ms: 1000,
            max_reconnect_attempts: 3,
            cpu_warning_threshold: 70.0,
            cpu_error_threshold: 90.0,
            memory_warning_threshold: 75.0,
            memory_error_threshold: 90.0,
            disk_warning_threshold_mb: 5000,
            disk_error_threshold_mb: 1000,
            enable_auto_reconnect: true,
            enable_system_monitoring: true,
            enable_network_monitoring: true,
            alert_after_reconnect_attempts: 2,
        };
        
        let monitor = Arc::new(HealthMonitor::new(config));
        
        // Đăng ký alert handler
        let alerts = Arc::new(RwLock::new(Vec::new()));
        let handler = TestAlertHandler {
            alerts: alerts.clone(),
        };
        
        if let Err(e) = monitor.register_alert_handler(handler).await {
            panic!("Failed to register alert handler: {}", e);
        }
        
        // Đăng ký service
        if let Err(e) = monitor.register_service("test_service", || async { false }).await {
            panic!("Failed to register service: {}", e);
        }
        
        // Khởi động monitor
        if let Err(e) = monitor.clone().start().await {
            panic!("Failed to start monitor: {}", e);
        }
        
        // Chờ để monitor chạy một vài lần
        sleep(Duration::from_secs(3)).await;
        
        // Dừng monitor
        if let Err(e) = monitor.stop().await {
            panic!("Failed to stop monitor: {}", e);
        }
        
        // Kiểm tra alerts
        let received_alerts = alerts.read().await;
        assert!(!received_alerts.is_empty());
        
        // Kiểm tra network health
        let network_health = monitor.get_network_health().await;
        assert!(network_health.contains_key("test_service"));
        
        // Kiểm tra system health
        let system_health = monitor.get_system_health().await;
        assert!(system_health.cpu_usage > 0.0);
        assert!(system_health.memory_usage > 0.0);
        assert!(system_health.free_disk_space > 0);
        
        // Kiểm tra alert history
        let history = monitor.get_alert_history(None).await;
        assert!(!history.is_empty());
    }
} 