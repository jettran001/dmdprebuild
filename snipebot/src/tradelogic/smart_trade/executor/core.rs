//! Core của Smart Trade Executor
//!
//! Module này triển khai TradeExecutor trait cho smart trading logic.
//! Quản lý state và lifecycle của executor, và điều phối giữa các components.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::Utc;
use tracing::{debug, error, info, warn};
use anyhow::{Result, Context, anyhow};
use uuid;
use async_trait::async_trait;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::MempoolAnalyzer;
use crate::analys::risk_analyzer::RiskAnalyzer;
use crate::types::{TradeParams};
use crate::tradelogic::traits::{
    TradeExecutor, TradeCoordinator, ExecutorType, SharedOpportunity
};
use crate::tradelogic::common::{
    NetworkStatus, NetworkReliabilityManager, RetryStrategy, get_network_manager, CircuitBreakerConfig,
    HealthMonitor, HealthMonitorConfig, get_health_monitor, AlertSeverity, HealthAlert
};

// Module imports
use super::types::{
    TradeResult, 
    TradeStatus, 
    TradeTracker,
    NonceManager
};
use crate::tradelogic::smart_trade::types::SmartTradeConfig;
use crate::tradelogic::smart_trade::analys_client::SmartTradeAnalysisClient;

// Imports from other executor modules
use super::market_monitor::{monitor_loop, process_opportunity, check_opportunities};

/// Executor for smart trading strategies
#[derive(Clone)]
pub struct SmartTradeExecutor {
    /// EVM adapter for each chain
    pub evm_adapters: HashMap<u32, Arc<EvmAdapter>>,
    
    /// Analysis client that provides access to all analysis services
    pub analys_client: Arc<SmartTradeAnalysisClient>,
    
    /// Risk analyzer (legacy - kept for backward compatibility)
    pub risk_analyzer: Arc<RwLock<RiskAnalyzer>>,
    
    /// Mempool analyzer for each chain (legacy - kept for backward compatibility)
    pub mempool_analyzers: HashMap<u32, Arc<MempoolAnalyzer>>,
    
    /// Configuration
    pub config: RwLock<SmartTradeConfig>,
    
    /// Active trades being monitored
    pub active_trades: RwLock<Vec<TradeTracker>>,
    
    /// History of completed trades
    pub trade_history: RwLock<Vec<TradeResult>>,
    
    /// Running state
    pub running: RwLock<bool>,
    
    /// Coordinator for shared state between executors
    pub coordinator: Option<Arc<dyn TradeCoordinator>>,
    
    /// Unique ID for this executor instance
    pub executor_id: String,
    
    /// Subscription ID for coordinator notifications
    pub coordinator_subscription: RwLock<Option<String>>,
    
    /// Network reliability manager
    pub network_manager: Arc<NetworkReliabilityManager>,
    
    /// Health check task handle
    pub health_check_task: RwLock<Option<tokio::task::JoinHandle<()>>>,
    
    /// Health monitor
    pub health_monitor: Arc<HealthMonitor>,
    
    /// Notification manager for sending alerts
    pub notification_manager: Option<Arc<crate::notifications::NotificationManager>>,
    
    /// Trade log database for storing trade history
    pub trade_log_db: Option<Arc<dyn crate::tradelogic::smart_trade::alert::DbOperations>>,
    
    /// Nonce manager for tracking transaction nonces
    pub nonce_manager: RwLock<NonceManager>,
}

impl SmartTradeExecutor {
    /// Tạo một SmartTradeExecutor mới
    pub async fn new(
        evm_adapters: HashMap<u32, Arc<EvmAdapter>>,
        config: SmartTradeConfig,
    ) -> Result<Self> {
        // Khởi tạo risk analyzer
        let risk_analyzer = Arc::new(RwLock::new(RiskAnalyzer::new()));
        
        // Khởi tạo mempool analyzers
        let mut mempool_analyzers = HashMap::new();
        for (chain_id, _) in &evm_adapters {
            let analyzer = MempoolAnalyzer::new(*chain_id);
            mempool_analyzers.insert(*chain_id, Arc::new(analyzer));
        }
        
        // Khởi tạo analysis client với chain adapters
        let analys_client = SmartTradeAnalysisClient::new(evm_adapters.clone());
        
        // Lấy network manager global
        let network_manager = get_network_manager();
        
        // Lấy health monitor global
        let health_monitor = get_health_monitor();
        
        // Thiết lập cấu hình circuit breaker cho từng chain
        for chain_id in evm_adapters.keys() {
            let service_id = format!("blockchain_rpc_{}", chain_id);
            
            // Cấu hình circuit breaker tùy chỉnh cho chain
            let circuit_config = match chain_id {
                // Ethereum mainnet - có nhiều node, ít lỗi hơn
                1 => CircuitBreakerConfig {
                    failure_threshold: 7,
                    reset_timeout_seconds: 60,
                    use_half_open: true,
                    max_retries: config.max_retries as usize,
                    success_threshold: 2,
                },
                // Các testnet và chain phổ biến khác
                5 | 56 | 137 | 42161 | 10 => CircuitBreakerConfig {
                    failure_threshold: 5,
                    reset_timeout_seconds: 45,
                    use_half_open: true,
                    max_retries: config.max_retries as usize,
                    success_threshold: 2,
                },
                // Các chain ít phổ biến hơn
                _ => CircuitBreakerConfig {
                    failure_threshold: 3,
                    reset_timeout_seconds: 30,
                    use_half_open: true,
                    max_retries: config.max_retries as usize,
                    success_threshold: 2,
                },
            };
            
            network_manager.set_config(&service_id, circuit_config).await;
            
            // Đăng ký chain với health monitor
            let adapter_clone = evm_adapters.get(chain_id).cloned();
            health_monitor.register_service(&service_id, move || {
                let adapter_clone = adapter_clone.clone();
                async move {
                    if let Some(adapter) = adapter_clone {
                        match adapter.get_block_number().await {
                            Ok(_) => true,
                            Err(e) => {
                                debug!("Health check failed for chain {}: {}", chain_id, e);
                                false
                            }
                        }
                    } else {
                        false
                    }
                }
            }).await.ok(); // Ignore errors during registration
        }
        
        // Khởi tạo executor
        let executor = Self {
            evm_adapters,
            analys_client: Arc::new(analys_client),
            risk_analyzer,
            mempool_analyzers,
            config: RwLock::new(config),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            coordinator: None,
            executor_id: uuid::Uuid::new_v4().to_string(),
            coordinator_subscription: RwLock::new(None),
            network_manager,
            health_check_task: RwLock::new(None),
            health_monitor,
            notification_manager: None,
            trade_log_db: None,
            nonce_manager: RwLock::new(NonceManager::new()),
        };
        
        Ok(executor)
    }
    
    /// Clone executor với config hiện tại
    pub async fn clone_with_config(&self) -> Self {
        let config = self.config.read().await.clone();
        Self {
            evm_adapters: self.evm_adapters.clone(),
            analys_client: self.analys_client.clone(),
            risk_analyzer: self.risk_analyzer.clone(),
            mempool_analyzers: self.mempool_analyzers.clone(),
            config: RwLock::new(config),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(*self.running.read().await),
            coordinator: self.coordinator.clone(),
            executor_id: self.executor_id.clone(),
            coordinator_subscription: RwLock::new(self.coordinator_subscription.read().await.clone()),
            network_manager: self.network_manager.clone(),
            health_check_task: RwLock::new(None),
            health_monitor: self.health_monitor.clone(),
            notification_manager: self.notification_manager.clone(),
            trade_log_db: self.trade_log_db.clone(),
            nonce_manager: RwLock::new(self.nonce_manager.read().await.clone()),
        }
    }
    
    /// Thiết lập coordinator
    pub async fn set_coordinator(&self, coordinator: Arc<dyn TradeCoordinator>) -> Result<()> {
        // Thiết lập coordinator
        {
            let was_running = *self.running.read().await;
            
            // Tạm dừng nếu đang chạy
            if was_running {
                self.unregister_from_coordinator().await?;
            }
            
            // Cập nhật coordinator theo cách thread-safe
            {
                // Khai báo biến Option<Arc<dyn TradeCoordinator>> để có thể take và replace
                let field = tokio::task::block_in_place(|| {
                    // Chuyển đổi Arc<SmartTradeExecutor> thành &mut Option<Arc<dyn TradeCoordinator>>
                    // Sử dụng std::ptr để lấy địa chỉ của field không đổi
                    unsafe {
                        let self_ptr = self as *const SmartTradeExecutor as *mut SmartTradeExecutor;
                        &mut (*self_ptr).coordinator
                    }
                });
                
                // Thiết lập coordinator mới
                *field = Some(coordinator);
            }
            
            // Đăng ký lại nếu đang chạy
            if was_running {
                self.register_with_coordinator().await?;
            }
        }
        
        Ok(())
    }
    
    /// Đăng ký với coordinator
    pub async fn register_with_coordinator(&self) -> Result<()> {
        if let Some(coordinator) = &self.coordinator {
            let subscription_id = coordinator
                .subscribe_to_opportunities(self.executor_id.clone(),
                    Arc::new(|opportunity| {
                        // Xử lý opportunity trong future
                        Ok(())
                    })
                )
                .await
                .context("Failed to subscribe to coordinator")?;
                
            let mut subscription = self.coordinator_subscription.write().await;
            *subscription = Some(subscription_id);
            
            info!("Registered with coordinator, subscription ID: {}", subscription_id);
        }
        
        Ok(())
    }
    
    /// Hủy đăng ký với coordinator
    pub async fn unregister_from_coordinator(&self) -> Result<()> {
        if let Some(coordinator) = &self.coordinator {
            let subscription = self.coordinator_subscription.read().await;
            if let Some(subscription_id) = &*subscription {
                coordinator.unsubscribe_from_opportunities(subscription_id).await?;
                info!("Unregistered from coordinator, subscription ID: {}", subscription_id);
            }
        }
        
        Ok(())
    }
    
    /// Khôi phục trạng thái từ lưu trữ
    pub async fn restore_state(&self) -> Result<()> {
        // Trong triển khai thực tế, đọc trạng thái từ database hoặc file
        // Hiện tại chỉ khởi tạo trạng thái mới
        
        Ok(())
    }
    
    /// Thực hiện health check cho tất cả các chain được hỗ trợ
    pub async fn perform_health_check(&self) -> HashMap<String, NetworkStatus> {
        let network_manager = &self.network_manager;
        let check_fn = |service_id: &str| -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>> {
            let evm_adapters = self.evm_adapters.clone();
            let service_id = service_id.to_string();
            Box::pin(async move {
                if let Some(chain_id_str) = service_id.strip_prefix("blockchain_rpc_") {
                    if let Ok(chain_id) = chain_id_str.parse::<u32>() {
                        if let Some(adapter) = evm_adapters.get(&chain_id) {
                            match adapter.get_block_number().await {
                                Ok(_) => true,
                                Err(e) => {
                                    warn!("Health check failed for chain {}: {:?}", chain_id, e);
                                    false
                                }
                            }
                        } else {
                            false
                        }
                    } else {
                        true
                    }
                } else {
                    true
                }
            })
        };
        match network_manager.perform_health_check(check_fn).await {
            Ok(status_map) => status_map,
            Err(e) => {
                warn!("perform_health_check failed: {}", e);
                HashMap::new()
            }
        }
    }
    
    /// Khởi động health check định kỳ
    pub async fn start_health_check(&self, interval_seconds: u64) -> Result<()> {
        let network_manager = self.network_manager.clone();
        let executor_clone = self.clone();
        
        // Tạo closure thực hiện health check
        let check_fn = move |service_id: &str| -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>> {
            // Clone các giá trị cần thiết
            let evm_adapters = executor_clone.evm_adapters.clone();
            let service_id = service_id.to_string();
            
            Box::pin(async move {
                // Kiểm tra xem service_id có phải là blockchain_rpc_X không
                if let Some(chain_id_str) = service_id.strip_prefix("blockchain_rpc_") {
                    if let Ok(chain_id) = chain_id_str.parse::<u32>() {
                        if let Some(adapter) = evm_adapters.get(&chain_id) {
                            match adapter.get_block_number().await {
                                Ok(_) => true,
                                Err(e) => {
                                    warn!("Health check failed for chain {}: {}", chain_id, e);
                                    false
                                }
                            }
                        } else {
                            false
                        }
                    } else {
                        true
                    }
                } else {
                    true
                }
            })
        };
        
        // Khởi động health check tự động
        let task = tokio::spawn(async move {
            network_manager.clone().start_auto_health_check(check_fn, interval_seconds).await;
        });
        
        // Lưu task handle
        let mut health_check_task = self.health_check_task.write().await;
        *health_check_task = Some(task);
        
        Ok(())
    }
    
    /// Thực hiện blockchain RPC call với circuit breaker và retry
    pub async fn with_network_protection<F, Fut, T>(
        &self,
        chain_id: u32,
        operation_name: &str,
        future_fn: F,
    ) -> Result<T>
    where
        F: Fn() -> Fut + Send + Sync + Clone + 'static,
        Fut: std::future::Future<Output = Result<T>> + Send,
    {
        let service_id = format!("blockchain_rpc_{}", chain_id);
        
        // Lấy cấu hình retry từ config
        let config = self.config.read().await;
        let retry_strategy = RetryStrategy {
            max_retries: config.max_retries as usize,
            base_delay_ms: config.retry_delay_ms,
            backoff_factor: 1.5,
            max_delay_ms: config.max_retry_delay_ms,
            add_jitter: true,
        };

        // Tạo health check function cho chain này
        let adapter = self.evm_adapters.get(&chain_id).cloned();
        let health_check_fn = move || {
            let adapter_clone = adapter.clone();
            Box::pin(async move {
                if let Some(adapter) = adapter_clone {
                    match adapter.get_block_number().await {
                        Ok(_) => true,
                        Err(e) => {
                            debug!("Health check failed for chain {}: {}", chain_id, e);
                            false
                        }
                    }
                } else {
                    false
                }
            }) as std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>>
        };
        
        // Sử dụng with_auto_reconnect thay vì with_circuit_breaker để có khả năng tự kết nối lại
        self.network_manager.clone().with_auto_reconnect(
            &service_id,
            future_fn,
            health_check_fn,
            Some(retry_strategy),
        ).await.with_context(|| format!("Failed to execute {} on chain {}", operation_name, chain_id))
    }

    /// Khởi động health monitor
    pub async fn start_health_monitoring(&self) -> Result<()> {
        // Cấu hình cho health monitor
        let config_read = self.config.read().await;
        let monitor_config = HealthMonitorConfig {
            check_interval_seconds: config_read.health_check_interval_seconds,
            reconnect_interval_ms: config_read.retry_delay_ms,
            max_reconnect_attempts: config_read.max_retries as usize,
            enable_auto_reconnect: true,
            enable_system_monitoring: true,
            enable_network_monitoring: true,
            ..Default::default()
        };
        
        // Sử dụng phương thức update_config thay vì truy cập trực tiếp field private
        self.health_monitor.update_config(monitor_config).await.unwrap_or_else(|e| {
            warn!("Failed to update health monitor config: {}", e);
        });
        
        // Đăng ký alert handler tùy chỉnh
        struct ExecutorAlertHandler {
            executor: Arc<SmartTradeExecutor>,
        }
        
        #[async_trait]
        impl crate::tradelogic::common::health_monitor::HealthAlertHandler for ExecutorAlertHandler {
            async fn handle_alert(&self, alert: HealthAlert) -> Result<()> {
                match alert.severity {
                    AlertSeverity::Error | AlertSeverity::Critical => {
                        // Lưu lại cảnh báo và thông báo cho người dùng
                        error!("CRITICAL ALERT: {} - {}", alert.resource_type, alert.message);
                        
                        // Thực hiện hành động tự động khắc phục nếu cần
                        if alert.resource_type.starts_with("network_service_blockchain_rpc_") {
                            // Lấy chain_id từ tên service
                            if let Some(chain_id_str) = alert.resource_type.strip_prefix("network_service_blockchain_rpc_") {
                                if let Ok(chain_id) = chain_id_str.parse::<u32>() {
                                    // Thực hiện kiểm tra sâu hơn và khắc phục nếu cần
                                    self.executor.deep_network_health_check(chain_id).await.ok();
                                }
                            }
                        }
                    },
                    _ => {
                        // Ghi log các cảnh báo khác
                        debug!("Health alert: {} - {}", alert.resource_type, alert.message);
                    }
                }
                
                Ok(())
            }
        }
        
        // Khởi động health monitor
        self.health_monitor.register_alert_handler(ExecutorAlertHandler {
            executor: Arc::new(self.clone()),
        }).await?;
        
        self.health_monitor.clone().start().await?;
        
        Ok(())
    }
    
    /// Deep health check untuk chain tertentu, kiểm tra nhiều thông số
    async fn deep_network_health_check(&self, chain_id: u32) -> Result<()> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Kiểm tra block number
        if let Err(e) = adapter.get_block_number().await {
            warn!("Block number check failed for chain {}: {}", chain_id, e);
            return Err(anyhow!("Block number check failed: {}", e));
        }
        
        // Kiểm tra gas price không còn khả dụng nên bỏ qua
        // Kiểm tra số lượng peers không còn khả dụng nên bỏ qua
        
        // Thực hiện một gọi view function đơn giản để kiểm tra
        let weth_address = crate::tradelogic::smart_trade::optimization::TradeOptimizer::get_chain_native_token(chain_id);
        if let Err(e) = adapter.get_token_info(&weth_address).await {
            warn!("Token info check failed for chain {}: {}", chain_id, e);
            return Err(anyhow!("Token info check failed: {}", e));
        }
        
        Ok(())
    }
    
    /// Cập nhật thông tin thị trường của các token đang theo dõi
    /// 
    /// Hàm này thu thập dữ liệu thị trường cho tất cả các token đang theo dõi
    /// và cập nhật các thông tin như giá, thanh khoản, và biến động
    pub async fn update_market_status(&self) -> Result<()> {
        // Lấy danh sách các giao dịch đang hoạt động
        let active_trades = self.active_trades.read().await.clone();
        
        // Nhóm theo chain_id để giảm số lượng gọi RPC
        let mut token_by_chains = std::collections::HashMap::new();
        
        for trade in active_trades.iter() {
            let chain_id = match trade.params.chain_type {
                crate::types::ChainType::EVM(chain_id) => chain_id,
                _ => continue,
            };
            
            let tokens = token_by_chains
                .entry(chain_id)
                .or_insert_with(std::collections::HashSet::new);
            tokens.insert(trade.params.token_address.clone());
        }
        
        // Cập nhật thông tin thị trường cho từng token trên mỗi chain
        for (chain_id, tokens) in token_by_chains {
            let adapter = match self.evm_adapters.get(&chain_id) {
                Some(adapter) => adapter,
                None => {
                    warn!("No adapter found for chain ID {}", chain_id);
                    continue;
                }
            };
            
            for token_address in tokens {
                // Lấy giá token
                match adapter.get_token_price(&token_address).await {
                    Ok(price) => {
                        debug!("Updated price for token {} on chain {}: {}", token_address, chain_id, price);
                    },
                    Err(e) => {
                        warn!("Failed to get price for token {} on chain {}: {}", token_address, chain_id, e);
                    }
                }
                
                // Lấy thông tin thanh khoản nếu có
                if let Ok(liquidity) = adapter.get_token_liquidity(&token_address).await {
                    debug!("Updated liquidity for token {} on chain {}: {} USD", token_address, chain_id, liquidity);
                }
            }
        }
        
        Ok(())
    }
    
    /// Thử phục hồi kết nối mạng cho chain cụ thể
    async fn attempt_network_recovery(&self, chain_id: u32) -> Result<()> {
        let service_id = format!("blockchain_rpc_{}", chain_id);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter.clone(),
            None => return Err(anyhow!("No adapter found for chain {}", chain_id)),
        };
        
        // Tạo health check function
        let health_check_fn = move || {
            let adapter_clone = adapter.clone();
            async move {
                match adapter_clone.get_block_number().await {
                    Ok(_) => true,
                    Err(_) => false,
                }
            }
        };
        
        // Thử kết nối lại
        let success = self.network_manager.try_reconnect(&service_id, health_check_fn).await;
        
        if success {
            info!("Successfully reconnected to chain {}", chain_id);
        } else {
            warn!("Failed to reconnect to chain {}. Scheduling automatic reconnect attempts...", chain_id);
            
            // Lấy adapter một lần nữa cho closure mới
            let adapter2 = match self.evm_adapters.get(&chain_id) {
                Some(adapter) => adapter.clone(),
                None => return Err(anyhow!("No adapter found for chain {}", chain_id)),
            };
            
            // Lên lịch thử kết nối lại tự động
            let health_check = move || {
                let adapter_clone = adapter2.clone();
                async move {
                    match adapter_clone.get_block_number().await {
                        Ok(_) => true,
                        Err(_) => false,
                    }
                }
            };
            
            self.network_manager.clone().schedule_reconnect(&service_id, health_check, Some(10)).await;
        }
        
        Ok(())
    }
}

// Implement the TradeExecutor trait for SmartTradeExecutor
#[async_trait]
impl TradeExecutor for SmartTradeExecutor {
    /// Configuration type
    type Config = SmartTradeConfig;
    
    /// Result type for trades
    type TradeResult = TradeResult;
    
    /// Start the trading executor
    async fn start(&self) -> anyhow::Result<()> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }
        
        *running = true;
        
        // Register with coordinator if available
        if self.coordinator.is_some() {
            self.register_with_coordinator().await?;
        }
        
        // Phục hồi trạng thái trước khi khởi động
        self.restore_state().await?;
        
        // Khởi động health monitor
        self.start_health_monitoring().await?;
        
        // Khởi động health check định kỳ
        let config = self.config.read().await;
        self.start_health_check(config.health_check_interval_seconds).await?;
        
        // Khởi động vòng lặp theo dõi
        let executor = Arc::new(self.clone_with_config().await);
        
        tokio::spawn(async move {
            monitor_loop(executor).await;
        });
        
        Ok(())
    }
    
    /// Stop the trading executor
    async fn stop(&self) -> anyhow::Result<()> {
        let mut running = self.running.write().await;
        *running = false;
        
        // Unregister from coordinator if available
        if self.coordinator.is_some() {
            self.unregister_from_coordinator().await?;
        }
        
        // Hủy tất cả các task đang chạy
        let mut active_trades = self.active_trades.write().await;
        for trade in active_trades.iter_mut() {
            if let Some(task_handle) = &trade.task_handle {
                // Abort task nếu vẫn đang chạy
                debug!("Aborting task for trade {}: {}", trade.trade_id, task_handle.description);
                task_handle.join_handle.abort();
                trade.task_handle = None;
            }
        }
        
        // Hủy health check task
        let mut health_check_task = self.health_check_task.write().await;
        if let Some(task) = health_check_task.take() {
            task.abort();
            debug!("Aborted health check task");
        }
        
        Ok(())
    }
    
    /// Update the executor's configuration
    async fn update_config(&self, config: Self::Config) -> anyhow::Result<()> {
        let mut current_config = self.config.write().await;
        *current_config = config;
        Ok(())
    }
    
    /// Add a new blockchain to monitor
    async fn add_chain(&mut self, chain_id: u32, adapter: Arc<EvmAdapter>) -> anyhow::Result<()> {
        // Create a mempool analyzer if it doesn't exist
        let mempool_analyzer = if let Some(analyzer) = self.mempool_analyzers.get(&chain_id) {
            analyzer.clone()
        } else {
            Arc::new(MempoolAnalyzer::new(chain_id, adapter.clone()))
        };
        
        self.evm_adapters.insert(chain_id, adapter);
        self.mempool_analyzers.insert(chain_id, mempool_analyzer);
        
        // Thiết lập circuit breaker cho chain mới
        let service_id = format!("blockchain_rpc_{}", chain_id);
        
        // Cấu hình circuit breaker mặc định
        let circuit_config = CircuitBreakerConfig {
            failure_threshold: 5,
            reset_timeout_seconds: 30,
            use_half_open: true,
            max_retries: 3,
            success_threshold: 2,
        };
        
        self.network_manager.set_config(&service_id, circuit_config).await;
        
        Ok(())
    }
    
    /// Thực thi giao dịch
    async fn execute_trade(&self, params: TradeParams) -> Result<TradeResult> {
        debug!("Thực thi giao dịch với params: {:?}", params);
        
        // Kiểm tra adapter cho chain
        let adapter = self.get_chain_adapter(params.chain_id).await?;
        
        // Tạo ID giao dịch
        let trade_id = Uuid::new_v4().to_string();
        
        // Cập nhật trạng thái giao dịch đang hoạt động
        {
            let mut active_trades = self.active_trades.write().await;
            active_trades.insert(trade_id.clone(), TradeStatus::Pending);
        }
        
        // Sử dụng throttling để kiểm soát tải
        use crate::tradelogic::common::throttle::execute_with_throttling;
        
        let trade_handler = self.trade_handler.clone();
        let adapter_clone = adapter.clone();
        let params_clone = params.clone();
        let trade_id_clone = trade_id.clone();
        
        let result = execute_with_throttling(params.chain_id, move || {
            let handler = trade_handler;
            let adapter = adapter_clone;
            let params = params_clone;
            let trade_id = trade_id_clone;
            
            // Thực thi đồng bộ để tránh vấn đề khi dùng throttling với async closure
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    match params.trade_type {
                        crate::types::TradeType::Buy => {
                            handler.execute_buy(&adapter, &params, &trade_id).await
                        },
                        crate::types::TradeType::Sell => {
                            handler.execute_sell(&adapter, &params, &trade_id).await
                        },
                        _ => {
                            Err(anyhow!("Loại giao dịch không được hỗ trợ: {:?}", params.trade_type))
                        }
                    }
                })
            })
        }).await?;
        
        // Cập nhật trạng thái và lịch sử
        {
            let mut active_trades = self.active_trades.write().await;
            active_trades.remove(&trade_id);
            
            let mut history = self.trade_history.write().await;
            history.push(result.clone());
            
            // Giới hạn kích thước lịch sử
            if history.len() > 1000 {
                history.drain(0..history.len() - 1000);
            }
        }
        
        Ok(result)
    }
    
    /// Get a list of active trades
    async fn get_active_trades(&self) -> anyhow::Result<Vec<Self::TradeResult>> {
        let active_trades = self.active_trades.read().await;
        
        // Chuyển đổi từ TradeTracker sang TradeResult
        let mut results = Vec::new();
        for tracker in active_trades.iter() {
            let result = TradeResult {
                trade_id: tracker.trade_id.clone(),
                params: tracker.params.clone(),
                status: tracker.status.clone(),
                created_at: tracker.created_at,
                completed_at: None,
                tx_hash: tracker.tx_hash.clone(),
                actual_amount: None,
                actual_price: tracker.initial_price,
                fee: None,
                profit_loss: None,
                error: None,
                explorer_url: None,
            };
            
            results.push(result);
        }
        
        Ok(results)
    }
    
    /// Get trade history
    async fn get_trade_history(&self, from_timestamp: u64, to_timestamp: u64) -> anyhow::Result<Vec<Self::TradeResult>> {
        let history = self.trade_history.read().await;
        
        // Filter trades between the specified timestamps
        let results: Vec<Self::TradeResult> = history
            .iter()
            .filter(|trade| {
                trade.created_at >= from_timestamp && 
                (trade.completed_at.unwrap_or(u64::MAX) <= to_timestamp || trade.created_at <= to_timestamp)
            })
            .cloned()
            .collect();
        
        Ok(results)
    }
    
    /// Cancel a trade
    async fn cancel_trade(&self, trade_id: &str) -> anyhow::Result<()> {
        let mut active_trades = self.active_trades.write().await;
        
        // Find the trade to cancel
        let trade_index = active_trades.iter().position(|t| t.trade_id == trade_id);
        
        if let Some(index) = trade_index {
            let mut trade = active_trades.remove(index);
            trade.status = TradeStatus::Cancelled;
            
            // Add to history
            let result = TradeResult {
                trade_id: trade.trade_id.clone(),
                params: trade.params.clone(),
                status: TradeStatus::Cancelled,
                created_at: trade.created_at,
                completed_at: Some(Utc::now().timestamp() as u64),
                tx_hash: trade.tx_hash.clone(),
                actual_amount: None,
                actual_price: trade.initial_price,
                fee: None,
                profit_loss: None,
                error: Some("Trade cancelled by user".to_string()),
                explorer_url: None,
            };
            
            let mut history = self.trade_history.write().await;
            history.push(result);
            
            Ok(())
        } else {
            Err(anyhow::anyhow!("Trade with ID {} not found", trade_id))
        }
    }
    
    /// Process a shared opportunity from the coordinator
    async fn process_opportunity(&self, opportunity: SharedOpportunity) -> anyhow::Result<bool> {
        // Implement opportunity processing logic here
        // Forward to the process_opportunity function in market_monitor.rs
        let result = process_opportunity(self, opportunity).await?;
        Ok(result)
    }
    
    /// Check for opportunities and share with coordinator
    async fn check_opportunities(&self) -> anyhow::Result<Vec<SharedOpportunity>> {
        // Delegate to market_monitor
        let results = check_opportunities(self).await?;
        Ok(results)
    }
    
    /// Get executor type
    fn executor_type(&self) -> ExecutorType {
        ExecutorType::SmartTrade
    }
    
    /// Get executor ID
    fn executor_id(&self) -> String {
        self.executor_id.clone()
    }
    
    /// Get the current status of an active trade
    /// 
    /// Retrieves the current state of a trade by its ID.
    /// Returns None if the trade doesn't exist.
    async fn get_trade_status(&self, trade_id: &str) -> Result<Option<Self::TradeResult>> {
        // First check active trades
        let active_trades = self.active_trades.read().await;
        if let Some(trade) = active_trades.iter().find(|t| t.trade_id == trade_id) {
            // Convert TradeTracker to TradeResult
            let result = TradeResult {
                trade_id: trade.trade_id.clone(),
                params: trade.params.clone(),
                status: trade.status.clone(),
                created_at: trade.created_at,
                completed_at: None,
                tx_hash: trade.tx_hash.clone(),
                actual_amount: None,
                actual_price: trade.initial_price,
                fee: None,
                profit_loss: None,
                error: None,
                explorer_url: None,
            };
            return Ok(Some(result));
        }
        
        // If not found in active trades, check history
        let history = self.trade_history.read().await;
        let result = history.iter().find(|t| t.trade_id == trade_id).cloned();
        Ok(result)
    }
    
    /// Evaluate a token for potential trading
    ///
    /// Analyzes a token to determine if it's suitable for trading.
    /// Returns token safety information if available.
    async fn evaluate_token(&self, chain_id: u32, token_address: &str) -> Result<Option<TokenSafety>> {
        // Use analys_client to evaluate token
        self.analys_client.get_token_safety(chain_id, token_address).await
    }
} 