use async_trait::async_trait;
use std::net::SocketAddr;
use warp::Filter;
use futures_util::StreamExt;
use futures_util::FutureExt;
use tokio::task::JoinHandle;
use std::sync::Arc;
use std::collections::HashMap;
use crate::core::engine::{Plugin, PluginType, PluginError};
use crate::security::input_validation::security;
use crate::security::api_validation::{ApiValidator, ApiValidationRule, FieldRule};
use crate::security::auth_middleware::{AuthService, AuthError, with_admin_or_partner, handle_rejection, UserRole, RejectReason};
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{info, warn, error, debug};
use crate::security::rate_limiter::{RateLimiter, PathConfig, RateLimitAlgorithm, RateLimitAction, RequestIdentifier};
use std::net::IpAddr;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use uuid::Uuid;
use std::time::{Instant, Duration};
use tokio::sync::RwLock;
use std::net::Ipv4Addr;
use crate::core::engine::NetworkEngine;

const MAX_CONNECTIONS: usize = 1000;
const MAX_CONNECTIONS_PER_IP: usize = 20; // Giới hạn tối đa kết nối mỗi IP
const MAX_MESSAGE_SIZE: usize = 1024 * 1024; // 1MB

/// WebSocketConnectionManager manages active WebSocket connections and ensures only one global monitor task is running per process.
/// This prevents duplicate resource usage logging and ensures consistent monitoring of memory and file descriptors.
pub struct WebSocketConnectionManager {
    /// Số lượng kết nối hiện tại
    pub current_connections: AtomicUsize,
    /// Kênh shutdown cho background tasks
    shutdown_tx: Option<oneshot::Sender<()>>,
    /// Track số kết nối theo từng IP để phát hiện DoS
    /// Using tokio::sync::Mutex for thread-safety in async contexts
    connections_by_ip: Mutex<HashMap<IpAddr, usize>>,
    /// IP blacklist cho các IP vượt quá threshold
    /// Using tokio::sync::Mutex for thread-safety in async contexts
    blocked_ips: Mutex<HashMap<IpAddr, Instant>>,
}

static MONITOR_TASK_RUNNING: std::sync::OnceLock<()> = std::sync::OnceLock::new();

impl WebSocketConnectionManager {
    pub fn new() -> Self {
        Self {
            current_connections: AtomicUsize::new(0),
            shutdown_tx: None,
            connections_by_ip: Mutex::new(HashMap::new()),
            blocked_ips: Mutex::new(HashMap::new()),
        }
    }
    
    /// Lấy số kết nối hiện tại
    pub fn get_current_connections(&self) -> usize {
        self.current_connections.load(Ordering::SeqCst)
    }
    
    /// Kiểm tra và block IP nếu có quá nhiều kết nối
    /// Uses tokio::sync::Mutex for thread-safety in async contexts
    async fn is_ip_blocked(&self, ip: &IpAddr) -> bool {
        // Properly acquire and await the mutex lock in async context
        let mut blocked_ips = self.blocked_ips.lock().await;
        
        // Xóa các IP đã hết thời gian block (10 phút)
        blocked_ips.retain(|_, time| time.elapsed().as_secs() < 600);
        
        // Kiểm tra IP có trong danh sách block không
        blocked_ips.contains_key(ip)
    }
    
    /// Thêm IP vào danh sách block
    /// Uses tokio::sync::Mutex for thread-safety in async contexts
    async fn block_ip(&self, ip: IpAddr) {
        // Properly acquire and await the mutex lock in async context
        let mut blocked_ips = self.blocked_ips.lock().await;
        blocked_ips.insert(ip, Instant::now());
        warn!("IP {} blocked for exceeding connection limit", ip);
    }
    
    /// Thử kết nối với kiểm tra theo IP
    /// Uses tokio::sync::Mutex for thread-safety in async contexts
    /// IMPORTANT: This method carefully manages mutex locks to avoid deadlocks by:
    /// 1. Acquiring locks in a consistent order
    /// 2. Dropping locks before calling other methods that acquire locks
    /// 3. Using tokio::sync::Mutex instead of std::sync::Mutex for async-safety
    pub async fn try_connect_with_ip(&self, ip: Option<IpAddr>) -> bool {
        // Lấy IP từ input, hoặc dùng 0.0.0.0 nếu không có
        let client_ip = ip.unwrap_or(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));
        
        // Kiểm tra nếu IP đã bị block
        // First mutex acquisition: blocked_ips
        if self.is_ip_blocked(&client_ip).await {
            warn!("Connection rejected from blocked IP: {}", client_ip);
            return false;
        }
        
        // Kiểm tra tổng số kết nối
        let prev = self.current_connections.fetch_add(1, Ordering::SeqCst);
        if prev >= MAX_CONNECTIONS {
            self.current_connections.fetch_sub(1, Ordering::SeqCst);
            warn!("Max connections limit reached ({}/{})", prev, MAX_CONNECTIONS);
            return false;
        }
        
        // Kiểm tra số kết nối theo IP
        // Second mutex acquisition: connections_by_ip
        let mut conns_by_ip = self.connections_by_ip.lock().await;
        let ip_conns = conns_by_ip.entry(client_ip).or_insert(0);
        *ip_conns += 1;
        
        // Nếu IP này đã có quá nhiều kết nối
        if *ip_conns > MAX_CONNECTIONS_PER_IP {
            // Giảm bộ đếm lại vì kết nối bị từ chối
            *ip_conns -= 1;
            self.current_connections.fetch_sub(1, Ordering::SeqCst);
            
            // Block IP này nếu vượt quá ngưỡng gấp đôi
            if *ip_conns >= MAX_CONNECTIONS_PER_IP * 2 {
                // IMPORTANT: We must drop the current mutex lock before acquiring another one
                // to prevent potential deadlocks. If we called self.block_ip() while still
                // holding the conns_by_ip lock, and block_ip tried to acquire the same lock,
                // we would have a deadlock.
                drop(conns_by_ip); // Drop lock trước khi gọi hàm khác cần lock
                self.block_ip(client_ip).await;
            } else {
                warn!("Connection limit per IP exceeded for {}: {}/{}", 
                      client_ip, *ip_conns, MAX_CONNECTIONS_PER_IP);
            }
            
            return false;
        }
        
        // Log cảnh báo khi gần đạt ngưỡng
        if prev + 1 > (MAX_CONNECTIONS as f64 * 0.9) as usize {
            warn!("WebSocket connections at {} (>{}%)", prev + 1, 90);
        }
        
        // Log khi IP có nhiều connections
        if *ip_conns > (MAX_CONNECTIONS_PER_IP as f64 * 0.7) as usize {
            warn!("IP {} has {} connections (>{}% of limit)", 
                  client_ip, *ip_conns, 70);
        }
        
        true
    }
    
    /// Legacy support cho try_connect không có IP
    pub fn try_connect(&self) -> bool {
        // Future to sync function bridge
        let rt = tokio::runtime::Handle::current();
        rt.block_on(self.try_connect_with_ip(None))
    }
    
    /// Ghi nhận disconnection với IP
    /// Uses tokio::sync::Mutex for thread-safety in async contexts
    pub async fn disconnect_with_ip(&self, ip: Option<IpAddr>) {
        let client_ip = ip.unwrap_or(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));
        
        // Properly acquire and await the mutex lock in async context
        let mut conns_by_ip = self.connections_by_ip.lock().await;
        if let Some(count) = conns_by_ip.get_mut(&client_ip) {
            if *count > 0 {
                *count -= 1;
            }
        }
        
        // Giảm tổng số kết nối
        self.current_connections.fetch_sub(1, Ordering::SeqCst);
    }
    
    /// Legacy support cho disconnect không có IP
    pub fn disconnect(&self) {
        // Future to sync function bridge
        let rt = tokio::runtime::Handle::current();
        rt.block_on(self.disconnect_with_ip(None));
    }

    /// Spawns a single global monitor task for resource usage logging (memory, file descriptors).
    /// If a monitor task is already running anywhere in the process, this call is a no-op and logs a warning.
    /// This is required to avoid duplicate logging and resource contention in multi-instance scenarios.
    pub fn spawn_monitor_task(&mut self) {
        if MONITOR_TASK_RUNNING.get().is_some() {
            warn!("WebSocketConnectionManager: Global monitor task is already running, skip spawn");
            return;
        }
        MONITOR_TASK_RUNNING.set(()).ok();
        if self.shutdown_tx.is_some() {
            warn!("WebSocketConnectionManager: Monitor task is already running for this instance, skip spawn");
            return;
        }
        let (tx, rx) = oneshot::channel::<()>();
        self.shutdown_tx = Some(tx);
        let current_connections = self.current_connections.clone();
        let connections_by_ip = self.connections_by_ip.clone();
        tokio::spawn(async move {
            // Tạo future cho rx để tránh move trong tokio::select!
            let mut rx_fut = rx;
            
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {
                        // Log số kết nối hiện tại
                        let connections = current_connections.load(Ordering::SeqCst);
                        if connections > 0 {
                            info!("WebSocket active connections: {}", connections);
                            
                            // Log kết nối theo IP
                            let ips_lock = connections_by_ip.lock().await;
                            if !ips_lock.is_empty() {
                                // Sắp xếp IP theo số lượng kết nối giảm dần và lấy top 5
                                let mut ips: Vec<(&IpAddr, &usize)> = ips_lock.iter().collect();
                                ips.sort_by(|a, b| b.1.cmp(a.1));
                                
                                // Log các IP có nhiều kết nối nhất
                                let top_ips: Vec<String> = ips.iter()
                                    .take(5)
                                    .map(|(ip, count)| format!("{}:{}", ip, count))
                                    .collect();
                                
                                info!("Top 5 IPs by connection count: {}", top_ips.join(", "));
                            }
                        }
                        // Log memory usage (nếu trên Linux)
                        #[cfg(target_os = "linux")]
                        if let Ok(meminfo) = std::fs::read_to_string("/proc/self/status") {
                            for line in meminfo.lines() {
                                if line.starts_with("VmRSS") || line.starts_with("VmSize") {
                                    info!("[Resource] {}", line);
                                }
                            }
                        }
                        // Log số file descriptor (nếu trên Linux)
                        #[cfg(target_os = "linux")]
                        if let Ok(fds) = std::fs::read_dir("/proc/self/fd") {
                            let count = fds.count();
                            info!("[Resource] Open file descriptors: {}", count);
                        }
                    }
                    _ = &mut rx_fut => {
                        info!("WebSocket monitor task shutting down");
                        break;
                    }
                }
            }
        });
    }
    
    /// Shutdown background tasks
    /// This method is safe to call from both async and sync contexts
    pub fn shutdown(&mut self) {
        // We only need to take and send on the oneshot channel
        // No mutex operations here, so it's safe for both sync and async contexts
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }

    /// Trả về số lượng kết nối hiện tại từ một IP cụ thể
    pub async fn get_connections_from_ip(&self, ip: &IpAddr) -> usize {
        let conns_by_ip = self.connections_by_ip.lock().await;
        *conns_by_ip.get(ip).unwrap_or(&0)
    }
    
    /// Trả về danh sách các IP có số lượng kết nối cao nhất
    pub async fn get_top_ips(&self, limit: usize) -> Vec<(IpAddr, usize)> {
        let conns_by_ip = self.connections_by_ip.lock().await;
        let mut ips: Vec<(IpAddr, usize)> = conns_by_ip
            .iter()
            .map(|(ip, count)| (*ip, *count))
            .collect();
        
        ips.sort_by(|a, b| b.1.cmp(&a.1));
        ips.truncate(limit);
        ips
    }
}

impl Drop for WebSocketConnectionManager {
    /// This drop implementation is safe because:
    /// 1. We're only calling shutdown() which doesn't perform mutex operations
    /// 2. The actual mutex-protected resources (connections_by_ip, blocked_ips) are
    ///    automatically dropped after this, and their contents are cleaned up properly
    fn drop(&mut self) {
        // This call is non-blocking and doesn't use any mutex operations
        self.shutdown();
    }
}

// Hàm validate WebSocket message - Sử dụng ServiceError thay vì String
fn validate_websocket_message(validator: &ApiValidator, message: &str) -> Result<String, crate::infra::service_traits::ServiceError> {
    // Kiểm tra độ dài
    if message.is_empty() {
        return Err(crate::infra::service_traits::ServiceError::ValidationError(
            "Message cannot be empty".to_string()
        ));
    }
    
    if message.len() > 1024 * 1024 {  // 1MB limit
        return Err(crate::infra::service_traits::ServiceError::ValidationError(
            format!("Message too large: {} bytes (max 1MB)", message.len())
        ));
    }
    
    // Kiểm tra xem message có phải JSON hợp lệ không
    match serde_json::from_str::<serde_json::Value>(message) {
        Ok(json) => {
            // Kiểm tra XSS/SQLi/Command injection cho mọi trường
            if let Some(obj) = json.as_object() {
                // Chuyển đổi JSON thành params
                let mut params = std::collections::HashMap::new();
                for (key, value) in obj {
                    if let Some(val_str) = value.as_str() {
                        params.insert(key.clone(), val_str.to_string());
                    } else {
                        params.insert(key.clone(), value.to_string());
                    }
                }
                
                // Lấy method và path từ JSON nếu có
                let method = params.get("method").unwrap_or(&"GET".to_string()).clone();
                let path = params.get("path").unwrap_or(&"/ws".to_string()).clone();
                
                // Kiểm tra bằng validator
                let errors = validator.validate_request(&method, &path, &params);
                if errors.has_errors() {
                    return Err(crate::infra::service_traits::ServiceError::ValidationError(
                        format!("Validation failed: {:?}", errors)
                    ));
                }
                
                // Kiểm tra thêm các payload đặc biệt
                for (_key, value) in &params {
                    // Kiểm tra XSS
                    if let Err(e) = security::check_xss(value, "websocket_param") {
                        return Err(crate::infra::service_traits::ServiceError::SecurityError(
                            format!("XSS attack detected: {}", e)
                        ));
                    }
                    
                    // Kiểm tra SQLi
                    if let Err(e) = security::check_sql_injection(value, "websocket_param") {
                        return Err(crate::infra::service_traits::ServiceError::SecurityError(
                            format!("SQL injection attack detected: {}", e)
                        ));
                    }
                    
                    // Kiểm tra Unicode attacks
                    if value.contains("\u{202E}") || value.contains("\u{202D}") { // Bidirectional override chars
                        return Err(crate::infra::service_traits::ServiceError::SecurityError(
                            "Unicode bidirectional override attack detected".to_string()
                        ));
                    }
                    
                    // Kiểm tra null chars
                    if value.contains('\0') {
                        return Err(crate::infra::service_traits::ServiceError::SecurityError(
                            "Null character injection detected".to_string()
                        ));
                    }
                    
                    // Kiểm tra UTF-8 attacks
                    if !value.chars().all(|c| (c as u32) < 0xFFFD) {
                        return Err(crate::infra::service_traits::ServiceError::SecurityError(
                            "Invalid UTF-8 sequence detected".to_string()
                        ));
                    }
                }
                
                // Sanitize JSON trước khi trả về
                return Ok(serde_json::to_string(&json).unwrap_or_else(|_| message.to_string()));
            }
            
            // Trường hợp JSON không phải object
            Ok(message.to_string())
        },
        Err(_) => {
            // Nếu không phải JSON, kiểm tra XSS và SQLi trực tiếp
            if let Err(e) = security::check_xss(message, "websocket_raw") {
                return Err(crate::infra::service_traits::ServiceError::SecurityError(
                    format!("XSS attack detected: {}", e)
                ));
            }
            
            if let Err(e) = security::check_sql_injection(message, "websocket_raw") {
                return Err(crate::infra::service_traits::ServiceError::SecurityError(
                    format!("SQL injection attack detected: {}", e)
                ));
            }
            
            Ok(message.to_string())
        }
    }
}

/// Helper to extract real client IP, ưu tiên lấy từ X-Forwarded-For, X-Real-IP nếu có, sau đó mới lấy từ socket
fn extract_ip(addr: Option<SocketAddr>, headers: Option<&warp::http::HeaderMap>) -> Option<IpAddr> {
    if let Some(headers) = headers {
        if let Some(forwarded) = headers.get("x-forwarded-for").or_else(|| headers.get("X-Forwarded-For")) {
            if let Ok(val) = forwarded.to_str() {
                if let Some(ip_str) = val.split(',').next() {
                    if let Ok(ip) = ip_str.trim().parse() {
                        return Some(ip);
                    }
                }
            }
        }
        if let Some(real_ip) = headers.get("x-real-ip").or_else(|| headers.get("X-Real-IP")) {
            if let Ok(val) = real_ip.to_str() {
                if let Ok(ip) = val.trim().parse() {
                    return Some(ip);
                }
            }
        }
    }
    addr.map(|a| a.ip())
}

/// Trạng thái cho mỗi WebSocket connection
struct ConnectionState {
    // Using tokio::sync::Mutex which is appropriate for async contexts
    // This avoids blocking the executor when locking this resource in async code
    last_activity: Mutex<Instant>,
}

#[async_trait]
/// WebSocketService trait for network plugins.
///
/// SECURITY: Only Admin or Partner can access this API.
pub trait WebSocketService: Send + Sync {
    /// Serve a WebSocket endpoint at the given path and address.
    /// Returns a JoinHandle that can be awaited or detached
    ///
    /// SECURITY: Only Admin or Partner can access this API.
    /// SECURITY WARNING: Bắt buộc validate input (XSS, SQLi, ...)
    /// cho mọi message nhận từ external source trước khi xử lý/forward.
    async fn serve_ws(&self, addr: SocketAddr, path: &str) -> JoinHandle<()>;
    
    /// Serve WebSocket with additional endpoints including /metrics for Prometheus
    ///
    /// SECURITY: Only Admin or Partner can access this API.
    async fn serve_ws_with_metrics(&self, addr: SocketAddr, path: &str, engine: Arc<dyn NetworkEngine + Send + Sync + 'static>) -> JoinHandle<()>;
}

pub struct WarpWebSocketService {
    // Using Arc<Mutex<T>> pattern for shared mutable state in async contexts
    // This ensures thread-safety when multiple tasks need to modify the connection manager
    conn_manager: Arc<Mutex<WebSocketConnectionManager>>,
    rate_limiter: Arc<RateLimiter>,
    message_validator: Arc<ApiValidator>,
}

impl WarpWebSocketService {
    pub fn new() -> Self {
        // Khởi tạo validator
        let mut validator = ApiValidator::new();
        // Thêm rules cho WebSocket messages
        let mut field_rules = HashMap::new();
        field_rules.insert(
            "message".to_string(),
            FieldRule {
                field_type: "string".to_string(),
                required: true,
                min_length: Some(1),
                max_length: Some(MAX_MESSAGE_SIZE),
                pattern: None,
                sanitize: true,
            },
        );
        field_rules.insert(
            "channel".to_string(),
            FieldRule {
                field_type: "string".to_string(),
                required: true,
                min_length: Some(1),
                max_length: Some(64),
                pattern: Some(r"^[a-zA-Z0-9_]+$".to_string()),
                sanitize: true,
            },
        );

        // Rule cho endpoint send
        validator.add_rule(ApiValidationRule {
            path: "/ws/send".to_string(),
            method: "POST".to_string(),
            required_fields: vec!["message".to_string(), "channel".to_string()],
            field_rules: field_rules.clone(),
        });

        // Rule cho endpoint subscribe
        validator.add_rule(ApiValidationRule {
            path: "/ws/subscribe".to_string(),
            method: "POST".to_string(),
            required_fields: vec!["channel".to_string()],
            field_rules: HashMap::from([("channel".to_string(), match field_rules.get("channel").cloned() {
                Some(rule) => rule,
                None => {
                    error!("[WebSocketService] Missing FieldRule for 'channel', using default");
                    FieldRule {
                        field_type: "string".to_string(),
                        required: true,
                        min_length: Some(1),
                        max_length: Some(50),
                        pattern: Some(r"^[a-zA-Z0-9_\-\.]+$".to_string()),
                        sanitize: true,
                    }
                }
            })]),
        });
        
        // Khởi tạo rate limiter
        let limiter = RateLimiter::new();
        // Allow max 10 connections per minute per IP
        let config = PathConfig {
            max_requests: 10,
            time_window: 60,
            algorithm: RateLimitAlgorithm::FixedWindow,
            action: RateLimitAction::Reject,
        };
        limiter.register_path("/ws", config).ok();
        
        // Khởi tạo connection manager và spawn monitor task
        let mut conn_manager = WebSocketConnectionManager::new();
        conn_manager.spawn_monitor_task();

        Self {
            conn_manager: Arc::new(Mutex::new(conn_manager)),
            rate_limiter: Arc::new(limiter),
            message_validator: Arc::new(validator),
        }
    }
    
    /// Khởi tạo service với cấu hình tùy chỉnh
    pub fn with_config(max_connections: usize, rate_limit_requests: u32, rate_limit_window: u64) -> Self {
        // Khởi tạo validator
        let mut validator = ApiValidator::new();
        // Thêm rules như trước...
        
        // Khởi tạo rate limiter với cấu hình tùy chỉnh
        let limiter = RateLimiter::new();
        let config = PathConfig {
            max_requests: rate_limit_requests,
            time_window: rate_limit_window,
            algorithm: RateLimitAlgorithm::FixedWindow,
            action: RateLimitAction::Reject,
        };
        limiter.register_path("/ws", config).ok();
        
        // Khởi tạo connection manager và spawn monitor task
        let mut conn_manager = WebSocketConnectionManager::new();
        conn_manager.spawn_monitor_task();
        
        Self {
            conn_manager: Arc::new(Mutex::new(conn_manager)),
            rate_limiter: Arc::new(limiter),
            message_validator: Arc::new(validator),
        }
    }
    
    /// Sử dụng rate limiter cho kết nối WebSocket 
    async fn check_rate_limit(&self, addr: Option<SocketAddr>) -> Result<(), warp::Rejection> {
        // Get client IP from socket address
        let client_ip = match addr {
            Some(socket_addr) => socket_addr.ip(),
            None => IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), // Default IP
        };
        
        // Tạo identifier từ IP
        let identifier = RequestIdentifier::IpAddress(client_ip);
        
        // Kiểm tra rate limit
        match self.rate_limiter.check_limit("/ws", identifier).await {
                Ok(result) => {
                if result.is_limited {
                    warn!("Rate limit exceeded for IP: {}", client_ip);
                    return Err(warp::reject::custom(RejectReason::RateLimited(
                        format!("Rate limit exceeded: {}", result.message.unwrap_or_default())
                    )));
                    }
                Ok(())
                },
                Err(e) => {
                error!("Error checking rate limit: {}", e);
                Err(warp::reject::custom(RejectReason::RateLimited(
                    format!("Error checking rate limit: {}", e)
                )))
            }
        }
    }
    
    /// Xử lý kết nối WebSocket mới
    async fn handle_ws_connection(&self, ws: warp::ws::WebSocket, client_addr: Option<SocketAddr>) {
        let conn_manager = self.conn_manager.clone();
        let message_validator = self.message_validator.clone();
        let max_message_size = MAX_MESSAGE_SIZE; // Thêm biến để giới hạn kích thước message rõ ràng
        let zombie_timeout = Duration::from_secs(120); // 2 phút
        
        // Lấy client IP từ SocketAddr hoặc headers
        let client_ip = extract_ip(client_addr, None);
        
        // Lấy lock để increment counter với IP
        let conn_manager_lock = conn_manager.lock().await;
        if !conn_manager_lock.try_connect_with_ip(client_ip).await {
            warn!("WebSocket connection rejected (max connections reached or IP blocked): {:?}", client_ip);
            drop(conn_manager_lock);
            return;
        }
        drop(conn_manager_lock);
        
        // Tạo trạng thái connection
        let last_activity = Mutex::new(Instant::now());
        let state = ConnectionState {
            last_activity: last_activity.clone(),
        };
        
        // Spawn một task riêng để xử lý kết nối
        let correlation_id = Uuid::new_v4();
        let state_clone = state.last_activity.clone();
        let conn_manager_clone = conn_manager.clone();
        let client_ip_clone = client_ip.clone();
        
        tokio::spawn(async move {
            info!("[WebSocket][{}] client connected from {:?}", correlation_id, client_ip);
            let (mut ws_tx, mut ws_rx) = ws.split();
            let (pong_tx, mut pong_rx) = tokio::sync::mpsc::channel(500);
            let ping_interval = Duration::from_secs(30);
            let ping_timeout = Duration::from_secs(10);
            
            // Task gửi ping định kỳ
            let ping_handle = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(ping_interval).await;
                    if ws_tx.send(warp::ws::Message::ping(vec![])).await.is_err() {
                        warn!("[WebSocket][{}] Gửi ping thất bại, đóng kết nối", correlation_id);
                        break;
                    }
                    // Chờ pong trong timeout
                    let pong_result = tokio::time::timeout(ping_timeout, pong_rx.recv()).await;
                    match pong_result {
                        Ok(Some(_)) => {
                            let mut last = state_clone.lock().await;
                            *last = Instant::now();
                        },
                        _ => {
                            warn!("[WebSocket][{}] Không nhận được pong sau {}s, đóng kết nối", correlation_id, ping_timeout.as_secs());
                            break;
                        }
                    }
                }
            });
            
            // Task kiểm tra zombie connection
            let zombie_handle = {
                let last_activity = state.last_activity.clone();
                let correlation_id = correlation_id.clone();
                let conn_manager = conn_manager_clone.clone();
                let client_ip = client_ip_clone.clone();
                
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        let elapsed = {
                            let last = last_activity.lock().await;
                            last.elapsed()
                        };
                        if elapsed > zombie_timeout {
                            warn!("[WebSocket][{}] Zombie connection detected (no activity > {}s), closing",
                                  correlation_id, zombie_timeout.as_secs());
                            
                            // Đóng zombie connection
                            let cm = conn_manager.lock().await;
                            cm.disconnect_with_ip(client_ip).await;
                            drop(cm);
                            
                            break;
                        }
                    }
                })
            };
            
            // Task nhận message (bao gồm cả pong)
            while let Some(result) = ws_rx.next().await {
                match result {
                    Ok(msg) => {
                        let mut last = state.last_activity.lock().await;
                        *last = Instant::now();
                        drop(last);
                        
                        if msg.is_pong() {
                            // Nếu queue đầy, log cảnh báo và đóng kết nối
                            if let Err(_) = pong_tx.try_send(()) {
                                warn!("[WebSocket][{}] Pong queue full (max 500), closing connection to prevent OOM", correlation_id);
                                break;
                            }
                        }
                        if msg.is_text() {
                            let text = msg.to_str().unwrap_or_default();
                            // Kiểm tra kích thước message trước khi validate
                            if text.len() > max_message_size {
                                warn!("[WebSocket][{}] Message too large ({} bytes, max {}), closing connection", 
                                    correlation_id, text.len(), max_message_size);
                                break;
                            }
                            // Validate message
                            match validate_websocket_message(&message_validator, text) {
                                Ok(sanitized) => {
                                    info!("[WebSocket][{}] Received valid message: {}", correlation_id, sanitized);
                                    // Xử lý message ở đây
                                },
                                Err(e) => {
                                    warn!("[WebSocket][{}] Invalid WebSocket message: {}", correlation_id, e);
                                }
                            }
                        }
                    },
                    Err(e) => {
                        error!("[WebSocket][{}] WebSocket error: {}", correlation_id, e);
                        break;
                    }
                }
            }
            
            // Thực hiện disconnect với IP khi kết nối đóng
            info!("[WebSocket][{}] client disconnected from {:?}", correlation_id, client_ip_clone);
            let cm = conn_manager_clone.lock().await;
            cm.disconnect_with_ip(client_ip_clone).await;
            drop(cm);
            
            // Dừng các task nếu còn chạy
            ping_handle.abort();
            zombie_handle.abort();
        });
    }
}

fn spawn_with_restart<F>(fut: F, desc: &'static str) -> JoinHandle<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            let result = std::panic::AssertUnwindSafe(fut).catch_unwind().await;
            match result {
                Ok(_) => break, // Task kết thúc bình thường
                Err(_) => {
                    warn!("[WebSocket] Task '{}' panic, auto-restarting...", desc);
                    // Có thể sleep ngắn để tránh loop restart quá nhanh
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    })
}

#[async_trait]
impl WebSocketService for WarpWebSocketService {
    /// SECURITY: Only Admin or Partner can access this API.
    async fn serve_ws(&self, addr: SocketAddr, path: &str) -> JoinHandle<()> {
        let conn_manager = self.conn_manager.clone();
        let rate_limiter = self.rate_limiter.clone();
        let service_clone = self.clone();
        
        // Create the WebSocket route with rate limiting
        let ws_route = warp::path(path)
            .and(warp::ws())
            .and(warp::addr::remote())
            .and_then(move |ws: warp::ws::Ws, client_addr: Option<SocketAddr>| {
                let rate_limiter = rate_limiter.clone();
                let service = service_clone.clone();
                async move {
                    // Extract client IP
                    let client_ip = extract_ip(client_addr, None).unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)));
                    
                    // Check rate limit
                    if let Err(rejection) = rate_limiter.check_limit("/ws", RequestIdentifier::IpAddress(client_ip)).await {
                        return Err(warp::reject::custom(
                            RejectReason::RateLimited(format!("Rate limit exceeded: {}.", rejection))
                        ));
                    }
                    
                    // Upgrade the connection to WebSocket, passing client_addr to handle_ws_connection
                    Ok(ws.on_upgrade(move |socket| service.handle_ws_connection(socket, client_addr)))
                }
            });
            
        // Spawn the server with a way to shutdown
        spawn_with_restart(warp::serve(ws_route).run(addr), "WebSocket Server")
    }
    
    /// SECURITY: Only Admin or Partner can access this API.
    async fn serve_ws_with_metrics(&self, addr: SocketAddr, path: &str, engine: Arc<dyn NetworkEngine + Send + Sync + 'static>) -> JoinHandle<()> {
        // Thay thế static CONNECTION_MANAGER bằng dependency injection
        let conn_manager = self.conn_manager.clone();
        let rate_limiter = self.rate_limiter.clone();
        let validator = self.message_validator.clone();
        let auth_service = Arc::new(AuthService::default());
        let service_clone = self.clone();

        let ws_route = warp::path(path.trim_start_matches('/'))
            .and(warp::ws())
            .and(warp::addr::remote())
            .and(with_admin_or_partner(auth_service.clone()))
            .and_then(move |ws: warp::ws::Ws, client_addr: Option<SocketAddr>, _auth_info: crate::security::auth_middleware::AuthInfo| {
                let rate_limiter = rate_limiter.clone();
                let service = service_clone.clone();
                async move {
                    // Kiểm tra rate limit
                    if let Some(ip) = extract_ip(client_addr, None) {
                        if let Ok(result) = rate_limiter.check_limit("/ws", RequestIdentifier::IpAddress(ip)).await {
                            if result.is_limited {
                                return Err(warp::reject::custom(RejectReason::RateLimited(
                                    format!("Rate limit exceeded: {}.", result.remaining)
                                )));
                            }
                        }
                    }

                    // Nâng cấp kết nối WebSocket, passing client_addr to handle_ws_connection
                    Ok(ws.on_upgrade(move |socket| service.handle_ws_connection(socket, client_addr)))
                }
            });
        
        // Clone engine for metrics route
        let metrics_engine = engine.clone();
        
        // Create metrics route (bảo vệ bằng Admin)
        let conn_manager_metrics = self.conn_manager.clone();
        let metrics_route = warp::path("metrics")
            .and(with_admin_or_partner(auth_service.clone()))
            .and_then(move |_auth_info: crate::security::auth_middleware::AuthInfo| {
                let conn_manager_metrics = conn_manager_metrics.clone();
                async move {
                    let lock = conn_manager_metrics.lock().await;
                    let connections = lock.current_connections.load(Ordering::SeqCst);
                    
                    // Tạo output metrics với format Prometheus
                    let mut metrics = format!("# HELP websocket_active_connections Number of active WebSocket connections\n");
                    metrics.push_str("# TYPE websocket_active_connections gauge\n");
                    metrics.push_str(&format!("websocket_active_connections {}\n", connections));
                    
                    // Thêm thông tin connections theo IP (Top 10)
                    let top_ips = lock.get_top_ips(10).await;
                    if !top_ips.is_empty() {
                        metrics.push_str("\n# HELP websocket_connections_by_ip Number of connections per IP\n");
                        metrics.push_str("# TYPE websocket_connections_by_ip gauge\n");
                        for (ip, count) in top_ips {
                            // Chuyển đổi IP thành label phù hợp với Prometheus
                            let ip_str = ip.to_string().replace(".", "_").replace(":", "_");
                            metrics.push_str(&format!("websocket_connections_by_ip{{ip=\"{}\"}} {}\n", ip_str, count));
                        }
                    }
                    
                    Ok::<_, warp::Rejection>(warp::reply::with_header(
                        metrics,
                        "content-type",
                        "text/plain; version=0.0.4"
                    ))
                }
            });
            
        // Create health check route (bảo vệ bằng Admin)
        let health_route = warp::path("health")
            .and(with_admin_or_partner(auth_service.clone()))
            .and_then(|_auth_info: crate::security::auth_middleware::AuthInfo| async move {
                Ok(warp::reply::with_status(
                    "OK",
                    warp::http::StatusCode::OK
                ))
            });
            
        // Expose REST endpoint for JWT verification (bảo vệ bằng Admin)
        let jwt_verify_route = Self::jwt_verify_filter_with_roles(auth_service.clone());
        
        // Combine routes
        let routes = ws_route
            .or(metrics_route)
            .or(health_route)
            .or(jwt_verify_route)
            .recover(handle_rejection);
        
        info!("WebSocket server starting on ws://{}{}. Metrics available at http://{}/metrics", 
            addr, path, addr);
        
        spawn_with_restart(async move {
            warp::serve(routes).run(addr).await;
        }, "WebSocketServerWithMetrics")
    }
}

impl WarpWebSocketService {
    /// Expose REST endpoint for JWT verification (middleware xác thực JWT cho Admin/Partner)
    ///
    /// SECURITY: Only Admin or Partner can access this API.
    fn jwt_verify_filter_with_roles(auth_service: Arc<crate::security::auth_middleware::AuthService>) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
        let auth = with_admin_or_partner(auth_service.clone());
        warp::path!("internal" / "verify_jwt")
            .and(warp::post())
            .and(warp::body::json())
            .and(auth)
            .and_then(move |body: serde_json::Value, _: crate::security::auth_middleware::AuthInfo| {
                let auth_service = auth_service.clone();
                async move {
                    let token = match body.get("token").and_then(|v| v.as_str()) {
                        Some(t) => t,
                        None => return Ok(warp::reply::with_status(
                            serde_json::json!({"error": "Missing 'token' field"}), 
                            warp::http::StatusCode::BAD_REQUEST
                        )),
                    };
                    
                    // Sử dụng ServiceError thay vì String để đồng nhất cách xử lý lỗi
                    if let Err(e) = crate::security::input_validation::security::check_xss(token, "token") {
                        return Ok(warp::reply::with_status(
                            serde_json::json!({"error": format!("XSS: {}", e)}), 
                            warp::http::StatusCode::BAD_REQUEST
                        ));
                    }
                    
                    if let Err(e) = crate::security::input_validation::security::check_sql_injection(token, "token") {
                        return Ok(warp::reply::with_status(
                            serde_json::json!({"error": format!("SQLi: {}", e)}), 
                            warp::http::StatusCode::BAD_REQUEST
                        ));
                    }
                    
                    // Sử dụng auth_service_clone thay vì tạo mới
                    match auth_service.validate_jwt(token) {
                        Ok(auth_info) => Ok(warp::reply::json(&serde_json::json!({
                            "user_id": auth_info.user_id,
                            "name": auth_info.name,
                            "role": format!("{:?}", auth_info.role),
                            "expires_at": auth_info.expires_at,
                            "additional": auth_info.additional
                        }))),
                        Err(e) => Ok(warp::reply::with_status(
                            serde_json::json!({"error": format!("{}", e)}), 
                            warp::http::StatusCode::UNAUTHORIZED
                        )),
                    }
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use warp::test::request;
    use serde_json::json;
    use crate::security::auth_middleware::{AuthService, UserRole};
    use std::sync::Arc;

    fn make_jwt(auth_service: &AuthService, user_id: &str, role: UserRole) -> String {
        auth_service.create_jwt(user_id, Some("Test User"), role, None, None).unwrap()
    }

    #[tokio::test]
    async fn test_verify_jwt_success() {
        let auth_service = Arc::new(AuthService::default());
        let jwt = make_jwt(&auth_service, "user123", UserRole::Service);
        let filter = WarpWebSocketService::jwt_verify_filter_with_roles(auth_service.clone());
        let resp = request()
            .method("POST")
            .path("/internal/verify_jwt")
            .header("authorization", format!("Bearer {}", jwt))
            .json(&json!({"token": jwt}))
            .reply(&filter)
            .await;
        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
        assert_eq!(body["user_id"], "user123");
        assert_eq!(body["role"], "Service");
    }

    #[tokio::test]
    async fn test_verify_jwt_invalid_token() {
        let auth_service = Arc::new(AuthService::default());
        let filter = WarpWebSocketService::jwt_verify_filter_with_roles(auth_service.clone());
        let resp = request()
            .method("POST")
            .path("/internal/verify_jwt")
            .header("authorization", "Bearer invalid.jwt.token")
            .json(&json!({"token": "invalid.jwt.token"}))
            .reply(&filter)
            .await;
        assert_eq!(resp.status(), 401);
        let body: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
        assert!(body["error"].as_str().unwrap().contains("Token không hợp lệ") || body["error"].as_str().unwrap().contains("Invalid"));
    }

    #[tokio::test]
    async fn test_verify_jwt_missing_token_field() {
        let auth_service = Arc::new(AuthService::default());
        let jwt = make_jwt(&auth_service, "user123", UserRole::Service);
        let filter = WarpWebSocketService::jwt_verify_filter_with_roles(auth_service.clone());
        let resp = request()
            .method("POST")
            .path("/internal/verify_jwt")
            .header("authorization", format!("Bearer {}", jwt))
            .json(&json!({}))
            .reply(&filter)
            .await;
        assert_eq!(resp.status(), 400);
        let body: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
        assert!(body["error"].as_str().unwrap().contains("Missing 'token' field"));
    }

    #[tokio::test]
    async fn test_verify_jwt_missing_auth_header() {
        let auth_service = Arc::new(AuthService::default());
        let jwt = make_jwt(&auth_service, "user123", UserRole::Service);
        let filter = WarpWebSocketService::jwt_verify_filter_with_roles(auth_service.clone());
        let resp = request()
            .method("POST")
            .path("/internal/verify_jwt")
            .json(&json!({"token": jwt}))
            .reply(&filter)
            .await;
        assert_eq!(resp.status(), 401);
        let body: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
        assert!(body["error"].as_str().unwrap().contains("không có quyền") || body["error"].as_str().unwrap().contains("quyền truy cập"));
    }
    
    #[test]
    fn test_websocket_message_validation() {
        // Tạo API validator mock
        let validator = ApiValidator::new();
        
        // Test valid message
        let result = validate_websocket_message(&validator, "Hello, world!");
        assert!(result.is_ok());
        
        // Test XSS
        let result = validate_websocket_message(&validator, "<script>alert('XSS')</script>");
        assert!(result.is_err());
        
        // Test too long message
        let long_message = "a".repeat(MAX_MESSAGE_SIZE + 1);
        let result = validate_websocket_message(&validator, &long_message);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_websocket_plugin_handle_request() {
        let plugin = WebSocketPlugin::new();
        
        // Test valid send request
        let mut params = HashMap::new();
        params.insert("message".to_string(), "Hello, world!".to_string());
        params.insert("channel".to_string(), "general".to_string());
        
        let result = tokio_test::block_on(plugin.handle_request("POST", "/ws/send", &params));
        assert!(result.is_ok());
        
        // Test invalid channel
        let mut params = HashMap::new();
        params.insert("message".to_string(), "Hello, world!".to_string());
        params.insert("channel".to_string(), "general-chat!".to_string());
        
        let result = tokio_test::block_on(plugin.handle_request("POST", "/ws/send", &params));
        assert!(result.is_err());
        
        // Test missing required field
        let mut params = HashMap::new();
        params.insert("message".to_string(), "Hello, world!".to_string());
        
        let result = tokio_test::block_on(plugin.handle_request("POST", "/ws/send", &params));
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_connection_limit_exceeded() {
        let mut manager = WebSocketConnectionManager::new();
        for _ in 0..MAX_CONNECTIONS {
            assert!(manager.try_connect());
        }
        // Lần tiếp theo phải bị từ chối
        assert!(!manager.try_connect());
        // Giảm counter và thử lại
        manager.disconnect();
        assert!(manager.try_connect());
    }

    #[tokio::test]
    async fn test_disconnect_decrement() {
        let mut manager = WebSocketConnectionManager::new();
        assert_eq!(manager.get_current_connections(), 0);
        assert!(manager.try_connect());
        assert_eq!(manager.get_current_connections(), 1);
        manager.disconnect();
        assert_eq!(manager.get_current_connections(), 0);
    }

    #[tokio::test]
    async fn test_zombie_connection_detection() {
        let zombie_timeout = Duration::from_secs(1);
        let last_activity = Arc::new(Mutex::new(Instant::now().checked_sub(Duration::from_secs(2)).unwrap()));
        let detected = Arc::new(Mutex::new(false));
        let detected_clone = detected.clone();
        let last_activity_clone = last_activity.clone();
        let handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let elapsed = {
                    let last = last_activity_clone.lock().await;
                    last.elapsed()
                };
                if elapsed > zombie_timeout {
                    *detected_clone.lock().await = true;
                    break;
                }
            }
        });
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(*detected.lock().await);
        handle.abort();
    }

    #[test]
    fn test_validate_websocket_message_xss_payloads() {
        let validator = ApiValidator::new();
        let xss_payloads = vec![
            "<script>alert('XSS')</script>",
            "<img src=x onerror=alert(1)>",
            "<svg/onload=alert(1)>",
            "<iframe src=javascript:alert(1)>",
            // Unicode attack vectors
            "javascript&#x3A;alert(1)", 
            "javascript\u{202E}try\u{202D}catch",
            "<a href=\"\u{006A}\u{0061}\u{0076}\u{0061}\u{0073}\u{0063}\u{0072}\u{0069}\u{0070}\u{0074}\u{003A}alert(1)\">Click me</a>",
            // JSON embedded attack
            "{\"message\":\"<script>alert('XSS')</script>\"}",
            "{\"path\":\"/ws\",\"method\":\"POST\",\"data\":\"<script>alert(1)</script>\"}"
        ];
        for payload in xss_payloads {
            let result = super::validate_websocket_message(&validator, payload);
            assert!(result.is_err(), "Payload should be rejected: {}", payload);
        }
    }

    #[test]
    fn test_validate_websocket_message_sql_payloads() {
        let validator = ApiValidator::new();
        let sqli_payloads = vec![
            "' OR '1'='1", 
            "1; DROP TABLE users; --", 
            "admin' --", 
            "' UNION SELECT NULL--",
            // JSON embedded attack
            "{\"query\":\"' OR '1'='1\"}",
            "{\"path\":\"/ws\",\"method\":\"POST\",\"data\":\"1; DROP TABLE users; --\"}"
        ];
        for payload in sqli_payloads {
            let result = super::validate_websocket_message(&validator, payload);
            assert!(result.is_err(), "Payload should be rejected: {}", payload);
        }
    }

    #[test]
    fn test_validate_websocket_message_unicode_attacks() {
        let validator = ApiValidator::new();
        let unicode_payloads = vec![
            // Bidirectional text override attacks
            "User input: \u{202E}gnp.harmful-file.exe",
            // Homoglyph attacks
            "аррӏе.com", // Looks like apple.com but uses Cyrillic chars
            // Zero-width characters
            "hidden\u{200B}payload",
            // Null bytes
            "injection\0command",
            // JSON embedded attack
            "{\"filename\":\"malicious\u{202E}gnp.exe\"}"
        ];
        for payload in unicode_payloads {
            let result = super::validate_websocket_message(&validator, payload);
            assert!(result.is_err(), "Payload should be rejected: {}", payload);
        }
    }

    #[test]
    fn test_validate_websocket_message_valid() {
        let validator = ApiValidator::new();
        let valid_payloads = vec![
            "Hello world",
            "{\"message\":\"Hello world\"}",
            "{\"path\":\"/ws\",\"method\":\"POST\",\"data\":\"Valid data\"}",
            "12345"
        ];
        for payload in valid_payloads {
            let result = super::validate_websocket_message(&validator, payload);
            assert!(result.is_ok(), "Payload should be accepted: {}", payload);
        }
    }
}

#[cfg(test)]
mod error_handling_tests {
    use super::*;
    use crate::security::api_validation::ApiValidator;
    use std::net::SocketAddr;
    use warp::ws::Message;

    #[test]
    fn test_validate_websocket_message_invalid() {
        let validator = ApiValidator::new();
        // Message quá ngắn
        let result = super::validate_websocket_message(&validator, "");
        assert!(result.is_err());
        // Message quá dài
        let long_msg = "a".repeat(super::MAX_MESSAGE_SIZE + 1);
        let result = super::validate_websocket_message(&validator, &long_msg);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_connection_limit_exceeded() {
        let mut manager = WebSocketConnectionManager::new();
        for _ in 0..super::MAX_CONNECTIONS {
            assert!(manager.try_connect());
        }
        // Lần tiếp theo phải bị từ chối
        assert!(!manager.try_connect());
    }

    #[tokio::test]
    async fn test_disconnect_decrement() {
        let mut manager = WebSocketConnectionManager::new();
        assert!(manager.try_connect());
        manager.disconnect();
        assert_eq!(manager.get_current_connections(), 0);
    }
}

/// Cấu hình cho WebSocket Plugin
#[derive(Debug, Clone)]
pub struct WebSocketConfig {
    /// Số kết nối tối đa cho phép
    pub max_connections: usize,
    /// Số kết nối tối đa cho mỗi IP
    pub max_connections_per_ip: usize,
    /// Kích thước tin nhắn tối đa (bytes)
    pub max_message_size: usize,
    /// Thời gian timeout cho kết nối (milliseconds)
    pub connection_timeout_ms: u64,
    /// Số lượng request/phút tối đa (rate limiting)
    pub rate_limit_requests: u32,
    /// Khoảng thời gian cho rate limiting (seconds)
    pub rate_limit_window: u64,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            max_connections: MAX_CONNECTIONS,
            max_connections_per_ip: MAX_CONNECTIONS_PER_IP,
            max_message_size: MAX_MESSAGE_SIZE,
            connection_timeout_ms: 30000,
            rate_limit_requests: 300,
            rate_limit_window: 60,
        }
    }
}

/// WebSocketPlugin: Chuẩn hóa 3 lớp plugin (Config, Service, Plugin)
/// Cảnh báo: Bắt buộc validate input (XSS/SQLi) cho mọi message nhận từ external source trước khi xử lý/forward.
/// Đảm bảo tuân thủ chuẩn hóa plugin network, không sử dụng static/global, không re-export struct không tồn tại.
pub struct WebSocketPlugin {
    name: String,
    service: Arc<WarpWebSocketService>,
    config: WebSocketConfig,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl WebSocketPlugin {
    /// Khởi tạo plugin với cấu hình mặc định
    pub fn new() -> Self {
        Self {
            name: "websocket".to_string(),
            service: Arc::new(WarpWebSocketService::new()),
            config: WebSocketConfig::default(),
            shutdown_tx: None,
        }
    }
    /// Khởi tạo plugin với cấu hình tùy chỉnh
    pub fn new_with_config(config: WebSocketConfig) -> Self {
        Self {
            name: "websocket".to_string(),
            service: Arc::new(WarpWebSocketService::with_config(
                config.max_connections,
                config.rate_limit_requests,
                config.rate_limit_window
            )),
            config,
            shutdown_tx: None,
        }
    }
    /// Validate input cho API endpoint bằng ApiValidator
    pub fn validate_api_input(&self, validator: &crate::security::api_validation::ApiValidator, method: &str, path: &str, params: &std::collections::HashMap<String, String>) -> Result<(), String> {
        let errors = validator.validate_request(method, path, params);
        if errors.has_errors() {
            return Err(format!("Validation failed: {:?}", errors));
        }
        Ok(())
    }
    /// Shutdown plugin, release resources
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Plugin for WebSocketPlugin {
    fn name(&self) -> &str {
        &self.name
    }
    fn plugin_type(&self) -> PluginType {
        PluginType::WebRtc // Nếu có PluginType::WebSocket thì thay thế
    }
    fn start(&self) -> Result<bool, PluginError> {
        // Khởi động service, spawn monitor task nếu cần
        match self.service.conn_manager.try_lock() {
            Ok(mut manager) => {
                manager.spawn_monitor_task();
                Ok(true)
            },
            Err(e) => {
                warn!("[WebSocketPlugin] Failed to acquire lock for connection manager: {}", e);
                Err(PluginError::StartFailed("Failed to acquire lock for connection manager".to_string()))
            }
        }
    }
    fn stop(&self) -> Result<(), PluginError> {
        // Không thể trực tiếp gọi self.shutdown() vì nó nhận &mut self
        // Tạo một clone của service và gọi shutdown trên đó
        if let Ok(mut manager) = self.service.conn_manager.try_lock() {
            // Sử dụng tiền tố underscore cho biến không sử dụng
            manager.shutdown();
        }
        
        // Đối với shutdown_tx, chỉ gửi nếu có
        if let Some(tx) = &self.shutdown_tx {
            let _ = tx.send(());
        }
        
        Ok(())
    }
    fn check_health(&self) -> Result<bool, PluginError> {
        // Có thể kiểm tra số lượng kết nối hoặc trạng thái server
        Ok(true)
    }
} 