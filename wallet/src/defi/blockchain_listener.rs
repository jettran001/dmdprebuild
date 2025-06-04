//! Module quản lý việc lắng nghe sự kiện blockchain
//! 
//! Module này xây dựng trên nền tảng của connection_monitor.rs 
//! và cung cấp các chức năng nâng cao:
//! - Lắng nghe các sự kiện blockchain
//! - Quản lý các kết nối và tự động khôi phục
//! - Ghi nhật ký các sự kiện quan trọng
//! - Thông báo cho người dùng về trạng thái kết nối
//! 
//! Module này giúp giải quyết phần còn lại của lỗi "Không xử lý lỗi đúng cách khi kết nối blockchain bị ngắt"

use std::sync::Arc;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, mpsc, broadcast, watch};
use tokio::time;
use tracing::{info, warn, error, debug};
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use futures::stream::StreamExt;

use super::blockchain::{BlockchainProvider, BlockchainType};
use super::chain::ChainId;
use super::error::{DefiError, BlockchainConnectionErrorType};
use super::connection_monitor::{ConnectionMonitor, ConnectionStatus, ConnectionEvent, ConnectionEventHandler};

/// Kiểu của sự kiện blockchain
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockchainEventType {
    /// Có block mới
    NewBlock,
    /// Có giao dịch mới
    NewTransaction,
    /// Có log mới
    NewLog,
    /// Kết nối thay đổi
    ConnectionChange,
    /// Lỗi
    Error,
}

/// Sự kiện blockchain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainEvent {
    /// Loại sự kiện
    pub event_type: BlockchainEventType,
    /// Chain ID
    pub chain_id: ChainId,
    /// Thời gian phát sinh sự kiện
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Dữ liệu sự kiện
    pub data: serde_json::Value,
}

/// Listener quản lý sự kiện blockchain
pub struct BlockchainListenerManager {
    /// Danh sách các các chain đang lắng nghe
    listeners: Arc<RwLock<HashMap<ChainId, BlockchainListener>>>,
    /// Kênh phát các sự kiện blockchain
    event_sender: broadcast::Sender<BlockchainEvent>,
    /// Connection monitor
    connection_monitor: Arc<ConnectionMonitor>,
    /// Trạng thái toàn cục
    global_status: watch::Sender<bool>,
}

/// Listener cho một blockchain cụ thể
struct BlockchainListener {
    /// Chain ID
    chain_id: ChainId,
    /// Loại blockchain
    blockchain_type: BlockchainType,
    /// Provider
    provider: Box<dyn BlockchainProvider>,
    /// Kênh gửi sự kiện
    event_sender: broadcast::Sender<BlockchainEvent>,
    /// Trạng thái lắng nghe (đang chạy hay không)
    running: bool,
    /// Trạng thái kết nối
    connection_status: ConnectionStatus,
}

impl BlockchainListenerManager {
    /// Tạo manager mới
    pub fn new(connection_monitor: Arc<ConnectionMonitor>) -> Self {
        let (event_sender, _) = broadcast::channel(100);
        let (global_status, _) = watch::channel(true);
        
        Self {
            listeners: Arc::new(RwLock::new(HashMap::new())),
            event_sender,
            connection_monitor,
            global_status,
        }
    }
    
    /// Thêm listener cho một blockchain
    pub async fn add_listener(&self, provider: Box<dyn BlockchainProvider>) -> Result<(), DefiError> {
        let chain_id = provider.chain_id();
        let blockchain_type = provider.blockchain_type();
        
        // Kiểm tra kết nối
        if !provider.is_connected().await? {
            return Err(DefiError::simple_connection_error(
                format!("Không thể kết nối đến blockchain {:?}", chain_id)
            ));
        }
        
        let listener = BlockchainListener {
            chain_id,
            blockchain_type,
            provider,
            event_sender: self.event_sender.clone(),
            running: false,
            connection_status: ConnectionStatus::Connected,
        };
        
        let mut listeners = self.listeners.write().await;
        listeners.insert(chain_id, listener);
        
        info!("Đã thêm listener cho blockchain {:?}", chain_id);
        Ok(())
    }
    
    /// Khởi động tất cả các listener
    pub async fn start_all_listeners(&self) -> Vec<ChainId> {
        let mut started_chains = Vec::new();
        let mut listeners = self.listeners.write().await;
        
        for (chain_id, listener) in listeners.iter_mut() {
            if !listener.running {
                // Khởi động task lắng nghe
                let chain_id_copy = *chain_id;
                let provider = Box::new(listener.provider.clone());
                let event_sender = self.event_sender.clone();
                let global_status = self.global_status.subscribe();
                
                tokio::spawn(async move {
                    Self::run_listener_task(chain_id_copy, provider, event_sender, global_status).await;
                });
                
                listener.running = true;
                started_chains.push(*chain_id);
                info!("Đã khởi động listener cho blockchain {:?}", chain_id);
            }
        }
        
        started_chains
    }
    
    /// Task lắng nghe blockchain
    async fn run_listener_task(
        chain_id: ChainId,
        provider: Box<dyn BlockchainProvider>,
        event_sender: broadcast::Sender<BlockchainEvent>,
        mut global_status: watch::Receiver<bool>,
    ) {
        info!("Bắt đầu lắng nghe blockchain {:?}", chain_id);
        
        let mut current_block: Option<u64> = None;
        let mut error_count = 0;
        let max_errors = 10;
        
        while *global_status.borrow_and_update() {
            // Kiểm tra kết nối
            match provider.is_connected().await {
                Ok(connected) => {
                    if connected {
                        // Lấy block mới nhất
                        match provider.get_block_number().await {
                            Ok(block_number) => {
                                // Reset lỗi khi thành công
                                error_count = 0;
                                
                                // Nếu có block mới
                                if let Some(last_block) = current_block {
                                    if block_number > last_block {
                                        // Phát sự kiện block mới
                                        let event = BlockchainEvent {
                                            event_type: BlockchainEventType::NewBlock,
                                            chain_id,
                                            timestamp: chrono::Utc::now(),
                                            data: serde_json::json!({
                                                "block_number": block_number,
                                                "previous_block": last_block
                                            }),
                                        };
                                        
                                        let _ = event_sender.send(event);
                                        debug!("Blockchain {:?}: Block mới #{}", chain_id, block_number);
                                    }
                                }
                                
                                // Cập nhật block hiện tại
                                current_block = Some(block_number);
                            },
                            Err(e) => {
                                error_count += 1;
                                error!("Lỗi khi lấy số block từ blockchain {:?}: {}", chain_id, e);
                                
                                // Exponential backoff cho lỗi liên tiếp
                                if error_count <= max_errors {
                                    let wait_time = std::cmp::min(
                                        Duration::from_secs(2u64.pow(error_count as u32)), 
                                        Duration::from_secs(60)
                                    );
                                    
                                    warn!(
                                        "Lỗi #{}/{} khi kết nối blockchain {:?}, thử lại sau {:?} (exponential backoff)",
                                        error_count, max_errors, chain_id, wait_time
                                    );
                                    
                                    time::sleep(wait_time).await;
                                } else {
                                    error!(
                                        "Vượt quá số lần thử kết nối tới blockchain {:?} ({}), dừng lắng nghe",
                                        chain_id, max_errors
                                    );
                                    
                                    // Phát sự kiện lỗi
                                    let event = BlockchainEvent {
                                        event_type: BlockchainEventType::Error,
                                        chain_id,
                                        timestamp: chrono::Utc::now(),
                                        data: serde_json::json!({
                                            "error": "connection_failed",
                                            "message": format!("Không thể kết nối đến blockchain sau {} lần thử", max_errors)
                                        }),
                                    };
                                    
                                    let _ = event_sender.send(event);
                                    break;
                                }
                            }
                        }
                    } else {
                        // Mất kết nối, báo lỗi và thử lại
                        warn!("Mất kết nối đến blockchain {:?}", chain_id);
                        
                        // Phát sự kiện mất kết nối
                        let event = BlockchainEvent {
                            event_type: BlockchainEventType::ConnectionChange,
                            chain_id,
                            timestamp: chrono::Utc::now(),
                            data: serde_json::json!({
                                "status": "disconnected"
                            }),
                        };
                        
                        let _ = event_sender.send(event);
                        
                        // Đợi 5 giây trước khi thử lại
                        time::sleep(Duration::from_secs(5)).await;
                    }
                },
                Err(e) => {
                    error!("Lỗi khi kiểm tra kết nối đến blockchain {:?}: {}", chain_id, e);
                    
                    // Phát sự kiện lỗi
                    let event = BlockchainEvent {
                        event_type: BlockchainEventType::Error,
                        chain_id,
                        timestamp: chrono::Utc::now(),
                        data: serde_json::json!({
                            "error": "Lỗi kiểm tra kết nối",
                            "details": e.to_string()
                        }),
                    };
                    
                    let _ = event_sender.send(event);
                    
                    // Đợi 5 giây trước khi thử lại
                    time::sleep(Duration::from_secs(5)).await;
                }
            }
            
            // Đợi 1 giây trước khi tiếp tục
            time::sleep(Duration::from_secs(1)).await;
        }
        
        info!("Dừng lắng nghe blockchain {:?}", chain_id);
    }
    
    /// Dừng tất cả các listener
    pub async fn stop_all_listeners(&self) {
        // Thông báo cho tất cả các task dừng lại
        self.global_status.send(false).unwrap_or_default();
        
        // Đánh dấu tất cả các listener là không chạy
        let mut listeners = self.listeners.write().await;
        for (chain_id, listener) in listeners.iter_mut() {
            if listener.running {
                listener.running = false;
                info!("Đã dừng listener cho blockchain {:?}", chain_id);
            }
        }
    }
    
    /// Dừng một listener cụ thể
    pub async fn stop_listener(&self, chain_id: ChainId) -> bool {
        let mut listeners = self.listeners.write().await;
        if let Some(listener) = listeners.get_mut(&chain_id) {
            if listener.running {
                listener.running = false;
                info!("Đã dừng listener cho blockchain {:?}", chain_id);
                return true;
            }
        }
        
        false
    }
    
    /// Lấy subscriber cho các sự kiện blockchain
    pub fn subscribe(&self) -> broadcast::Receiver<BlockchainEvent> {
        self.event_sender.subscribe()
    }
    
    /// Kiểm tra nếu đang lắng nghe một blockchain
    pub async fn is_listening(&self, chain_id: ChainId) -> bool {
        let listeners = self.listeners.read().await;
        if let Some(listener) = listeners.get(&chain_id) {
            listener.running
        } else {
            false
        }
    }
}

/// Handler xử lý sự kiện kết nối
pub struct BlockchainConnectionHandler {
    /// Listener manager
    listener_manager: Arc<BlockchainListenerManager>,
}

#[async_trait]
impl ConnectionEventHandler for BlockchainConnectionHandler {
    async fn handle_event(&self, event: &ConnectionEvent) {
        match event.status {
            ConnectionStatus::Connected => {
                info!("Blockchain {:?} đã kết nối lại, khởi động lại listener", event.chain_id);
                
                // Nếu không đang lắng nghe, khởi động lại listener
                if !self.listener_manager.is_listening(event.chain_id).await {
                    // Triển khai logic khởi động lại listener khi kết nối được khôi phục
                    let listeners = self.listener_manager.listeners.read().await;
                    
                    if let Some(listener) = listeners.get(&event.chain_id) {
                        let chain_id = listener.chain_id;
                        let provider_clone = Box::new(listener.provider.clone());
                        let event_sender = self.listener_manager.event_sender.clone();
                        let global_status = self.listener_manager.global_status.subscribe();
                        
                        // Khởi động lại task listener
                        tokio::spawn(async move {
                            BlockchainListenerManager::run_listener_task(
                                chain_id, 
                                provider_clone, 
                                event_sender, 
                                global_status
                            ).await;
                        });
                        
                        // Cập nhật trạng thái listener
                        if let Some(listener) = self.listener_manager.listeners.write().await.get_mut(&event.chain_id) {
                            listener.running = true;
                        }
                        
                        // Phát sự kiện kết nối được khôi phục
                        let recovery_event = BlockchainEvent {
                            event_type: BlockchainEventType::ConnectionChange,
                            chain_id: event.chain_id,
                            timestamp: chrono::Utc::now(),
                            data: serde_json::json!({
                                "status": "connected",
                                "message": "Kết nối blockchain được khôi phục, listener đã khởi động lại"
                            }),
                        };
                        
                        let _ = self.listener_manager.event_sender.send(recovery_event);
                        info!("Đã khởi động lại listener cho blockchain {:?}", event.chain_id);
                    }
                }
            },
            ConnectionStatus::Disconnected => {
                warn!("Blockchain {:?} mất kết nối hoàn toàn, dừng listener", event.chain_id);
                
                // Dừng listener
                self.listener_manager.stop_listener(event.chain_id).await;
                
                // Phát sự kiện lỗi
                let event = BlockchainEvent {
                    event_type: BlockchainEventType::ConnectionChange,
                    chain_id: event.chain_id,
                    timestamp: chrono::Utc::now(),
                    data: serde_json::json!({
                        "status": "disconnected",
                        "permanent": true,
                        "message": "Mất kết nối hoàn toàn đến blockchain"
                    }),
                };
                
                let _ = self.listener_manager.event_sender.send(event);
            },
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    
    struct MockBlockchainProvider {
        chain_id: ChainId,
        connected: Arc<RwLock<bool>>,
        block_number: Arc<RwLock<u64>>,
    }
    
    impl Clone for MockBlockchainProvider {
        fn clone(&self) -> Self {
            Self {
                chain_id: self.chain_id,
                connected: self.connected.clone(),
                block_number: self.block_number.clone(),
            }
        }
    }
    
    impl MockBlockchainProvider {
        fn new(chain_id: ChainId) -> Self {
            Self {
                chain_id,
                connected: Arc::new(RwLock::new(true)),
                block_number: Arc::new(RwLock::new(100)),
            }
        }
        
        async fn set_connected(&self, connected: bool) {
            let mut c = self.connected.write().await;
            *c = connected;
        }
        
        async fn increment_block(&self) {
            let mut bn = self.block_number.write().await;
            *bn += 1;
        }
    }
    
    #[async_trait]
    impl BlockchainProvider for MockBlockchainProvider {
        fn chain_id(&self) -> ChainId {
            self.chain_id
        }
        
        fn blockchain_type(&self) -> BlockchainType {
            BlockchainType::Evm
        }
        
        async fn is_connected(&self) -> Result<bool, DefiError> {
            let connected = self.connected.read().await;
            Ok(*connected)
        }
        
        async fn get_block_number(&self) -> Result<u64, DefiError> {
            let connected = self.connected.read().await;
            let block_number = self.block_number.read().await;
            
            if *connected {
                Ok(*block_number)
            } else {
                Err(DefiError::ProviderError("Không kết nối được".to_string()))
            }
        }
        
        async fn get_balance(&self, _address: &str) -> Result<ethers::types::U256, DefiError> {
            let connected = self.connected.read().await;
            if *connected {
                Ok(ethers::types::U256::from(1000))
            } else {
                Err(DefiError::ProviderError("Không kết nối được".to_string()))
            }
        }
    }
    
    // Các test sẽ được triển khai
} 