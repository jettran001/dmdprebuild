//! Module giám sát kết nối blockchain
//! 
//! Module này cung cấp các công cụ để giám sát và xử lý kết nối đến các blockchain khác nhau:
//! - Phát hiện mất kết nối
//! - Tự động kết nối lại khi mất kết nối
//! - Thông báo khi kết nối được khôi phục
//! - Thu thập metrics về tình trạng kết nối
//! - Hỗ trợ fallback giữa các RPC endpoints
//! 
//! Module này giúp giải quyết lỗi "Không xử lý lỗi đúng cách khi kết nối blockchain bị ngắt"

use std::sync::Arc;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, broadcast};
use tokio::time;
use tracing::{info, warn, error, debug};
use async_trait::async_trait;
use serde::{Serialize, Deserialize};

use super::blockchain::{BlockchainProvider, BlockchainType};
use super::chain::ChainId;
use super::error::DefiError;
use super::constants::BLOCKCHAIN_TIMEOUT;

/// Trạng thái kết nối blockchain
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionStatus {
    /// Đang kết nối
    Connected,
    /// Đã mất kết nối, đang thử kết nối lại
    Reconnecting,
    /// Mất kết nối hoàn toàn sau nhiều lần thử
    Disconnected,
    /// Không xác định
    Unknown,
}

/// Sự kiện kết nối blockchain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionEvent {
    /// Chain ID
    pub chain_id: ChainId,
    /// Loại blockchain
    pub blockchain_type: BlockchainType,
    /// Trạng thái kết nối mới
    pub status: ConnectionStatus,
    /// Thời gian sự kiện
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Thông tin thêm
    pub message: Option<String>,
}

/// Handler cho sự kiện kết nối blockchain
#[async_trait]
pub trait ConnectionEventHandler: Send + Sync {
    /// Xử lý sự kiện kết nối
    async fn handle_event(&self, event: &ConnectionEvent);
}

/// Cấu hình monitor kết nối
#[derive(Debug, Clone)]
pub struct ConnectionMonitorConfig {
    /// Khoảng thời gian kiểm tra kết nối (ms)
    pub check_interval_ms: u64,
    /// Số lần thử kết nối lại tối đa
    pub max_reconnect_attempts: u32,
    /// Thời gian chờ giữa các lần thử kết nối lại (ms)
    pub reconnect_delay_base_ms: u64,
    /// Kích thước buffer cho kênh sự kiện
    pub event_channel_size: usize,
}

impl Default for ConnectionMonitorConfig {
    fn default() -> Self {
        Self {
            check_interval_ms: 15000, // 15 giây
            max_reconnect_attempts: 10,
            reconnect_delay_base_ms: 500,
            event_channel_size: 100,
        }
    }
}

/// Monitor kết nối blockchain
pub struct ConnectionMonitor {
    /// Cấu hình
    config: ConnectionMonitorConfig,
    /// Trạng thái kết nối của các blockchain
    status: Arc<RwLock<HashMap<ChainId, ConnectionStatus>>>,
    /// Kênh phát sự kiện kết nối
    event_sender: broadcast::Sender<ConnectionEvent>,
    /// Danh sách các event handler
    handlers: Arc<RwLock<Vec<Box<dyn ConnectionEventHandler>>>>,
    /// RPC URLs thay thế
    fallback_urls: Arc<RwLock<HashMap<ChainId, Vec<String>>>>,
}

impl ConnectionMonitor {
    /// Tạo monitor mới
    pub fn new(config: ConnectionMonitorConfig) -> Self {
        let (sender, _) = broadcast::channel(config.event_channel_size);
        Self {
            config,
            status: Arc::new(RwLock::new(HashMap::new())),
            event_sender: sender,
            handlers: Arc::new(RwLock::new(Vec::new())),
            fallback_urls: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Khởi động monitor
    pub fn start(self: &Arc<Self>, providers: Vec<Box<dyn BlockchainProvider>>) -> broadcast::Receiver<ConnectionEvent> {
        let monitor = self.clone();
        
        // Clone providers vào một HashMap
        let provider_map = Arc::new(RwLock::new(HashMap::new()));
        for provider in providers {
            let chain_id = provider.chain_id();
            tokio::spawn(async move {
                let mut map = provider_map.write().await;
                map.insert(chain_id, provider);
            });
        }
        
        // Khởi động task giám sát
        tokio::spawn(async move {
            let check_interval = Duration::from_millis(monitor.config.check_interval_ms);
            loop {
                let provider_map_read = provider_map.read().await;
                
                for (chain_id, provider) in provider_map_read.iter() {
                    let chain_id = *chain_id;
                    let blockchain_type = provider.blockchain_type();
                    
                    // Kiểm tra trạng thái kết nối
                    match provider.is_connected().await {
                        Ok(is_connected) => {
                            let mut status_changed = false;
                            let mut new_status = ConnectionStatus::Unknown;
                            
                            {
                                let mut status_map = monitor.status.write().await;
                                let current_status = status_map.get(&chain_id).copied().unwrap_or(ConnectionStatus::Unknown);
                                
                                if is_connected && current_status != ConnectionStatus::Connected {
                                    // Đã kết nối thành công
                                    status_map.insert(chain_id, ConnectionStatus::Connected);
                                    status_changed = true;
                                    new_status = ConnectionStatus::Connected;
                                    
                                    info!("Kết nối blockchain ({:?}) khôi phục", chain_id);
                                } else if !is_connected && current_status == ConnectionStatus::Connected {
                                    // Mất kết nối
                                    status_map.insert(chain_id, ConnectionStatus::Reconnecting);
                                    status_changed = true;
                                    new_status = ConnectionStatus::Reconnecting;
                                    
                                    warn!("Mất kết nối đến blockchain ({:?}), đang thử kết nối lại...", chain_id);
                                    
                                    // Khởi động task kết nối lại
                                    let monitor_clone = monitor.clone();
                                    let provider_clone = provider_map.clone();
                                    tokio::spawn(async move {
                                        monitor_clone.handle_reconnect(chain_id, blockchain_type, provider_clone).await;
                                    });
                                }
                            }
                            
                            // Gửi sự kiện nếu trạng thái thay đổi
                            if status_changed {
                                let event = ConnectionEvent {
                                    chain_id,
                                    blockchain_type,
                                    status: new_status,
                                    timestamp: chrono::Utc::now(),
                                    message: Some(format!("Trạng thái kết nối blockchain thay đổi: {:?}", new_status)),
                                };
                                
                                monitor.emit_event(event).await;
                            }
                        },
                        Err(e) => {
                            // Lỗi khi kiểm tra kết nối
                            warn!("Lỗi khi kiểm tra kết nối blockchain ({:?}): {}", chain_id, e);
                            
                            // Đánh dấu là đang thử kết nối lại
                            let mut status_map = monitor.status.write().await;
                            if status_map.get(&chain_id).copied() != Some(ConnectionStatus::Reconnecting) {
                                status_map.insert(chain_id, ConnectionStatus::Reconnecting);
                                
                                let event = ConnectionEvent {
                                    chain_id,
                                    blockchain_type,
                                    status: ConnectionStatus::Reconnecting,
                                    timestamp: chrono::Utc::now(),
                                    message: Some(format!("Lỗi kết nối: {}", e)),
                                };
                                
                                monitor.emit_event(event).await;
                                
                                // Khởi động task kết nối lại
                                let monitor_clone = monitor.clone();
                                let provider_clone = provider_map.clone();
                                tokio::spawn(async move {
                                    monitor_clone.handle_reconnect(chain_id, blockchain_type, provider_clone).await;
                                });
                            }
                        }
                    }
                }
                
                // Chờ đến lần kiểm tra tiếp theo
                time::sleep(check_interval).await;
            }
        });
        
        self.event_sender.subscribe()
    }
    
    /// Xử lý kết nối lại khi mất kết nối
    async fn handle_reconnect(
        &self, 
        chain_id: ChainId, 
        blockchain_type: BlockchainType,
        provider_map: Arc<RwLock<HashMap<ChainId, Box<dyn BlockchainProvider>>>>
    ) {
        let max_attempts = self.config.max_reconnect_attempts;
        let base_delay = self.config.reconnect_delay_base_ms;
        let mut attempt = 0;
        
        // Lấy danh sách URL thay thế
        let fallback_urls = {
            let urls = self.fallback_urls.read().await;
            urls.get(&chain_id).cloned().unwrap_or_else(|| vec![])
        };
        
        // Lấy URL hiện tại từ provider để thêm vào đầu danh sách
        let mut all_urls = Vec::new();
        {
            let providers = provider_map.read().await;
            if let Some(provider) = providers.get(&chain_id) {
                if let Ok(current_url) = provider.get_rpc_url().await {
                    all_urls.push(current_url);
                }
            }
        }
        
        // Thêm các URL thay thế
        all_urls.extend(fallback_urls);
        
        // Nếu không có URL nào, không thể kết nối lại
        if all_urls.is_empty() {
            error!("Không có URL nào cho blockchain {:?}, không thể kết nối lại", chain_id);
            
            // Gửi sự kiện mất kết nối hoàn toàn
            let event = ConnectionEvent {
                chain_id,
                blockchain_type,
                status: ConnectionStatus::Disconnected,
                timestamp: chrono::Utc::now(),
                message: Some("Không có URL nào cho blockchain này".to_string()),
            };
            
            self.emit_event(event).await;
            return;
        }
        
        // Vòng lặp thử kết nối lại
        let mut current_url_index = 0;
        loop {
            attempt += 1;
            if attempt > max_attempts {
                break;
            }
            
            // Tính thời gian chờ với exponential backoff
            // Tránh chờ quá lâu, tối đa 30 giây
            let delay = std::cmp::min(
                base_delay * 2u64.pow(attempt as u32 - 1),
                30000 // 30 giây
            );
            
            // Chờ trước khi thử lại
            time::sleep(Duration::from_millis(delay)).await;
            
            // Lấy URL hiện tại
            let current_url = &all_urls[current_url_index];
            info!("Thử kết nối lại đến blockchain {:?} lần {}/{} với URL: {}", 
                 chain_id, attempt, max_attempts, current_url);
            
            // Triển khai logic tạo provider với URL thay thế
            let reconnect_result = {
                let mut providers = provider_map.write().await;
                if let Some(provider) = providers.get_mut(&chain_id) {
                    // Tạo cấu hình mới với URL thay thế
                    if let Ok(mut config) = provider.get_config().await {
                        // Cập nhật URL trong cấu hình
                        config.rpc_url = current_url.clone();
                        
                        // Tạo provider mới với URL thay thế
                        match create_provider_with_config(blockchain_type, config).await {
                            Ok(new_provider) => {
                                // Thay thế provider cũ bằng provider mới
                                *provider = new_provider;
                                
                                // Kiểm tra kết nối với provider mới
                                match provider.is_connected().await {
                                    Ok(connected) => Ok(connected),
                                    Err(e) => {
                                        warn!("Không thể kết nối đến URL thay thế {}: {}", current_url, e);
                                        Ok(false)
                                    }
                                }
                            },
                            Err(e) => {
                                error!("Không thể tạo provider mới với URL {}: {}", current_url, e);
                                Ok(false)
                            }
                        }
                    } else {
                        error!("Không thể lấy cấu hình từ provider");
                        Ok(false)
                    }
                } else {
                    error!("Không tìm thấy provider cho blockchain {:?}", chain_id);
                    Ok(false)
                }
            };
            
            match reconnect_result {
                Ok(true) => {
                    // Kết nối thành công
                    info!("Kết nối lại thành công đến blockchain {:?} sau {} lần thử", chain_id, attempt);
                    
                    // Cập nhật trạng thái
                    {
                        let mut status_map = self.status.write().await;
                        status_map.insert(chain_id, ConnectionStatus::Connected);
                    }
                    
                    // Gửi sự kiện
                    let event = ConnectionEvent {
                        chain_id,
                        blockchain_type,
                        status: ConnectionStatus::Connected,
                        timestamp: chrono::Utc::now(),
                        message: Some(format!("Kết nối lại thành công sau {} lần thử", attempt)),
                    };
                    
                    self.emit_event(event).await;
                    return;
                },
                _ => {
                    // Kết nối thất bại, thử URL tiếp theo
                    if current_url_index < all_urls.len() - 1 {
                        current_url_index += 1;
                    }
                }
            }
        }
        
        // Nếu đến đây, đã thử hết các lần và không thành công
        {
            let mut status_map = self.status.write().await;
            status_map.insert(chain_id, ConnectionStatus::Disconnected);
        }
        
        // Gửi sự kiện mất kết nối hoàn toàn
        let event = ConnectionEvent {
            chain_id,
            blockchain_type,
            status: ConnectionStatus::Disconnected,
            timestamp: chrono::Utc::now(),
            message: Some(format!("Không thể kết nối lại sau {} lần thử", max_attempts)),
        };
        
        self.emit_event(event).await;
        error!("Mất kết nối hoàn toàn đến blockchain {:?} sau {} lần thử", chain_id, max_attempts);
    }
    
    /// Thêm event handler
    pub async fn add_handler(&self, handler: Box<dyn ConnectionEventHandler>) {
        let mut handlers = self.handlers.write().await;
        handlers.push(handler);
    }
    
    /// Thêm URL thay thế cho một chain
    pub async fn add_fallback_url(&self, chain_id: ChainId, url: String) {
        let mut urls = self.fallback_urls.write().await;
        urls.entry(chain_id)
            .or_insert_with(Vec::new)
            .push(url);
    }
    
    /// Thiết lập danh sách URL thay thế cho một chain
    pub async fn set_fallback_urls(&self, chain_id: ChainId, urls: Vec<String>) {
        let mut fallback_urls = self.fallback_urls.write().await;
        fallback_urls.insert(chain_id, urls);
    }
    
    /// Phát sự kiện kết nối
    async fn emit_event(&self, event: ConnectionEvent) {
        // Gửi qua broadcast channel
        let _ = self.event_sender.send(event.clone());
        
        // Gửi đến các handler
        let handlers = self.handlers.read().await;
        for handler in handlers.iter() {
            handler.handle_event(&event).await;
        }
    }
    
    /// Lấy trạng thái kết nối hiện tại của một blockchain
    pub async fn get_connection_status(&self, chain_id: ChainId) -> ConnectionStatus {
        let status_map = self.status.read().await;
        status_map.get(&chain_id).copied().unwrap_or(ConnectionStatus::Unknown)
    }
}

/// Handler đơn giản để ghi log các sự kiện kết nối
pub struct LoggingConnectionHandler;

#[async_trait]
impl ConnectionEventHandler for LoggingConnectionHandler {
    async fn handle_event(&self, event: &ConnectionEvent) {
        match event.status {
            ConnectionStatus::Connected => {
                info!("Blockchain {:?} đã kết nối. Thời gian: {}", 
                      event.chain_id, event.timestamp);
            },
            ConnectionStatus::Reconnecting => {
                warn!("Blockchain {:?} mất kết nối, đang thử kết nối lại. Thời gian: {}, Chi tiết: {}", 
                      event.chain_id, event.timestamp, event.message.as_deref().unwrap_or(""));
            },
            ConnectionStatus::Disconnected => {
                error!("Blockchain {:?} mất kết nối hoàn toàn. Thời gian: {}, Chi tiết: {}", 
                       event.chain_id, event.timestamp, event.message.as_deref().unwrap_or(""));
            },
            ConnectionStatus::Unknown => {
                warn!("Trạng thái kết nối blockchain {:?} không xác định. Thời gian: {}", 
                      event.chain_id, event.timestamp);
            },
        }
    }
}

/// Tạo provider mới với URL thay thế
async fn create_provider_with_config(
    blockchain_type: BlockchainType,
    config: super::blockchain::BlockchainConfig,
) -> Result<Box<dyn BlockchainProvider>, DefiError> {
    // Tạo provider mới dựa vào loại blockchain
    match blockchain_type {
        BlockchainType::EVM => {
            let provider = super::blockchain::EvmBlockchainProvider::new(config);
            Ok(Box::new(provider))
        },
        BlockchainType::Diamond => {
            let provider = super::blockchain::non_evm::diamond::DiamondBlockchainProvider::new(config)
                .await
                .map_err(|e| DefiError::BlockchainConnection {
                    message: format!("Không thể tạo Diamond provider: {}", e),
                    error_type: super::error::BlockchainConnectionErrorType::ProviderError,
                })?;
            Ok(Box::new(provider))
        },
        BlockchainType::Tron => {
            let provider = super::blockchain::non_evm::tron::TronProvider::new(config)
                .await
                .map_err(|e| DefiError::BlockchainConnection {
                    message: format!("Không thể tạo Tron provider: {}", e),
                    error_type: super::error::BlockchainConnectionErrorType::ProviderError,
                })?;
            Ok(Box::new(provider))
        },
        BlockchainType::Solana => {
            let provider = super::blockchain::non_evm::solana::SolanaProvider::new(config)
                .await
                .map_err(|e| DefiError::BlockchainConnection {
                    message: format!("Không thể tạo Solana provider: {}", e),
                    error_type: super::error::BlockchainConnectionErrorType::ProviderError,
                })?;
            Ok(Box::new(provider))
        },
        // Thêm các loại blockchain khác tại đây
        _ => Err(DefiError::BlockchainConnection {
            message: format!("Loại blockchain không được hỗ trợ: {:?}", blockchain_type),
            error_type: super::error::BlockchainConnectionErrorType::UnsupportedBlockchain,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    struct MockBlockchainProvider {
        chain_id: ChainId,
        connected: Arc<RwLock<bool>>,
    }
    
    impl MockBlockchainProvider {
        fn new(chain_id: ChainId) -> Self {
            Self {
                chain_id,
                connected: Arc::new(RwLock::new(true)),
            }
        }
        
        async fn set_connected(&self, connected: bool) {
            let mut c = self.connected.write().await;
            *c = connected;
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
            if *connected {
                Ok(100)
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
    
    #[tokio::test]
    async fn test_connection_monitor() {
        // Tạo cấu hình để test nhanh
        let config = ConnectionMonitorConfig {
            check_interval_ms: 100, // 100ms
            max_reconnect_attempts: 3,
            reconnect_delay_base_ms: 10,
            event_channel_size: 10,
        };
        
        // Tạo mock provider
        let provider = Box::new(MockBlockchainProvider::new(ChainId::EthereumMainnet));
        let provider_clone = provider.clone();
        
        // Tạo monitor
        let monitor = Arc::new(ConnectionMonitor::new(config));
        let mut receiver = monitor.start(vec![provider]);
        
        // Giả lập mất kết nối
        tokio::time::sleep(Duration::from_millis(200)).await;
        if let Ok(provider) = provider_clone.downcast_ref::<MockBlockchainProvider>() {
            provider.set_connected(false).await;
        }
        
        // Đợi và kiểm tra sự kiện
        let mut received_reconnecting = false;
        let timeout = time::timeout(Duration::from_millis(500), async {
            while let Ok(event) = receiver.recv().await {
                if event.status == ConnectionStatus::Reconnecting {
                    received_reconnecting = true;
                    break;
                }
            }
        });
        
        // Nếu nhận được sự kiện, test pass
        assert!(received_reconnecting, "Không nhận được sự kiện mất kết nối");
    }
} 