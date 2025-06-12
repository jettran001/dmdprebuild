use crate::errors::NetworkError;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, mpsc};
use tokio::task::JoinHandle;
use tracing::{info, warn, error, debug};
use std::time::Duration;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};

/// Event type cho các sự kiện trong network domain
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventType {
    /// Sự kiện khi một node kết nối
    NodeConnected,
    /// Sự kiện khi một node ngắt kết nối
    NodeDisconnected,
    /// Sự kiện khi một plugin được đăng ký
    PluginRegistered,
    /// Sự kiện khi một plugin bị hủy đăng ký
    PluginUnregistered,
    /// Sự kiện khi nhận được một message
    MessageReceived,
    /// Sự kiện khi gửi một message
    MessageSent,
    /// Sự kiện khi có lỗi xảy ra
    Error,
    /// Sự kiện khi cấu hình thay đổi
    ConfigChanged,
    /// Sự kiện khi service bắt đầu
    ServiceStarted,
    /// Sự kiện khi service dừng lại
    ServiceStopped,
    /// Sự kiện tùy chỉnh
    Custom(String),
}

/// Event struct cho các sự kiện trong network domain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Loại sự kiện
    pub event_type: EventType,
    /// Dữ liệu sự kiện
    pub data: String,
    /// Timestamp khi sự kiện xảy ra
    pub timestamp: u64,
}

/// EventHandler trait cho việc xử lý sự kiện
#[async_trait]
pub trait EventHandler: Send + Sync + 'static {
    /// Xử lý một sự kiện
    async fn handle_event(&self, event: Event) -> Result<(), NetworkError>;
}

/// Cấu hình cho EventRouter
#[derive(Debug, Clone)]
pub struct EventRouterConfig {
    /// Kích thước buffer cho kênh xử lý event
    pub channel_buffer_size: usize,
    /// Thời gian timeout cho việc xử lý event (ms)
    pub event_timeout_ms: u64,
    /// Thời gian giữa các lần kiểm tra và làm sạch tài nguyên (ms)
    pub cleanup_interval_ms: u64,
    /// Số lượng worker tối đa cho việc xử lý event
    pub max_workers: usize,
}

impl Default for EventRouterConfig {
    fn default() -> Self {
        Self {
            channel_buffer_size: 1000,
            event_timeout_ms: 5000,
            cleanup_interval_ms: 30000,
            max_workers: 10,
        }
    }
}

/// Manager quản lý tất cả các background task để đảm bảo chúng được dọn dẹp đúng cách
/// khi EventRouter bị drop, tránh memory leak.
pub struct TaskManager {
    /// Danh sách các background tasks
    tasks: Vec<JoinHandle<()>>,
    /// Shutdown signal sender
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl TaskManager {
    /// Tạo task manager mới
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            shutdown_tx: None,
        }
    }

    /// Thêm task mới vào danh sách quản lý
    pub fn add_task(&mut self, task: JoinHandle<()>) {
        self.tasks.push(task);
    }

    /// Thiết lập shutdown sender
    pub fn set_shutdown_sender(&mut self, tx: mpsc::Sender<()>) {
        self.shutdown_tx = Some(tx);
    }

    /// Dọn dẹp tất cả các tasks
    pub async fn cleanup(&mut self) {
        // Gửi tín hiệu shutdown nếu có
        if let Some(tx) = &self.shutdown_tx {
            if let Err(e) = tx.send(()).await {
                error!("[TaskManager] Failed to send shutdown signal: {}", e);
            }
        }

        // Abort tất cả các background tasks
        for task in self.tasks.drain(..) {
            task.abort();
        }

        debug!("[TaskManager] All background tasks have been aborted");
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TaskManager {
    fn drop(&mut self) {
        // Sử dụng tokio runtime hiện tại nếu có
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            rt.block_on(async {
                // Dọn dẹp tất cả tasks
                for task in self.tasks.drain(..) {
                    task.abort();
                }
                
                // Gửi tín hiệu shutdown nếu có
                if let Some(tx) = &self.shutdown_tx {
                    let _ = tx.send(()).await;
                }
                
                debug!("[TaskManager] All tasks cleaned up on drop");
            });
        } else {
            // Không có runtime, chúng ta vẫn cần abort các tasks
            for task in self.tasks.drain(..) {
                task.abort();
            }
            warn!("[TaskManager] No tokio runtime available, tasks aborted without sending shutdown signal");
        }
    }
}

/// Map chuỗi EventType đến danh sách các event handler
pub type EventHandlerMap = Arc<RwLock<HashMap<EventType, Vec<Arc<dyn EventHandler>>>>>;

/// EventRouter chịu trách nhiệm định tuyến các sự kiện đến các handler tương ứng
pub struct EventRouter {
    /// Map từ EventType -> Vec<Arc<dyn EventHandler>>
    handlers: EventHandlerMap,
    /// Kênh gửi event
    event_sender: mpsc::Sender<Event>,
    /// Kênh nhận event
    event_receiver: Arc<Mutex<mpsc::Receiver<Event>>>,
    /// Cấu hình router
    config: EventRouterConfig,
    /// Task manager
    task_manager: Arc<RwLock<TaskManager>>,
    /// Đang chạy?
    is_running: Arc<RwLock<bool>>,
}

impl EventRouter {
    /// Tạo EventRouter mới với cấu hình mặc định
    pub fn new() -> Self {
        Self::with_config(EventRouterConfig::default())
    }
    
    /// Tạo EventRouter với cấu hình tùy chỉnh
    pub fn with_config(config: EventRouterConfig) -> Self {
        let (event_sender, event_receiver) = mpsc::channel(config.channel_buffer_size);
        
        Self {
            handlers: Arc::new(RwLock::new(HashMap::<EventType, Vec<Arc<dyn EventHandler>>>::new())),
            event_sender,
            event_receiver: Arc::new(Mutex::new(event_receiver)),
            config,
            task_manager: Arc::new(RwLock::new(TaskManager::new())),
            is_running: Arc::new(RwLock::new(false)),
        }
    }
    
    /// Đăng ký handler cho một loại sự kiện
    pub async fn register_handler(&self, event_type: EventType, handler: Arc<dyn EventHandler>) -> Result<(), NetworkError> {
        // Lấy lock cho handlers
        let mut handlers = self.handlers.write().await;
        
        handlers.entry(event_type.clone())
            .or_insert_with(Vec::<Arc<dyn EventHandler>>::new)
            .push(handler);
            
        debug!("[EventRouter] Registered handler for event type: {:?}", event_type);
        Ok(())
    }
    
    /// Hủy đăng ký handler cho một loại sự kiện
    pub async fn unregister_handler(&self, event_type: &EventType) -> Result<(), NetworkError> {
        // Lấy lock cho handlers
        let mut handlers = self.handlers.write().await;
        
        handlers.remove(event_type);
        
        debug!("[EventRouter] Unregistered all handlers for event type: {:?}", event_type);
        Ok(())
    }
    
    /// Gửi sự kiện mới vào router để xử lý
    pub async fn emit(&self, event: Event) -> Result<(), NetworkError> {
        // Kiểm tra nếu router đang chạy
        let is_running = self.is_running.read().await;
        if !*is_running {
            return Err(NetworkError::EventError("EventRouter is not running".to_string()));
        }
        
        // Sử dụng match thay vì unwrap để xử lý lỗi khi gửi event
        match self.event_sender.send(event.clone()).await {
            Ok(_) => {
                debug!("[EventRouter] Emitted event of type: {:?}", event.event_type);
                Ok(())
            },
            Err(e) => {
                error!("[EventRouter] Failed to emit event: {}", e);
                Err(NetworkError::EventError(format!("Failed to emit event: {}", e)))
            }
        }
    }
    
    /// Khởi động router
    pub async fn start(&self) -> Result<(), NetworkError> {
        // Lấy lock cho is_running
        let mut is_running_guard = self.is_running.write().await;
        
        if *is_running_guard {
            return Err(NetworkError::EventError("EventRouter is already running".to_string()));
        }
        
        *is_running_guard = true;
        
        // Tạo kênh shutdown
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        
        // Thiết lập shutdown sender cho task manager
        {
            let mut task_manager = self.task_manager.write().await;
            task_manager.set_shutdown_sender(shutdown_tx.clone());
        }
        
        // Khởi động worker task
        let handlers = self.handlers.clone();
        let event_receiver = self.event_receiver.clone();
        let _config = self.config.clone();
        let _task_manager = self.task_manager.clone();
        
        let worker_handle = tokio::spawn(async move {
            info!("[EventRouter] Starting event processing worker");
            
            // Lấy receiver từ mutex
            let mut receiver = event_receiver.lock().await;
            let handlers = handlers;
            
            // Xử lý events cho đến khi nhận được tín hiệu shutdown
            loop {
                tokio::select! {
                    // Nhận event mới để xử lý
                    Some(event) = receiver.recv() => {
                        debug!("[EventRouter] Received event: {:?}", event);
                        let handlers_guard = handlers.read().await;
                        
                        // Tìm handlers cho event type
                        if let Some(event_handlers) = handlers_guard.get(&event.event_type) {
                            for handler in event_handlers {
                                match handler.handle_event(event.clone()).await {
                                    Ok(_) => {},
                                    Err(e) => {
                                        error!("[EventRouter] Handler error: {:?}", e);
                                    }
                                }
                            }
                        }
                    },
                    // Nhận tín hiệu shutdown
                    _ = shutdown_rx.recv() => {
                        info!("[EventRouter] Received shutdown signal, stopping worker");
                        break;
                    }
                }
            }
            
            info!("[EventRouter] Event processing worker stopped");
        });
        
        // Lưu worker task vào task manager
        {
            let mut task_manager = self.task_manager.write().await;
            task_manager.add_task(worker_handle);
        }
        
        // Khởi động task dọn dẹp định kỳ
        let _task_manager_clone = self.task_manager.clone();
        let cleanup_interval = self.config.cleanup_interval_ms;
        
        // Tạo một kênh shutdown riêng cho cleanup task
        let (cleanup_shutdown_tx, mut cleanup_shutdown_rx) = mpsc::channel::<()>(1);
        
        // Thêm cleanup_shutdown_tx vào task manager để có thể gửi signal khi cần shutdown
        {
            let mut task_manager = self.task_manager.write().await;
            task_manager.set_shutdown_sender(cleanup_shutdown_tx);
        }
        
        let cleanup_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(cleanup_interval));
            
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // Implement resource cleaning if needed
                        debug!("[EventRouter] Running periodic cleanup");
                        
                        // Additional cleanup logic can be added here
                    }
                    // Check shutdown signal
                    _ = cleanup_shutdown_rx.recv() => {
                        debug!("[EventRouter] Cleanup task received shutdown signal");
                        break;
                    }
                }
            }
            
            debug!("[EventRouter] Cleanup task stopped");
        });
        
        // Lưu cleanup task vào task manager
        {
            let mut task_manager = self.task_manager.write().await;
            task_manager.add_task(cleanup_handle);
        }
        
        info!("[EventRouter] Started successfully");
        Ok(())
    }
    
    /// Dừng router
    pub async fn stop(&self) -> Result<(), NetworkError> {
        // Lấy lock cho is_running
        let mut is_running_guard = self.is_running.write().await;
        
        if !*is_running_guard {
            return Err(NetworkError::EventError("EventRouter is not running".to_string()));
        }
        
        *is_running_guard = false;
        
        // Cleanup tất cả các task
        let mut task_manager = self.task_manager.write().await;
        task_manager.cleanup().await;
        
        info!("[EventRouter] Stopped successfully");
        Ok(())
    }
}

impl Default for EventRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for EventRouter {
    fn drop(&mut self) {
        // Sử dụng tokio runtime hiện tại nếu có
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            rt.block_on(async {
                let mut task_manager = self.task_manager.write().await;
                // Gọi cleanup() để dọn dẹp tài nguyên
                let _ = task_manager.cleanup().await;
                info!("[EventRouter] Dropped, all resources have been cleaned up");
            });
        } else {
            warn!("[EventRouter] No tokio runtime available during drop, some resources may leak");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    
    // Mock EventHandler để kiểm thử
    #[derive(Debug)]
    struct MockEventHandler {
        pub event_received: Arc<AtomicBool>,
        pub handle_count: Arc<AtomicUsize>,
    }
    
    #[async_trait]
    impl EventHandler for MockEventHandler {
        async fn handle_event(&self, event: Event) -> Result<(), NetworkError> {
            self.event_received.store(true, Ordering::SeqCst);
            self.handle_count.fetch_add(1, Ordering::SeqCst);
            
            // Simulate processing time
            tokio::time::sleep(Duration::from_millis(10)).await;
            
            debug!("[MockEventHandler] Handled event: {:?}", event.event_type);
            Ok(())
        }
    }
    
    #[tokio::test]
    async fn test_register_and_emit_event() -> Result<(), Box<dyn std::error::Error>> {
        // Khởi tạo router
        let router = EventRouter::new();
        
        // Khởi tạo mock handler
        let event_received = Arc::new(AtomicBool::new(false));
        let handle_count = Arc::new(AtomicUsize::new(0));
        let handler = Arc::new(MockEventHandler {
            event_received: event_received.clone(),
            handle_count: handle_count.clone(),
        });
        
        // Đăng ký handler
        let event_type = EventType::NodeConnected;
        router.register_handler(event_type.clone(), handler).await?;
        
        // Khởi động router
        router.start().await?;
        
        // Emit sự kiện
        let event = Event {
            event_type: event_type.clone(),
            data: "Test Data".to_string(),
            timestamp: 0,
        };
        router.emit(event).await?;
        
        // Cho thời gian để xử lý
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        // Kiểm tra xem sự kiện đã được xử lý chưa
        assert!(event_received.load(Ordering::SeqCst), "Event should have been received");
        assert_eq!(handle_count.load(Ordering::SeqCst), 1, "Handler should have been called once");
        
        // Dừng router
        router.stop().await?;
        
        Ok(())
    }
    
    #[tokio::test]
    async fn test_unregister_handler() -> Result<(), Box<dyn std::error::Error>> {
        // Khởi tạo router
        let router = EventRouter::new();
        
        // Khởi tạo mock handler
        let event_received = Arc::new(AtomicBool::new(false));
        let handle_count = Arc::new(AtomicUsize::new(0));
        let handler = Arc::new(MockEventHandler {
            event_received: event_received.clone(),
            handle_count: handle_count.clone(),
        });
        
        // Đăng ký handler
        let event_type = EventType::NodeConnected;
        router.register_handler(event_type.clone(), handler).await?;
        
        // Khởi động router
        router.start().await?;
        
        // Hủy đăng ký handler
        router.unregister_handler(&event_type).await?;
        
        // Emit sự kiện
        let event = Event {
            event_type: event_type.clone(),
            data: "Test Data".to_string(),
            timestamp: 0,
        };
        router.emit(event).await?;
        
        // Cho thời gian để xử lý
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        // Sự kiện không nên được xử lý vì đã hủy đăng ký handler
        assert!(!event_received.load(Ordering::SeqCst), "Event should not have been received");
        assert_eq!(handle_count.load(Ordering::SeqCst), 0, "Handler should not have been called");
        
        // Dừng router
        router.stop().await?;
        
        Ok(())
    }
    
    #[tokio::test]
    async fn test_multiple_handlers() -> Result<(), Box<dyn std::error::Error>> {
        // Khởi tạo router
        let router = EventRouter::new();
        
        // Khởi tạo mock handlers
        let event_received1 = Arc::new(AtomicBool::new(false));
        let handle_count1 = Arc::new(AtomicUsize::new(0));
        let handler1 = Arc::new(MockEventHandler {
            event_received: event_received1.clone(),
            handle_count: handle_count1.clone(),
        });
        
        let event_received2 = Arc::new(AtomicBool::new(false));
        let handle_count2 = Arc::new(AtomicUsize::new(0));
        let handler2 = Arc::new(MockEventHandler {
            event_received: event_received2.clone(),
            handle_count: handle_count2.clone(),
        });
        
        // Đăng ký handlers
        let event_type = EventType::NodeConnected;
        router.register_handler(event_type.clone(), handler1).await?;
        router.register_handler(event_type.clone(), handler2).await?;
        
        // Khởi động router
        router.start().await?;
        
        // Emit sự kiện
        let event = Event {
            event_type: event_type.clone(),
            data: "Test Data".to_string(),
            timestamp: 0,
        };
        router.emit(event).await?;
        
        // Cho thời gian để xử lý
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        // Cả hai handler đều nên được gọi
        assert!(event_received1.load(Ordering::SeqCst), "First handler should have received the event");
        assert!(event_received2.load(Ordering::SeqCst), "Second handler should have received the event");
        assert_eq!(handle_count1.load(Ordering::SeqCst), 1, "First handler should have been called once");
        assert_eq!(handle_count2.load(Ordering::SeqCst), 1, "Second handler should have been called once");
        
        // Dừng router
        router.stop().await?;
        
        Ok(())
    }
}

// Mở rộng NetworkError với các variant cần thiết cho event_router
// Tất cả các phương thức đã được triển khai trong errors.rs nên không cần lặp lại ở đây 