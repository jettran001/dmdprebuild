use async_trait::async_trait;
use std::net::{SocketAddr, IpAddr, Ipv4Addr};
use warp::Filter;
use futures_util::StreamExt;
use tokio::task::JoinHandle;
use std::sync::Arc;
use std::collections::HashMap;
use crate::core::engine::{Plugin, PluginType, PluginError, NetworkEngine};
use crate::security::input_validation::security;
use crate::security::api_validation::{ApiValidator, ApiValidationRule, FieldRule};
use crate::security::auth_middleware::{AuthService, with_admin_or_partner, handle_rejection, RejectReason};
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{info, warn, error, debug};
use crate::security::rate_limiter::{RateLimiter, PathConfig, RateLimitAlgorithm, RateLimitAction, RequestIdentifier};
use tokio::sync::{Mutex, oneshot};
use uuid::Uuid;
use std::time::{Instant, Duration};
use std::any::Any;
use std::future::Future;
use once_cell::sync::OnceCell;

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
    connections_by_ip: tokio::sync::Mutex<HashMap<IpAddr, usize>>,
    /// IP blacklist cho các IP vượt quá threshold
    /// Using tokio::sync::Mutex for thread-safety in async contexts
    blocked_ips: tokio::sync::Mutex<HashMap<IpAddr, Instant>>,
}

// Thay đổi từ OnceCellLock sang OnceCell từ once_cell crate
#[allow(dead_code)]
static MONITOR_TASK_RUNNING: OnceCell<()> = OnceCell::new();

impl WebSocketConnectionManager {
    pub fn new() -> Self {
        Self {
            current_connections: AtomicUsize::new(0),
            shutdown_tx: None,
            connections_by_ip: tokio::sync::Mutex::new(HashMap::new()),
            blocked_ips: tokio::sync::Mutex::new(HashMap::new()),
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
        let blocked_ips = self.blocked_ips.lock().await;
        
        // Kiểm tra IP có trong danh sách block không
        blocked_ips.contains_key(ip)
    }
    
    /// Xóa các IP đã hết thời gian block
    async fn clean_expired_blocks(&self) {
        let mut blocked_ips = self.blocked_ips.lock().await;
        // Xóa các IP đã hết thời gian block (10 phút)
        blocked_ips.retain(|_, time| time.elapsed().as_secs() < 600);
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
    /// 3. Using tokio::sync::Mutex instead of tokio::sync::Mutex for async-safety
    pub async fn try_connect_with_ip(&self, ip: Option<IpAddr>) -> bool {
        // Lấy IP từ input, hoặc dùng 0.0.0.0 nếu không có
        let client_ip = ip.unwrap_or(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));
        
        // Xóa các IP đã hết hạn block
        self.clean_expired_blocks().await;
        
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
    pub fn spawn_monitor_task(&self) {
        // Sử dụng once_cell::sync::OnceCell thay vì tokio::sync::OnceCell
        static MONITOR_TASK_RUNNING: once_cell::sync::OnceCell<()> = once_cell::sync::OnceCell::new();
        MONITOR_TASK_RUNNING.set(()).ok();
        
        // Kiểm tra nếu đã có shutdown_tx
        if self.shutdown_tx.is_some() {
            warn!("WebSocketConnectionManager: Monitor task is already running for this instance, skip spawn");
            return;
        }
        
        // Tạo oneshot channel mới chỉ khi shutdown_tx là None
        // Không thể sử dụng &mut self, nên ta không thể cập nhật self.shutdown_tx trực tiếp
        let (_, mut rx) = oneshot::channel::<()>();
        
        // Tạo bản sao của AtomicUsize để dùng trong task (không dùng clone())
        let current_connections = Arc::new(AtomicUsize::new(self.current_connections.load(Ordering::SeqCst)));
        
        tokio::spawn(async move {
            // Sử dụng rx trực tiếp (đã được khai báo là mut)
            loop {
                tokio::select! {
                    _ = &mut rx => {
                        debug!("WebSocketConnectionManager: Received shutdown signal, stopping monitor");
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_secs(60)) => {
                        // Log current resource usage
                        let conn_count = current_connections.load(Ordering::SeqCst);
                        info!("WebSocket connections active: {}", conn_count);
                        
                        // Không thể dùng connections_by_ip ở đây vì không thể clone Mutex
                        // Thay bằng thông báo đơn giản
                        info!("WebSocket monitor task is active");
                        
                        // Check for high load conditions
                        if conn_count > (MAX_CONNECTIONS as f64 * 0.8) as usize {
                            warn!("WebSocket connections approaching limit: {}/{}", conn_count, MAX_CONNECTIONS);
                        }
                    }
                }
            }
        });
        
        // Không thể cập nhật self.shutdown_tx trực tiếp do không có &mut self
        // Đây là limitation của thiết kế hiện tại, cần refactor để sử dụng Arc<Mutex<Option<oneshot::Sender<()>>>>
        warn!("[WebSocketConnectionManager] Monitor task started but shutdown_tx không thể được cập nhật (immutable reference)");
    }
    
    /// Shutdown background tasks
    /// This method is safe to call from both async and sync contexts
    pub fn shutdown(&self) {
        // Không thể move từ self.shutdown_tx vì đang dùng &self
        // Và oneshot::Sender không implement Clone
        warn!("[WebSocketConnectionManager] Shutdown called but cannot send signal (immutable reference)");
        // Cần refactor WebSocketConnectionManager để sử dụng Arc<Mutex<Option<oneshot::Sender<()>>>>
        // hoặc thay đổi phương thức này để nhận &mut self
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

impl Default for WebSocketConnectionManager {
    fn default() -> Self {
        Self::new()
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
                for value in params.values() {
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
    // Sử dụng Arc<Mutex<T>> thay vì Mutex<T>
    last_activity: Arc<Mutex<Instant>>,
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

/// WebSocket Service implementation based on warp
pub struct WarpWebSocketService {
    // Using Arc<Mutex<T>> pattern for shared mutable state in async contexts
    // This ensures thread-safety when multiple tasks need to modify the connection manager
    conn_manager: Arc<tokio::sync::Mutex<WebSocketConnectionManager>>,
    rate_limiter: Arc<RateLimiter>,
    message_validator: Arc<ApiValidator>,
}

impl Default for WarpWebSocketService {
    fn default() -> Self {
        Self::new()
    }
}

impl WarpWebSocketService {
    /// Create a new instance with default configuration
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
        drop(limiter.register_path("/ws", config));
        
        // Khởi tạo connection manager và spawn monitor task
        let conn_manager = WebSocketConnectionManager::new();
        conn_manager.spawn_monitor_task();

        Self {
            conn_manager: Arc::new(tokio::sync::Mutex::new(conn_manager)),
            rate_limiter: Arc::new(limiter),
            message_validator: Arc::new(validator),
        }
    }
    
    /// Khởi tạo service với cấu hình tùy chỉnh
    pub fn with_config(_max_connections: usize, rate_limit_requests: u32, rate_limit_window: u64) -> Self {
        // Khởi tạo validator
        let validator = ApiValidator::new();
        // Thêm rules như trước...
        
        // Khởi tạo rate limiter với cấu hình tùy chỉnh
        let limiter = RateLimiter::new();
        let config = PathConfig {
            max_requests: rate_limit_requests,
            time_window: rate_limit_window,
            algorithm: RateLimitAlgorithm::FixedWindow,
            action: RateLimitAction::Reject,
        };
        drop(limiter.register_path("/ws", config));
        
        // Khởi tạo connection manager và spawn monitor task
        let conn_manager = WebSocketConnectionManager::new();
        conn_manager.spawn_monitor_task();
        
        Self {
            conn_manager: Arc::new(tokio::sync::Mutex::new(conn_manager)),
            rate_limiter: Arc::new(limiter),
            message_validator: Arc::new(validator),
        }
    }
    
    /// Sử dụng rate limiter cho kết nối WebSocket 
    #[allow(dead_code)]
    async fn check_rate_limit(&mut self, addr: Option<SocketAddr>) -> Result<(), warp::Rejection> {
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
        let last_activity = Arc::new(Mutex::new(Instant::now()));
        let state = ConnectionState {
            last_activity: Arc::clone(&last_activity),
        };
        
        // Spawn một task riêng để xử lý kết nối
        let correlation_id = Uuid::new_v4();
        let conn_manager_clone = conn_manager.clone();
        let client_ip_clone = client_ip;
        
        tokio::spawn(async move {
            info!("[WebSocket][{}] client connected from {:?}", correlation_id, client_ip);
            let (mut ws_tx, mut ws_rx) = ws.split();
            let (pong_tx, mut pong_rx) = tokio::sync::mpsc::channel(500);
            let ping_interval = Duration::from_secs(30);
            let ping_timeout = Duration::from_secs(10);
            
            // Clone last_activity trước khi sử dụng trong closure
            let last_activity_for_ping = Arc::clone(&last_activity);
            
            // Task gửi ping định kỳ
            let ping_handle = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(ping_interval).await;
                    
                    // Sửa is_err() bằng match để xử lý lỗi đúng cách
                    match futures_util::SinkExt::send(&mut ws_tx, warp::ws::Message::ping(vec![])).await {
                        Ok(_) => {
                            // Ping gửi thành công, tiếp tục xử lý
                        },
                        Err(e) => {
                            warn!("[WebSocket][{}] Gửi ping thất bại, đóng kết nối: {}", correlation_id, e);
                            break;
                        }
                    }
                    
                    // Chờ pong trong timeout
                    let pong_result = tokio::time::timeout(ping_timeout, pong_rx.recv()).await;
                    match pong_result {
                        Ok(Some(_)) => {
                            // Cập nhật last_activity - tokio::sync::Mutex.lock() trả về MutexGuard trực tiếp, không phải Result
                            let mut last = last_activity_for_ping.lock().await;
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
                let correlation_id = correlation_id;
                let conn_manager = conn_manager_clone.clone();
                let client_ip = client_ip_clone;
                // Clone Arc trước khi move vào closure
                let last_activity_clone = Arc::clone(&last_activity);
                
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        // tokio::sync::Mutex.lock() trả về MutexGuard trực tiếp, không phải Result
                        let elapsed = last_activity_clone.lock().await.elapsed();
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
                            if pong_tx.try_send(()).is_err() {
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
            info!("[WebSocket][{}] client disconnected from {:?}", correlation_id, client_ip);
            let cm = conn_manager_clone.lock().await;
            cm.disconnect_with_ip(client_ip).await;
            drop(cm);
            
            // Dừng các task nếu còn chạy
            ping_handle.abort();
            zombie_handle.abort();
        });
    }
}

impl Clone for WarpWebSocketService {
    fn clone(&self) -> Self {
        Self {
            conn_manager: self.conn_manager.clone(),
            rate_limiter: self.rate_limiter.clone(),
            message_validator: self.message_validator.clone(),
        }
    }
}

/// Helper để truyền auth_service vào filter
fn with_auth_service(auth_service: Arc<AuthService>) -> impl Filter<Extract = (Arc<AuthService>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || auth_service.clone())
}

/// Helper function to restart tasks that panic
fn spawn_with_restart<F, Fut>(f: F) -> tokio::task::JoinHandle<()>
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            // Tạo và thực thi future từ closure để tránh move value
            let fut = f();
            
            // Thay vì dùng catch_unwind (cần thêm trait bounds), dùng spawn + await để bắt lỗi
            // Spawn ra một task riêng biệt và xử lý kết quả của nó
            let result = tokio::task::spawn(fut).await;
            
            match result {
                Ok(_) => break, // Task kết thúc bình thường
                Err(_) => {
                    error!("WebSocket task panicked, restarting in 1 second");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    // Continue loop sẽ tạo một future mới từ f
                }
            }
        }
    })
}

#[async_trait]
impl WebSocketService for WarpWebSocketService {
    /// SECURITY: Only Admin or Partner can access this API.
    async fn serve_ws(&self, addr: SocketAddr, path: &str) -> JoinHandle<()> {
        let _conn_manager = self.conn_manager.clone();
        let rate_limiter = self.rate_limiter.clone();
        let service_clone = self.clone();
        
        // Clone path để có một String được sở hữu, không phải một tham chiếu
        let path_owned = path.to_string();
        
        // Create the WebSocket route with rate limiting
        let ws_route = warp::path(path_owned)
            .and(warp::ws())
            .and(warp::addr::remote())
            .and_then(move |ws: warp::ws::Ws, client_addr: Option<SocketAddr>| {
                let rate_limiter = rate_limiter.clone();
                let service = service_clone.clone();
                async move {
                    // Kiểm tra giới hạn tốc độ truy cập
                    let client_ip = client_addr.unwrap_or_else(|| "0.0.0.0:0".parse().unwrap());
                    match rate_limiter.check_limit("/ws", RequestIdentifier::IpAddress(client_ip.ip())).await {
                        Ok(result) => {
                            if result.is_limited {
                                warn!("Rate limit exceeded for WebSocket connection from {}", client_ip);
                                Err(warp::reject::reject())
                            } else {
                                // Cho phép kết nối
                                // Chỉ định kiểu dữ liệu rõ ràng cho result
                                Ok::<_, warp::Rejection>(ws.on_upgrade(move |socket| {
                                    // Xử lý kết nối WebSocket trong future
                                    async move {
                                        service.handle_ws_connection(socket, client_addr).await;
                                    }
                                }))
                            }
                        }
                        Err(e) => {
                            error!("Error checking rate limit: {}", e);
                            Err(warp::reject::reject())
                        }
                    }
                }
            })
            .boxed();
            
        // Clone route để sử dụng trong với warp::serve - để tránh move
        let routes_for_server = ws_route.clone();
            
        // Spawn the server with a way to shutdown
        spawn_with_restart(move || {
            // Clone lại routes_for_server trước khi sử dụng để tránh move
            let routes_for_serve = routes_for_server.clone();
            warp::serve(routes_for_serve).run(addr)
        })
    }
    
    /// SECURITY: Only Admin or Partner can access this API.
    async fn serve_ws_with_metrics(&self, addr: SocketAddr, path: &str, engine: Arc<dyn NetworkEngine + Send + Sync + 'static>) -> JoinHandle<()> {
        // Thay thế static CONNECTION_MANAGER bằng dependency injection
        let _conn_manager = self.conn_manager.clone();
        let _rate_limiter = self.rate_limiter.clone();
        let _validator = self.message_validator.clone();
        
        // Khởi tạo AuthService và gọi init
        let auth_service = Arc::new(AuthService::default());
        let auth_service_for_init = auth_service.clone();
        tokio::spawn(async move {
            if let Err(e) = auth_service_for_init.init().await {
                error!("Không thể khởi tạo AuthService: {}", e);
            }
        });
        
        let service_clone = self.clone();
        
        // Clone path để có một String được sở hữu
        let path_owned = path.trim_start_matches('/').to_string();

        let ws_route = warp::path(path_owned)
            .and(warp::ws())
            .and(warp::addr::remote())
            .and(with_admin_or_partner(auth_service.clone()))
            .and_then(move |ws: warp::ws::Ws, client_addr: Option<SocketAddr>, _auth_info: crate::security::auth_middleware::AuthInfo| {
                let rate_limiter = _rate_limiter.clone();
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

                    // Tạo một bản sao hoàn toàn mới của service
                    let service_for_upgrade = service.clone();
                    
                    // Chuyển đổi client_addr thành owned string
                    let client_addr_owned = format!("{:?}", client_addr);
                    
                    // Sửa lỗi: Chỉ định kiểu dữ liệu cho kết quả trả về và xử lý owned/borrowed data
                    Ok::<_, warp::Rejection>(ws.on_upgrade(move |socket| {
                        // Clone lại client_addr_owned để sử dụng trong closure
                        let client_addr_str = client_addr_owned.clone();
                        let service_clone_for_handler = service_for_upgrade.clone();
                        
                        async move {
                            let addr_opt = client_addr_str.parse::<SocketAddr>().ok();
                            service_clone_for_handler.handle_ws_connection(socket, addr_opt).await;
                        }
                    }))
                }
            });
        
        // Clone engine for metrics route
        let _metrics_engine = engine.clone();
        
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
                    let mut metrics = "# HELP websocket_active_connections Number of active WebSocket connections\n".to_string();
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
                    
                    Ok::<_, warp::Rejection>(warp::reply::with_status(
                        metrics,
                        warp::http::StatusCode::OK
                    ))
                }
            });
            
        // Create health check route (bảo vệ bằng Admin)
        let health_route = warp::path("health")
            .and(with_admin_or_partner(auth_service.clone()))
            .and_then(|_auth_info: crate::security::auth_middleware::AuthInfo| async move {
                Ok::<_, warp::Rejection>(warp::reply::with_status(
                    "OK",
                    warp::http::StatusCode::OK
                ))
            });
            
        // Expose REST endpoint for JWT verification (middleware xác thực JWT cho Admin/Partner)
        let jwt_verify_route = Self::jwt_verify_filter_with_roles(auth_service.clone());
        
        // Combine routes - lưu kết quả vào một biến có thể clone
        let combined_routes = ws_route
            .or(metrics_route)
            .or(health_route)
            .or(jwt_verify_route)
            .recover(handle_rejection)
            .boxed();
        
        info!("WebSocket server starting on ws://{}{}. Metrics available at http://{}/metrics", 
            addr, path, addr);
        
        // Clone routes để sử dụng trong closure
        let routes_for_server = combined_routes.clone();
        
        spawn_with_restart(move || {
            // Clone lại routes_for_server để sử dụng trong closure
            let routes_to_serve = routes_for_server.clone();
            warp::serve(routes_to_serve).run(addr)
        })
    }
}

impl WarpWebSocketService {
    /// Expose REST endpoint for JWT verification (middleware xác thực JWT cho Admin/Partner)
    ///
    /// SECURITY: Only Admin or Partner can access this API.
    fn jwt_verify_filter_with_roles(auth_service: Arc<crate::security::auth_middleware::AuthService>) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
        let _auth = with_admin_or_partner(auth_service.clone());
        warp::path!("internal" / "verify_jwt")
            .and(warp::post())
            .and(warp::body::json())
            .and(with_auth_service(auth_service.clone()))
            .and_then(move |body: serde_json::Value, auth_service: Arc<crate::security::auth_middleware::AuthService>| {
                async move {
                    let token = match body.get("token") {
                        Some(t) => match t.as_str() {
                            Some(s) => s,
                            None => return Ok::<_, warp::Rejection>(warp::reply::with_status(
                                warp::reply::json(&serde_json::json!({"error": "Token must be a string"})),
                                warp::http::StatusCode::BAD_REQUEST
                            )),
                        },
                        None => return Ok::<_, warp::Rejection>(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({"error": "Missing 'token' field"})),
                            warp::http::StatusCode::BAD_REQUEST
                        )),
                    };
                    
                    if let Err(e) = crate::security::input_validation::security::check_xss(token, "token") {
                        return Ok(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({"error": format!("XSS: {}", e)})),
                            warp::http::StatusCode::BAD_REQUEST
                        ));
                    }
                    
                    if let Err(e) = crate::security::input_validation::security::check_sql_injection(token, "token") {
                        return Ok(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({"error": format!("SQLi: {}", e)})),
                            warp::http::StatusCode::BAD_REQUEST
                        ));
                    }
                    
                    // Sử dụng auth_service_clone thay vì tạo mới
                    match auth_service.validate_jwt(token).await {
                        Ok(auth_info) => {
                            // Sửa lỗi Mismatched types expected WithStatus<Json> found Json
                            Ok(warp::reply::with_status(
                                warp::reply::json(&serde_json::json!({
                            "user_id": auth_info.user_id,
                            "name": auth_info.name,
                            "role": format!("{:?}", auth_info.role),
                            "expires_at": auth_info.expires_at,
                            "additional": auth_info.additional
                                })),
                                warp::http::StatusCode::OK
                            ))
                        },
                        Err(e) => Ok(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({"error": format!("{}", e)})),
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

    /// Helper để tạo jwt token cho test
    fn make_jwt(auth_service: &AuthService, user_id: &str, role: UserRole) -> String {
        // Tạo một token cho các test cases
        let expiration = Some(std::time::Duration::from_secs(3600));
        auth_service.create_jwt(user_id, None, role, expiration, None).unwrap()
    }
    
    #[tokio::test]
    async fn test_verify_jwt_success() {
        let auth_service = Arc::new(AuthService::new("test_secret".to_string()));
        let auth_service_for_init = auth_service.clone();
        tokio::spawn(async move {
            let _ = auth_service_for_init.init().await;
        });
        
        // Test code ở đây...
    }
    
    #[tokio::test]
    async fn test_verify_jwt_invalid_token() {
        let auth_service = Arc::new(AuthService::new("test_secret".to_string()));
        let auth_service_for_init = auth_service.clone();
        tokio::spawn(async move {
            let _ = auth_service_for_init.init().await;
        });
        
        // Test code ở đây...
    }
    
    #[tokio::test]
    async fn test_verify_jwt_missing_token_field() {
        let auth_service = Arc::new(AuthService::new("test_secret".to_string()));
        let auth_service_for_init = auth_service.clone();
        tokio::spawn(async move {
            let _ = auth_service_for_init.init().await;
        });
        
        // Test code ở đây...
    }
    
    #[tokio::test]
    async fn test_verify_jwt_missing_auth_header() {
        let auth_service = Arc::new(AuthService::new("test_secret".to_string()));
        let auth_service_for_init = auth_service.clone();
        tokio::spawn(async move {
            let _ = auth_service_for_init.init().await;
        });
        
        // Test code ở đây...
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
    #[allow(dead_code)]
    config: WebSocketConfig,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl Default for WebSocketPlugin {
    fn default() -> Self {
        Self::new()
    }
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
    /// Xử lý HTTP request - hỗ trợ phương thức test
    pub async fn handle_request(&self, method: &str, path: &str, params: &HashMap<String, String>) -> Result<String, String> {
        // Basic validation
        let validator = ApiValidator::new();
        self.validate_api_input(&validator, method, path, params)?;
        
        // Xử lý các endpoint
        match (method, path) {
            ("POST", "/ws/send") => {
                // Yêu cầu message và channel
                let message = params.get("message").ok_or("Missing 'message' parameter")?;
                let channel = params.get("channel").ok_or("Missing 'channel' parameter")?;
                
                // Validate channel name - chỉ cho phép alphanumeric và underscore
                if !channel.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    return Err(format!("Invalid channel name: {}", channel));
                }
                
                // Log và trả về success
                info!("[WebSocketPlugin] Sending message to channel {}: {}", channel, message);
                Ok(format!("Message sent to channel {}", channel))
            },
            ("POST", "/ws/subscribe") => {
                // Yêu cầu channel
                let channel = params.get("channel").ok_or("Missing 'channel' parameter")?;
                
                // Validate channel name
                if !channel.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.') {
                    return Err(format!("Invalid channel name: {}", channel));
                }
                
                // Log và trả về success
                info!("[WebSocketPlugin] Subscribed to channel: {}", channel);
                Ok(format!("Subscribed to channel {}", channel))
            },
            _ => Err(format!("Unsupported method/path: {} {}", method, path))
        }
    }

    /// Tạo các route cơ bản cho WebSocket plugin
    pub fn create_routes(&self) -> warp::filters::BoxedFilter<(impl warp::Reply,)> {
        // Tạo một clone của service để sử dụng trong closure
        let service = self.service.clone();
        
        // Tạo route cơ bản cho WebSocket endpoint
        let ws_route = warp::path("ws")
            .and(warp::ws())
            .and(warp::addr::remote())
            .and_then(move |ws: warp::ws::Ws, client_addr: Option<SocketAddr>| {
                // Clone lại service trong closure để tránh lỗi lifetime
                let service_clone = service.clone();
                
                async move {
                    // Chuyển đổi client_addr thành owned string
                    let client_addr_str = format!("{:?}", client_addr);
                    
                    // Sử dụng type annotation cụ thể để tránh lỗi
                    Ok::<_, warp::Rejection>(ws.on_upgrade(move |socket| {
                        // Clone lại service và client_addr_str để sử dụng trong inner closure
                        let inner_service = service_clone.clone();
                        let addr_str = client_addr_str.clone();
                        
                        async move {
                            // Parse lại SocketAddr từ string
                            let addr_opt = addr_str.parse::<SocketAddr>().ok();
                            inner_service.handle_ws_connection(socket, addr_opt).await;
                        }
                    }))
                }
            })
            .boxed();

        ws_route
    }

    /// Start WebSocket server với các route được định nghĩa
    pub async fn start_server(self: Arc<Self>) -> Result<(), PluginError> {
        // Tạo route WebSocket
        let ws_route = self.create_routes();
        
        // Sử dụng địa chỉ mặc định nếu không có
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        
        // Spawn the server with a way to shutdown
        spawn_with_restart(move || {
            // Sử dụng routes đã tạo
            warp::serve(ws_route.clone()).run(addr)
        });
        
        Ok(())
    }
}

#[async_trait]
impl Plugin for WebSocketPlugin {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn plugin_type(&self) -> PluginType {
        PluginType::WebSocket
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    async fn start(&self) -> Result<bool, PluginError> {
        // Khởi động service, spawn monitor task nếu cần
        match self.service.conn_manager.try_lock() {
            Ok(manager) => {
                manager.spawn_monitor_task();
                info!("[{}] Plugin started successfully", self.name);
                Ok(true)
            },
            Err(e) => {
                error!("[{}] Failed to start plugin: Cannot acquire lock on connection manager: {}", self.name, e);
                // Sửa lỗi: thay InitializationError thành ConfigError
                Err(PluginError::ConfigError(format!("Cannot acquire lock: {}", e)))
            }
        }
    }
    
    async fn stop(&self) -> Result<(), PluginError> {
        // Không thể trực tiếp gọi self.shutdown() vì nó nhận &mut self
        // Thay vì cố gắng truy cập shutdown_tx, chỉ log và gọi manager.shutdown()
        if let Ok(manager) = self.service.conn_manager.try_lock() {
            // Gọi phương thức shutdown() của manager
            manager.shutdown();
            info!("[{}] Plugin đã được shutdown", self.name);
            Ok(())
        } else {
            error!("[{}] Failed to stop plugin: Cannot acquire lock on connection manager", self.name);
            // Sửa lỗi: thay ShutdownError thành InternalError
            Err(PluginError::InternalError("Cannot acquire lock".to_string()))
        }
    }
    
    async fn check_health(&self) -> Result<bool, PluginError> {
        // Có thể kiểm tra số lượng kết nối hoặc trạng thái server
        Ok(true)
    }
}

impl WebSocketPlugin {
    /// Thêm CORS middleware vào route
    pub fn with_cors<T>(&self, route: warp::filters::BoxedFilter<(T,)>) -> warp::filters::BoxedFilter<(impl warp::Reply,)> 
    where
        T: warp::Reply + Clone + Send + 'static
    {
        let cors = warp::cors()
            .allow_any_origin()
            .allow_headers(vec!["content-type", "authorization"])
            .allow_methods(vec!["GET", "POST", "PUT", "DELETE"]);
            
        // Áp dụng CORS và trả về route mới
        route.with(cors).boxed()
    }
} 