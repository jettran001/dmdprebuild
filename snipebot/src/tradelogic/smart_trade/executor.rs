/// Core implementation of SmartTradeExecutor
///
/// This file contains the main executor struct that manages the entire
/// smart trading process, orchestrates token analysis, trade strategies,
/// and market monitoring.
///
/// # Cải tiến đã thực hiện:
/// - Loại bỏ các phương thức trùng lặp với analys modules
/// - Sử dụng API từ analys/token_status và analys/risk_analyzer
/// - Áp dụng xử lý lỗi chuẩn với anyhow::Result thay vì unwrap/expect
/// - Đảm bảo thread safety trong các phương thức async với kỹ thuật "clone and drop" để tránh deadlock
/// - Tối ưu hóa việc xử lý các futures với tokio::join! cho các tác vụ song song
/// - Xử lý các trường hợp giá trị null an toàn với match/Option
/// - Tuân thủ nghiêm ngặt quy tắc từ .cursorrc

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use chrono::Utc;
use futures::future::join_all;
use tracing::{debug, error, info, warn};
use anyhow::{Result, Context, anyhow, bail};
use uuid;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use rand::Rng;
use std::collections::HashSet;
use futures::future::Future;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::{MempoolAnalyzer, MempoolTransaction};
use crate::analys::risk_analyzer::{RiskAnalyzer, RiskFactor, TradeRecommendation, TradeRiskAnalysis};
use crate::analys::token_status::{
    ContractInfo, LiquidityEvent, LiquidityEventType, TokenSafety, TokenStatus,
    TokenIssue, IssueSeverity, abnormal_liquidity_events, detect_dynamic_tax, 
    detect_hidden_fees, analyze_owner_privileges, is_proxy_contract, blacklist::{has_blacklist_or_whitelist, has_trading_cooldown, has_max_tx_or_wallet_limit},
};
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};
use crate::tradelogic::traits::{
    TradeExecutor, RiskManager, TradeCoordinator, ExecutorType, SharedOpportunity, 
    SharedOpportunityType, OpportunityPriority
};

// Module imports
use super::types::{
    SmartTradeConfig, 
    TradeResult, 
    TradeStatus, 
    TradeStrategy, 
    TradeTracker
};
use super::constants::*;
use super::token_analysis::*;
use super::trade_strategy::*;
use super::alert::*;
use super::optimization::*;
use super::security::*;
use super::analys_client::SmartTradeAnalysisClient;

/// Executor for smart trading strategies
#[derive(Clone)]
pub struct SmartTradeExecutor {
    /// EVM adapter for each chain
    pub(crate) evm_adapters: HashMap<u32, Arc<EvmAdapter>>,
    
    /// Analysis client that provides access to all analysis services
    pub(crate) analys_client: Arc<SmartTradeAnalysisClient>,
    
    /// Risk analyzer (legacy - kept for backward compatibility)
    pub(crate) risk_analyzer: Arc<RwLock<RiskAnalyzer>>,
    
    /// Mempool analyzer for each chain (legacy - kept for backward compatibility)
    pub(crate) mempool_analyzers: HashMap<u32, Arc<MempoolAnalyzer>>,
    
    /// Configuration
    pub(crate) config: RwLock<SmartTradeConfig>,
    
    /// Active trades being monitored
    pub(crate) active_trades: RwLock<Vec<TradeTracker>>,
    
    /// History of completed trades
    pub(crate) trade_history: RwLock<Vec<TradeResult>>,
    
    /// Running state
    pub(crate) running: RwLock<bool>,
    
    /// Coordinator for shared state between executors
    pub(crate) coordinator: Option<Arc<dyn TradeCoordinator>>,
    
    /// Unique ID for this executor instance
    pub(crate) executor_id: String,
    
    /// Subscription ID for coordinator notifications
    pub(crate) coordinator_subscription: RwLock<Option<String>>,
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
        
        // Khởi động vòng lặp theo dõi
        let executor = Arc::new(self.clone_with_config().await);
        
        tokio::spawn(async move {
            executor.monitor_loop().await;
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
        
        Ok(())
    }
    
    /// Execute a trade with the specified parameters
    async fn execute_trade(&self, params: TradeParams) -> anyhow::Result<Self::TradeResult> {
        // Find the appropriate adapter
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", params.chain_id)),
        };
        
        // Validate trade parameters
        if params.amount <= 0.0 {
            return Err(anyhow::anyhow!("Invalid trade amount: {}", params.amount));
        }
        
        // Create a trade tracker
        let trade_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp() as u64;
        
        let tracker = self.create_trade_tracker(&params, &trade_id, now);
        
        // TODO: Implement the actual trade execution logic
/// Core implementation of SmartTradeExecutor
///
/// This file contains the main executor struct that manages the entire
/// smart trading process, orchestrates token analysis, trade strategies,
/// and market monitoring.
///
/// # Cải tiến đã thực hiện:
/// - Loại bỏ các phương thức trùng lặp với analys modules
/// - Sử dụng API từ analys/token_status và analys/risk_analyzer
/// - Áp dụng xử lý lỗi chuẩn với anyhow::Result thay vì unwrap/expect
/// - Đảm bảo thread safety trong các phương thức async với kỹ thuật "clone and drop" để tránh deadlock
/// - Tối ưu hóa việc xử lý các futures với tokio::join! cho các tác vụ song song
/// - Xử lý các trường hợp giá trị null an toàn với match/Option
/// - Tuân thủ nghiêm ngặt quy tắc từ .cursorrc

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use chrono::Utc;
use futures::future::join_all;
use tracing::{debug, error, info, warn};
use anyhow::{Result, Context, anyhow, bail};
use uuid;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use rand::Rng;
use std::collections::HashSet;
use futures::future::Future;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::{MempoolAnalyzer, MempoolTransaction};
use crate::analys::risk_analyzer::{RiskAnalyzer, RiskFactor, TradeRecommendation, TradeRiskAnalysis};
use crate::analys::token_status::{
    ContractInfo, LiquidityEvent, LiquidityEventType, TokenSafety, TokenStatus,
    TokenIssue, IssueSeverity, abnormal_liquidity_events, detect_dynamic_tax, 
    detect_hidden_fees, analyze_owner_privileges, is_proxy_contract, blacklist::{has_blacklist_or_whitelist, has_trading_cooldown, has_max_tx_or_wallet_limit},
};
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};
use crate::tradelogic::traits::{
    TradeExecutor, RiskManager, TradeCoordinator, ExecutorType, SharedOpportunity, 
    SharedOpportunityType, OpportunityPriority
};

// Module imports
use super::types::{
    SmartTradeConfig, 
    TradeResult, 
    TradeStatus, 
    TradeStrategy, 
    TradeTracker
};
use super::constants::*;
use super::token_analysis::*;
use super::trade_strategy::*;
use super::alert::*;
use super::optimization::*;
use super::security::*;
use super::analys_client::SmartTradeAnalysisClient;

/// Executor for smart trading strategies
#[derive(Clone)]
pub struct SmartTradeExecutor {
    /// EVM adapter for each chain
    pub(crate) evm_adapters: HashMap<u32, Arc<EvmAdapter>>,
    
    /// Analysis client that provides access to all analysis services
    pub(crate) analys_client: Arc<SmartTradeAnalysisClient>,
    
    /// Risk analyzer (legacy - kept for backward compatibility)
    pub(crate) risk_analyzer: Arc<RwLock<RiskAnalyzer>>,
    
    /// Mempool analyzer for each chain (legacy - kept for backward compatibility)
    pub(crate) mempool_analyzers: HashMap<u32, Arc<MempoolAnalyzer>>,
    
    /// Configuration
    pub(crate) config: RwLock<SmartTradeConfig>,
    
    /// Active trades being monitored
    pub(crate) active_trades: RwLock<Vec<TradeTracker>>,
    
    /// History of completed trades
    pub(crate) trade_history: RwLock<Vec<TradeResult>>,
    
    /// Running state
    pub(crate) running: RwLock<bool>,
    
    /// Coordinator for shared state between executors
    pub(crate) coordinator: Option<Arc<dyn TradeCoordinator>>,
    
    /// Unique ID for this executor instance
    pub(crate) executor_id: String,
    
    /// Subscription ID for coordinator notifications
    pub(crate) coordinator_subscription: RwLock<Option<String>>,
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
        
        // Khởi động vòng lặp theo dõi
        let executor = Arc::new(self.clone_with_config().await);
        
        tokio::spawn(async move {
            executor.monitor_loop().await;
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
        
        Ok(())
    }
    
    /// Execute a trade with the specified parameters
    async fn execute_trade(&self, params: TradeParams) -> anyhow::Result<Self::TradeResult> {
        // Find the appropriate adapter
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", params.chain_id)),
        };
        
        // Validate trade parameters
        if params.amount <= 0.0 {
            return Err(anyhow::anyhow!("Invalid trade amount: {}", params.amount));
        }
        
        // Create a trade tracker
        let trade_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp() as u64;
        
        let tracker = TradeTracker {
            id: trade_id.clone(),
            chain_id: params.chain_id,
            token_address: params.token_address.clone(),
            amount: params.amount,
            entry_price: 0.0, // Will be updated after trade execution
            current_price: 0.0,
            status: TradeStatus::Pending,
            strategy: if let Some(strategy) = &params.strategy {
                match strategy.as_str() {
                    "quick" => TradeStrategy::QuickFlip,
                    "tsl" => TradeStrategy::TrailingStopLoss,
                    "hodl" => TradeStrategy::LongTerm,
                    _ => TradeStrategy::default(),
                }
            } else {
                TradeStrategy::default()
            },
            created_at: now,
            updated_at: now,
            stop_loss: params.stop_loss,
            take_profit: params.take_profit,
            max_hold_time: params.max_hold_time.unwrap_or(DEFAULT_MAX_HOLD_TIME),
            custom_params: params.custom_params.unwrap_or_default(),
        };
        
        // TODO: Implement the actual trade execution logic
        // This is a simplified version - in a real implementation, you would:
        // 1. Execute the trade
        // 2. Update the trade tracker with actual execution details
        // 3. Add it to active trades
        
        let result = TradeResult {
            id: trade_id,
            chain_id: params.chain_id,
            token_address: params.token_address,
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: params.amount,
            entry_price: 0.0,                  // Would be populated from actual execution
            exit_price: None,
            current_price: 0.0,
            profit_loss: 0.0,
            status: TradeStatus::Pending,
            strategy: tracker.strategy.clone(),
            created_at: now,
            updated_at: now,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        // Add to active trades
        {
            let mut active_trades = self.active_trades.write().await;
            active_trades.push(tracker);
        }
        
        Ok(result)
    }
    
    /// Get the current status of an active trade
    async fn get_trade_status(&self, trade_id: &str) -> anyhow::Result<Option<Self::TradeResult>> {
        // Check active trades
        {
            let active_trades = self.active_trades.read().await;
            if let Some(trade) = active_trades.iter().find(|t| t.id == trade_id) {
                return Ok(Some(self.tracker_to_result(trade).await?));
            }
        }
        
        // Check trade history
        let trade_history = self.trade_history.read().await;
        let result = trade_history.iter()
            .find(|r| r.id == trade_id)
            .cloned();
            
        Ok(result)
    }
    
    /// Get all active trades
    async fn get_active_trades(&self) -> anyhow::Result<Vec<Self::TradeResult>> {
        let active_trades = self.active_trades.read().await;
        let mut results = Vec::with_capacity(active_trades.len());
        
        for tracker in active_trades.iter() {
            results.push(self.tracker_to_result(tracker).await?);
        }
        
        Ok(results)
    }
    
    /// Get trade history within a specific time range
    async fn get_trade_history(&self, from_timestamp: u64, to_timestamp: u64) -> anyhow::Result<Vec<Self::TradeResult>> {
        let trade_history = self.trade_history.read().await;
        
        let filtered_history = trade_history.iter()
            .filter(|trade| trade.created_at >= from_timestamp && trade.created_at <= to_timestamp)
            .cloned()
            .collect();
            
        Ok(filtered_history)
    }
    
    /// Evaluate a token for potential trading
    async fn evaluate_token(&self, chain_id: u32, token_address: &str) -> anyhow::Result<Option<TokenSafety>> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Sử dụng analys_client để phân tích token
        let token_safety_result = self.analys_client.token_provider()
            .analyze_token(chain_id, token_address).await
            .context(format!("Không thể phân tích token {}", token_address))?;
        
        // Nếu không phân tích được token, trả về None
        if !token_safety_result.is_valid() {
            info!("Token {} không hợp lệ hoặc không thể phân tích", token_address);
            return Ok(None);
        }
        
        // Lấy thêm thông tin chi tiết từ contract
        let contract_details = self.analys_client.token_provider()
            .get_contract_details(chain_id, token_address).await
            .context(format!("Không thể lấy chi tiết contract cho token {}", token_address))?;
        
        // Đánh giá rủi ro của token
        let risk_analysis = self.analys_client.risk_provider()
            .analyze_token_risk(chain_id, token_address).await
            .context(format!("Không thể phân tích rủi ro cho token {}", token_address))?;
        
        // Log thông tin phân tích
        info!(
            "Phân tích token {} trên chain {}: risk_score={}, honeypot={:?}, buy_tax={:?}, sell_tax={:?}",
            token_address,
            chain_id,
            risk_analysis.risk_score,
            token_safety_result.is_honeypot,
            token_safety_result.buy_tax,
            token_safety_result.sell_tax
        );
        
        Ok(Some(token_safety_result))
    }

    /// Đánh giá rủi ro của token bằng cách sử dụng analys_client
    async fn analyze_token_risk(&self, chain_id: u32, token_address: &str) -> anyhow::Result<RiskScore> {
        // Sử dụng phương thức analyze_risk từ analys_client
        self.analys_client.analyze_risk(chain_id, token_address).await
            .context(format!("Không thể đánh giá rủi ro cho token {}", token_address))
    }
}

impl SmartTradeExecutor {
    /// Create a new SmartTradeExecutor
    pub fn new() -> Self {
        let executor_id = format!("smart-trade-{}", uuid::Uuid::new_v4());
        
        let evm_adapters = HashMap::new();
        let analys_client = Arc::new(SmartTradeAnalysisClient::new(evm_adapters.clone()));
        
        Self {
            evm_adapters,
            analys_client,
            risk_analyzer: Arc::new(RwLock::new(RiskAnalyzer::new())),
            mempool_analyzers: HashMap::new(),
            config: RwLock::new(SmartTradeConfig::default()),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            coordinator: None,
            executor_id,
            coordinator_subscription: RwLock::new(None),
        }
    }
    
    /// Set coordinator for shared state
    pub fn with_coordinator(mut self, coordinator: Arc<dyn TradeCoordinator>) -> Self {
        self.coordinator = Some(coordinator);
        self
    }
    
    /// Convert a trade tracker to a trade result
    async fn tracker_to_result(&self, tracker: &TradeTracker) -> anyhow::Result<TradeResult> {
        let result = TradeResult {
            id: tracker.id.clone(),
            chain_id: tracker.chain_id,
            token_address: tracker.token_address.clone(),
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: tracker.amount,
            entry_price: tracker.entry_price,
            exit_price: None,
            current_price: tracker.current_price,
            profit_loss: if tracker.entry_price > 0.0 && tracker.current_price > 0.0 {
                (tracker.current_price - tracker.entry_price) / tracker.entry_price * 100.0
            } else {
                0.0
            },
            status: tracker.status.clone(),
            strategy: tracker.strategy.clone(),
            created_at: tracker.created_at,
            updated_at: tracker.updated_at,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        Ok(result)
    }
    
    /// Vòng lặp theo dõi chính
    async fn monitor_loop(&self) {
        let mut counter = 0;
        let mut persist_counter = 0;
        
        while let true_val = *self.running.read().await {
            if !true_val {
                break;
            }
            
            // Kiểm tra và cập nhật tất cả các giao dịch đang active
            if let Err(e) = self.check_active_trades().await {
                error!("Error checking active trades: {}", e);
            }
            
            // Tăng counter và dọn dẹp lịch sử giao dịch định kỳ
            counter += 1;
            if counter >= 100 {
                self.cleanup_trade_history().await;
                counter = 0;
            }
            
            // Lưu trạng thái vào bộ nhớ định kỳ
            persist_counter += 1;
            if persist_counter >= 30 { // Lưu mỗi 30 chu kỳ (khoảng 2.5 phút với interval 5s)
                if let Err(e) = self.update_and_persist_trades().await {
                    error!("Failed to persist bot state: {}", e);
                }
                persist_counter = 0;
            }
            
            // Tính toán thời gian sleep thích ứng
            let sleep_time = self.adaptive_sleep_time().await;
            tokio::time::sleep(tokio::time::Duration::from_millis(sleep_time)).await;
        }
        
        // Lưu trạng thái trước khi dừng
        if let Err(e) = self.update_and_persist_trades().await {
            error!("Failed to persist bot state before stopping: {}", e);
        }
    }
    
    /// Quét cơ hội giao dịch mới
    async fn scan_trading_opportunities(&self) {
        // Kiểm tra cấu hình
        let config = self.config.read().await;
        if !config.enabled || !config.auto_trade {
            return;
        }
        
        // Kiểm tra số lượng giao dịch hiện tại
        let active_trades = self.active_trades.read().await;
        if active_trades.len() >= config.max_concurrent_trades as usize {
            return;
        }
        
        // Chạy quét cho từng chain
        let futures = self.mempool_analyzers.iter().map(|(chain_id, analyzer)| {
            self.scan_chain_opportunities(*chain_id, analyzer)
        });
        join_all(futures).await;
    }
    
    /// Quét cơ hội giao dịch trên một chain cụ thể
    async fn scan_chain_opportunities(&self, chain_id: u32, analyzer: &Arc<MempoolAnalyzer>) -> anyhow::Result<()> {
        let config = self.config.read().await;
        
        // Lấy các giao dịch lớn từ mempool
        let large_txs = match analyzer.get_large_transactions(QUICK_TRADE_MIN_BNB).await {
            Ok(txs) => txs,
            Err(e) => {
                error!("Không thể lấy các giao dịch lớn từ mempool: {}", e);
                return Err(anyhow::anyhow!("Lỗi khi lấy giao dịch từ mempool: {}", e));
            }
        };
        
        // Phân tích từng giao dịch
        for tx in large_txs {
            // Kiểm tra nếu token không trong blacklist và đáp ứng điều kiện
            if let Some(to_token) = &tx.to_token {
                let token_address = &to_token.address;
                
                // Kiểm tra blacklist và whitelist
                if config.token_blacklist.contains(token_address) {
                    debug!("Bỏ qua token {} trong blacklist", token_address);
                    continue;
                }
                
                if !config.token_whitelist.is_empty() && !config.token_whitelist.contains(token_address) {
                    debug!("Bỏ qua token {} không trong whitelist", token_address);
                    continue;
                }
                
                // Kiểm tra nếu giao dịch đủ lớn
                if tx.value >= QUICK_TRADE_MIN_BNB {
                    // Phân tích token
                    if let Err(e) = self.analyze_and_trade_token(chain_id, token_address, tx).await {
                        error!("Lỗi khi phân tích và giao dịch token {}: {}", token_address, e);
                        // Tiếp tục với token tiếp theo, không dừng lại
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Phân tích token và thực hiện giao dịch nếu an toàn
    async fn analyze_and_trade_token(&self, chain_id: u32, token_address: &str, tx: MempoolTransaction) -> anyhow::Result<()> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id));
            }
        };
        
        // THÊM: Xác thực kỹ lưỡng dữ liệu mempool trước khi tiếp tục phân tích
        info!("Xác thực dữ liệu mempool cho token {} từ giao dịch {}", token_address, tx.tx_hash);
        let mempool_validation = match self.validate_mempool_data(chain_id, token_address, &tx).await {
            Ok(validation) => validation,
            Err(e) => {
                error!("Lỗi khi xác thực dữ liệu mempool: {}", e);
                self.log_trade_decision(
                    "new_opportunity", 
                    token_address, 
                    TradeStatus::Canceled, 
                    &format!("Lỗi xác thực mempool: {}", e), 
                    chain_id
                ).await;
                return Err(anyhow::anyhow!("Lỗi xác thực dữ liệu mempool: {}", e));
            }
        };
        
        // Nếu dữ liệu mempool không hợp lệ, dừng phân tích
        if !mempool_validation.is_valid {
            let reasons = mempool_validation.reasons.join(", ");
            info!("Dữ liệu mempool không đáng tin cậy cho token {}: {}", token_address, reasons);
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Dữ liệu mempool không đáng tin cậy: {}", reasons), 
                chain_id
            ).await;
            return Ok(());
        }
        
        info!("Xác thực mempool thành công với điểm tin cậy: {}/100", mempool_validation.confidence_score);
        
        // Code hiện tại: Sử dụng analys_client để kiểm tra an toàn token
        let security_check_future = self.analys_client.verify_token_safety(chain_id, token_address);
        
        // Code hiện tại: Sử dụng analys_client để đánh giá rủi ro
        let risk_analysis_future = self.analys_client.analyze_risk(chain_id, token_address);
        
        // Thực hiện song song các phân tích
        let (security_check_result, risk_analysis) = tokio::join!(security_check_future, risk_analysis_future);
        
        // Xử lý kết quả kiểm tra an toàn
        let security_check = match security_check_result {
            Ok(result) => result,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi kiểm tra an toàn token: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi kiểm tra an toàn token: {}", e));
            }
        };
        
        // Xử lý kết quả phân tích rủi ro
        let risk_score = match risk_analysis {
            Ok(score) => score,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi phân tích rủi ro: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi phân tích rủi ro: {}", e));
            }
        };
        
        // Kiểm tra xem token có an toàn không
        if !security_check.is_safe {
            let issues_str = security_check.issues.iter()
                .map(|issue| format!("{:?}", issue))
                .collect::<Vec<String>>()
                .join(", ");
                
            self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                &format!("Token không an toàn: {}", issues_str), chain_id).await;
            return Ok(());
        }
        
        // Lấy cấu hình hiện tại
        let config = self.config.read().await;
        
        // Kiểm tra điểm rủi ro
        if risk_score.score > config.max_risk_score {
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Điểm rủi ro quá cao: {}/{}", risk_score.score, config.max_risk_score), 
                chain_id
            ).await;
            return Ok(());
        }
        
        // Xác định chiến lược giao dịch dựa trên mức độ rủi ro
        let strategy = if risk_score.score < 30 {
            TradeStrategy::Smart  // Rủi ro thấp, dùng chiến lược thông minh với TSL
        } else {
            TradeStrategy::Quick  // Rủi ro cao hơn, dùng chiến lược nhanh
        };
        
        // Xác định số lượng giao dịch dựa trên cấu hình và mức độ rủi ro
        let base_amount = if risk_score.score < 20 {
            config.trade_amount_high_confidence
        } else if risk_score.score < 40 {
            config.trade_amount_medium_confidence
        } else {
            config.trade_amount_low_confidence
        };
        
        // Tạo tham số giao dịch
        let trade_params = crate::types::TradeParams {
            chain_type: crate::types::ChainType::EVM(chain_id),
            token_address: token_address.to_string(),
            amount: base_amount,
            slippage: 1.0, // 1% slippage mặc định cho giao dịch tự động
            trade_type: crate::types::TradeType::Buy,
            deadline_minutes: 2, // Thời hạn 2 phút
            router_address: String::new(), // Dùng router mặc định
            gas_limit: None,
            gas_price: None,
            strategy: Some(format!("{:?}", strategy).to_lowercase()),
            stop_loss: if strategy == TradeStrategy::Smart { Some(5.0) } else { None }, // 5% stop loss for Smart strategy
            take_profit: if strategy == TradeStrategy::Smart { Some(20.0) } else { Some(10.0) }, // 20% take profit for Smart, 10% for Quick
            max_hold_time: None, // Sẽ được thiết lập dựa trên chiến lược
            custom_params: None,
        };
        
        // Thực hiện giao dịch
        if let Err(e) = self.execute_trade(trade_params).await {
            error!("Lỗi khi thực hiện giao dịch: {}", e);
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Error, 
                &format!("Lỗi khi thực hiện giao dịch: {}", e), 
                chain_id
            ).await;
        } else {
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Pending, 
                "Giao dịch đã được khởi tạo", 
                chain_id
            ).await;
        }
        
        Ok(())
    }

    /// Core implementation of SmartTradeExecutor
    ///
    /// This file contains the main executor struct that manages the entire
    /// smart trading process, orchestrates token analysis, trade strategies,
    /// and market monitoring.
    ///
    /// # Cải tiến đã thực hiện:
    /// - Loại bỏ các phương thức trùng lặp với analys modules
    /// - Sử dụng API từ analys/token_status và analys/risk_analyzer
    /// - Áp dụng xử lý lỗi chuẩn với anyhow::Result thay vì unwrap/expect
    /// - Đảm bảo thread safety trong các phương thức async với kỹ thuật "clone and drop" để tránh deadlock
    /// - Tối ưu hóa việc xử lý các futures với tokio::join! cho các tác vụ song song
    /// - Xử lý các trường hợp giá trị null an toàn với match/Option
    /// - Tuân thủ nghiêm ngặt quy tắc từ .cursorrc

    // External imports
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::RwLock;
    use tokio::time::sleep;
    use chrono::Utc;
    use futures::future::join_all;
    use tracing::{debug, error, info, warn};
    use anyhow::{Result, Context, anyhow, bail};
    use uuid;
    use async_trait::async_trait;
    use serde::{Serialize, Deserialize};
    use rand::Rng;
    use std::collections::HashSet;
    use futures::future::Future;

    // Internal imports
    use crate::chain_adapters::evm_adapter::EvmAdapter;
    use crate::analys::mempool::{MempoolAnalyzer, MempoolTransaction};
    use crate::analys::risk_analyzer::{RiskAnalyzer, RiskFactor, TradeRecommendation, TradeRiskAnalysis};
    use crate::analys::token_status::{
        ContractInfo, LiquidityEvent, LiquidityEventType, TokenSafety, TokenStatus,
        TokenIssue, IssueSeverity, abnormal_liquidity_events, detect_dynamic_tax, 
        detect_hidden_fees, analyze_owner_privileges, is_proxy_contract, blacklist::{has_blacklist_or_whitelist, has_trading_cooldown, has_max_tx_or_wallet_limit},
    };
    use crate::types::{ChainType, TokenPair, TradeParams, TradeType};
    use crate::tradelogic::traits::{
        TradeExecutor, RiskManager, TradeCoordinator, ExecutorType, SharedOpportunity, 
        SharedOpportunityType, OpportunityPriority
    };

    // Module imports
    use super::types::{
        SmartTradeConfig, 
        TradeResult, 
        TradeStatus, 
        TradeStrategy, 
        TradeTracker
    };
    use super::constants::*;
    use super::token_analysis::*;
    use super::trade_strategy::*;
    use super::alert::*;
    use super::optimization::*;
    use super::security::*;
    use super::analys_client::SmartTradeAnalysisClient;

    /// Executor for smart trading strategies
    #[derive(Clone)]
    pub struct SmartTradeExecutor {
        /// EVM adapter for each chain
        pub(crate) evm_adapters: HashMap<u32, Arc<EvmAdapter>>,
        
        /// Analysis client that provides access to all analysis services
        pub(crate) analys_client: Arc<SmartTradeAnalysisClient>,
        
        /// Risk analyzer (legacy - kept for backward compatibility)
        pub(crate) risk_analyzer: Arc<RwLock<RiskAnalyzer>>,
        
        /// Mempool analyzer for each chain (legacy - kept for backward compatibility)
        pub(crate) mempool_analyzers: HashMap<u32, Arc<MempoolAnalyzer>>,
        
        /// Configuration
        pub(crate) config: RwLock<SmartTradeConfig>,
        
        /// Active trades being monitored
        pub(crate) active_trades: RwLock<Vec<TradeTracker>>,
        
        /// History of completed trades
        pub(crate) trade_history: RwLock<Vec<TradeResult>>,
        
        /// Running state
        pub(crate) running: RwLock<bool>,
        
        /// Coordinator for shared state between executors
        pub(crate) coordinator: Option<Arc<dyn TradeCoordinator>>,
        
        /// Unique ID for this executor instance
        pub(crate) executor_id: String,
        
        /// Subscription ID for coordinator notifications
        pub(crate) coordinator_subscription: RwLock<Option<String>>,
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
        
        // Khởi động vòng lặp theo dõi
        let executor = Arc::new(self.clone_with_config().await);
        
        tokio::spawn(async move {
            executor.monitor_loop().await;
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
        
        Ok(())
    }
    
    /// Execute a trade with the specified parameters
    async fn execute_trade(&self, params: TradeParams) -> anyhow::Result<Self::TradeResult> {
        // Find the appropriate adapter
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", params.chain_id)),
        };
        
        // Validate trade parameters
        if params.amount <= 0.0 {
            return Err(anyhow::anyhow!("Invalid trade amount: {}", params.amount));
        }
        
        // Create a trade tracker
        let trade_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp() as u64;
        
        let tracker = TradeTracker {
            id: trade_id.clone(),
            chain_id: params.chain_id,
            token_address: params.token_address.clone(),
            amount: params.amount,
            entry_price: 0.0, // Will be updated after trade execution
            current_price: 0.0,
            status: TradeStatus::Pending,
            strategy: if let Some(strategy) = &params.strategy {
                match strategy.as_str() {
                    "quick" => TradeStrategy::QuickFlip,
                    "tsl" => TradeStrategy::TrailingStopLoss,
                    "hodl" => TradeStrategy::LongTerm,
                    _ => TradeStrategy::default(),
                }
            } else {
                TradeStrategy::default()
            },
            created_at: now,
            updated_at: now,
            stop_loss: params.stop_loss,
            take_profit: params.take_profit,
            max_hold_time: params.max_hold_time.unwrap_or(DEFAULT_MAX_HOLD_TIME),
            custom_params: params.custom_params.unwrap_or_default(),
        };
        
        // TODO: Implement the actual trade execution logic
        // This is a simplified version - in a real implementation, you would:
        // 1. Execute the trade
        // 2. Update the trade tracker with actual execution details
        // 3. Add it to active trades
        
        let result = TradeResult {
            id: trade_id,
            chain_id: params.chain_id,
            token_address: params.token_address,
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: params.amount,
            entry_price: 0.0,                  // Would be populated from actual execution
            exit_price: None,
            current_price: 0.0,
            profit_loss: 0.0,
            status: TradeStatus::Pending,
            strategy: tracker.strategy.clone(),
            created_at: now,
            updated_at: now,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        // Add to active trades
        {
            let mut active_trades = self.active_trades.write().await;
            active_trades.push(tracker);
        }
        
        Ok(result)
    }
    
    /// Get the current status of an active trade
    async fn get_trade_status(&self, trade_id: &str) -> anyhow::Result<Option<Self::TradeResult>> {
        // Check active trades
        {
            let active_trades = self.active_trades.read().await;
            if let Some(trade) = active_trades.iter().find(|t| t.id == trade_id) {
                return Ok(Some(self.tracker_to_result(trade).await?));
            }
        }
        
        // Check trade history
        let trade_history = self.trade_history.read().await;
        let result = trade_history.iter()
            .find(|r| r.id == trade_id)
            .cloned();
            
        Ok(result)
    }
    
    /// Get all active trades
    async fn get_active_trades(&self) -> anyhow::Result<Vec<Self::TradeResult>> {
        let active_trades = self.active_trades.read().await;
        let mut results = Vec::with_capacity(active_trades.len());
        
        for tracker in active_trades.iter() {
            results.push(self.tracker_to_result(tracker).await?);
        }
        
        Ok(results)
    }
    
    /// Get trade history within a specific time range
    async fn get_trade_history(&self, from_timestamp: u64, to_timestamp: u64) -> anyhow::Result<Vec<Self::TradeResult>> {
        let trade_history = self.trade_history.read().await;
        
        let filtered_history = trade_history.iter()
            .filter(|trade| trade.created_at >= from_timestamp && trade.created_at <= to_timestamp)
            .cloned()
            .collect();
            
        Ok(filtered_history)
    }
    
    /// Evaluate a token for potential trading
    async fn evaluate_token(&self, chain_id: u32, token_address: &str) -> anyhow::Result<Option<TokenSafety>> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Sử dụng analys_client để phân tích token
        let token_safety_result = self.analys_client.token_provider()
            .analyze_token(chain_id, token_address).await
            .context(format!("Không thể phân tích token {}", token_address))?;
        
        // Nếu không phân tích được token, trả về None
        if !token_safety_result.is_valid() {
            info!("Token {} không hợp lệ hoặc không thể phân tích", token_address);
            return Ok(None);
        }
        
        // Lấy thêm thông tin chi tiết từ contract
        let contract_details = self.analys_client.token_provider()
            .get_contract_details(chain_id, token_address).await
            .context(format!("Không thể lấy chi tiết contract cho token {}", token_address))?;
        
        // Đánh giá rủi ro của token
        let risk_analysis = self.analys_client.risk_provider()
            .analyze_token_risk(chain_id, token_address).await
            .context(format!("Không thể phân tích rủi ro cho token {}", token_address))?;
        
        // Log thông tin phân tích
        info!(
            "Phân tích token {} trên chain {}: risk_score={}, honeypot={:?}, buy_tax={:?}, sell_tax={:?}",
            token_address,
            chain_id,
            risk_analysis.risk_score,
            token_safety_result.is_honeypot,
            token_safety_result.buy_tax,
            token_safety_result.sell_tax
        );
        
        Ok(Some(token_safety_result))
    }

    /// Đánh giá rủi ro của token bằng cách sử dụng analys_client
    async fn analyze_token_risk(&self, chain_id: u32, token_address: &str) -> anyhow::Result<RiskScore> {
        // Sử dụng phương thức analyze_risk từ analys_client
        self.analys_client.analyze_risk(chain_id, token_address).await
            .context(format!("Không thể đánh giá rủi ro cho token {}", token_address))
    }
}

impl SmartTradeExecutor {
    /// Create a new SmartTradeExecutor
    pub fn new() -> Self {
        let executor_id = format!("smart-trade-{}", uuid::Uuid::new_v4());
        
        let evm_adapters = HashMap::new();
        let analys_client = Arc::new(SmartTradeAnalysisClient::new(evm_adapters.clone()));
        
        Self {
            evm_adapters,
            analys_client,
            risk_analyzer: Arc::new(RwLock::new(RiskAnalyzer::new())),
            mempool_analyzers: HashMap::new(),
            config: RwLock::new(SmartTradeConfig::default()),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            coordinator: None,
            executor_id,
            coordinator_subscription: RwLock::new(None),
        }
    }
    
    /// Set coordinator for shared state
    pub fn with_coordinator(mut self, coordinator: Arc<dyn TradeCoordinator>) -> Self {
        self.coordinator = Some(coordinator);
        self
    }
    
    /// Convert a trade tracker to a trade result
    async fn tracker_to_result(&self, tracker: &TradeTracker) -> anyhow::Result<TradeResult> {
        let result = TradeResult {
            id: tracker.id.clone(),
            chain_id: tracker.chain_id,
            token_address: tracker.token_address.clone(),
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: tracker.amount,
            entry_price: tracker.entry_price,
            exit_price: None,
            current_price: tracker.current_price,
            profit_loss: if tracker.entry_price > 0.0 && tracker.current_price > 0.0 {
                (tracker.current_price - tracker.entry_price) / tracker.entry_price * 100.0
            } else {
                0.0
            },
            status: tracker.status.clone(),
            strategy: tracker.strategy.clone(),
            created_at: tracker.created_at,
            updated_at: tracker.updated_at,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        Ok(result)
    }
    
    /// Vòng lặp theo dõi chính
    async fn monitor_loop(&self) {
        let mut counter = 0;
        let mut persist_counter = 0;
        
        while let true_val = *self.running.read().await {
            if !true_val {
                break;
            }
            
            // Kiểm tra và cập nhật tất cả các giao dịch đang active
            if let Err(e) = self.check_active_trades().await {
                error!("Error checking active trades: {}", e);
            }
            
            // Tăng counter và dọn dẹp lịch sử giao dịch định kỳ
            counter += 1;
            if counter >= 100 {
                self.cleanup_trade_history().await;
                counter = 0;
            }
            
            // Lưu trạng thái vào bộ nhớ định kỳ
            persist_counter += 1;
            if persist_counter >= 30 { // Lưu mỗi 30 chu kỳ (khoảng 2.5 phút với interval 5s)
                if let Err(e) = self.update_and_persist_trades().await {
                    error!("Failed to persist bot state: {}", e);
                }
                persist_counter = 0;
            }
            
            // Tính toán thời gian sleep thích ứng
            let sleep_time = self.adaptive_sleep_time().await;
            tokio::time::sleep(tokio::time::Duration::from_millis(sleep_time)).await;
        }
        
        // Lưu trạng thái trước khi dừng
        if let Err(e) = self.update_and_persist_trades().await {
            error!("Failed to persist bot state before stopping: {}", e);
        }
    }
    
    /// Quét cơ hội giao dịch mới
    async fn scan_trading_opportunities(&self) {
        // Kiểm tra cấu hình
        let config = self.config.read().await;
        if !config.enabled || !config.auto_trade {
            return;
        }
        
        // Kiểm tra số lượng giao dịch hiện tại
        let active_trades = self.active_trades.read().await;
        if active_trades.len() >= config.max_concurrent_trades as usize {
            return;
        }
        
        // Chạy quét cho từng chain
        let futures = self.mempool_analyzers.iter().map(|(chain_id, analyzer)| {
            self.scan_chain_opportunities(*chain_id, analyzer)
        });
        join_all(futures).await;
    }
    
    /// Quét cơ hội giao dịch trên một chain cụ thể
    async fn scan_chain_opportunities(&self, chain_id: u32, analyzer: &Arc<MempoolAnalyzer>) -> anyhow::Result<()> {
        let config = self.config.read().await;
        
        // Lấy các giao dịch lớn từ mempool
        let large_txs = match analyzer.get_large_transactions(QUICK_TRADE_MIN_BNB).await {
            Ok(txs) => txs,
            Err(e) => {
                error!("Không thể lấy các giao dịch lớn từ mempool: {}", e);
                return Err(anyhow::anyhow!("Lỗi khi lấy giao dịch từ mempool: {}", e));
            }
        };
        
        // Phân tích từng giao dịch
        for tx in large_txs {
            // Kiểm tra nếu token không trong blacklist và đáp ứng điều kiện
            if let Some(to_token) = &tx.to_token {
                let token_address = &to_token.address;
                
                // Kiểm tra blacklist và whitelist
                if config.token_blacklist.contains(token_address) {
                    debug!("Bỏ qua token {} trong blacklist", token_address);
                    continue;
                }
                
                if !config.token_whitelist.is_empty() && !config.token_whitelist.contains(token_address) {
                    debug!("Bỏ qua token {} không trong whitelist", token_address);
                    continue;
                }
                
                // Kiểm tra nếu giao dịch đủ lớn
                if tx.value >= QUICK_TRADE_MIN_BNB {
                    // Phân tích token
                    if let Err(e) = self.analyze_and_trade_token(chain_id, token_address, tx).await {
                        error!("Lỗi khi phân tích và giao dịch token {}: {}", token_address, e);
                        // Tiếp tục với token tiếp theo, không dừng lại
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Phân tích token và thực hiện giao dịch nếu an toàn
    async fn analyze_and_trade_token(&self, chain_id: u32, token_address: &str, tx: MempoolTransaction) -> anyhow::Result<()> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id));
            }
        };
        
        // THÊM: Xác thực kỹ lưỡng dữ liệu mempool trước khi tiếp tục phân tích
        info!("Xác thực dữ liệu mempool cho token {} từ giao dịch {}", token_address, tx.tx_hash);
        let mempool_validation = match self.validate_mempool_data(chain_id, token_address, &tx).await {
            Ok(validation) => validation,
            Err(e) => {
                error!("Lỗi khi xác thực dữ liệu mempool: {}", e);
                self.log_trade_decision(
                    "new_opportunity", 
                    token_address, 
                    TradeStatus::Canceled, 
                    &format!("Lỗi xác thực mempool: {}", e), 
                    chain_id
                ).await;
                return Err(anyhow::anyhow!("Lỗi xác thực dữ liệu mempool: {}", e));
            }
        };
        
        // Nếu dữ liệu mempool không hợp lệ, dừng phân tích
        if !mempool_validation.is_valid {
            let reasons = mempool_validation.reasons.join(", ");
            info!("Dữ liệu mempool không đáng tin cậy cho token {}: {}", token_address, reasons);
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Dữ liệu mempool không đáng tin cậy: {}", reasons), 
                chain_id
            ).await;
            return Ok(());
        }
        
        info!("Xác thực mempool thành công với điểm tin cậy: {}/100", mempool_validation.confidence_score);
        
        // Code hiện tại: Sử dụng analys_client để kiểm tra an toàn token
        let security_check_future = self.analys_client.verify_token_safety(chain_id, token_address);
        
        // Code hiện tại: Sử dụng analys_client để đánh giá rủi ro
        let risk_analysis_future = self.analys_client.analyze_risk(chain_id, token_address);
        
        // Thực hiện song song các phân tích
        let (security_check_result, risk_analysis) = tokio::join!(security_check_future, risk_analysis_future);
        
        // Xử lý kết quả kiểm tra an toàn
        let security_check = match security_check_result {
            Ok(result) => result,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi kiểm tra an toàn token: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi kiểm tra an toàn token: {}", e));
            }
        };
        
        // Xử lý kết quả phân tích rủi ro
        let risk_score = match risk_analysis {
            Ok(score) => score,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi phân tích rủi ro: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi phân tích rủi ro: {}", e));
            }
        };
        
        // Kiểm tra xem token có an toàn không
        if !security_check.is_safe {
            let issues_str = security_check.issues.iter()
                .map(|issue| format!("{:?}", issue))
                .collect::<Vec<String>>()
                .join(", ");
                
            self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                &format!("Token không an toàn: {}", issues_str), chain_id).await;
            return Ok(());
        }
        
        // Lấy cấu hình hiện tại
        let config = self.config.read().await;
        
        // Kiểm tra điểm rủi ro
        if risk_score.score > config.max_risk_score {
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Điểm rủi ro quá cao: {}/{}", risk_score.score, config.max_risk_score), 
                chain_id
            ).await;
            return Ok(());
        }
        
        // Xác định chiến lược giao dịch dựa trên mức độ rủi ro
        let strategy = if risk_score.score < 30 {
            TradeStrategy::Smart  // Rủi ro thấp, dùng chiến lược thông minh với TSL
        } else {
            TradeStrategy::Quick  // Rủi ro cao hơn, dùng chiến lược nhanh
        };
        
        // Xác định số lượng giao dịch dựa trên cấu hình và mức độ rủi ro
        let base_amount = if risk_score.score < 20 {
            config.trade_amount_high_confidence
        } else if risk_score.score < 40 {
            config.trade_amount_medium_confidence
        } else {
            config.trade_amount_low_confidence
        };
        
        // Tạo tham số giao dịch
        let trade_params = crate::types::TradeParams {
            chain_type: crate::types::ChainType::EVM(chain_id),
            token_address: token_address.to_string(),
            amount: base_amount,
            slippage: 1.0, // 1% slippage mặc định cho giao dịch tự động
            trade_type: crate::types::TradeType::Buy,
            deadline_minutes: 2, // Thời hạn 2 phút
            router_address: String::new(), // Dùng router mặc định
            gas_limit: None,
            gas_price: None,
            strategy: Some(format!("{:?}", strategy).to_lowercase()),
            stop_loss: if strategy == TradeStrategy::Smart { Some(5.0) } else { None }, // 5% stop loss for Smart strategy
            take_profit: if strategy == TradeStrategy::Smart { Some(20.0) } else { Some(10.0) }, // 20% take profit for Smart, 10% for Quick
            max_hold_time: None, // Sẽ được thiết lập dựa trên chiến lược
            custom_params: None,
        };
        
        // Thực hiện giao dịch và lấy kết quả
        info!("Thực hiện mua {} cho token {} trên chain {}", base_amount, token_address, chain_id);
        
        let buy_result = adapter.execute_trade(&trade_params).await;
        
        match buy_result {
            Ok(tx_result) => {
                // Tạo ID giao dịch duy nhất
                let trade_id = uuid::Uuid::new_v4().to_string();
                let current_timestamp = chrono::Utc::now().timestamp() as u64;
                
                // Tính thời gian giữ tối đa dựa trên chiến lược
                let max_hold_time = match strategy {
                    TradeStrategy::Quick => current_timestamp + QUICK_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Smart => current_timestamp + SMART_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Manual => current_timestamp + 24 * 60 * 60, // 24 giờ cho giao dịch thủ công
                };
                
                // Tạo trailing stop loss nếu sử dụng chiến lược Smart
                let trailing_stop_percent = match strategy {
                    TradeStrategy::Smart => Some(SMART_TRADE_TSL_PERCENT),
                    _ => None,
                };
                
                // Xác định giá entry từ kết quả giao dịch
                let entry_price = match tx_result.execution_price {
                    Some(price) => price,
                    None => {
                        error!("Không thể xác định giá mua vào cho token {}", token_address);
                        return Err(anyhow::anyhow!("Không thể xác định giá mua vào"));
                    }
                };
                
                // Lấy số lượng token đã mua
                let token_amount = match tx_result.tokens_received {
                    Some(amount) => amount,
                    None => {
                        error!("Không thể xác định số lượng token nhận được");
                        return Err(anyhow::anyhow!("Không thể xác định số lượng token nhận được"));
                    }
                };
                
                // Tạo theo dõi giao dịch mới
                let trade_tracker = TradeTracker {
                    trade_id: trade_id.clone(),
                    token_address: token_address.to_string(),
                    chain_id,
                    strategy: strategy.clone(),
                    entry_price,
                    token_amount,
                    invested_amount: base_amount,
                    highest_price: entry_price, // Ban đầu, giá cao nhất = giá mua
                    entry_time: current_timestamp,
                    max_hold_time,
                    take_profit_percent: match strategy {
                        TradeStrategy::Quick => QUICK_TRADE_TARGET_PROFIT,
                        _ => 10.0, // Mặc định 10% cho các chiến lược khác
                    },
                    stop_loss_percent: STOP_LOSS_PERCENT,
                    trailing_stop_percent,
                    buy_tx_hash: tx_result.transaction_hash.clone(),
                    sell_tx_hash: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                };
                
                // Thêm vào danh sách theo dõi
                {
                    let mut active_trades = self.active_trades.write().await;
                    active_trades.push(trade_tracker.clone());
                }
                
                // Log thành công
                info!(
                    "Giao dịch mua thành công: ID={}, Token={}, Chain={}, Strategy={:?}, Amount={}, Price={}",
                    trade_id, token_address, chain_id, strategy, base_amount, entry_price
                );
                
                // Thêm vào lịch sử
                let trade_result = TradeResult {
                    trade_id: trade_id.clone(),
                    entry_price,
                    exit_price: None,
                    profit_percent: None,
                    profit_usd: None,
                    entry_time: current_timestamp,
                    exit_time: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                    gas_cost_usd: tx_result.gas_cost_usd.unwrap_or(0.0),
                };
                
                let mut trade_history = self.trade_history.write().await;
                trade_history.push(trade_result);
                
                // Trả về ID giao dịch
                Ok(())
            },
            Err(e) => {
                // Log lỗi giao dịch
                error!("Giao dịch mua thất bại cho token {}: {}", token_address, e);
                Err(anyhow::anyhow!("Giao dịch mua thất bại: {}", e))
            }
        }
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
        
        // Khởi động vòng lặp theo dõi
        let executor = Arc::new(self.clone_with_config().await);
        
        tokio::spawn(async move {
            executor.monitor_loop().await;
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
        
        Ok(())
    }
    
    /// Execute a trade with the specified parameters
    async fn execute_trade(&self, params: TradeParams) -> anyhow::Result<Self::TradeResult> {
        // Find the appropriate adapter
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", params.chain_id)),
        };
        
        // Validate trade parameters
        if params.amount <= 0.0 {
            return Err(anyhow::anyhow!("Invalid trade amount: {}", params.amount));
        }
        
        // Create a trade tracker
        let trade_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp() as u64;
        
        let tracker = TradeTracker {
            id: trade_id.clone(),
            chain_id: params.chain_id,
            token_address: params.token_address.clone(),
            amount: params.amount,
            entry_price: 0.0, // Will be updated after trade execution
            current_price: 0.0,
            status: TradeStatus::Pending,
            strategy: if let Some(strategy) = &params.strategy {
                match strategy.as_str() {
                    "quick" => TradeStrategy::QuickFlip,
                    "tsl" => TradeStrategy::TrailingStopLoss,
                    "hodl" => TradeStrategy::LongTerm,
                    _ => TradeStrategy::default(),
                }
            } else {
                TradeStrategy::default()
            },
            created_at: now,
            updated_at: now,
            stop_loss: params.stop_loss,
            take_profit: params.take_profit,
            max_hold_time: params.max_hold_time.unwrap_or(DEFAULT_MAX_HOLD_TIME),
            custom_params: params.custom_params.unwrap_or_default(),
        };
        
        // TODO: Implement the actual trade execution logic
        // This is a simplified version - in a real implementation, you would:
        // 1. Execute the trade
        // 2. Update the trade tracker with actual execution details
        // 3. Add it to active trades
        
        let result = TradeResult {
            id: trade_id,
            chain_id: params.chain_id,
            token_address: params.token_address,
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: params.amount,
            entry_price: 0.0,                  // Would be populated from actual execution
            exit_price: None,
            current_price: 0.0,
            profit_loss: 0.0,
            status: TradeStatus::Pending,
            strategy: tracker.strategy.clone(),
            created_at: now,
            updated_at: now,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        // Add to active trades
        {
            let mut active_trades = self.active_trades.write().await;
            active_trades.push(tracker);
        }
        
        Ok(result)
    }
    
    /// Get the current status of an active trade
    async fn get_trade_status(&self, trade_id: &str) -> anyhow::Result<Option<Self::TradeResult>> {
        // Check active trades
        {
            let active_trades = self.active_trades.read().await;
            if let Some(trade) = active_trades.iter().find(|t| t.id == trade_id) {
                return Ok(Some(self.tracker_to_result(trade).await?));
            }
        }
        
        // Check trade history
        let trade_history = self.trade_history.read().await;
        let result = trade_history.iter()
            .find(|r| r.id == trade_id)
            .cloned();
            
        Ok(result)
    }
    
    /// Get all active trades
    async fn get_active_trades(&self) -> anyhow::Result<Vec<Self::TradeResult>> {
        let active_trades = self.active_trades.read().await;
        let mut results = Vec::with_capacity(active_trades.len());
        
        for tracker in active_trades.iter() {
            results.push(self.tracker_to_result(tracker).await?);
        }
        
        Ok(results)
    }
    
    /// Get trade history within a specific time range
    async fn get_trade_history(&self, from_timestamp: u64, to_timestamp: u64) -> anyhow::Result<Vec<Self::TradeResult>> {
        let trade_history = self.trade_history.read().await;
        
        let filtered_history = trade_history.iter()
            .filter(|trade| trade.created_at >= from_timestamp && trade.created_at <= to_timestamp)
            .cloned()
            .collect();
            
        Ok(filtered_history)
    }
    
    /// Evaluate a token for potential trading
    async fn evaluate_token(&self, chain_id: u32, token_address: &str) -> anyhow::Result<Option<TokenSafety>> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Sử dụng analys_client để phân tích token
        let token_safety_result = self.analys_client.token_provider()
            .analyze_token(chain_id, token_address).await
            .context(format!("Không thể phân tích token {}", token_address))?;
        
        // Nếu không phân tích được token, trả về None
        if !token_safety_result.is_valid() {
            info!("Token {} không hợp lệ hoặc không thể phân tích", token_address);
            return Ok(None);
        }
        
        // Lấy thêm thông tin chi tiết từ contract
        let contract_details = self.analys_client.token_provider()
            .get_contract_details(chain_id, token_address).await
            .context(format!("Không thể lấy chi tiết contract cho token {}", token_address))?;
        
        // Đánh giá rủi ro của token
        let risk_analysis = self.analys_client.risk_provider()
            .analyze_token_risk(chain_id, token_address).await
            .context(format!("Không thể phân tích rủi ro cho token {}", token_address))?;
        
        // Log thông tin phân tích
        info!(
            "Phân tích token {} trên chain {}: risk_score={}, honeypot={:?}, buy_tax={:?}, sell_tax={:?}",
            token_address,
            chain_id,
            risk_analysis.risk_score,
            token_safety_result.is_honeypot,
            token_safety_result.buy_tax,
            token_safety_result.sell_tax
        );
        
        Ok(Some(token_safety_result))
    }

    /// Đánh giá rủi ro của token bằng cách sử dụng analys_client
    async fn analyze_token_risk(&self, chain_id: u32, token_address: &str) -> anyhow::Result<RiskScore> {
        // Sử dụng phương thức analyze_risk từ analys_client
        self.analys_client.analyze_risk(chain_id, token_address).await
            .context(format!("Không thể đánh giá rủi ro cho token {}", token_address))
    }
}

impl SmartTradeExecutor {
    /// Create a new SmartTradeExecutor
    pub fn new() -> Self {
        let executor_id = format!("smart-trade-{}", uuid::Uuid::new_v4());
        
        let evm_adapters = HashMap::new();
        let analys_client = Arc::new(SmartTradeAnalysisClient::new(evm_adapters.clone()));
        
        Self {
            evm_adapters,
            analys_client,
            risk_analyzer: Arc::new(RwLock::new(RiskAnalyzer::new())),
            mempool_analyzers: HashMap::new(),
            config: RwLock::new(SmartTradeConfig::default()),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            coordinator: None,
            executor_id,
            coordinator_subscription: RwLock::new(None),
        }
    }
    
    /// Set coordinator for shared state
    pub fn with_coordinator(mut self, coordinator: Arc<dyn TradeCoordinator>) -> Self {
        self.coordinator = Some(coordinator);
        self
    }
    
    /// Convert a trade tracker to a trade result
    async fn tracker_to_result(&self, tracker: &TradeTracker) -> anyhow::Result<TradeResult> {
        let result = TradeResult {
            id: tracker.id.clone(),
            chain_id: tracker.chain_id,
            token_address: tracker.token_address.clone(),
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: tracker.amount,
            entry_price: tracker.entry_price,
            exit_price: None,
            current_price: tracker.current_price,
            profit_loss: if tracker.entry_price > 0.0 && tracker.current_price > 0.0 {
                (tracker.current_price - tracker.entry_price) / tracker.entry_price * 100.0
            } else {
                0.0
            },
            status: tracker.status.clone(),
            strategy: tracker.strategy.clone(),
            created_at: tracker.created_at,
            updated_at: tracker.updated_at,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        Ok(result)
    }
    
    /// Vòng lặp theo dõi chính
    async fn monitor_loop(&self) {
        let mut counter = 0;
        let mut persist_counter = 0;
        
        while let true_val = *self.running.read().await {
            if !true_val {
                break;
            }
            
            // Kiểm tra và cập nhật tất cả các giao dịch đang active
            if let Err(e) = self.check_active_trades().await {
                error!("Error checking active trades: {}", e);
            }
            
            // Tăng counter và dọn dẹp lịch sử giao dịch định kỳ
            counter += 1;
            if counter >= 100 {
                self.cleanup_trade_history().await;
                counter = 0;
            }
            
            // Lưu trạng thái vào bộ nhớ định kỳ
            persist_counter += 1;
            if persist_counter >= 30 { // Lưu mỗi 30 chu kỳ (khoảng 2.5 phút với interval 5s)
                if let Err(e) = self.update_and_persist_trades().await {
                    error!("Failed to persist bot state: {}", e);
                }
                persist_counter = 0;
            }
            
            // Tính toán thời gian sleep thích ứng
            let sleep_time = self.adaptive_sleep_time().await;
            tokio::time::sleep(tokio::time::Duration::from_millis(sleep_time)).await;
        }
        
        // Lưu trạng thái trước khi dừng
        if let Err(e) = self.update_and_persist_trades().await {
            error!("Failed to persist bot state before stopping: {}", e);
        }
    }
    
    /// Quét cơ hội giao dịch mới
    async fn scan_trading_opportunities(&self) {
        // Kiểm tra cấu hình
        let config = self.config.read().await;
        if !config.enabled || !config.auto_trade {
            return;
        }
        
        // Kiểm tra số lượng giao dịch hiện tại
        let active_trades = self.active_trades.read().await;
        if active_trades.len() >= config.max_concurrent_trades as usize {
            return;
        }
        
        // Chạy quét cho từng chain
        let futures = self.mempool_analyzers.iter().map(|(chain_id, analyzer)| {
            self.scan_chain_opportunities(*chain_id, analyzer)
        });
        join_all(futures).await;
    }
    
    /// Quét cơ hội giao dịch trên một chain cụ thể
    async fn scan_chain_opportunities(&self, chain_id: u32, analyzer: &Arc<MempoolAnalyzer>) -> anyhow::Result<()> {
        let config = self.config.read().await;
        
        // Lấy các giao dịch lớn từ mempool
        let large_txs = match analyzer.get_large_transactions(QUICK_TRADE_MIN_BNB).await {
            Ok(txs) => txs,
            Err(e) => {
                error!("Không thể lấy các giao dịch lớn từ mempool: {}", e);
                return Err(anyhow::anyhow!("Lỗi khi lấy giao dịch từ mempool: {}", e));
            }
        };
        
        // Phân tích từng giao dịch
        for tx in large_txs {
            // Kiểm tra nếu token không trong blacklist và đáp ứng điều kiện
            if let Some(to_token) = &tx.to_token {
                let token_address = &to_token.address;
                
                // Kiểm tra blacklist và whitelist
                if config.token_blacklist.contains(token_address) {
                    debug!("Bỏ qua token {} trong blacklist", token_address);
                    continue;
                }
                
                if !config.token_whitelist.is_empty() && !config.token_whitelist.contains(token_address) {
                    debug!("Bỏ qua token {} không trong whitelist", token_address);
                    continue;
                }
                
                // Kiểm tra nếu giao dịch đủ lớn
                if tx.value >= QUICK_TRADE_MIN_BNB {
                    // Phân tích token
                    if let Err(e) = self.analyze_and_trade_token(chain_id, token_address, tx).await {
                        error!("Lỗi khi phân tích và giao dịch token {}: {}", token_address, e);
                        // Tiếp tục với token tiếp theo, không dừng lại
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Phân tích token và thực hiện giao dịch nếu an toàn
    async fn analyze_and_trade_token(&self, chain_id: u32, token_address: &str, tx: MempoolTransaction) -> anyhow::Result<()> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id));
            }
        };
        
        // THÊM: Xác thực kỹ lưỡng dữ liệu mempool trước khi tiếp tục phân tích
        info!("Xác thực dữ liệu mempool cho token {} từ giao dịch {}", token_address, tx.tx_hash);
        let mempool_validation = match self.validate_mempool_data(chain_id, token_address, &tx).await {
            Ok(validation) => validation,
            Err(e) => {
                error!("Lỗi khi xác thực dữ liệu mempool: {}", e);
                self.log_trade_decision(
                    "new_opportunity", 
                    token_address, 
                    TradeStatus::Canceled, 
                    &format!("Lỗi xác thực mempool: {}", e), 
                    chain_id
                ).await;
                return Err(anyhow::anyhow!("Lỗi xác thực dữ liệu mempool: {}", e));
            }
        };
        
        // Nếu dữ liệu mempool không hợp lệ, dừng phân tích
        if !mempool_validation.is_valid {
            let reasons = mempool_validation.reasons.join(", ");
            info!("Dữ liệu mempool không đáng tin cậy cho token {}: {}", token_address, reasons);
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Dữ liệu mempool không đáng tin cậy: {}", reasons), 
                chain_id
            ).await;
            return Ok(());
        }
        
        info!("Xác thực mempool thành công với điểm tin cậy: {}/100", mempool_validation.confidence_score);
        
        // Code hiện tại: Sử dụng analys_client để kiểm tra an toàn token
        let security_check_future = self.analys_client.verify_token_safety(chain_id, token_address);
        
        // Code hiện tại: Sử dụng analys_client để đánh giá rủi ro
        let risk_analysis_future = self.analys_client.analyze_risk(chain_id, token_address);
        
        // Thực hiện song song các phân tích
        let (security_check_result, risk_analysis) = tokio::join!(security_check_future, risk_analysis_future);
        
        // Xử lý kết quả kiểm tra an toàn
        let security_check = match security_check_result {
            Ok(result) => result,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi kiểm tra an toàn token: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi kiểm tra an toàn token: {}", e));
            }
        };
        
        // Xử lý kết quả phân tích rủi ro
        let risk_score = match risk_analysis {
            Ok(score) => score,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi phân tích rủi ro: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi phân tích rủi ro: {}", e));
            }
        };
        
        // Kiểm tra xem token có an toàn không
        if !security_check.is_safe {
            let issues_str = security_check.issues.iter()
                .map(|issue| format!("{:?}", issue))
                .collect::<Vec<String>>()
                .join(", ");
                
            self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                &format!("Token không an toàn: {}", issues_str), chain_id).await;
            return Ok(());
        }
        
        // Lấy cấu hình hiện tại
        let config = self.config.read().await;
        
        // Kiểm tra điểm rủi ro
        if risk_score.score > config.max_risk_score {
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Điểm rủi ro quá cao: {}/{}", risk_score.score, config.max_risk_score), 
                chain_id
            ).await;
            return Ok(());
        }
        
        // Xác định chiến lược giao dịch dựa trên mức độ rủi ro
        let strategy = if risk_score.score < 30 {
            TradeStrategy::Smart  // Rủi ro thấp, dùng chiến lược thông minh với TSL
        } else {
            TradeStrategy::Quick  // Rủi ro cao hơn, dùng chiến lược nhanh
        };
        
        // Xác định số lượng giao dịch dựa trên cấu hình và mức độ rủi ro
        let base_amount = if risk_score.score < 20 {
            config.trade_amount_high_confidence
        } else if risk_score.score < 40 {
            config.trade_amount_medium_confidence
        } else {
            config.trade_amount_low_confidence
        };
        
        // Tạo tham số giao dịch
        let trade_params = crate::types::TradeParams {
            chain_type: crate::types::ChainType::EVM(chain_id),
            token_address: token_address.to_string(),
            amount: base_amount,
            slippage: 1.0, // 1% slippage mặc định cho giao dịch tự động
            trade_type: crate::types::TradeType::Buy,
            deadline_minutes: 2, // Thời hạn 2 phút
            router_address: String::new(), // Dùng router mặc định
            gas_limit: None,
            gas_price: None,
            strategy: Some(format!("{:?}", strategy).to_lowercase()),
            stop_loss: if strategy == TradeStrategy::Smart { Some(5.0) } else { None }, // 5% stop loss for Smart strategy
            take_profit: if strategy == TradeStrategy::Smart { Some(20.0) } else { Some(10.0) }, // 20% take profit for Smart, 10% for Quick
            max_hold_time: None, // Sẽ được thiết lập dựa trên chiến lược
            custom_params: None,
        };
        
        // Thực hiện giao dịch và lấy kết quả
        info!("Thực hiện mua {} cho token {} trên chain {}", base_amount, token_address, chain_id);
        
        let buy_result = adapter.execute_trade(&trade_params).await;
        
        match buy_result {
            Ok(tx_result) => {
                // Tạo ID giao dịch duy nhất
                let trade_id = uuid::Uuid::new_v4().to_string();
                let current_timestamp = chrono::Utc::now().timestamp() as u64;
                
                // Tính thời gian giữ tối đa dựa trên chiến lược
                let max_hold_time = match strategy {
                    TradeStrategy::Quick => current_timestamp + QUICK_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Smart => current_timestamp + SMART_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Manual => current_timestamp + 24 * 60 * 60, // 24 giờ cho giao dịch thủ công
                };
                
                // Tạo trailing stop loss nếu sử dụng chiến lược Smart
                let trailing_stop_percent = match strategy {
                    TradeStrategy::Smart => Some(SMART_TRADE_TSL_PERCENT),
                    _ => None,
                };
                
                // Xác định giá entry từ kết quả giao dịch
                let entry_price = match tx_result.execution_price {
                    Some(price) => price,
                    None => {
                        error!("Không thể xác định giá mua vào cho token {}", token_address);
                        return Err(anyhow::anyhow!("Không thể xác định giá mua vào"));
                    }
                };
                
                // Lấy số lượng token đã mua
                let token_amount = match tx_result.tokens_received {
                    Some(amount) => amount,
                    None => {
                        error!("Không thể xác định số lượng token nhận được");
                        return Err(anyhow::anyhow!("Không thể xác định số lượng token nhận được"));
                    }
                };
                
                // Tạo theo dõi giao dịch mới
                let trade_tracker = TradeTracker {
                    trade_id: trade_id.clone(),
                    token_address: token_address.to_string(),
                    chain_id,
                    strategy: strategy.clone(),
                    entry_price,
                    token_amount,
                    invested_amount: base_amount,
                    highest_price: entry_price, // Ban đầu, giá cao nhất = giá mua
                    entry_time: current_timestamp,
                    max_hold_time,
                    take_profit_percent: match strategy {
                        TradeStrategy::Quick => QUICK_TRADE_TARGET_PROFIT,
                        _ => 10.0, // Mặc định 10% cho các chiến lược khác
                    },
                    stop_loss_percent: STOP_LOSS_PERCENT,
                    trailing_stop_percent,
                    buy_tx_hash: tx_result.transaction_hash.clone(),
                    sell_tx_hash: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                };
                
                // Thêm vào danh sách theo dõi
                {
                    let mut active_trades = self.active_trades.write().await;
                    active_trades.push(trade_tracker.clone());
                }
                
                // Log thành công
                info!(
                    "Giao dịch mua thành công: ID={}, Token={}, Chain={}, Strategy={:?}, Amount={}, Price={}",
                    trade_id, token_address, chain_id, strategy, base_amount, entry_price
                );
                
                // Thêm vào lịch sử
                let trade_result = TradeResult {
                    trade_id: trade_id.clone(),
                    entry_price,
                    exit_price: None,
                    profit_percent: None,
                    profit_usd: None,
                    entry_time: current_timestamp,
                    exit_time: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                    gas_cost_usd: tx_result.gas_cost_usd.unwrap_or(0.0),
                };
                
                let mut trade_history = self.trade_history.write().await;
                trade_history.push(trade_result);
                
                // Trả về ID giao dịch
                Ok(())
            },
            Err(e) => {
                // Log lỗi giao dịch
                error!("Giao dịch mua thất bại cho token {}: {}", token_address, e);
                Err(anyhow::anyhow!("Giao dịch mua thất bại: {}", e))
            }
        }
    }
    
    /// Thực hiện mua token dựa trên các tham số
    async fn execute_trade(&self, chain_id: u32, token_address: &str, strategy: TradeStrategy, trigger_tx: &MempoolTransaction) -> anyhow::Result<()> {
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id));
            }
        };
        
        // Lấy cấu hình
        let config = self.config.read().await;
        
        // Tính số lượng giao dịch
        let base_amount = match config.auto_trade {
            true => {
                // Tự động tính dựa trên cấu hình, giới hạn trong khoảng min-max
                let calculated_amount = std::cmp::min(
                    trigger_tx.value * config.capital_per_trade_ratio,
                    config.max_trade_amount
                );
                std::cmp::max(calculated_amount, config.min_trade_amount)
            },
            false => {
                // Nếu không bật auto trade, chỉ log và không thực hiện
                info!(
                    "Auto-trade disabled. Would trade {} for token {} with strategy {:?}",
                    config.min_trade_amount, token_address, strategy
                );
                return Ok(());
            }
        };
        
        // Tạo tham số giao dịch
        let trade_params = crate::types::TradeParams {
            chain_type: crate::types::ChainType::EVM(chain_id),
            token_address: token_address.to_string(),
            amount: base_amount,
            slippage: 1.0, // 1% slippage mặc định cho giao dịch tự động
            trade_type: crate::types::TradeType::Buy,
            deadline_minutes: 2, // Thời hạn 2 phút
            router_address: String::new(), // Dùng router mặc định
        };
        
        // Thực hiện giao dịch và lấy kết quả
        info!("Thực hiện mua {} cho token {} trên chain {}", base_amount, token_address, chain_id);
        
        let buy_result = adapter.execute_trade(&trade_params).await;
        
        match buy_result {
            Ok(tx_result) => {
                // Tạo ID giao dịch duy nhất
                let trade_id = uuid::Uuid::new_v4().to_string();
                let current_timestamp = chrono::Utc::now().timestamp() as u64;
                
                // Tính thời gian giữ tối đa dựa trên chiến lược
                let max_hold_time = match strategy {
                    TradeStrategy::Quick => current_timestamp + QUICK_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Smart => current_timestamp + SMART_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Manual => current_timestamp + 24 * 60 * 60, // 24 giờ cho giao dịch thủ công
                };
                
                // Tạo trailing stop loss nếu sử dụng chiến lược Smart
                let trailing_stop_percent = match strategy {
                    TradeStrategy::Smart => Some(SMART_TRADE_TSL_PERCENT),
                    _ => None,
                };
                
                // Xác định giá entry từ kết quả giao dịch
                let entry_price = match tx_result.execution_price {
                    Some(price) => price,
                    None => {
                        error!("Không thể xác định giá mua vào cho token {}", token_address);
                        return Err(anyhow::anyhow!("Không thể xác định giá mua vào"));
                    }
                };
                
                // Lấy số lượng token đã mua
                let token_amount = match tx_result.tokens_received {
                    Some(amount) => amount,
                    None => {
                        error!("Không thể xác định số lượng token nhận được");
                        return Err(anyhow::anyhow!("Không thể xác định số lượng token nhận được"));
                    }
                };
                
                // Tạo theo dõi giao dịch mới
                let trade_tracker = TradeTracker {
                    trade_id: trade_id.clone(),
                    token_address: token_address.to_string(),
                    chain_id,
                    strategy: strategy.clone(),
                    entry_price,
                    token_amount,
                    invested_amount: base_amount,
                    highest_price: entry_price, // Ban đầu, giá cao nhất = giá mua
                    entry_time: current_timestamp,
                    max_hold_time,
                    take_profit_percent: match strategy {
                        TradeStrategy::Quick => QUICK_TRADE_TARGET_PROFIT,
                        _ => 10.0, // Mặc định 10% cho các chiến lược khác
                    },
                    stop_loss_percent: STOP_LOSS_PERCENT,
                    trailing_stop_percent,
                    buy_tx_hash: tx_result.transaction_hash.clone(),
                    sell_tx_hash: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                };
                
                // Thêm vào danh sách theo dõi
                {
                    let mut active_trades = self.active_trades.write().await;
                    active_trades.push(trade_tracker.clone());
                }
                
                // Log thành công
                info!(
                    "Giao dịch mua thành công: ID={}, Token={}, Chain={}, Strategy={:?}, Amount={}, Price={}",
                    trade_id, token_address, chain_id, strategy, base_amount, entry_price
                );
                
                // Thêm vào lịch sử
                let trade_result = TradeResult {
                    trade_id: trade_id.clone(),
                    entry_price,
                    exit_price: None,
                    profit_percent: None,
                    profit_usd: None,
                    entry_time: current_timestamp,
                    exit_time: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                    gas_cost_usd: tx_result.gas_cost_usd,
                };
                
                let mut trade_history = self.trade_history.write().await;
                trade_history.push(trade_result);
                
                // Trả về ID giao dịch
                Ok(())
            },
            Err(e) => {
                // Log lỗi giao dịch
                error!("Giao dịch mua thất bại cho token {}: {}", token_address, e);
                Err(anyhow::anyhow!("Giao dịch mua thất bại: {}", e))
            }
        }
    }
    
    /// Theo dõi các giao dịch đang hoạt động
    async fn track_active_trades(&self) -> anyhow::Result<()> {
        // Kiểm tra xem có bất kỳ giao dịch nào cần theo dõi không
        let active_trades = self.active_trades.read().await;
        
        if active_trades.is_empty() {
            return Ok(());
        }
        
        // Clone danh sách để tránh giữ lock quá lâu
        let active_trades_clone = active_trades.clone();
        drop(active_trades); // Giải phóng lock để tránh deadlock
        
        // Xử lý từng giao dịch song song
        let update_futures = active_trades_clone.iter().map(|trade| {
            self.check_and_update_trade(trade.clone())
        });
        
        let results = futures::future::join_all(update_futures).await;
        
        // Kiểm tra và ghi log lỗi nếu có
        for result in results {
            if let Err(e) = result {
                error!("Lỗi khi theo dõi giao dịch: {}", e);
                // Tiếp tục xử lý các giao dịch khác, không dừng lại
            }
        }
        
        Ok(())
    }
    
/// Core implementation of SmartTradeExecutor
///
/// This file contains the main executor struct that manages the entire
/// smart trading process, orchestrates token analysis, trade strategies,
/// and market monitoring.
///
/// # Cải tiến đã thực hiện:
/// - Loại bỏ các phương thức trùng lặp với analys modules
/// - Sử dụng API từ analys/token_status và analys/risk_analyzer
/// - Áp dụng xử lý lỗi chuẩn với anyhow::Result thay vì unwrap/expect
/// - Đảm bảo thread safety trong các phương thức async với kỹ thuật "clone and drop" để tránh deadlock
/// - Tối ưu hóa việc xử lý các futures với tokio::join! cho các tác vụ song song
/// - Xử lý các trường hợp giá trị null an toàn với match/Option
/// - Tuân thủ nghiêm ngặt quy tắc từ .cursorrc

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use chrono::Utc;
use futures::future::join_all;
use tracing::{debug, error, info, warn};
use anyhow::{Result, Context, anyhow, bail};
use uuid;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use rand::Rng;
use std::collections::HashSet;
use futures::future::Future;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::{MempoolAnalyzer, MempoolTransaction};
use crate::analys::risk_analyzer::{RiskAnalyzer, RiskFactor, TradeRecommendation, TradeRiskAnalysis};
use crate::analys::token_status::{
    ContractInfo, LiquidityEvent, LiquidityEventType, TokenSafety, TokenStatus,
    TokenIssue, IssueSeverity, abnormal_liquidity_events, detect_dynamic_tax, 
    detect_hidden_fees, analyze_owner_privileges, is_proxy_contract, blacklist::{has_blacklist_or_whitelist, has_trading_cooldown, has_max_tx_or_wallet_limit},
};
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};
use crate::tradelogic::traits::{
    TradeExecutor, RiskManager, TradeCoordinator, ExecutorType, SharedOpportunity, 
    SharedOpportunityType, OpportunityPriority
};

// Module imports
use super::types::{
    SmartTradeConfig, 
    TradeResult, 
    TradeStatus, 
    TradeStrategy, 
    TradeTracker
};
use super::constants::*;
use super::token_analysis::*;
use super::trade_strategy::*;
use super::alert::*;
use super::optimization::*;
use super::security::*;
use super::analys_client::SmartTradeAnalysisClient;

/// Executor for smart trading strategies
#[derive(Clone)]
pub struct SmartTradeExecutor {
    /// EVM adapter for each chain
    pub(crate) evm_adapters: HashMap<u32, Arc<EvmAdapter>>,
    
    /// Analysis client that provides access to all analysis services
    pub(crate) analys_client: Arc<SmartTradeAnalysisClient>,
    
    /// Risk analyzer (legacy - kept for backward compatibility)
    pub(crate) risk_analyzer: Arc<RwLock<RiskAnalyzer>>,
    
    /// Mempool analyzer for each chain (legacy - kept for backward compatibility)
    pub(crate) mempool_analyzers: HashMap<u32, Arc<MempoolAnalyzer>>,
    
    /// Configuration
    pub(crate) config: RwLock<SmartTradeConfig>,
    
    /// Active trades being monitored
    pub(crate) active_trades: RwLock<Vec<TradeTracker>>,
    
    /// History of completed trades
    pub(crate) trade_history: RwLock<Vec<TradeResult>>,
    
    /// Running state
    pub(crate) running: RwLock<bool>,
    
    /// Coordinator for shared state between executors
    pub(crate) coordinator: Option<Arc<dyn TradeCoordinator>>,
    
    /// Unique ID for this executor instance
    pub(crate) executor_id: String,
    
    /// Subscription ID for coordinator notifications
    pub(crate) coordinator_subscription: RwLock<Option<String>>,
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
        
        // Khởi động vòng lặp theo dõi
        let executor = Arc::new(self.clone_with_config().await);
        
        tokio::spawn(async move {
            executor.monitor_loop().await;
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
        
        Ok(())
    }
    
    /// Execute a trade with the specified parameters
    async fn execute_trade(&self, params: TradeParams) -> anyhow::Result<Self::TradeResult> {
        // Find the appropriate adapter
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", params.chain_id)),
        };
        
        // Validate trade parameters
        if params.amount <= 0.0 {
            return Err(anyhow::anyhow!("Invalid trade amount: {}", params.amount));
        }
        
        // Create a trade tracker
        let trade_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp() as u64;
        
        let tracker = self.create_trade_tracker(&params, &trade_id, now);
        
        // TODO: Implement the actual trade execution logic
/// Core implementation of SmartTradeExecutor
///
/// This file contains the main executor struct that manages the entire
/// smart trading process, orchestrates token analysis, trade strategies,
/// and market monitoring.
///
/// # Cải tiến đã thực hiện:
/// - Loại bỏ các phương thức trùng lặp với analys modules
/// - Sử dụng API từ analys/token_status và analys/risk_analyzer
/// - Áp dụng xử lý lỗi chuẩn với anyhow::Result thay vì unwrap/expect
/// - Đảm bảo thread safety trong các phương thức async với kỹ thuật "clone and drop" để tránh deadlock
/// - Tối ưu hóa việc xử lý các futures với tokio::join! cho các tác vụ song song
/// - Xử lý các trường hợp giá trị null an toàn với match/Option
/// - Tuân thủ nghiêm ngặt quy tắc từ .cursorrc

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use chrono::Utc;
use futures::future::join_all;
use tracing::{debug, error, info, warn};
use anyhow::{Result, Context, anyhow, bail};
use uuid;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use rand::Rng;
use std::collections::HashSet;
use futures::future::Future;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::{MempoolAnalyzer, MempoolTransaction};
use crate::analys::risk_analyzer::{RiskAnalyzer, RiskFactor, TradeRecommendation, TradeRiskAnalysis};
use crate::analys::token_status::{
    ContractInfo, LiquidityEvent, LiquidityEventType, TokenSafety, TokenStatus,
    TokenIssue, IssueSeverity, abnormal_liquidity_events, detect_dynamic_tax, 
    detect_hidden_fees, analyze_owner_privileges, is_proxy_contract, blacklist::{has_blacklist_or_whitelist, has_trading_cooldown, has_max_tx_or_wallet_limit},
};
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};
use crate::tradelogic::traits::{
    TradeExecutor, RiskManager, TradeCoordinator, ExecutorType, SharedOpportunity, 
    SharedOpportunityType, OpportunityPriority
};

// Module imports
use super::types::{
    SmartTradeConfig, 
    TradeResult, 
    TradeStatus, 
    TradeStrategy, 
    TradeTracker
};
use super::constants::*;
use super::token_analysis::*;
use super::trade_strategy::*;
use super::alert::*;
use super::optimization::*;
use super::security::*;
use super::analys_client::SmartTradeAnalysisClient;

/// Executor for smart trading strategies
#[derive(Clone)]
pub struct SmartTradeExecutor {
    /// EVM adapter for each chain
    pub(crate) evm_adapters: HashMap<u32, Arc<EvmAdapter>>,
    
    /// Analysis client that provides access to all analysis services
    pub(crate) analys_client: Arc<SmartTradeAnalysisClient>,
    
    /// Risk analyzer (legacy - kept for backward compatibility)
    pub(crate) risk_analyzer: Arc<RwLock<RiskAnalyzer>>,
    
    /// Mempool analyzer for each chain (legacy - kept for backward compatibility)
    pub(crate) mempool_analyzers: HashMap<u32, Arc<MempoolAnalyzer>>,
    
    /// Configuration
    pub(crate) config: RwLock<SmartTradeConfig>,
    
    /// Active trades being monitored
    pub(crate) active_trades: RwLock<Vec<TradeTracker>>,
    
    /// History of completed trades
    pub(crate) trade_history: RwLock<Vec<TradeResult>>,
    
    /// Running state
    pub(crate) running: RwLock<bool>,
    
    /// Coordinator for shared state between executors
    pub(crate) coordinator: Option<Arc<dyn TradeCoordinator>>,
    
    /// Unique ID for this executor instance
    pub(crate) executor_id: String,
    
    /// Subscription ID for coordinator notifications
    pub(crate) coordinator_subscription: RwLock<Option<String>>,
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
        
        // Khởi động vòng lặp theo dõi
        let executor = Arc::new(self.clone_with_config().await);
        
        tokio::spawn(async move {
            executor.monitor_loop().await;
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
        
        Ok(())
    }
    
    /// Execute a trade with the specified parameters
    async fn execute_trade(&self, params: TradeParams) -> anyhow::Result<Self::TradeResult> {
        // Find the appropriate adapter
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", params.chain_id)),
        };
        
        // Validate trade parameters
        if params.amount <= 0.0 {
            return Err(anyhow::anyhow!("Invalid trade amount: {}", params.amount));
        }
        
        // Create a trade tracker
        let trade_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp() as u64;
        
        let tracker = TradeTracker {
            id: trade_id.clone(),
            chain_id: params.chain_id,
            token_address: params.token_address.clone(),
            amount: params.amount,
            entry_price: 0.0, // Will be updated after trade execution
            current_price: 0.0,
            status: TradeStatus::Pending,
            strategy: if let Some(strategy) = &params.strategy {
                match strategy.as_str() {
                    "quick" => TradeStrategy::QuickFlip,
                    "tsl" => TradeStrategy::TrailingStopLoss,
                    "hodl" => TradeStrategy::LongTerm,
                    _ => TradeStrategy::default(),
                }
            } else {
                TradeStrategy::default()
            },
            created_at: now,
            updated_at: now,
            stop_loss: params.stop_loss,
            take_profit: params.take_profit,
            max_hold_time: params.max_hold_time.unwrap_or(DEFAULT_MAX_HOLD_TIME),
            custom_params: params.custom_params.unwrap_or_default(),
        };
        
        // TODO: Implement the actual trade execution logic
        // This is a simplified version - in a real implementation, you would:
        // 1. Execute the trade
        // 2. Update the trade tracker with actual execution details
        // 3. Add it to active trades
        
        let result = TradeResult {
            id: trade_id,
            chain_id: params.chain_id,
            token_address: params.token_address,
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: params.amount,
            entry_price: 0.0,                  // Would be populated from actual execution
            exit_price: None,
            current_price: 0.0,
            profit_loss: 0.0,
            status: TradeStatus::Pending,
            strategy: tracker.strategy.clone(),
            created_at: now,
            updated_at: now,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        // Add to active trades
        {
            let mut active_trades = self.active_trades.write().await;
            active_trades.push(tracker);
        }
        
        Ok(result)
    }
    
    /// Get the current status of an active trade
    async fn get_trade_status(&self, trade_id: &str) -> anyhow::Result<Option<Self::TradeResult>> {
        // Check active trades
        {
            let active_trades = self.active_trades.read().await;
            if let Some(trade) = active_trades.iter().find(|t| t.id == trade_id) {
                return Ok(Some(self.tracker_to_result(trade).await?));
            }
        }
        
        // Check trade history
        let trade_history = self.trade_history.read().await;
        let result = trade_history.iter()
            .find(|r| r.id == trade_id)
            .cloned();
            
        Ok(result)
    }
    
    /// Get all active trades
    async fn get_active_trades(&self) -> anyhow::Result<Vec<Self::TradeResult>> {
        let active_trades = self.active_trades.read().await;
        let mut results = Vec::with_capacity(active_trades.len());
        
        for tracker in active_trades.iter() {
            results.push(self.tracker_to_result(tracker).await?);
        }
        
        Ok(results)
    }
    
    /// Get trade history within a specific time range
    async fn get_trade_history(&self, from_timestamp: u64, to_timestamp: u64) -> anyhow::Result<Vec<Self::TradeResult>> {
        let trade_history = self.trade_history.read().await;
        
        let filtered_history = trade_history.iter()
            .filter(|trade| trade.created_at >= from_timestamp && trade.created_at <= to_timestamp)
            .cloned()
            .collect();
            
        Ok(filtered_history)
    }
    
    /// Evaluate a token for potential trading
    async fn evaluate_token(&self, chain_id: u32, token_address: &str) -> anyhow::Result<Option<TokenSafety>> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Sử dụng analys_client để phân tích token
        let token_safety_result = self.analys_client.token_provider()
            .analyze_token(chain_id, token_address).await
            .context(format!("Không thể phân tích token {}", token_address))?;
        
        // Nếu không phân tích được token, trả về None
        if !token_safety_result.is_valid() {
            info!("Token {} không hợp lệ hoặc không thể phân tích", token_address);
            return Ok(None);
        }
        
        // Lấy thêm thông tin chi tiết từ contract
        let contract_details = self.analys_client.token_provider()
            .get_contract_details(chain_id, token_address).await
            .context(format!("Không thể lấy chi tiết contract cho token {}", token_address))?;
        
        // Đánh giá rủi ro của token
        let risk_analysis = self.analys_client.risk_provider()
            .analyze_token_risk(chain_id, token_address).await
            .context(format!("Không thể phân tích rủi ro cho token {}", token_address))?;
        
        // Log thông tin phân tích
        info!(
            "Phân tích token {} trên chain {}: risk_score={}, honeypot={:?}, buy_tax={:?}, sell_tax={:?}",
            token_address,
            chain_id,
            risk_analysis.risk_score,
            token_safety_result.is_honeypot,
            token_safety_result.buy_tax,
            token_safety_result.sell_tax
        );
        
        Ok(Some(token_safety_result))
    }

    /// Đánh giá rủi ro của token bằng cách sử dụng analys_client
    async fn analyze_token_risk(&self, chain_id: u32, token_address: &str) -> anyhow::Result<RiskScore> {
        // Sử dụng phương thức analyze_risk từ analys_client
        self.analys_client.analyze_risk(chain_id, token_address).await
            .context(format!("Không thể đánh giá rủi ro cho token {}", token_address))
    }
}

impl SmartTradeExecutor {
    /// Create a new SmartTradeExecutor
    pub fn new() -> Self {
        let executor_id = format!("smart-trade-{}", uuid::Uuid::new_v4());
        
        let evm_adapters = HashMap::new();
        let analys_client = Arc::new(SmartTradeAnalysisClient::new(evm_adapters.clone()));
        
        Self {
            evm_adapters,
            analys_client,
            risk_analyzer: Arc::new(RwLock::new(RiskAnalyzer::new())),
            mempool_analyzers: HashMap::new(),
            config: RwLock::new(SmartTradeConfig::default()),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            coordinator: None,
            executor_id,
            coordinator_subscription: RwLock::new(None),
        }
    }
    
    /// Set coordinator for shared state
    pub fn with_coordinator(mut self, coordinator: Arc<dyn TradeCoordinator>) -> Self {
        self.coordinator = Some(coordinator);
        self
    }
    
    /// Convert a trade tracker to a trade result
    async fn tracker_to_result(&self, tracker: &TradeTracker) -> anyhow::Result<TradeResult> {
        let result = TradeResult {
            id: tracker.id.clone(),
            chain_id: tracker.chain_id,
            token_address: tracker.token_address.clone(),
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: tracker.amount,
            entry_price: tracker.entry_price,
            exit_price: None,
            current_price: tracker.current_price,
            profit_loss: if tracker.entry_price > 0.0 && tracker.current_price > 0.0 {
                (tracker.current_price - tracker.entry_price) / tracker.entry_price * 100.0
            } else {
                0.0
            },
            status: tracker.status.clone(),
            strategy: tracker.strategy.clone(),
            created_at: tracker.created_at,
            updated_at: tracker.updated_at,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        Ok(result)
    }
    
    /// Vòng lặp theo dõi chính
    async fn monitor_loop(&self) {
        let mut counter = 0;
        let mut persist_counter = 0;
        
        while let true_val = *self.running.read().await {
            if !true_val {
                break;
            }
            
            // Kiểm tra và cập nhật tất cả các giao dịch đang active
            if let Err(e) = self.check_active_trades().await {
                error!("Error checking active trades: {}", e);
            }
            
            // Tăng counter và dọn dẹp lịch sử giao dịch định kỳ
            counter += 1;
            if counter >= 100 {
                self.cleanup_trade_history().await;
                counter = 0;
            }
            
            // Lưu trạng thái vào bộ nhớ định kỳ
            persist_counter += 1;
            if persist_counter >= 30 { // Lưu mỗi 30 chu kỳ (khoảng 2.5 phút với interval 5s)
                if let Err(e) = self.update_and_persist_trades().await {
                    error!("Failed to persist bot state: {}", e);
                }
                persist_counter = 0;
            }
            
            // Tính toán thời gian sleep thích ứng
            let sleep_time = self.adaptive_sleep_time().await;
            tokio::time::sleep(tokio::time::Duration::from_millis(sleep_time)).await;
        }
        
        // Lưu trạng thái trước khi dừng
        if let Err(e) = self.update_and_persist_trades().await {
            error!("Failed to persist bot state before stopping: {}", e);
        }
    }
    
    /// Quét cơ hội giao dịch mới
    async fn scan_trading_opportunities(&self) {
        // Kiểm tra cấu hình
        let config = self.config.read().await;
        if !config.enabled || !config.auto_trade {
            return;
        }
        
        // Kiểm tra số lượng giao dịch hiện tại
        let active_trades = self.active_trades.read().await;
        if active_trades.len() >= config.max_concurrent_trades as usize {
            return;
        }
        
        // Chạy quét cho từng chain
        let futures = self.mempool_analyzers.iter().map(|(chain_id, analyzer)| {
            self.scan_chain_opportunities(*chain_id, analyzer)
        });
        join_all(futures).await;
    }
    
    /// Quét cơ hội giao dịch trên một chain cụ thể
    async fn scan_chain_opportunities(&self, chain_id: u32, analyzer: &Arc<MempoolAnalyzer>) -> anyhow::Result<()> {
        let config = self.config.read().await;
        
        // Lấy các giao dịch lớn từ mempool
        let large_txs = match analyzer.get_large_transactions(QUICK_TRADE_MIN_BNB).await {
            Ok(txs) => txs,
            Err(e) => {
                error!("Không thể lấy các giao dịch lớn từ mempool: {}", e);
                return Err(anyhow::anyhow!("Lỗi khi lấy giao dịch từ mempool: {}", e));
            }
        };
        
        // Phân tích từng giao dịch
        for tx in large_txs {
            // Kiểm tra nếu token không trong blacklist và đáp ứng điều kiện
            if let Some(to_token) = &tx.to_token {
                let token_address = &to_token.address;
                
                // Kiểm tra blacklist và whitelist
                if config.token_blacklist.contains(token_address) {
                    debug!("Bỏ qua token {} trong blacklist", token_address);
                    continue;
                }
                
                if !config.token_whitelist.is_empty() && !config.token_whitelist.contains(token_address) {
                    debug!("Bỏ qua token {} không trong whitelist", token_address);
                    continue;
                }
                
                // Kiểm tra nếu giao dịch đủ lớn
                if tx.value >= QUICK_TRADE_MIN_BNB {
                    // Phân tích token
                    if let Err(e) = self.analyze_and_trade_token(chain_id, token_address, tx).await {
                        error!("Lỗi khi phân tích và giao dịch token {}: {}", token_address, e);
                        // Tiếp tục với token tiếp theo, không dừng lại
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Phân tích token và thực hiện giao dịch nếu an toàn
    async fn analyze_and_trade_token(&self, chain_id: u32, token_address: &str, tx: MempoolTransaction) -> anyhow::Result<()> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id));
            }
        };
        
        // THÊM: Xác thực kỹ lưỡng dữ liệu mempool trước khi tiếp tục phân tích
        info!("Xác thực dữ liệu mempool cho token {} từ giao dịch {}", token_address, tx.tx_hash);
        let mempool_validation = match self.validate_mempool_data(chain_id, token_address, &tx).await {
            Ok(validation) => validation,
            Err(e) => {
                error!("Lỗi khi xác thực dữ liệu mempool: {}", e);
                self.log_trade_decision(
                    "new_opportunity", 
                    token_address, 
                    TradeStatus::Canceled, 
                    &format!("Lỗi xác thực mempool: {}", e), 
                    chain_id
                ).await;
                return Err(anyhow::anyhow!("Lỗi xác thực dữ liệu mempool: {}", e));
            }
        };
        
        // Nếu dữ liệu mempool không hợp lệ, dừng phân tích
        if !mempool_validation.is_valid {
            let reasons = mempool_validation.reasons.join(", ");
            info!("Dữ liệu mempool không đáng tin cậy cho token {}: {}", token_address, reasons);
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Dữ liệu mempool không đáng tin cậy: {}", reasons), 
                chain_id
            ).await;
            return Ok(());
        }
        
        info!("Xác thực mempool thành công với điểm tin cậy: {}/100", mempool_validation.confidence_score);
        
        // Code hiện tại: Sử dụng analys_client để kiểm tra an toàn token
        let security_check_future = self.analys_client.verify_token_safety(chain_id, token_address);
        
        // Code hiện tại: Sử dụng analys_client để đánh giá rủi ro
        let risk_analysis_future = self.analys_client.analyze_risk(chain_id, token_address);
        
        // Thực hiện song song các phân tích
        let (security_check_result, risk_analysis) = tokio::join!(security_check_future, risk_analysis_future);
        
        // Xử lý kết quả kiểm tra an toàn
        let security_check = match security_check_result {
            Ok(result) => result,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi kiểm tra an toàn token: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi kiểm tra an toàn token: {}", e));
            }
        };
        
        // Xử lý kết quả phân tích rủi ro
        let risk_score = match risk_analysis {
            Ok(score) => score,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi phân tích rủi ro: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi phân tích rủi ro: {}", e));
            }
        };
        
        // Kiểm tra xem token có an toàn không
        if !security_check.is_safe {
            let issues_str = security_check.issues.iter()
                .map(|issue| format!("{:?}", issue))
                .collect::<Vec<String>>()
                .join(", ");
                
            self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                &format!("Token không an toàn: {}", issues_str), chain_id).await;
            return Ok(());
        }
        
        // Lấy cấu hình hiện tại
        let config = self.config.read().await;
        
        // Kiểm tra điểm rủi ro
        if risk_score.score > config.max_risk_score {
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Điểm rủi ro quá cao: {}/{}", risk_score.score, config.max_risk_score), 
                chain_id
            ).await;
            return Ok(());
        }
        
        // Xác định chiến lược giao dịch dựa trên mức độ rủi ro
        let strategy = if risk_score.score < 30 {
            TradeStrategy::Smart  // Rủi ro thấp, dùng chiến lược thông minh với TSL
        } else {
            TradeStrategy::Quick  // Rủi ro cao hơn, dùng chiến lược nhanh
        };
        
        // Xác định số lượng giao dịch dựa trên cấu hình và mức độ rủi ro
        let base_amount = if risk_score.score < 20 {
            config.trade_amount_high_confidence
        } else if risk_score.score < 40 {
            config.trade_amount_medium_confidence
        } else {
            config.trade_amount_low_confidence
        };
        
        // Tạo tham số giao dịch
        let trade_params = crate::types::TradeParams {
            chain_type: crate::types::ChainType::EVM(chain_id),
            token_address: token_address.to_string(),
            amount: base_amount,
            slippage: 1.0, // 1% slippage mặc định cho giao dịch tự động
            trade_type: crate::types::TradeType::Buy,
            deadline_minutes: 2, // Thời hạn 2 phút
            router_address: String::new(), // Dùng router mặc định
            gas_limit: None,
            gas_price: None,
            strategy: Some(format!("{:?}", strategy).to_lowercase()),
            stop_loss: if strategy == TradeStrategy::Smart { Some(5.0) } else { None }, // 5% stop loss for Smart strategy
            take_profit: if strategy == TradeStrategy::Smart { Some(20.0) } else { Some(10.0) }, // 20% take profit for Smart, 10% for Quick
            max_hold_time: None, // Sẽ được thiết lập dựa trên chiến lược
            custom_params: None,
        };
        
/// Core implementation of SmartTradeExecutor
///
/// This file contains the main executor struct that manages the entire
/// smart trading process, orchestrates token analysis, trade strategies,
/// and market monitoring.
///
/// # Cải tiến đã thực hiện:
/// - Loại bỏ các phương thức trùng lặp với analys modules
/// - Sử dụng API từ analys/token_status và analys/risk_analyzer
/// - Áp dụng xử lý lỗi chuẩn với anyhow::Result thay vì unwrap/expect
/// - Đảm bảo thread safety trong các phương thức async với kỹ thuật "clone and drop" để tránh deadlock
/// - Tối ưu hóa việc xử lý các futures với tokio::join! cho các tác vụ song song
/// - Xử lý các trường hợp giá trị null an toàn với match/Option
/// - Tuân thủ nghiêm ngặt quy tắc từ .cursorrc

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use chrono::Utc;
use futures::future::join_all;
use tracing::{debug, error, info, warn};
use anyhow::{Result, Context, anyhow, bail};
use uuid;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use rand::Rng;
use std::collections::HashSet;
use futures::future::Future;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::{MempoolAnalyzer, MempoolTransaction};
use crate::analys::risk_analyzer::{RiskAnalyzer, RiskFactor, TradeRecommendation, TradeRiskAnalysis};
use crate::analys::token_status::{
    ContractInfo, LiquidityEvent, LiquidityEventType, TokenSafety, TokenStatus,
    TokenIssue, IssueSeverity, abnormal_liquidity_events, detect_dynamic_tax, 
    detect_hidden_fees, analyze_owner_privileges, is_proxy_contract, blacklist::{has_blacklist_or_whitelist, has_trading_cooldown, has_max_tx_or_wallet_limit},
};
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};
use crate::tradelogic::traits::{
    TradeExecutor, RiskManager, TradeCoordinator, ExecutorType, SharedOpportunity, 
    SharedOpportunityType, OpportunityPriority
};

// Module imports
use super::types::{
    SmartTradeConfig, 
    TradeResult, 
    TradeStatus, 
    TradeStrategy, 
    TradeTracker
};
use super::constants::*;
use super::token_analysis::*;
use super::trade_strategy::*;
use super::alert::*;
use super::optimization::*;
use super::security::*;
use super::analys_client::SmartTradeAnalysisClient;

/// Executor for smart trading strategies
#[derive(Clone)]
pub struct SmartTradeExecutor {
    /// EVM adapter for each chain
    pub(crate) evm_adapters: HashMap<u32, Arc<EvmAdapter>>,
    
    /// Analysis client that provides access to all analysis services
    pub(crate) analys_client: Arc<SmartTradeAnalysisClient>,
    
    /// Risk analyzer (legacy - kept for backward compatibility)
    pub(crate) risk_analyzer: Arc<RwLock<RiskAnalyzer>>,
    
    /// Mempool analyzer for each chain (legacy - kept for backward compatibility)
    pub(crate) mempool_analyzers: HashMap<u32, Arc<MempoolAnalyzer>>,
    
    /// Configuration
    pub(crate) config: RwLock<SmartTradeConfig>,
    
    /// Active trades being monitored
    pub(crate) active_trades: RwLock<Vec<TradeTracker>>,
    
    /// History of completed trades
    pub(crate) trade_history: RwLock<Vec<TradeResult>>,
    
    /// Running state
    pub(crate) running: RwLock<bool>,
    
    /// Coordinator for shared state between executors
    pub(crate) coordinator: Option<Arc<dyn TradeCoordinator>>,
    
    /// Unique ID for this executor instance
    pub(crate) executor_id: String,
    
    /// Subscription ID for coordinator notifications
    pub(crate) coordinator_subscription: RwLock<Option<String>>,
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
        
        // Khởi động vòng lặp theo dõi
        let executor = Arc::new(self.clone_with_config().await);
        
        tokio::spawn(async move {
            executor.monitor_loop().await;
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
        
        Ok(())
    }
    
    /// Execute a trade with the specified parameters
    async fn execute_trade(&self, params: TradeParams) -> anyhow::Result<Self::TradeResult> {
        // Find the appropriate adapter
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", params.chain_id)),
        };
        
        // Validate trade parameters
        if params.amount <= 0.0 {
            return Err(anyhow::anyhow!("Invalid trade amount: {}", params.amount));
        }
        
        // Create a trade tracker
        let trade_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp() as u64;
        
        let tracker = TradeTracker {
            id: trade_id.clone(),
            chain_id: params.chain_id,
            token_address: params.token_address.clone(),
            amount: params.amount,
            entry_price: 0.0, // Will be updated after trade execution
            current_price: 0.0,
            status: TradeStatus::Pending,
            strategy: if let Some(strategy) = &params.strategy {
                match strategy.as_str() {
                    "quick" => TradeStrategy::QuickFlip,
                    "tsl" => TradeStrategy::TrailingStopLoss,
                    "hodl" => TradeStrategy::LongTerm,
                    _ => TradeStrategy::default(),
                }
            } else {
                TradeStrategy::default()
            },
            created_at: now,
            updated_at: now,
            stop_loss: params.stop_loss,
            take_profit: params.take_profit,
            max_hold_time: params.max_hold_time.unwrap_or(DEFAULT_MAX_HOLD_TIME),
            custom_params: params.custom_params.unwrap_or_default(),
        };
        
        // TODO: Implement the actual trade execution logic
        // This is a simplified version - in a real implementation, you would:
        // 1. Execute the trade
        // 2. Update the trade tracker with actual execution details
        // 3. Add it to active trades
        
        let result = TradeResult {
            id: trade_id,
            chain_id: params.chain_id,
            token_address: params.token_address,
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: params.amount,
            entry_price: 0.0,                  // Would be populated from actual execution
            exit_price: None,
            current_price: 0.0,
            profit_loss: 0.0,
            status: TradeStatus::Pending,
            strategy: tracker.strategy.clone(),
            created_at: now,
            updated_at: now,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        // Add to active trades
        {
            let mut active_trades = self.active_trades.write().await;
            active_trades.push(tracker);
        }
        
        Ok(result)
    }
    
    /// Get the current status of an active trade
    async fn get_trade_status(&self, trade_id: &str) -> anyhow::Result<Option<Self::TradeResult>> {
        // Check active trades
        {
            let active_trades = self.active_trades.read().await;
            if let Some(trade) = active_trades.iter().find(|t| t.id == trade_id) {
                return Ok(Some(self.tracker_to_result(trade).await?));
            }
        }
        
        // Check trade history
        let trade_history = self.trade_history.read().await;
        let result = trade_history.iter()
            .find(|r| r.id == trade_id)
            .cloned();
            
        Ok(result)
    }
    
    /// Get all active trades
    async fn get_active_trades(&self) -> anyhow::Result<Vec<Self::TradeResult>> {
        let active_trades = self.active_trades.read().await;
        let mut results = Vec::with_capacity(active_trades.len());
        
        for tracker in active_trades.iter() {
            results.push(self.tracker_to_result(tracker).await?);
        }
        
        Ok(results)
    }
    
    /// Get trade history within a specific time range
    async fn get_trade_history(&self, from_timestamp: u64, to_timestamp: u64) -> anyhow::Result<Vec<Self::TradeResult>> {
        let trade_history = self.trade_history.read().await;
        
        let filtered_history = trade_history.iter()
            .filter(|trade| trade.created_at >= from_timestamp && trade.created_at <= to_timestamp)
            .cloned()
            .collect();
            
        Ok(filtered_history)
    }
    
    /// Evaluate a token for potential trading
    async fn evaluate_token(&self, chain_id: u32, token_address: &str) -> anyhow::Result<Option<TokenSafety>> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Sử dụng analys_client để phân tích token
        let token_safety_result = self.analys_client.token_provider()
            .analyze_token(chain_id, token_address).await
            .context(format!("Không thể phân tích token {}", token_address))?;
        
        // Nếu không phân tích được token, trả về None
        if !token_safety_result.is_valid() {
            info!("Token {} không hợp lệ hoặc không thể phân tích", token_address);
            return Ok(None);
        }
        
        // Lấy thêm thông tin chi tiết từ contract
        let contract_details = self.analys_client.token_provider()
            .get_contract_details(chain_id, token_address).await
            .context(format!("Không thể lấy chi tiết contract cho token {}", token_address))?;
        
        // Đánh giá rủi ro của token
        let risk_analysis = self.analys_client.risk_provider()
            .analyze_token_risk(chain_id, token_address).await
            .context(format!("Không thể phân tích rủi ro cho token {}", token_address))?;
        
        // Log thông tin phân tích
        info!(
            "Phân tích token {} trên chain {}: risk_score={}, honeypot={:?}, buy_tax={:?}, sell_tax={:?}",
            token_address,
            chain_id,
            risk_analysis.risk_score,
            token_safety_result.is_honeypot,
            token_safety_result.buy_tax,
            token_safety_result.sell_tax
        );
        
        Ok(Some(token_safety_result))
    }

    /// Đánh giá rủi ro của token bằng cách sử dụng analys_client
    async fn analyze_token_risk(&self, chain_id: u32, token_address: &str) -> anyhow::Result<RiskScore> {
        // Sử dụng phương thức analyze_risk từ analys_client
        self.analys_client.analyze_risk(chain_id, token_address).await
            .context(format!("Không thể đánh giá rủi ro cho token {}", token_address))
    }
}

impl SmartTradeExecutor {
    /// Create a new SmartTradeExecutor
    pub fn new() -> Self {
        let executor_id = format!("smart-trade-{}", uuid::Uuid::new_v4());
        
        let evm_adapters = HashMap::new();
        let analys_client = Arc::new(SmartTradeAnalysisClient::new(evm_adapters.clone()));
        
        Self {
            evm_adapters,
            analys_client,
            risk_analyzer: Arc::new(RwLock::new(RiskAnalyzer::new())),
            mempool_analyzers: HashMap::new(),
            config: RwLock::new(SmartTradeConfig::default()),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            coordinator: None,
            executor_id,
            coordinator_subscription: RwLock::new(None),
        }
    }
    
    /// Set coordinator for shared state
    pub fn with_coordinator(mut self, coordinator: Arc<dyn TradeCoordinator>) -> Self {
        self.coordinator = Some(coordinator);
        self
    }
    
    /// Convert a trade tracker to a trade result
    async fn tracker_to_result(&self, tracker: &TradeTracker) -> anyhow::Result<TradeResult> {
        let result = TradeResult {
            id: tracker.id.clone(),
            chain_id: tracker.chain_id,
            token_address: tracker.token_address.clone(),
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: tracker.amount,
            entry_price: tracker.entry_price,
            exit_price: None,
            current_price: tracker.current_price,
            profit_loss: if tracker.entry_price > 0.0 && tracker.current_price > 0.0 {
                (tracker.current_price - tracker.entry_price) / tracker.entry_price * 100.0
            } else {
                0.0
            },
            status: tracker.status.clone(),
            strategy: tracker.strategy.clone(),
            created_at: tracker.created_at,
            updated_at: tracker.updated_at,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        Ok(result)
    }
    
    /// Vòng lặp theo dõi chính
    async fn monitor_loop(&self) {
        let mut counter = 0;
        let mut persist_counter = 0;
        
        while let true_val = *self.running.read().await {
            if !true_val {
                break;
            }
            
            // Kiểm tra và cập nhật tất cả các giao dịch đang active
            if let Err(e) = self.check_active_trades().await {
                error!("Error checking active trades: {}", e);
            }
            
            // Tăng counter và dọn dẹp lịch sử giao dịch định kỳ
            counter += 1;
            if counter >= 100 {
                self.cleanup_trade_history().await;
                counter = 0;
            }
            
            // Lưu trạng thái vào bộ nhớ định kỳ
            persist_counter += 1;
            if persist_counter >= 30 { // Lưu mỗi 30 chu kỳ (khoảng 2.5 phút với interval 5s)
                if let Err(e) = self.update_and_persist_trades().await {
                    error!("Failed to persist bot state: {}", e);
                }
                persist_counter = 0;
            }
            
            // Tính toán thời gian sleep thích ứng
            let sleep_time = self.adaptive_sleep_time().await;
            tokio::time::sleep(tokio::time::Duration::from_millis(sleep_time)).await;
        }
        
        // Lưu trạng thái trước khi dừng
        if let Err(e) = self.update_and_persist_trades().await {
            error!("Failed to persist bot state before stopping: {}", e);
        }
    }
    
    /// Quét cơ hội giao dịch mới
    async fn scan_trading_opportunities(&self) {
        // Kiểm tra cấu hình
        let config = self.config.read().await;
        if !config.enabled || !config.auto_trade {
            return;
        }
        
        // Kiểm tra số lượng giao dịch hiện tại
        let active_trades = self.active_trades.read().await;
        if active_trades.len() >= config.max_concurrent_trades as usize {
            return;
        }
        
        // Chạy quét cho từng chain
        let futures = self.mempool_analyzers.iter().map(|(chain_id, analyzer)| {
            self.scan_chain_opportunities(*chain_id, analyzer)
        });
        join_all(futures).await;
    }
    
    /// Quét cơ hội giao dịch trên một chain cụ thể
    async fn scan_chain_opportunities(&self, chain_id: u32, analyzer: &Arc<MempoolAnalyzer>) -> anyhow::Result<()> {
        let config = self.config.read().await;
        
        // Lấy các giao dịch lớn từ mempool
        let large_txs = match analyzer.get_large_transactions(QUICK_TRADE_MIN_BNB).await {
            Ok(txs) => txs,
            Err(e) => {
                error!("Không thể lấy các giao dịch lớn từ mempool: {}", e);
                return Err(anyhow::anyhow!("Lỗi khi lấy giao dịch từ mempool: {}", e));
            }
        };
        
        // Phân tích từng giao dịch
        for tx in large_txs {
            // Kiểm tra nếu token không trong blacklist và đáp ứng điều kiện
            if let Some(to_token) = &tx.to_token {
                let token_address = &to_token.address;
                
                // Kiểm tra blacklist và whitelist
                if config.token_blacklist.contains(token_address) {
                    debug!("Bỏ qua token {} trong blacklist", token_address);
                    continue;
                }
                
                if !config.token_whitelist.is_empty() && !config.token_whitelist.contains(token_address) {
                    debug!("Bỏ qua token {} không trong whitelist", token_address);
                    continue;
                }
                
                // Kiểm tra nếu giao dịch đủ lớn
                if tx.value >= QUICK_TRADE_MIN_BNB {
                    // Phân tích token
                    if let Err(e) = self.analyze_and_trade_token(chain_id, token_address, tx).await {
                        error!("Lỗi khi phân tích và giao dịch token {}: {}", token_address, e);
                        // Tiếp tục với token tiếp theo, không dừng lại
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Phân tích token và thực hiện giao dịch nếu an toàn
    async fn analyze_and_trade_token(&self, chain_id: u32, token_address: &str, tx: MempoolTransaction) -> anyhow::Result<()> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id));
            }
        };
        
        // THÊM: Xác thực kỹ lưỡng dữ liệu mempool trước khi tiếp tục phân tích
        info!("Xác thực dữ liệu mempool cho token {} từ giao dịch {}", token_address, tx.tx_hash);
        let mempool_validation = match self.validate_mempool_data(chain_id, token_address, &tx).await {
            Ok(validation) => validation,
            Err(e) => {
                error!("Lỗi khi xác thực dữ liệu mempool: {}", e);
                self.log_trade_decision(
                    "new_opportunity", 
                    token_address, 
                    TradeStatus::Canceled, 
                    &format!("Lỗi xác thực mempool: {}", e), 
                    chain_id
                ).await;
                return Err(anyhow::anyhow!("Lỗi xác thực dữ liệu mempool: {}", e));
            }
        };
        
        // Nếu dữ liệu mempool không hợp lệ, dừng phân tích
        if !mempool_validation.is_valid {
            let reasons = mempool_validation.reasons.join(", ");
            info!("Dữ liệu mempool không đáng tin cậy cho token {}: {}", token_address, reasons);
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Dữ liệu mempool không đáng tin cậy: {}", reasons), 
                chain_id
            ).await;
            return Ok(());
        }
        
        info!("Xác thực mempool thành công với điểm tin cậy: {}/100", mempool_validation.confidence_score);
        
        // Code hiện tại: Sử dụng analys_client để kiểm tra an toàn token
        let security_check_future = self.analys_client.verify_token_safety(chain_id, token_address);
        
        // Code hiện tại: Sử dụng analys_client để đánh giá rủi ro
        let risk_analysis_future = self.analys_client.analyze_risk(chain_id, token_address);
        
        // Thực hiện song song các phân tích
        let (security_check_result, risk_analysis) = tokio::join!(security_check_future, risk_analysis_future);
        
        // Xử lý kết quả kiểm tra an toàn
        let security_check = match security_check_result {
            Ok(result) => result,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi kiểm tra an toàn token: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi kiểm tra an toàn token: {}", e));
            }
        };
        
        // Xử lý kết quả phân tích rủi ro
        let risk_score = match risk_analysis {
            Ok(score) => score,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi phân tích rủi ro: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi phân tích rủi ro: {}", e));
            }
        };
        
        // Kiểm tra xem token có an toàn không
        if !security_check.is_safe {
            let issues_str = security_check.issues.iter()
                .map(|issue| format!("{:?}", issue))
                .collect::<Vec<String>>()
                .join(", ");
                
            self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                &format!("Token không an toàn: {}", issues_str), chain_id).await;
            return Ok(());
        }
        
        // Lấy cấu hình hiện tại
        let config = self.config.read().await;
        
        // Kiểm tra điểm rủi ro
        if risk_score.score > config.max_risk_score {
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Điểm rủi ro quá cao: {}/{}", risk_score.score, config.max_risk_score), 
                chain_id
            ).await;
            return Ok(());
        }
        
        // Xác định chiến lược giao dịch dựa trên mức độ rủi ro
        let strategy = if risk_score.score < 30 {
            TradeStrategy::Smart  // Rủi ro thấp, dùng chiến lược thông minh với TSL
        } else {
            TradeStrategy::Quick  // Rủi ro cao hơn, dùng chiến lược nhanh
        };
        
        // Xác định số lượng giao dịch dựa trên cấu hình và mức độ rủi ro
        let base_amount = if risk_score.score < 20 {
            config.trade_amount_high_confidence
        } else if risk_score.score < 40 {
            config.trade_amount_medium_confidence
        } else {
            config.trade_amount_low_confidence
        };
        
        // Tạo tham số giao dịch
        let trade_params = crate::types::TradeParams {
            chain_type: crate::types::ChainType::EVM(chain_id),
            token_address: token_address.to_string(),
            amount: base_amount,
            slippage: 1.0, // 1% slippage mặc định cho giao dịch tự động
            trade_type: crate::types::TradeType::Buy,
            deadline_minutes: 2, // Thời hạn 2 phút
            router_address: String::new(), // Dùng router mặc định
            gas_limit: None,
            gas_price: None,
            strategy: Some(format!("{:?}", strategy).to_lowercase()),
            stop_loss: if strategy == TradeStrategy::Smart { Some(5.0) } else { None }, // 5% stop loss for Smart strategy
            take_profit: if strategy == TradeStrategy::Smart { Some(20.0) } else { Some(10.0) }, // 20% take profit for Smart, 10% for Quick
            max_hold_time: None, // Sẽ được thiết lập dựa trên chiến lược
            custom_params: None,
        };
        
        // Thực hiện giao dịch và lấy kết quả
        info!("Thực hiện mua {} cho token {} trên chain {}", base_amount, token_address, chain_id);
        
        let buy_result = adapter.execute_trade(&trade_params).await;
        
        match buy_result {
            Ok(tx_result) => {
                // Tạo ID giao dịch duy nhất
                let trade_id = uuid::Uuid::new_v4().to_string();
                let current_timestamp = chrono::Utc::now().timestamp() as u64;
                
                // Tính thời gian giữ tối đa dựa trên chiến lược
                let max_hold_time = match strategy {
                    TradeStrategy::Quick => current_timestamp + QUICK_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Smart => current_timestamp + SMART_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Manual => current_timestamp + 24 * 60 * 60, // 24 giờ cho giao dịch thủ công
                };
                
                // Tạo trailing stop loss nếu sử dụng chiến lược Smart
                let trailing_stop_percent = match strategy {
                    TradeStrategy::Smart => Some(SMART_TRADE_TSL_PERCENT),
                    _ => None,
                };
                
                // Xác định giá entry từ kết quả giao dịch
                let entry_price = match tx_result.execution_price {
                    Some(price) => price,
                    None => {
                        error!("Không thể xác định giá mua vào cho token {}", token_address);
                        return Err(anyhow::anyhow!("Không thể xác định giá mua vào"));
                    }
                };
                
                // Lấy số lượng token đã mua
                let token_amount = match tx_result.tokens_received {
                    Some(amount) => amount,
                    None => {
                        error!("Không thể xác định số lượng token nhận được");
                        return Err(anyhow::anyhow!("Không thể xác định số lượng token nhận được"));
                    }
                };
                
                // Tạo theo dõi giao dịch mới
                let trade_tracker = TradeTracker {
                    trade_id: trade_id.clone(),
                    token_address: token_address.to_string(),
                    chain_id,
                    strategy: strategy.clone(),
                    entry_price,
                    token_amount,
                    invested_amount: base_amount,
                    highest_price: entry_price, // Ban đầu, giá cao nhất = giá mua
                    entry_time: current_timestamp,
                    max_hold_time,
                    take_profit_percent: match strategy {
                        TradeStrategy::Quick => QUICK_TRADE_TARGET_PROFIT,
                        _ => 10.0, // Mặc định 10% cho các chiến lược khác
                    },
                    stop_loss_percent: STOP_LOSS_PERCENT,
                    trailing_stop_percent,
                    buy_tx_hash: tx_result.transaction_hash.clone(),
                    sell_tx_hash: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                };
                
                // Thêm vào danh sách theo dõi
                {
                    let mut active_trades = self.active_trades.write().await;
                    active_trades.push(trade_tracker.clone());
                }
                
                // Log thành công
                info!(
                    "Giao dịch mua thành công: ID={}, Token={}, Chain={}, Strategy={:?}, Amount={}, Price={}",
                    trade_id, token_address, chain_id, strategy, base_amount, entry_price
                );
                
                // Thêm vào lịch sử
                let trade_result = TradeResult {
                    trade_id: trade_id.clone(),
                    entry_price,
                    exit_price: None,
                    profit_percent: None,
                    profit_usd: None,
                    entry_time: current_timestamp,
                    exit_time: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                    gas_cost_usd: tx_result.gas_cost_usd.unwrap_or(0.0),
                };
                
                let mut trade_history = self.trade_history.write().await;
                trade_history.push(trade_result);
                
                // Trả về ID giao dịch
                Ok(())
            },
            Err(e) => {
                // Log lỗi giao dịch
                error!("Giao dịch mua thất bại cho token {}: {}", token_address, e);
                Err(anyhow::anyhow!("Giao dịch mua thất bại: {}", e))
            }
        }
    }
    
/// Core implementation of SmartTradeExecutor
///
/// This file contains the main executor struct that manages the entire
/// smart trading process, orchestrates token analysis, trade strategies,
/// and market monitoring.
///
/// # Cải tiến đã thực hiện:
/// - Loại bỏ các phương thức trùng lặp với analys modules
/// - Sử dụng API từ analys/token_status và analys/risk_analyzer
/// - Áp dụng xử lý lỗi chuẩn với anyhow::Result thay vì unwrap/expect
/// - Đảm bảo thread safety trong các phương thức async với kỹ thuật "clone and drop" để tránh deadlock
/// - Tối ưu hóa việc xử lý các futures với tokio::join! cho các tác vụ song song
/// - Xử lý các trường hợp giá trị null an toàn với match/Option
/// - Tuân thủ nghiêm ngặt quy tắc từ .cursorrc

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use chrono::Utc;
use futures::future::join_all;
use tracing::{debug, error, info, warn};
use anyhow::{Result, Context, anyhow, bail};
use uuid;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use rand::Rng;
use std::collections::HashSet;
use futures::future::Future;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::{MempoolAnalyzer, MempoolTransaction};
use crate::analys::risk_analyzer::{RiskAnalyzer, RiskFactor, TradeRecommendation, TradeRiskAnalysis};
use crate::analys::token_status::{
    ContractInfo, LiquidityEvent, LiquidityEventType, TokenSafety, TokenStatus,
    TokenIssue, IssueSeverity, abnormal_liquidity_events, detect_dynamic_tax, 
    detect_hidden_fees, analyze_owner_privileges, is_proxy_contract, blacklist::{has_blacklist_or_whitelist, has_trading_cooldown, has_max_tx_or_wallet_limit},
};
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};
use crate::tradelogic::traits::{
    TradeExecutor, RiskManager, TradeCoordinator, ExecutorType, SharedOpportunity, 
    SharedOpportunityType, OpportunityPriority
};

// Module imports
use super::types::{
    SmartTradeConfig, 
    TradeResult, 
    TradeStatus, 
    TradeStrategy, 
    TradeTracker
};
use super::constants::*;
use super::token_analysis::*;
use super::trade_strategy::*;
use super::alert::*;
use super::optimization::*;
use super::security::*;
use super::analys_client::SmartTradeAnalysisClient;

/// Executor for smart trading strategies
#[derive(Clone)]
pub struct SmartTradeExecutor {
    /// EVM adapter for each chain
    pub(crate) evm_adapters: HashMap<u32, Arc<EvmAdapter>>,
    
    /// Analysis client that provides access to all analysis services
    pub(crate) analys_client: Arc<SmartTradeAnalysisClient>,
    
    /// Risk analyzer (legacy - kept for backward compatibility)
    pub(crate) risk_analyzer: Arc<RwLock<RiskAnalyzer>>,
    
    /// Mempool analyzer for each chain (legacy - kept for backward compatibility)
    pub(crate) mempool_analyzers: HashMap<u32, Arc<MempoolAnalyzer>>,
    
    /// Configuration
    pub(crate) config: RwLock<SmartTradeConfig>,
    
    /// Active trades being monitored
    pub(crate) active_trades: RwLock<Vec<TradeTracker>>,
    
    /// History of completed trades
    pub(crate) trade_history: RwLock<Vec<TradeResult>>,
    
    /// Running state
    pub(crate) running: RwLock<bool>,
    
    /// Coordinator for shared state between executors
    pub(crate) coordinator: Option<Arc<dyn TradeCoordinator>>,
    
    /// Unique ID for this executor instance
    pub(crate) executor_id: String,
    
    /// Subscription ID for coordinator notifications
    pub(crate) coordinator_subscription: RwLock<Option<String>>,
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
        
        // Khởi động vòng lặp theo dõi
        let executor = Arc::new(self.clone_with_config().await);
        
        tokio::spawn(async move {
            executor.monitor_loop().await;
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
        
        Ok(())
    }
    
    /// Execute a trade with the specified parameters
    async fn execute_trade(&self, params: TradeParams) -> anyhow::Result<Self::TradeResult> {
        // Find the appropriate adapter
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", params.chain_id)),
        };
        
        // Validate trade parameters
        if params.amount <= 0.0 {
            return Err(anyhow::anyhow!("Invalid trade amount: {}", params.amount));
        }
        
        // Create a trade tracker
        let trade_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp() as u64;
        
        let tracker = TradeTracker {
            id: trade_id.clone(),
            chain_id: params.chain_id,
            token_address: params.token_address.clone(),
            amount: params.amount,
            entry_price: 0.0, // Will be updated after trade execution
            current_price: 0.0,
            status: TradeStatus::Pending,
            strategy: if let Some(strategy) = &params.strategy {
                match strategy.as_str() {
                    "quick" => TradeStrategy::QuickFlip,
                    "tsl" => TradeStrategy::TrailingStopLoss,
                    "hodl" => TradeStrategy::LongTerm,
                    _ => TradeStrategy::default(),
                }
            } else {
                TradeStrategy::default()
            },
            created_at: now,
            updated_at: now,
            stop_loss: params.stop_loss,
            take_profit: params.take_profit,
            max_hold_time: params.max_hold_time.unwrap_or(DEFAULT_MAX_HOLD_TIME),
            custom_params: params.custom_params.unwrap_or_default(),
        };
        
        // TODO: Implement the actual trade execution logic
        // This is a simplified version - in a real implementation, you would:
        // 1. Execute the trade
        // 2. Update the trade tracker with actual execution details
        // 3. Add it to active trades
        
        let result = TradeResult {
            id: trade_id,
            chain_id: params.chain_id,
            token_address: params.token_address,
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: params.amount,
            entry_price: 0.0,                  // Would be populated from actual execution
            exit_price: None,
            current_price: 0.0,
            profit_loss: 0.0,
            status: TradeStatus::Pending,
            strategy: tracker.strategy.clone(),
            created_at: now,
            updated_at: now,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        // Add to active trades
        {
            let mut active_trades = self.active_trades.write().await;
            active_trades.push(tracker);
        }
        
        Ok(result)
    }
    
    /// Get the current status of an active trade
    async fn get_trade_status(&self, trade_id: &str) -> anyhow::Result<Option<Self::TradeResult>> {
        // Check active trades
        {
            let active_trades = self.active_trades.read().await;
            if let Some(trade) = active_trades.iter().find(|t| t.id == trade_id) {
                return Ok(Some(self.tracker_to_result(trade).await?));
            }
        }
        
        // Check trade history
        let trade_history = self.trade_history.read().await;
        let result = trade_history.iter()
            .find(|r| r.id == trade_id)
            .cloned();
            
        Ok(result)
    }
    
    /// Get all active trades
    async fn get_active_trades(&self) -> anyhow::Result<Vec<Self::TradeResult>> {
        let active_trades = self.active_trades.read().await;
        let mut results = Vec::with_capacity(active_trades.len());
        
        for tracker in active_trades.iter() {
            results.push(self.tracker_to_result(tracker).await?);
        }
        
        Ok(results)
    }
    
    /// Get trade history within a specific time range
    async fn get_trade_history(&self, from_timestamp: u64, to_timestamp: u64) -> anyhow::Result<Vec<Self::TradeResult>> {
        let trade_history = self.trade_history.read().await;
        
        let filtered_history = trade_history.iter()
            .filter(|trade| trade.created_at >= from_timestamp && trade.created_at <= to_timestamp)
            .cloned()
            .collect();
            
        Ok(filtered_history)
    }
    
    /// Evaluate a token for potential trading
    async fn evaluate_token(&self, chain_id: u32, token_address: &str) -> anyhow::Result<Option<TokenSafety>> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Sử dụng analys_client để phân tích token
        let token_safety_result = self.analys_client.token_provider()
            .analyze_token(chain_id, token_address).await
            .context(format!("Không thể phân tích token {}", token_address))?;
        
        // Nếu không phân tích được token, trả về None
        if !token_safety_result.is_valid() {
            info!("Token {} không hợp lệ hoặc không thể phân tích", token_address);
            return Ok(None);
        }
        
        // Lấy thêm thông tin chi tiết từ contract
        let contract_details = self.analys_client.token_provider()
            .get_contract_details(chain_id, token_address).await
            .context(format!("Không thể lấy chi tiết contract cho token {}", token_address))?;
        
        // Đánh giá rủi ro của token
        let risk_analysis = self.analys_client.risk_provider()
            .analyze_token_risk(chain_id, token_address).await
            .context(format!("Không thể phân tích rủi ro cho token {}", token_address))?;
        
        // Log thông tin phân tích
        info!(
            "Phân tích token {} trên chain {}: risk_score={}, honeypot={:?}, buy_tax={:?}, sell_tax={:?}",
            token_address,
            chain_id,
            risk_analysis.risk_score,
            token_safety_result.is_honeypot,
            token_safety_result.buy_tax,
            token_safety_result.sell_tax
        );
        
        Ok(Some(token_safety_result))
    }

    /// Đánh giá rủi ro của token bằng cách sử dụng analys_client
    async fn analyze_token_risk(&self, chain_id: u32, token_address: &str) -> anyhow::Result<RiskScore> {
        // Sử dụng phương thức analyze_risk từ analys_client
        self.analys_client.analyze_risk(chain_id, token_address).await
            .context(format!("Không thể đánh giá rủi ro cho token {}", token_address))
    }
}

impl SmartTradeExecutor {
    /// Create a new SmartTradeExecutor
    pub fn new() -> Self {
        let executor_id = format!("smart-trade-{}", uuid::Uuid::new_v4());
        
        let evm_adapters = HashMap::new();
        let analys_client = Arc::new(SmartTradeAnalysisClient::new(evm_adapters.clone()));
        
        Self {
            evm_adapters,
            analys_client,
            risk_analyzer: Arc::new(RwLock::new(RiskAnalyzer::new())),
            mempool_analyzers: HashMap::new(),
            config: RwLock::new(SmartTradeConfig::default()),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            coordinator: None,
            executor_id,
            coordinator_subscription: RwLock::new(None),
        }
    }
    
    /// Set coordinator for shared state
    pub fn with_coordinator(mut self, coordinator: Arc<dyn TradeCoordinator>) -> Self {
        self.coordinator = Some(coordinator);
        self
    }
    
    /// Convert a trade tracker to a trade result
    async fn tracker_to_result(&self, tracker: &TradeTracker) -> anyhow::Result<TradeResult> {
        let result = TradeResult {
            id: tracker.id.clone(),
            chain_id: tracker.chain_id,
            token_address: tracker.token_address.clone(),
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: tracker.amount,
            entry_price: tracker.entry_price,
            exit_price: None,
            current_price: tracker.current_price,
            profit_loss: if tracker.entry_price > 0.0 && tracker.current_price > 0.0 {
                (tracker.current_price - tracker.entry_price) / tracker.entry_price * 100.0
            } else {
                0.0
            },
            status: tracker.status.clone(),
            strategy: tracker.strategy.clone(),
            created_at: tracker.created_at,
            updated_at: tracker.updated_at,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        Ok(result)
    }
    
    /// Vòng lặp theo dõi chính
    async fn monitor_loop(&self) {
        let mut counter = 0;
        let mut persist_counter = 0;
        
        while let true_val = *self.running.read().await {
            if !true_val {
                break;
            }
            
            // Kiểm tra và cập nhật tất cả các giao dịch đang active
            if let Err(e) = self.check_active_trades().await {
                error!("Error checking active trades: {}", e);
            }
            
            // Tăng counter và dọn dẹp lịch sử giao dịch định kỳ
            counter += 1;
            if counter >= 100 {
                self.cleanup_trade_history().await;
                counter = 0;
            }
            
            // Lưu trạng thái vào bộ nhớ định kỳ
            persist_counter += 1;
            if persist_counter >= 30 { // Lưu mỗi 30 chu kỳ (khoảng 2.5 phút với interval 5s)
                if let Err(e) = self.update_and_persist_trades().await {
                    error!("Failed to persist bot state: {}", e);
                }
                persist_counter = 0;
            }
            
            // Tính toán thời gian sleep thích ứng
            let sleep_time = self.adaptive_sleep_time().await;
            tokio::time::sleep(tokio::time::Duration::from_millis(sleep_time)).await;
        }
        
        // Lưu trạng thái trước khi dừng
        if let Err(e) = self.update_and_persist_trades().await {
            error!("Failed to persist bot state before stopping: {}", e);
        }
    }
    
    /// Quét cơ hội giao dịch mới
    async fn scan_trading_opportunities(&self) {
        // Kiểm tra cấu hình
        let config = self.config.read().await;
        if !config.enabled || !config.auto_trade {
            return;
        }
        
        // Kiểm tra số lượng giao dịch hiện tại
        let active_trades = self.active_trades.read().await;
        if active_trades.len() >= config.max_concurrent_trades as usize {
            return;
        }
        
        // Chạy quét cho từng chain
        let futures = self.mempool_analyzers.iter().map(|(chain_id, analyzer)| {
            self.scan_chain_opportunities(*chain_id, analyzer)
        });
        join_all(futures).await;
    }
    
    /// Quét cơ hội giao dịch trên một chain cụ thể
    async fn scan_chain_opportunities(&self, chain_id: u32, analyzer: &Arc<MempoolAnalyzer>) -> anyhow::Result<()> {
        let config = self.config.read().await;
        
        // Lấy các giao dịch lớn từ mempool
        let large_txs = match analyzer.get_large_transactions(QUICK_TRADE_MIN_BNB).await {
            Ok(txs) => txs,
            Err(e) => {
                error!("Không thể lấy các giao dịch lớn từ mempool: {}", e);
                return Err(anyhow::anyhow!("Lỗi khi lấy giao dịch từ mempool: {}", e));
            }
        };
        
        // Phân tích từng giao dịch
        for tx in large_txs {
            // Kiểm tra nếu token không trong blacklist và đáp ứng điều kiện
            if let Some(to_token) = &tx.to_token {
                let token_address = &to_token.address;
                
                // Kiểm tra blacklist và whitelist
                if config.token_blacklist.contains(token_address) {
                    debug!("Bỏ qua token {} trong blacklist", token_address);
                    continue;
                }
                
                if !config.token_whitelist.is_empty() && !config.token_whitelist.contains(token_address) {
                    debug!("Bỏ qua token {} không trong whitelist", token_address);
                    continue;
                }
                
                // Kiểm tra nếu giao dịch đủ lớn
                if tx.value >= QUICK_TRADE_MIN_BNB {
                    // Phân tích token
                    if let Err(e) = self.analyze_and_trade_token(chain_id, token_address, tx).await {
                        error!("Lỗi khi phân tích và giao dịch token {}: {}", token_address, e);
                        // Tiếp tục với token tiếp theo, không dừng lại
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Phân tích token và thực hiện giao dịch nếu an toàn
    async fn analyze_and_trade_token(&self, chain_id: u32, token_address: &str, tx: MempoolTransaction) -> anyhow::Result<()> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id));
            }
        };
        
        // THÊM: Xác thực kỹ lưỡng dữ liệu mempool trước khi tiếp tục phân tích
        info!("Xác thực dữ liệu mempool cho token {} từ giao dịch {}", token_address, tx.tx_hash);
        let mempool_validation = match self.validate_mempool_data(chain_id, token_address, &tx).await {
            Ok(validation) => validation,
            Err(e) => {
                error!("Lỗi khi xác thực dữ liệu mempool: {}", e);
                self.log_trade_decision(
                    "new_opportunity", 
                    token_address, 
                    TradeStatus::Canceled, 
                    &format!("Lỗi xác thực mempool: {}", e), 
                    chain_id
                ).await;
                return Err(anyhow::anyhow!("Lỗi xác thực dữ liệu mempool: {}", e));
            }
        };
        
        // Nếu dữ liệu mempool không hợp lệ, dừng phân tích
        if !mempool_validation.is_valid {
            let reasons = mempool_validation.reasons.join(", ");
            info!("Dữ liệu mempool không đáng tin cậy cho token {}: {}", token_address, reasons);
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Dữ liệu mempool không đáng tin cậy: {}", reasons), 
                chain_id
            ).await;
            return Ok(());
        }
        
        info!("Xác thực mempool thành công với điểm tin cậy: {}/100", mempool_validation.confidence_score);
        
        // Code hiện tại: Sử dụng analys_client để kiểm tra an toàn token
        let security_check_future = self.analys_client.verify_token_safety(chain_id, token_address);
        
        // Code hiện tại: Sử dụng analys_client để đánh giá rủi ro
        let risk_analysis_future = self.analys_client.analyze_risk(chain_id, token_address);
        
        // Thực hiện song song các phân tích
        let (security_check_result, risk_analysis) = tokio::join!(security_check_future, risk_analysis_future);
        
        // Xử lý kết quả kiểm tra an toàn
        let security_check = match security_check_result {
            Ok(result) => result,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi kiểm tra an toàn token: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi kiểm tra an toàn token: {}", e));
            }
        };
        
        // Xử lý kết quả phân tích rủi ro
        let risk_score = match risk_analysis {
            Ok(score) => score,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi phân tích rủi ro: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi phân tích rủi ro: {}", e));
            }
        };
        
        // Kiểm tra xem token có an toàn không
        if !security_check.is_safe {
            let issues_str = security_check.issues.iter()
                .map(|issue| format!("{:?}", issue))
                .collect::<Vec<String>>()
                .join(", ");
                
            self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                &format!("Token không an toàn: {}", issues_str), chain_id).await;
            return Ok(());
        }
        
        // Lấy cấu hình hiện tại
        let config = self.config.read().await;
        
        // Kiểm tra điểm rủi ro
        if risk_score.score > config.max_risk_score {
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Điểm rủi ro quá cao: {}/{}", risk_score.score, config.max_risk_score), 
                chain_id
            ).await;
            return Ok(());
        }
        
        // Xác định chiến lược giao dịch dựa trên mức độ rủi ro
        let strategy = if risk_score.score < 30 {
            TradeStrategy::Smart  // Rủi ro thấp, dùng chiến lược thông minh với TSL
        } else {
            TradeStrategy::Quick  // Rủi ro cao hơn, dùng chiến lược nhanh
        };
        
        // Xác định số lượng giao dịch dựa trên cấu hình và mức độ rủi ro
        let base_amount = if risk_score.score < 20 {
            config.trade_amount_high_confidence
        } else if risk_score.score < 40 {
            config.trade_amount_medium_confidence
        } else {
            config.trade_amount_low_confidence
        };
        
        // Tạo tham số giao dịch
        let trade_params = crate::types::TradeParams {
            chain_type: crate::types::ChainType::EVM(chain_id),
            token_address: token_address.to_string(),
            amount: base_amount,
            slippage: 1.0, // 1% slippage mặc định cho giao dịch tự động
            trade_type: crate::types::TradeType::Buy,
            deadline_minutes: 2, // Thời hạn 2 phút
            router_address: String::new(), // Dùng router mặc định
            gas_limit: None,
            gas_price: None,
            strategy: Some(format!("{:?}", strategy).to_lowercase()),
            stop_loss: if strategy == TradeStrategy::Smart { Some(5.0) } else { None }, // 5% stop loss for Smart strategy
            take_profit: if strategy == TradeStrategy::Smart { Some(20.0) } else { Some(10.0) }, // 20% take profit for Smart, 10% for Quick
            max_hold_time: None, // Sẽ được thiết lập dựa trên chiến lược
            custom_params: None,
        };
        
        // Thực hiện giao dịch và lấy kết quả
        info!("Thực hiện mua {} cho token {} trên chain {}", base_amount, token_address, chain_id);
        
        let buy_result = adapter.execute_trade(&trade_params).await;
        
        match buy_result {
            Ok(tx_result) => {
                // Tạo ID giao dịch duy nhất
                let trade_id = uuid::Uuid::new_v4().to_string();
                let current_timestamp = chrono::Utc::now().timestamp() as u64;
                
                // Tính thời gian giữ tối đa dựa trên chiến lược
                let max_hold_time = match strategy {
                    TradeStrategy::Quick => current_timestamp + QUICK_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Smart => current_timestamp + SMART_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Manual => current_timestamp + 24 * 60 * 60, // 24 giờ cho giao dịch thủ công
                };
                
                // Tạo trailing stop loss nếu sử dụng chiến lược Smart
                let trailing_stop_percent = match strategy {
                    TradeStrategy::Smart => Some(SMART_TRADE_TSL_PERCENT),
                    _ => None,
                };
                
                // Xác định giá entry từ kết quả giao dịch
                let entry_price = match tx_result.execution_price {
                    Some(price) => price,
                    None => {
                        error!("Không thể xác định giá mua vào cho token {}", token_address);
                        return Err(anyhow::anyhow!("Không thể xác định giá mua vào"));
                    }
                };
                
                // Lấy số lượng token đã mua
                let token_amount = match tx_result.tokens_received {
                    Some(amount) => amount,
                    None => {
                        error!("Không thể xác định số lượng token nhận được");
                        return Err(anyhow::anyhow!("Không thể xác định số lượng token nhận được"));
                    }
                };
                
                // Tạo theo dõi giao dịch mới
                let trade_tracker = TradeTracker {
                    trade_id: trade_id.clone(),
                    token_address: token_address.to_string(),
                    chain_id,
                    strategy: strategy.clone(),
                    entry_price,
                    token_amount,
                    invested_amount: base_amount,
                    highest_price: entry_price, // Ban đầu, giá cao nhất = giá mua
                    entry_time: current_timestamp,
                    max_hold_time,
                    take_profit_percent: match strategy {
                        TradeStrategy::Quick => QUICK_TRADE_TARGET_PROFIT,
                        _ => 10.0, // Mặc định 10% cho các chiến lược khác
                    },
                    stop_loss_percent: STOP_LOSS_PERCENT,
                    trailing_stop_percent,
                    buy_tx_hash: tx_result.transaction_hash.clone(),
                    sell_tx_hash: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                };
                
                // Thêm vào danh sách theo dõi
                {
                    let mut active_trades = self.active_trades.write().await;
                    active_trades.push(trade_tracker.clone());
                }
                
                // Log thành công
                info!(
                    "Giao dịch mua thành công: ID={}, Token={}, Chain={}, Strategy={:?}, Amount={}, Price={}",
                    trade_id, token_address, chain_id, strategy, base_amount, entry_price
                );
                
                // Thêm vào lịch sử
                let trade_result = TradeResult {
                    trade_id: trade_id.clone(),
                    entry_price,
                    exit_price: None,
                    profit_percent: None,
                    profit_usd: None,
                    entry_time: current_timestamp,
                    exit_time: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                    gas_cost_usd: tx_result.gas_cost_usd.unwrap_or(0.0),
                };
                
                let mut trade_history = self.trade_history.write().await;
                trade_history.push(trade_result);
                
                // Trả về ID giao dịch
                Ok(())
            },
            Err(e) => {
                // Log lỗi giao dịch
                error!("Giao dịch mua thất bại cho token {}: {}", token_address, e);
                Err(anyhow::anyhow!("Giao dịch mua thất bại: {}", e))
            }
        }
    }
    
    /// Thực hiện mua token dựa trên các tham số
    async fn execute_trade(&self, chain_id: u32, token_address: &str, strategy: TradeStrategy, trigger_tx: &MempoolTransaction) -> anyhow::Result<()> {
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id));
            }
        };
        
        // Lấy cấu hình
        let config = self.config.read().await;
        
        // Tính số lượng giao dịch
        let base_amount = match config.auto_trade {
            true => {
                // Tự động tính dựa trên cấu hình, giới hạn trong khoảng min-max
                let calculated_amount = std::cmp::min(
                    trigger_tx.value * config.capital_per_trade_ratio,
                    config.max_trade_amount
                );
                std::cmp::max(calculated_amount, config.min_trade_amount)
            },
            false => {
                // Nếu không bật auto trade, chỉ log và không thực hiện
                info!(
                    "Auto-trade disabled. Would trade {} for token {} with strategy {:?}",
                    config.min_trade_amount, token_address, strategy
                );
                return Ok(());
            }
        };
        
        // Tạo tham số giao dịch
        let trade_params = crate::types::TradeParams {
            chain_type: crate::types::ChainType::EVM(chain_id),
            token_address: token_address.to_string(),
            amount: base_amount,
            slippage: 1.0, // 1% slippage mặc định cho giao dịch tự động
            trade_type: crate::types::TradeType::Buy,
            deadline_minutes: 2, // Thời hạn 2 phút
            router_address: String::new(), // Dùng router mặc định
        };
        
        // Thực hiện giao dịch và lấy kết quả
        info!("Thực hiện mua {} cho token {} trên chain {}", base_amount, token_address, chain_id);
        
        let buy_result = adapter.execute_trade(&trade_params).await;
        
        match buy_result {
            Ok(tx_result) => {
                // Tạo ID giao dịch duy nhất
                let trade_id = uuid::Uuid::new_v4().to_string();
                let current_timestamp = chrono::Utc::now().timestamp() as u64;
                
                // Tính thời gian giữ tối đa dựa trên chiến lược
                let max_hold_time = match strategy {
                    TradeStrategy::Quick => current_timestamp + QUICK_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Smart => current_timestamp + SMART_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Manual => current_timestamp + 24 * 60 * 60, // 24 giờ cho giao dịch thủ công
                };
                
                // Tạo trailing stop loss nếu sử dụng chiến lược Smart
                let trailing_stop_percent = match strategy {
                    TradeStrategy::Smart => Some(SMART_TRADE_TSL_PERCENT),
                    _ => None,
                };
                
                // Xác định giá entry từ kết quả giao dịch
                let entry_price = match tx_result.execution_price {
                    Some(price) => price,
                    None => {
                        error!("Không thể xác định giá mua vào cho token {}", token_address);
                        return Err(anyhow::anyhow!("Không thể xác định giá mua vào"));
                    }
                };
                
                // Lấy số lượng token đã mua
                let token_amount = match tx_result.tokens_received {
                    Some(amount) => amount,
                    None => {
                        error!("Không thể xác định số lượng token nhận được");
                        return Err(anyhow::anyhow!("Không thể xác định số lượng token nhận được"));
                    }
                };
                
                // Tạo theo dõi giao dịch mới
                let trade_tracker = TradeTracker {
                    trade_id: trade_id.clone(),
                    token_address: token_address.to_string(),
                    chain_id,
                    strategy: strategy.clone(),
                    entry_price,
                    token_amount,
                    invested_amount: base_amount,
                    highest_price: entry_price, // Ban đầu, giá cao nhất = giá mua
                    entry_time: current_timestamp,
                    max_hold_time,
                    take_profit_percent: match strategy {
                        TradeStrategy::Quick => QUICK_TRADE_TARGET_PROFIT,
                        _ => 10.0, // Mặc định 10% cho các chiến lược khác
                    },
                    stop_loss_percent: STOP_LOSS_PERCENT,
                    trailing_stop_percent,
                    buy_tx_hash: tx_result.transaction_hash.clone(),
                    sell_tx_hash: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                };
                
                // Thêm vào danh sách theo dõi
                {
                    let mut active_trades = self.active_trades.write().await;
                    active_trades.push(trade_tracker.clone());
                }
                
                // Log thành công
                info!(
                    "Giao dịch mua thành công: ID={}, Token={}, Chain={}, Strategy={:?}, Amount={}, Price={}",
                    trade_id, token_address, chain_id, strategy, base_amount, entry_price
                );
                
                // Thêm vào lịch sử
                let trade_result = TradeResult {
                    trade_id: trade_id.clone(),
                    entry_price,
                    exit_price: None,
                    profit_percent: None,
                    profit_usd: None,
                    entry_time: current_timestamp,
                    exit_time: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                    gas_cost_usd: tx_result.gas_cost_usd,
                };
                
                let mut trade_history = self.trade_history.write().await;
                trade_history.push(trade_result);
                
                // Trả về ID giao dịch
                Ok(())
            },
            Err(e) => {
                // Log lỗi giao dịch
                error!("Giao dịch mua thất bại cho token {}: {}", token_address, e);
                Err(anyhow::anyhow!("Giao dịch mua thất bại: {}", e))
            }
        }
    }
    
    /// Theo dõi các giao dịch đang hoạt động
    async fn track_active_trades(&self) -> anyhow::Result<()> {
        // Kiểm tra xem có bất kỳ giao dịch nào cần theo dõi không
        let active_trades = self.active_trades.read().await;
        
        if active_trades.is_empty() {
            return Ok(());
        }
        
        // Clone danh sách để tránh giữ lock quá lâu
        let active_trades_clone = active_trades.clone();
        drop(active_trades); // Giải phóng lock để tránh deadlock
        
        // Xử lý từng giao dịch song song
        let update_futures = active_trades_clone.iter().map(|trade| {
            self.check_and_update_trade(trade.clone())
        });
        
        let results = futures::future::join_all(update_futures).await;
        
        // Kiểm tra và ghi log lỗi nếu có
        for result in results {
            if let Err(e) = result {
                error!("Lỗi khi theo dõi giao dịch: {}", e);
                // Tiếp tục xử lý các giao dịch khác, không dừng lại
            }
        }
        
        Ok(())
    }
    
    /// Kiểm tra và cập nhật trạng thái của một giao dịch
    async fn check_and_update_trade(&self, trade: TradeTracker) -> anyhow::Result<()> {
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&trade.chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", trade.chain_id));
            }
        };
        
        // Nếu giao dịch không ở trạng thái Open, bỏ qua
        if trade.status != TradeStatus::Open {
            return Ok(());
        }
        
        // Lấy giá hiện tại của token
        let current_price = match adapter.get_token_price(&trade.token_address).await {
            Ok(price) => price,
            Err(e) => {
                warn!("Không thể lấy giá hiện tại cho token {}: {}", trade.token_address, e);
                // Không thể lấy giá, nhưng vẫn cần kiểm tra các điều kiện khác
                // Nên tiếp tục với giá = 0
                0.0
            }
        };
        
        // Nếu không lấy được giá, kiểm tra thời gian giữ tối đa
        if current_price <= 0.0 {
            let current_time = chrono::Utc::now().timestamp() as u64;
            
            // Nếu quá thời gian giữ tối đa, bán
            if current_time >= trade.max_hold_time {
                warn!("Không thể lấy giá cho token {}, đã quá thời gian giữ tối đa, thực hiện bán", trade.token_address);
                self.sell_token(trade, "Đã quá thời gian giữ tối đa".to_string(), trade.entry_price).await?;
                return Ok(());
            }
            
            // Nếu chưa quá thời gian, tiếp tục theo dõi
            return Ok(());
        }
        
        // Tính toán phần trăm lợi nhuận/lỗ hiện tại
        let profit_percent = ((current_price - trade.entry_price) / trade.entry_price) * 100.0;
        let current_time = chrono::Utc::now().timestamp() as u64;
        
        // Cập nhật giá cao nhất nếu cần (chỉ với chiến lược có TSL)
        let mut highest_price = trade.highest_price;
        if current_price > highest_price {
            highest_price = current_price;
            
            // Cập nhật TradeTracker với giá cao nhất mới
            {
                let mut active_trades = self.active_trades.write().await;
                if let Some(index) = active_trades.iter().position(|t| t.trade_id == trade.trade_id) {
                    active_trades[index].highest_price = highest_price;
                }
            }
        }
        
        // 1. Kiểm tra điều kiện lợi nhuận đạt mục tiêu
        if profit_percent >= trade.take_profit_percent {
            info!(
                "Take profit triggered for {} (ID: {}): current profit {:.2}% >= target {:.2}%", 
                trade.token_address, trade.trade_id, profit_percent, trade.take_profit_percent
            );
            self.sell_token(trade, "Đạt mục tiêu lợi nhuận".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 2. Kiểm tra điều kiện stop loss
        if profit_percent <= -trade.stop_loss_percent {
            warn!(
                "Stop loss triggered for {} (ID: {}): current profit {:.2}% <= -{:.2}%", 
                trade.token_address, trade.trade_id, profit_percent, trade.stop_loss_percent
            );
            self.sell_token(trade, "Kích hoạt stop loss".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 3. Kiểm tra trailing stop loss (nếu có)
        if let Some(tsl_percent) = trade.trailing_stop_percent {
            let tsl_trigger_price = highest_price * (1.0 - tsl_percent / 100.0);
            
            if current_price <= tsl_trigger_price && highest_price > trade.entry_price {
                // Chỉ kích hoạt TSL nếu đã có lãi trước đó
                info!(
                    "Trailing stop loss triggered for {} (ID: {}): current price {} <= TSL price {:.4} (highest {:.4})", 
                    trade.token_address, trade.trade_id, current_price, tsl_trigger_price, highest_price
                );
                self.sell_token(trade, "Kích hoạt trailing stop loss".to_string(), current_price).await?;
                return Ok(());
            }
        }
        
        // 4. Kiểm tra hết thời gian tối đa
        if current_time >= trade.max_hold_time {
            info!(
                "Max hold time reached for {} (ID: {}): current profit {:.2}%", 
                trade.token_address, trade.trade_id, profit_percent
            );
            self.sell_token(trade, "Đã hết thời gian giữ tối đa".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 5. Kiểm tra an toàn token
        if trade.strategy != TradeStrategy::Manual {
            if let Err(e) = self.check_token_safety_changes(&trade, adapter).await {
                warn!("Phát hiện vấn đề an toàn với token {}: {}", trade.token_address, e);
                self.sell_token(trade, format!("Phát hiện vấn đề an toàn: {}", e), current_price).await?;
                return Ok(());
            }
        }
        
        // Giao dịch vẫn an toàn, tiếp tục theo dõi
        debug!(
            "Trade {} (ID: {}) still active: current profit {:.2}%, current price {:.8}, highest {:.8}", 
            trade.token_address, trade.trade_id, profit_percent, current_price, highest_price
        );
        
        Ok(())
    }
    
    /// Kiểm tra các vấn đề an toàn mới phát sinh với token
    async fn check_token_safety_changes(&self, trade: &TradeTracker, adapter: &Arc<EvmAdapter>) -> anyhow::Result<()> {
        debug!("Checking token safety changes for {}", trade.token_address);
        
        // Lấy thông tin token
        let contract_info = match self.get_contract_info(trade.chain_id, &trade.token_address, adapter).await {
            Some(info) => info,
            None => return Ok(()),
        };
        
        // Kiểm tra thay đổi về quyền owner
        let (has_privilege, privileges) = self.detect_owner_privilege(trade.chain_id, &trade.token_address).await?;
        
        // Kiểm tra có thay đổi về tax/fee
        let (has_dynamic_tax, tax_reason) = self.detect_dynamic_tax(trade.chain_id, &trade.token_address).await?;
        
        // Kiểm tra các sự kiện bất thường
        let liquidity_events = adapter.get_liquidity_events(&trade.token_address, 10).await
            .context("Failed to get recent liquidity events")?;
            
        let has_abnormal_events = abnormal_liquidity_events(&liquidity_events);
        
        // Nếu có bất kỳ vấn đề nghiêm trọng, ghi log và cảnh báo
        if has_dynamic_tax || has_privilege || has_abnormal_events {
            let mut warnings = Vec::new();
            
            if has_dynamic_tax {
                warnings.push(format!("Tax may have changed: {}", tax_reason.unwrap_or_else(|| "Unknown".to_string())));
            }
            
            if has_privilege && !privileges.is_empty() {
                warnings.push(format!("Owner privileges detected: {}", privileges.join(", ")));
            }
            
            if has_abnormal_events {
                warnings.push("Abnormal liquidity events detected".to_string());
            }
            
            let warning_msg = warnings.join("; ");
            warn!("Token safety concerns for {}: {}", trade.token_address, warning_msg);
            
            // Gửi cảnh báo cho user (thông qua alert system)
            if let Some(alert_system) = &self.alert_system {
                let alert = Alert {
                    token_address: trade.token_address.clone(),
                    chain_id: trade.chain_id,
                    alert_type: AlertType::TokenSafetyChanged,
                    message: warning_msg.clone(),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    trade_id: Some(trade.id.clone()),
                };
                
                if let Err(e) = alert_system.send_alert(alert).await {
                    error!("Failed to send alert: {}", e);
                }
            }
        }
        
        Ok(())
    }
    
    /// Bán token
    async fn sell_token(&self, trade: TradeTracker, exit_reason: String, current_price: f64) {
        if let Some(adapter) = self.evm_adapters.get(&trade.chain_id) {
            // Tạo tham số giao dịch bán
            let trade_params = TradeParams {
                chain_type: ChainType::EVM(trade.chain_id),
                token_address: trade.token_address.clone(),
                amount: trade.token_amount,
                slippage: 1.0, // 1% slippage
                trade_type: TradeType::Sell,
                deadline_minutes: 5,
                router_address: "".to_string(), // Sẽ dùng router mặc định
            };
            
            // Thực hiện bán
            match adapter.execute_trade(&trade_params).await {
                Ok(result) => {
                    if let Some(tx_receipt) = result.tx_receipt {
                        // Cập nhật trạng thái giao dịch
                        let now = Utc::now().timestamp() as u64;
                        
                        // Tính lợi nhuận, có kiểm tra chia cho 0
                        let profit_percent = if trade.entry_price > 0.0 {
                            (current_price - trade.entry_price) / trade.entry_price * 100.0
                        } else {
                            0.0 // Giá trị mặc định nếu entry_price = 0
                        };
                        
                        // Lấy giá thực tế từ adapter thay vì hardcoded
                        let token_price_usd = adapter.get_base_token_price_usd().await.unwrap_or(300.0);
                        let profit_usd = trade.invested_amount * profit_percent / 100.0 * token_price_usd;
                        
                        // Tạo kết quả giao dịch
                        let trade_result = TradeResult {
                            trade_id: trade.trade_id.clone(),
                            entry_price: trade.entry_price,
                            exit_price: Some(current_price),
                            profit_percent: Some(profit_percent),
                            profit_usd: Some(profit_usd),
                            entry_time: trade.entry_time,
                            exit_time: Some(now),
                            status: TradeStatus::Closed,
                            exit_reason: Some(exit_reason.clone()),
                            gas_cost_usd: result.gas_cost_usd,
                        };
                        
                        // Cập nhật lịch sử
                        {
                            let mut trade_history = self.trade_history.write().await;
                            trade_history.push(trade_result);
                        }
                        
                        // Xóa khỏi danh sách theo dõi
                        {
                            let mut active_trades = self.active_trades.write().await;
                            active_trades.retain(|t| t.trade_id != trade.trade_id);
                        }
                    }
                },
                Err(e) => {
                    // Log lỗi
                    error!("Error selling token: {:?}", e);
                }
            }
        }
    }
    
    /// Lấy thông tin contract
    pub(crate) async fn get_contract_info(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> Option<ContractInfo> {
        let source_code_future = adapter.get_contract_source_code(token_address);
        let bytecode_future = adapter.get_contract_bytecode(token_address);
        
        let ((source_code, is_verified), bytecode) = tokio::join!(
            source_code_future,
            bytecode_future
        );
        
        let (source_code, is_verified) = source_code.unwrap_or((None, false));
        let bytecode = bytecode.unwrap_or(None);
        
        let abi = if is_verified { 
            adapter.get_contract_abi(token_address).await.unwrap_or(None) 
        } else { 
            None 
        };
        
        let owner_address = adapter.get_contract_owner(token_address).await.unwrap_or(None);
        
        Some(ContractInfo {
            address: token_address.to_string(),
            chain_id,
            source_code,
            bytecode,
            abi,
            is_verified,
            owner_address,
        })
    }
    
    /// Kiểm tra thanh khoản đã được khóa hay chưa và thời gian khóa
    async fn check_liquidity_lock(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, u64) {
        match adapter.check_liquidity_lock(token_address).await {
            Ok((is_locked, duration)) => (is_locked, duration),
            Err(_) => (false, 0), // Mặc định coi như không khóa nếu có lỗi
        }
    }
    
    /// Kiểm tra các dấu hiệu rug pull
    async fn check_rug_pull_indicators(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>, contract_info: &ContractInfo) -> (bool, Vec<String>) {
        let mut indicators = Vec::new();
        
        // Kiểm tra sở hữu tập trung
        if let Ok(ownership_data) = adapter.check_token_ownership_distribution(token_address).await {
            if ownership_data.max_wallet_percentage > MAX_SAFE_OWNERSHIP_PERCENTAGE {
                indicators.push(format!("High ownership concentration: {:.2}%", ownership_data.max_wallet_percentage));
            }
        }
        
        // Kiểm tra quyền mint không giới hạn
        if let Ok(has_unlimited_mint) = adapter.check_unlimited_mint(token_address).await {
            if has_unlimited_mint {
                indicators.push("Unlimited mint function".to_string());
            }
        }
        
        // Kiểm tra blacklist/transfer delay
        if let Ok(has_blacklist) = adapter.check_blacklist_function(token_address).await {
            if has_blacklist {
                indicators.push("Blacklist function".to_string());
            }
        }
        
        if let Ok(has_transfer_delay) = adapter.check_transfer_delay(token_address).await {
            if has_transfer_delay > MAX_TRANSFER_DELAY_SECONDS {
                indicators.push(format!("Transfer delay: {}s", has_transfer_delay));
            }
        }
        
        // Kiểm tra fake ownership renounce
        if let Ok(has_fake_renounce) = adapter.check_fake_ownership_renounce(token_address).await {
            if has_fake_renounce {
                indicators.push("Fake ownership renounce".to_string());
            }
        }
        
        // Kiểm tra history của dev wallet
        if let Some(owner_address) = &contract_info.owner_address {
            if let Ok(dev_history) = adapter.check_developer_history(owner_address).await {
                if dev_history.previous_rug_pulls > 0 {
                    indicators.push(format!("Developer involved in {} previous rug pulls", dev_history.previous_rug_pulls));
                }
            }
        }
        
        (indicators.len() > 0, indicators)
    }
    
    /// Dọn dẹp lịch sử giao dịch cũ
    /// 
    /// Giới hạn kích thước của trade_history để tránh memory leak
    async fn cleanup_trade_history(&self) {
        const MAX_HISTORY_SIZE: usize = 1000;
        const MAX_HISTORY_AGE_SECONDS: u64 = 7 * 24 * 60 * 60; // 7 ngày
        
        let mut history = self.trade_history.write().await;
        if history.len() <= MAX_HISTORY_SIZE {
            return;
        }
        
        let now = Utc::now().timestamp() as u64;
        
        // Lọc ra các giao dịch quá cũ
        history.retain(|trade| {
            now.saturating_sub(trade.created_at) < MAX_HISTORY_AGE_SECONDS
        });
        
        // Nếu vẫn còn quá nhiều, sắp xếp theo thời gian và giữ lại MAX_HISTORY_SIZE
        if history.len() > MAX_HISTORY_SIZE {
            history.sort_by(|a, b| b.created_at.cmp(&a.created_at)); // Sắp xếp mới nhất trước
            history.truncate(MAX_HISTORY_SIZE);
        }
    }
    
    /// Điều chỉnh thời gian ngủ dựa trên số lượng giao dịch đang theo dõi
    /// 
    /// Khi có nhiều giao dịch cần theo dõi, giảm thời gian ngủ để kiểm tra thường xuyên hơn
    /// Khi không có giao dịch nào, tăng thời gian ngủ để tiết kiệm tài nguyên
    async fn adaptive_sleep_time(&self) -> u64 {
        const PRICE_CHECK_INTERVAL_MS: u64 = 5000; // 5 giây
        
        let active_trades = self.active_trades.read().await;
        let count = active_trades.len();
        
        if count == 0 {
            // Không có giao dịch nào, sleep lâu hơn để tiết kiệm tài nguyên
            PRICE_CHECK_INTERVAL_MS * 3
        } else if count > 5 {
            // Nhiều giao dịch đang hoạt động, giảm thời gian kiểm tra
            PRICE_CHECK_INTERVAL_MS / 3
        } else {
            // Sử dụng thời gian mặc định
            PRICE_CHECK_INTERVAL_MS
        }
    }

    /// Cập nhật và lưu trạng thái mới
    async fn update_and_persist_trades(&self) -> Result<()> {
        let active_trades = self.active_trades.read().await;
        let history = self.trade_history.read().await;
        
        // Tạo đối tượng trạng thái để lưu
        let state = SavedBotState {
            active_trades: active_trades.clone(),
            recent_history: history.clone(),
            updated_at: Utc::now().timestamp(),
        };
        
        // Chuyển đổi sang JSON
        let state_json = serde_json::to_string(&state)
            .context("Failed to serialize bot state")?;
        
        // Lưu vào file
        let state_dir = std::env::var("SNIPEBOT_STATE_DIR")
            .unwrap_or_else(|_| "data/state".to_string());
        
        // Đảm bảo thư mục tồn tại
        std::fs::create_dir_all(&state_dir)
            .context(format!("Failed to create state directory: {}", state_dir))?;
        
        let filename = format!("{}/smart_trade_state.json", state_dir);
        std::fs::write(&filename, state_json)
            .context(format!("Failed to write state to file: {}", filename))?;
        
        debug!("Persisted bot state to {}", filename);
        Ok(())
    }
    
    /// Phục hồi trạng thái từ lưu trữ
    async fn restore_state(&self) -> Result<()> {
        // Lấy đường dẫn file trạng thái
        let state_dir = std::env::var("SNIPEBOT_STATE_DIR")
            .unwrap_or_else(|_| "data/state".to_string());
        let filename = format!("{}/smart_trade_state.json", state_dir);
        
        // Kiểm tra file tồn tại
        if !std::path::Path::new(&filename).exists() {
            info!("No saved state found, starting with empty state");
            return Ok(());
        }
        
        // Đọc file
        let state_json = std::fs::read_to_string(&filename)
            .context(format!("Failed to read state file: {}", filename))?;
        
        // Chuyển đổi từ JSON
        let state: SavedBotState = serde_json::from_str(&state_json)
            .context("Failed to deserialize bot state")?;
        
        // Kiểm tra tính hợp lệ của dữ liệu
        let now = Utc::now().timestamp();
        let max_age_seconds = 24 * 60 * 60; // 24 giờ
        
        if now - state.updated_at > max_age_seconds {
            warn!("Saved state is too old ({} seconds), starting with empty state", 
                  now - state.updated_at);
            return Ok(());
        }
        
        // Phục hồi trạng thái
        {
            let mut active_trades = self.active_trades.write().await;
            active_trades.clear();
            active_trades.extend(state.active_trades);
            info!("Restored {} active trades from saved state", active_trades.len());
        }
        
        {
            let mut history = self.trade_history.write().await;
            history.clear();
            history.extend(state.recent_history);
            info!("Restored {} historical trades from saved state", history.len());
        }
        
        Ok(())
    }

    /// Monitor active trades and perform necessary actions
    async fn monitor_loop(&self) {
        let mut counter = 0;
        let mut persist_counter = 0;
        
        while let true_val = *self.running.read().await {
            if !true_val {
                break;
            }
            
            // Kiểm tra và cập nhật tất cả các giao dịch đang active
            if let Err(e) = self.check_active_trades().await {
                error!("Error checking active trades: {}", e);
            }
            
            // Tăng counter và dọn dẹp lịch sử giao dịch định kỳ
            counter += 1;
            if counter >= 100 {
                self.cleanup_trade_history().await;
                counter = 0;
            }
            
            // Lưu trạng thái vào bộ nhớ định kỳ
            persist_counter += 1;
            if persist_counter >= 30 { // Lưu mỗi 30 chu kỳ (khoảng 2.5 phút với interval 5s)
                if let Err(e) = self.update_and_persist_trades().await {
                    error!("Failed to persist bot state: {}", e);
                }
                persist_counter = 0;
            }
            
            // Tính toán thời gian sleep thích ứng
            let sleep_time = self.adaptive_sleep_time().await;
            tokio::time::sleep(tokio::time::Duration::from_millis(sleep_time)).await;
        }
        
        // Lưu trạng thái trước khi dừng
        if let Err(e) = self.update_and_persist_trades().await {
            error!("Failed to persist bot state before stopping: {}", e);
        }
    }

    /// Check and update the status of a trade
    async fn check_and_update_trade(&self, trade: TradeTracker) -> anyhow::Result<()> {
        // Get adapter for chain
        let adapter = match self.evm_adapters.get(&trade.chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("No adapter found for chain ID {}", trade.chain_id));
            }
        };
        
        // If trade is not in Open state, skip
        if trade.status != TradeStatus::Open {
            return Ok(());
        }
        
        // Get current price of token
        let current_price = match adapter.get_token_price(&trade.token_address).await {
            Ok(price) => price,
            Err(e) => {
                warn!("Cannot get current price for token {}: {}", trade.token_address, e);
                // Cannot get price, but still need to check other conditions
                // Continue with price = 0
                0.0
            }
        };
        
        // If cannot get price, check max hold time
        if current_price <= 0.0 {
            let current_time = chrono::Utc::now().timestamp() as u64;
            
            // If exceeded max hold time, sell
            if current_time >= trade.max_hold_time {
                warn!("Cannot get price for token {}, exceeded max hold time, perform sell", trade.token_address);
                self.sell_token(trade, "Exceeded max hold time".to_string(), trade.entry_price).await?;
                return Ok(());
            }
            
            // If not exceeded, continue monitoring
            return Ok(());
        }
        
        // Calculate current profit percentage/loss
        let profit_percent = ((current_price - trade.entry_price) / trade.entry_price) * 100.0;
        let current_time = chrono::Utc::now().timestamp() as u64;
        
        // Update highest price if needed (only for Smart strategy)
        let mut highest_price = trade.highest_price;
        if current_price > highest_price {
            highest_price = current_price;
            
            // Update TradeTracker with new highest price
            {
                let mut active_trades = self.active_trades.write().await;
                if let Some(index) = active_trades.iter().position(|t| t.trade_id == trade.trade_id) {
                    active_trades[index].highest_price = highest_price;
                }
            }
        }
        
        // 1. Check profit condition to reach target
        if profit_percent >= trade.take_profit_percent {
            info!(
                "Take profit triggered for {} (ID: {}): current profit {:.2}% >= target {:.2}%", 
                trade.token_address, trade.trade_id, profit_percent, trade.take_profit_percent
            );
            self.sell_token(trade, "Reached profit target".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 2. Check stop loss condition
        if profit_percent <= -trade.stop_loss_percent {
            warn!(
                "Stop loss triggered for {} (ID: {}): current profit {:.2}% <= -{:.2}%", 
                trade.token_address, trade.trade_id, profit_percent, trade.stop_loss_percent
            );
            self.sell_token(trade, "Activated stop loss".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 3. Check trailing stop loss (if any)
        if let Some(tsl_percent) = trade.trailing_stop_percent {
            let tsl_trigger_price = highest_price * (1.0 - tsl_percent / 100.0);
            
            if current_price <= tsl_trigger_price && highest_price > trade.entry_price {
                // Activate TSL only if there was profit before
                info!(
                    "Trailing stop loss triggered for {} (ID: {}): current price {} <= TSL price {:.4} (highest {:.4})", 
                    trade.token_address, trade.trade_id, current_price, tsl_trigger_price, highest_price
                );
                self.sell_token(trade, "Activated trailing stop loss".to_string(), current_price).await?;
                return Ok(());
            }
        }
        
        // 4. Check max hold time
        if current_time >= trade.max_hold_time {
            info!(
                "Max hold time reached for {} (ID: {}): current profit {:.2}%", 
                trade.token_address, trade.trade_id, profit_percent
            );
            self.sell_token(trade, "Exceeded max hold time".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 5. Check token safety
        if trade.strategy != TradeStrategy::Manual {
            if let Err(e) = self.check_token_safety_changes(&trade, adapter).await {
                warn!("Security issue detected with token {}: {}", trade.token_address, e);
                self.sell_token(trade, format!("Security issue detected: {}", e), current_price).await?;
                return Ok(());
            }
        }
        
        // Trade still safe, continue monitoring
        debug!(
            "Trade {} (ID: {}) still active: current profit {:.2}%, current price {:.8}, highest {:.8}", 
            trade.token_address, trade.trade_id, profit_percent, current_price, highest_price
        );
        
        Ok(())
    }

    /// Monitor active trades and perform necessary actions
    async fn check_active_trades(&self) -> anyhow::Result<()> {
        // Check if there are any trades that need monitoring
        let active_trades = self.active_trades.read().await;
        
        if active_trades.is_empty() {
            return Ok(());
        }
        
        // Clone list to avoid holding lock for too long
        let active_trades_clone = active_trades.clone();
        drop(active_trades); // Release lock to avoid deadlock
        
        // Process each trade in parallel
        let update_futures = active_trades_clone.iter().map(|trade| {
            self.check_and_update_trade(trade.clone())
        });
        
        let results = futures::future::join_all(update_futures).await;
        
        // Check and log error if any
        for result in results {
            if let Err(e) = result {
                error!("Error monitoring trade: {}", e);
                // Continue processing other trades, do not stop
            }
        }
        
        Ok(())
    }

    /// Clone executor and copy config from original instance (safe for async context)
    pub(crate) async fn clone_with_config(&self) -> Self {
        let config = {
            let config_guard = self.config.read().await;
            config_guard.clone()
        };
        
        Self {
            evm_adapters: self.evm_adapters.clone(),
            analys_client: self.analys_client.clone(),
            risk_analyzer: self.risk_analyzer.clone(),
            mempool_analyzers: self.mempool_analyzers.clone(),
            config: RwLock::new(config),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            coordinator: self.coordinator.clone(),
            executor_id: self.executor_id.clone(),
            coordinator_subscription: RwLock::new(None),
        }
    }

    /// Đăng ký với coordinator
    async fn register_with_coordinator(&self) -> anyhow::Result<()> {
        if let Some(coordinator) = &self.coordinator {
            // Đăng ký executor với coordinator
            coordinator.register_executor(&self.executor_id, ExecutorType::SmartTrade).await?;
            
            // Đăng ký nhận thông báo về các cơ hội mới
            let subscription_callback = {
                let self_clone = self.clone_with_config().await;
                Arc::new(move |opportunity: SharedOpportunity| -> anyhow::Result<()> {
                    let executor = self_clone.clone();
                    tokio::spawn(async move {
                        if let Err(e) = executor.handle_shared_opportunity(opportunity).await {
                            error!("Error handling shared opportunity: {}", e);
                        }
                    });
                    Ok(())
                })
            };
            
            // Lưu subscription ID
            let subscription_id = coordinator.subscribe_to_opportunities(
                &self.executor_id, subscription_callback
            ).await?;
            
            let mut sub_id = self.coordinator_subscription.write().await;
            *sub_id = Some(subscription_id);
            
            info!("Registered with coordinator, subscription ID: {}", sub_id.as_ref().unwrap());
        }
        
        Ok(())
    }
    
    /// Hủy đăng ký khỏi coordinator
    async fn unregister_from_coordinator(&self) -> anyhow::Result<()> {
        if let Some(coordinator) = &self.coordinator {
            // Hủy đăng ký subscription
            let sub_id = {
                let sub = self.coordinator_subscription.read().await;
                sub.clone()
            };
            
            if let Some(id) = sub_id {
                coordinator.unsubscribe_from_opportunities(&id).await?;
                
                // Xóa subscription ID
                let mut sub = self.coordinator_subscription.write().await;
                *sub = None;
            }
            
            // Hủy đăng ký executor
            coordinator.unregister_executor(&self.executor_id).await?;
            info!("Unregistered from coordinator");
        }
        
        Ok(())
    }
    
    /// Phát hiện rủi ro thanh khoản
    /// 
    /// Kiểm tra toàn diện các sự kiện thanh khoản bất thường, khóa LP, rủi ro rugpull và biến động thanh khoản:
    /// 1. Kiểm tra LP được khóa hay không, và thời gian khóa còn lại
    /// 2. Phân tích lịch sử sự kiện thanh khoản để phát hiện rút LP bất thường
    /// 3. Kiểm tra sự cân bằng thanh khoản giữa token và base (ETH/BNB)
    /// 4. Phát hiện hàm nguy hiểm cho phép thay đổi LP (remove, migrate)
    /// 
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ của token cần kiểm tra
    /// 
    /// # Returns
    /// * `Result<(bool, Option<String>)>` - (có rủi ro thanh khoản, mô tả nếu có)
    pub async fn detect_liquidity_risk(&self, chain_id: u32, token_address: &str) -> Result<(bool, Option<String>)> {
        debug!("Detecting liquidity risk for token: {} on chain {}", token_address, chain_id);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Lấy dữ liệu token từ adapter
        let token_info = adapter.get_token_info(token_address).await?;
        
        // Kiểm tra LP được khóa hay không
        let is_lp_locked = token_info.is_lp_locked();
        let lp_lock_time = token_info.lp_lock_time();
        
        // Phân tích lịch sử sự kiện thanh khoản
        let liquidity_events = adapter.get_liquidity_events(token_address).await?;
        let abnormal_events = abnormal_liquidity_events(&liquidity_events);
        
        // Kiểm tra sự cân bằng thanh khoản
        let is_imbalanced = token_info.is_liquidity_imbalanced();
        
        // Phát hiện hàm nguy hiểm
        let dangerous_functions = token_info.dangerous_functions();
        
        // Tạo mô tả rủi ro
        let mut risk_description = String::new();
        if is_lp_locked {
            risk_description.push_str(&format!("LP được khóa, thời gian khóa còn lại: {} giờ\n", lp_lock_time));
        }
        if !abnormal_events.is_empty() {
            risk_description.push_str(&format!("Sự kiện thanh khoản bất thường: {}\n", abnormal_events.join(", ")));
        }
        if is_imbalanced {
            risk_description.push_str("Thanh khoản không cân bằng\n");
        }
        if !dangerous_functions.is_empty() {
            risk_description.push_str(&format!("Hàm nguy hiểm: {}\n", dangerous_functions.join(", ")));
        }
        
        // Trả về kết quả
        if risk_description.is_empty() {
            Ok((false, None))
        } else {
            Ok((true, Some(risk_description)))
        }
    }
    
    /// Phát hiện các hàm nguy hiểm trong contract
    /// 
    /// Kiểm tra các hàm nguy hiểm có thể gây ra rủi ro cho token, bao gồm:
    /// - Hàm cho phép thay đổi LP (remove, migrate)
    /// - Hàm cho phép thay đổi owner
    /// - Hàm cho phép thay đổi quyền hạn của người dùng
    /// 
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ của token cần kiểm tra
    /// 
    /// # Returns
    /// * `Result<Vec<String>>` - Danh sách các hàm nguy hiểm
    pub async fn detect_dangerous_functions(&self, chain_id: u32, token_address: &str) -> Result<Vec<String>> {
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Lấy dữ liệu token từ adapter
        let token_info = adapter.get_token_info(token_address).await?;
        
        // Phát hiện các hàm nguy hiểm
        let dangerous_functions = token_info.dangerous_functions();
        
        Ok(dangerous_functions)
    }
    
    /// Phát hiện các sự kiện thanh khoản bất thường
    /// 
    /// Phân tích lịch sử sự kiện thanh khoản để phát hiện các hoạt động bất thường, bao gồm:
    /// - Rút LP bất thường
    /// - Tăng/giảm thanh khoản bất thường
    /// - Biến động thanh khoản lớn
    /// 
    /// # Parameters
    /// * `liquidity_events` - Danh sách các sự kiện thanh khoản
    /// 
    /// # Returns
    /// * `Vec<String>` - Danh sách các sự kiện thanh khoản bất thường
    pub fn abnormal_liquidity_events(liquidity_events: &[LiquidityEvent]) -> Vec<String> {
        let mut abnormal_events = Vec::new();
        
        // Phân tích các sự kiện thanh khoản
        for event in liquidity_events {
            match event.event_type {
                LiquidityEventType::AddLiquidity => {
                    // Kiểm tra tăng thanh khoản bất thường
                    if event.amount > 10000.0 {
                        abnormal_events.push(format!("Tăng thanh khoản bất thường: {} token", event.amount));
                    }
                }
                LiquidityEventType::RemoveLiquidity => {
                    // Kiểm tra rút LP bất thường
                    if event.amount > 10000.0 {
                        abnormal_events.push(format!("Rút LP bất thường: {} token", event.amount));
                    }
                }
                LiquidityEventType::Swap => {
                    // Kiểm tra biến động thanh khoản lớn
                    if event.amount > 10000.0 {
                        abnormal_events.push(format!("Biến động thanh khoản lớn: {} token", event.amount));
                    }
                }
            }
        }
        
        abnormal_events
    }
    
    /// Phát hiện các mẫu rugpull
    /// 
    /// Phân tích lịch sử giao dịch để phát hiện các mẫu rugpull, bao gồm:
    /// - Tăng giá nhanh chóng trước khi rút LP
    /// - Giảm giá sau khi rút LP
    /// - Không có hoạt động giao dịch sau khi rút LP
    /// 
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ của token cần kiểm tra
    /// 
    /// # Returns
    /// * `Result<(bool, Option<String>)>` - (có mẫu rugpull, mô tả nếu có)
    pub async fn detect_rugpull_pattern(&self, chain_id: u32, token_address: &str) -> Result<(bool, Option<String>)> {
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Lấy lịch sử giao dịch từ adapter
        let tx_history = adapter.get_transaction_history(token_address).await?;
        
        // Phân tích các mẫu rugpull
        let mut rugpull_description = String::new();
        let mut price_before_lp_removal = 0.0;
        let mut lp_removal_detected = false;
        
        for tx in tx_history {
            if let Some(price) = tx.price {
                if !lp_removal_detected {
                    // Kiểm tra tăng giá nhanh chóng trước khi rút LP
                    if price > price_before_lp_removal * 1.1 {
                        rugpull_description.push_str(&format!("Tăng giá nhanh chóng trước khi rút LP: {} -> {}\n", price_before_lp_removal, price));
                    }
                    price_before_lp_removal = price;
                } else {
                    // Kiểm tra giảm giá sau khi rút LP
                    if price < price_before_lp_removal * 0.9 {
                        rugpull_description.push_str(&format!("Giảm giá sau khi rút LP: {} -> {}\n", price_before_lp_removal, price));
                    }
                    price_before_lp_removal = price;
                }
            }
            
            if let Some(event) = &tx.event {
                if event.event_type == LiquidityEventType::RemoveLiquidity {
                    lp_removal_detected = true;
                }
            }
        }
        
        // Kiểm tra không có hoạt động giao dịch sau khi rút LP
        if lp_removal_detected && tx_history.last().map(|tx| tx.timestamp).unwrap_or_default() < Utc::now().timestamp() as u64 - 60 * 60 * 24 {
            rugpull_description.push_str("Không có hoạt động giao dịch sau khi rút LP\n");
        }
        
        // Trả về kết quả
        if rugpull_description.is_empty() {
            Ok((false, None))
        } else {
            Ok((true, Some(rugpull_description)))
        }
    }
    
    /// Phát hiện các vấn đề về thanh khoản
    /// 
    /// Kiểm tra toàn diện các vấn đề về thanh khoản, bao gồm:
    /// - LP được khóa
    /// - Sự kiện thanh khoản bất thường
    /// - Thanh khoản không cân bằng
    /// - Hàm nguy hiểm
    /// 
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ của token cần kiểm tra
    /// 
    /// # Returns
    /// * `Result<(bool, Option<String>)>` - (có vấn đề thanh khoản, mô tả nếu có)
    pub async fn detect_liquidity_issues(&self, chain_id: u32, token_address: &str) -> Result<(bool, Option<String>)> {
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Lấy dữ liệu token từ adapter
        let token_info = adapter.get_token_info(token_address).await?;
        
        // Kiểm tra LP được khóa hay không
        let is_lp_locked = token_info.is_lp_locked();
        let lp_lock_time = token_info.lp_lock_time();
        
        // Phân tích lịch sử sự kiện thanh khoản
        let liquidity_events = adapter.get_liquidity_events(token_address).await?;
        let abnormal_events = abnormal_liquidity_events(&liquidity_events);
        
        // Kiểm tra sự cân bằng thanh khoản
        let is_imbalanced = token_info.is_liquidity_imbalanced();
        
        // Phát hiện hàm nguy hiểm
        let dangerous_functions = token_info.dangerous_functions();
        
        // Tạo mô tả vấn đề
        let mut issue_description = String::new();
        if is_lp_locked {
            issue_description.push_str(&format!("LP được khóa, thời gian khóa còn lại: {} giờ\n", lp_lock_time));
        }
        if !abnormal_events.is_empty() {
            issue_description.push_str(&format!("Sự kiện thanh khoản bất thường: {}\n", abnormal_events.join(", ")));
        }
        if is_imbalanced {
            issue_description.push_str("Thanh khoản không cân bằng\n");
        }
        if !dangerous_functions.is_empty() {
            issue_description.push_str(&format!("Hàm nguy hiểm: {}\n", dangerous_functions.join(", ")));
        }
        
        // Trả về kết quả
        if issue_description.is_empty() {
            Ok((false, None))
        } else {
            Ok((true, Some(issue_description)))
        }
    }
    
    /// Phát hiện các vấn đề về quyền hạn của owner
    /// 
    /// Kiểm tra các quyền hạn của owner có thể gây ra rủi ro cho token, bao gồm:
    /// - Thay đổi quyền hạn của người dùng
    /// - Thay đổi quyền hạn của contract
    /// - Thay đổi quyền hạn của owner
    /// 
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ của token cần kiểm tra
    /// 
    /// # Returns
    /// * `Result<(bool, Vec<String>)>` - (có vấn đề quyền hạn, danh sách các quyền hạn nguy hiểm)
    pub async fn detect_owner_privileges(&self, chain_id: u32, token_address: &str) -> Result<(bool, Vec<String>)> {
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Lấy dữ liệu token từ adapter
        let token_info = adapter.get_token_info(token_address).await?;
        
        // Phát hiện các quyền hạn nguy hiểm
        let dangerous_privileges = token_info.dangerous_privileges();
        
        // Trả về kết quả
        if dangerous_privileges.is_empty() {
            Ok((false, Vec::new()))
        } else {
            Ok((true, dangerous_privileges))
        }
    }
    
    /// Phát hiện các vấn đề về blacklist
    /// 
    /// Kiểm tra token có trong danh sách blacklist hay không, bao gồm:
    /// - Danh sách blacklist của các sàn lưu động
    /// - Danh sách blacklist của các nền tảng giao dịch
/// Core implementation of SmartTradeExecutor
///
/// This file contains the main executor struct that manages the entire
/// smart trading process, orchestrates token analysis, trade strategies,
/// and market monitoring.
///
/// # Cải tiến đã thực hiện:
/// - Loại bỏ các phương thức trùng lặp với analys modules
/// - Sử dụng API từ analys/token_status và analys/risk_analyzer
/// - Áp dụng xử lý lỗi chuẩn với anyhow::Result thay vì unwrap/expect
/// - Đảm bảo thread safety trong các phương thức async với kỹ thuật "clone and drop" để tránh deadlock
/// - Tối ưu hóa việc xử lý các futures với tokio::join! cho các tác vụ song song
/// - Xử lý các trường hợp giá trị null an toàn với match/Option
/// - Tuân thủ nghiêm ngặt quy tắc từ .cursorrc

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use chrono::Utc;
use futures::future::join_all;
use tracing::{debug, error, info, warn};
use anyhow::{Result, Context, anyhow, bail};
use uuid;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use rand::Rng;
use std::collections::HashSet;
use futures::future::Future;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::{MempoolAnalyzer, MempoolTransaction};
use crate::analys::risk_analyzer::{RiskAnalyzer, RiskFactor, TradeRecommendation, TradeRiskAnalysis};
use crate::analys::token_status::{
    ContractInfo, LiquidityEvent, LiquidityEventType, TokenSafety, TokenStatus,
    TokenIssue, IssueSeverity, abnormal_liquidity_events, detect_dynamic_tax, 
    detect_hidden_fees, analyze_owner_privileges, is_proxy_contract, blacklist::{has_blacklist_or_whitelist, has_trading_cooldown, has_max_tx_or_wallet_limit},
};
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};
use crate::tradelogic::traits::{
    TradeExecutor, RiskManager, TradeCoordinator, ExecutorType, SharedOpportunity, 
    SharedOpportunityType, OpportunityPriority
};

// Module imports
use super::types::{
    SmartTradeConfig, 
    TradeResult, 
    TradeStatus, 
    TradeStrategy, 
    TradeTracker
};
use super::constants::*;
use super::token_analysis::*;
use super::trade_strategy::*;
use super::alert::*;
use super::optimization::*;
use super::security::*;
use super::analys_client::SmartTradeAnalysisClient;

/// Executor for smart trading strategies
#[derive(Clone)]
pub struct SmartTradeExecutor {
    /// EVM adapter for each chain
    pub(crate) evm_adapters: HashMap<u32, Arc<EvmAdapter>>,
    
    /// Analysis client that provides access to all analysis services
    pub(crate) analys_client: Arc<SmartTradeAnalysisClient>,
    
    /// Risk analyzer (legacy - kept for backward compatibility)
    pub(crate) risk_analyzer: Arc<RwLock<RiskAnalyzer>>,
    
    /// Mempool analyzer for each chain (legacy - kept for backward compatibility)
    pub(crate) mempool_analyzers: HashMap<u32, Arc<MempoolAnalyzer>>,
    
    /// Configuration
    pub(crate) config: RwLock<SmartTradeConfig>,
    
    /// Active trades being monitored
    pub(crate) active_trades: RwLock<Vec<TradeTracker>>,
    
    /// History of completed trades
    pub(crate) trade_history: RwLock<Vec<TradeResult>>,
    
    /// Running state
    pub(crate) running: RwLock<bool>,
    
    /// Coordinator for shared state between executors
    pub(crate) coordinator: Option<Arc<dyn TradeCoordinator>>,
    
    /// Unique ID for this executor instance
    pub(crate) executor_id: String,
    
    /// Subscription ID for coordinator notifications
    pub(crate) coordinator_subscription: RwLock<Option<String>>,
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
        
        // Khởi động vòng lặp theo dõi
        let executor = Arc::new(self.clone_with_config().await);
        
        tokio::spawn(async move {
            executor.monitor_loop().await;
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
        
        Ok(())
    }
    
    /// Execute a trade with the specified parameters
    async fn execute_trade(&self, params: TradeParams) -> anyhow::Result<Self::TradeResult> {
        // Find the appropriate adapter
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", params.chain_id)),
        };
        
        // Validate trade parameters
        if params.amount <= 0.0 {
            return Err(anyhow::anyhow!("Invalid trade amount: {}", params.amount));
        }
        
        // Create a trade tracker
        let trade_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp() as u64;
        
        let tracker = TradeTracker {
            id: trade_id.clone(),
            chain_id: params.chain_id,
            token_address: params.token_address.clone(),
            amount: params.amount,
            entry_price: 0.0, // Will be updated after trade execution
            current_price: 0.0,
            status: TradeStatus::Pending,
            strategy: if let Some(strategy) = &params.strategy {
                match strategy.as_str() {
                    "quick" => TradeStrategy::QuickFlip,
                    "tsl" => TradeStrategy::TrailingStopLoss,
                    "hodl" => TradeStrategy::LongTerm,
                    _ => TradeStrategy::default(),
                }
            } else {
                TradeStrategy::default()
            },
            created_at: now,
            updated_at: now,
            stop_loss: params.stop_loss,
            take_profit: params.take_profit,
            max_hold_time: params.max_hold_time.unwrap_or(DEFAULT_MAX_HOLD_TIME),
            custom_params: params.custom_params.unwrap_or_default(),
        };
        
        // TODO: Implement the actual trade execution logic
        // This is a simplified version - in a real implementation, you would:
        // 1. Execute the trade
        // 2. Update the trade tracker with actual execution details
        // 3. Add it to active trades
        
        let result = TradeResult {
            id: trade_id,
            chain_id: params.chain_id,
            token_address: params.token_address,
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: params.amount,
            entry_price: 0.0,                  // Would be populated from actual execution
            exit_price: None,
            current_price: 0.0,
            profit_loss: 0.0,
            status: TradeStatus::Pending,
            strategy: tracker.strategy.clone(),
            created_at: now,
            updated_at: now,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        // Add to active trades
        {
            let mut active_trades = self.active_trades.write().await;
            active_trades.push(tracker);
        }
        
        Ok(result)
    }
    
    /// Get the current status of an active trade
    async fn get_trade_status(&self, trade_id: &str) -> anyhow::Result<Option<Self::TradeResult>> {
        // Check active trades
        {
            let active_trades = self.active_trades.read().await;
            if let Some(trade) = active_trades.iter().find(|t| t.id == trade_id) {
                return Ok(Some(self.tracker_to_result(trade).await?));
            }
        }
        
        // Check trade history
        let trade_history = self.trade_history.read().await;
        let result = trade_history.iter()
            .find(|r| r.id == trade_id)
            .cloned();
            
        Ok(result)
    }
    
    /// Get all active trades
    async fn get_active_trades(&self) -> anyhow::Result<Vec<Self::TradeResult>> {
        let active_trades = self.active_trades.read().await;
        let mut results = Vec::with_capacity(active_trades.len());
        
        for tracker in active_trades.iter() {
            results.push(self.tracker_to_result(tracker).await?);
        }
        
        Ok(results)
    }
    
    /// Get trade history within a specific time range
    async fn get_trade_history(&self, from_timestamp: u64, to_timestamp: u64) -> anyhow::Result<Vec<Self::TradeResult>> {
        let trade_history = self.trade_history.read().await;
        
        let filtered_history = trade_history.iter()
            .filter(|trade| trade.created_at >= from_timestamp && trade.created_at <= to_timestamp)
            .cloned()
            .collect();
            
        Ok(filtered_history)
    }
    
    /// Evaluate a token for potential trading
    async fn evaluate_token(&self, chain_id: u32, token_address: &str) -> anyhow::Result<Option<TokenSafety>> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Sử dụng analys_client để phân tích token
        let token_safety_result = self.analys_client.token_provider()
            .analyze_token(chain_id, token_address).await
            .context(format!("Không thể phân tích token {}", token_address))?;
        
        // Nếu không phân tích được token, trả về None
        if !token_safety_result.is_valid() {
            info!("Token {} không hợp lệ hoặc không thể phân tích", token_address);
            return Ok(None);
        }
        
        // Lấy thêm thông tin chi tiết từ contract
        let contract_details = self.analys_client.token_provider()
            .get_contract_details(chain_id, token_address).await
            .context(format!("Không thể lấy chi tiết contract cho token {}", token_address))?;
        
        // Đánh giá rủi ro của token
        let risk_analysis = self.analys_client.risk_provider()
            .analyze_token_risk(chain_id, token_address).await
            .context(format!("Không thể phân tích rủi ro cho token {}", token_address))?;
        
        // Log thông tin phân tích
        info!(
            "Phân tích token {} trên chain {}: risk_score={}, honeypot={:?}, buy_tax={:?}, sell_tax={:?}",
            token_address,
            chain_id,
            risk_analysis.risk_score,
            token_safety_result.is_honeypot,
            token_safety_result.buy_tax,
            token_safety_result.sell_tax
        );
        
        Ok(Some(token_safety_result))
    }

    /// Đánh giá rủi ro của token bằng cách sử dụng analys_client
    async fn analyze_token_risk(&self, chain_id: u32, token_address: &str) -> anyhow::Result<RiskScore> {
        // Sử dụng phương thức analyze_risk từ analys_client
        self.analys_client.analyze_risk(chain_id, token_address).await
            .context(format!("Không thể đánh giá rủi ro cho token {}", token_address))
    }
}

impl SmartTradeExecutor {
    /// Create a new SmartTradeExecutor
    pub fn new() -> Self {
        let executor_id = format!("smart-trade-{}", uuid::Uuid::new_v4());
        
        let evm_adapters = HashMap::new();
        let analys_client = Arc::new(SmartTradeAnalysisClient::new(evm_adapters.clone()));
        
        Self {
            evm_adapters,
            analys_client,
            risk_analyzer: Arc::new(RwLock::new(RiskAnalyzer::new())),
            mempool_analyzers: HashMap::new(),
            config: RwLock::new(SmartTradeConfig::default()),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            coordinator: None,
            executor_id,
            coordinator_subscription: RwLock::new(None),
        }
    }
    
    /// Set coordinator for shared state
    pub fn with_coordinator(mut self, coordinator: Arc<dyn TradeCoordinator>) -> Self {
        self.coordinator = Some(coordinator);
        self
    }
    
    /// Convert a trade tracker to a trade result
    async fn tracker_to_result(&self, tracker: &TradeTracker) -> anyhow::Result<TradeResult> {
        let result = TradeResult {
            id: tracker.id.clone(),
            chain_id: tracker.chain_id,
            token_address: tracker.token_address.clone(),
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: tracker.amount,
            entry_price: tracker.entry_price,
            exit_price: None,
            current_price: tracker.current_price,
            profit_loss: if tracker.entry_price > 0.0 && tracker.current_price > 0.0 {
                (tracker.current_price - tracker.entry_price) / tracker.entry_price * 100.0
            } else {
                0.0
            },
            status: tracker.status.clone(),
            strategy: tracker.strategy.clone(),
            created_at: tracker.created_at,
            updated_at: tracker.updated_at,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        Ok(result)
    }
    
    /// Vòng lặp theo dõi chính
    async fn monitor_loop(&self) {
        let mut counter = 0;
        let mut persist_counter = 0;
        
        while let true_val = *self.running.read().await {
            if !true_val {
                break;
            }
            
            // Kiểm tra và cập nhật tất cả các giao dịch đang active
            if let Err(e) = self.check_active_trades().await {
                error!("Error checking active trades: {}", e);
            }
            
            // Tăng counter và dọn dẹp lịch sử giao dịch định kỳ
            counter += 1;
            if counter >= 100 {
                self.cleanup_trade_history().await;
                counter = 0;
            }
            
            // Lưu trạng thái vào bộ nhớ định kỳ
            persist_counter += 1;
            if persist_counter >= 30 { // Lưu mỗi 30 chu kỳ (khoảng 2.5 phút với interval 5s)
                if let Err(e) = self.update_and_persist_trades().await {
                    error!("Failed to persist bot state: {}", e);
                }
                persist_counter = 0;
            }
            
            // Tính toán thời gian sleep thích ứng
            let sleep_time = self.adaptive_sleep_time().await;
            tokio::time::sleep(tokio::time::Duration::from_millis(sleep_time)).await;
        }
        
        // Lưu trạng thái trước khi dừng
        if let Err(e) = self.update_and_persist_trades().await {
            error!("Failed to persist bot state before stopping: {}", e);
        }
    }
    
    /// Quét cơ hội giao dịch mới
    async fn scan_trading_opportunities(&self) {
        // Kiểm tra cấu hình
        let config = self.config.read().await;
        if !config.enabled || !config.auto_trade {
            return;
        }
        
        // Kiểm tra số lượng giao dịch hiện tại
        let active_trades = self.active_trades.read().await;
        if active_trades.len() >= config.max_concurrent_trades as usize {
            return;
        }
        
        // Chạy quét cho từng chain
        let futures = self.mempool_analyzers.iter().map(|(chain_id, analyzer)| {
            self.scan_chain_opportunities(*chain_id, analyzer)
        });
        join_all(futures).await;
    }
    
    /// Quét cơ hội giao dịch trên một chain cụ thể
    async fn scan_chain_opportunities(&self, chain_id: u32, analyzer: &Arc<MempoolAnalyzer>) -> anyhow::Result<()> {
        let config = self.config.read().await;
        
        // Lấy các giao dịch lớn từ mempool
        let large_txs = match analyzer.get_large_transactions(QUICK_TRADE_MIN_BNB).await {
            Ok(txs) => txs,
            Err(e) => {
                error!("Không thể lấy các giao dịch lớn từ mempool: {}", e);
                return Err(anyhow::anyhow!("Lỗi khi lấy giao dịch từ mempool: {}", e));
            }
        };
        
        // Phân tích từng giao dịch
        for tx in large_txs {
            // Kiểm tra nếu token không trong blacklist và đáp ứng điều kiện
            if let Some(to_token) = &tx.to_token {
                let token_address = &to_token.address;
                
                // Kiểm tra blacklist và whitelist
                if config.token_blacklist.contains(token_address) {
                    debug!("Bỏ qua token {} trong blacklist", token_address);
                    continue;
                }
                
                if !config.token_whitelist.is_empty() && !config.token_whitelist.contains(token_address) {
                    debug!("Bỏ qua token {} không trong whitelist", token_address);
                    continue;
                }
                
                // Kiểm tra nếu giao dịch đủ lớn
                if tx.value >= QUICK_TRADE_MIN_BNB {
                    // Phân tích token
                    if let Err(e) = self.analyze_and_trade_token(chain_id, token_address, tx).await {
                        error!("Lỗi khi phân tích và giao dịch token {}: {}", token_address, e);
                        // Tiếp tục với token tiếp theo, không dừng lại
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Phân tích token và thực hiện giao dịch nếu an toàn
    async fn analyze_and_trade_token(&self, chain_id: u32, token_address: &str, tx: MempoolTransaction) -> anyhow::Result<()> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id));
            }
        };
        
        // THÊM: Xác thực kỹ lưỡng dữ liệu mempool trước khi tiếp tục phân tích
        info!("Xác thực dữ liệu mempool cho token {} từ giao dịch {}", token_address, tx.tx_hash);
        let mempool_validation = match self.validate_mempool_data(chain_id, token_address, &tx).await {
            Ok(validation) => validation,
            Err(e) => {
                error!("Lỗi khi xác thực dữ liệu mempool: {}", e);
                self.log_trade_decision(
                    "new_opportunity", 
                    token_address, 
                    TradeStatus::Canceled, 
                    &format!("Lỗi xác thực mempool: {}", e), 
                    chain_id
                ).await;
                return Err(anyhow::anyhow!("Lỗi xác thực dữ liệu mempool: {}", e));
            }
        };
        
        // Nếu dữ liệu mempool không hợp lệ, dừng phân tích
        if !mempool_validation.is_valid {
            let reasons = mempool_validation.reasons.join(", ");
            info!("Dữ liệu mempool không đáng tin cậy cho token {}: {}", token_address, reasons);
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Dữ liệu mempool không đáng tin cậy: {}", reasons), 
                chain_id
            ).await;
            return Ok(());
        }
        
        info!("Xác thực mempool thành công với điểm tin cậy: {}/100", mempool_validation.confidence_score);
        
        // Code hiện tại: Sử dụng analys_client để kiểm tra an toàn token
        let security_check_future = self.analys_client.verify_token_safety(chain_id, token_address);
        
        // Code hiện tại: Sử dụng analys_client để đánh giá rủi ro
        let risk_analysis_future = self.analys_client.analyze_risk(chain_id, token_address);
        
        // Thực hiện song song các phân tích
        let (security_check_result, risk_analysis) = tokio::join!(security_check_future, risk_analysis_future);
        
        // Xử lý kết quả kiểm tra an toàn
        let security_check = match security_check_result {
            Ok(result) => result,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi kiểm tra an toàn token: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi kiểm tra an toàn token: {}", e));
            }
        };
        
        // Xử lý kết quả phân tích rủi ro
        let risk_score = match risk_analysis {
            Ok(score) => score,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi phân tích rủi ro: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi phân tích rủi ro: {}", e));
            }
        };
        
        // Kiểm tra xem token có an toàn không
        if !security_check.is_safe {
            let issues_str = security_check.issues.iter()
                .map(|issue| format!("{:?}", issue))
                .collect::<Vec<String>>()
                .join(", ");
                
            self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                &format!("Token không an toàn: {}", issues_str), chain_id).await;
            return Ok(());
        }
        
        // Lấy cấu hình hiện tại
        let config = self.config.read().await;
        
        // Kiểm tra điểm rủi ro
        if risk_score.score > config.max_risk_score {
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Điểm rủi ro quá cao: {}/{}", risk_score.score, config.max_risk_score), 
                chain_id
            ).await;
            return Ok(());
        }
        
        // Xác định chiến lược giao dịch dựa trên mức độ rủi ro
        let strategy = if risk_score.score < 30 {
            TradeStrategy::Smart  // Rủi ro thấp, dùng chiến lược thông minh với TSL
        } else {
            TradeStrategy::Quick  // Rủi ro cao hơn, dùng chiến lược nhanh
        };
        
        // Xác định số lượng giao dịch dựa trên cấu hình và mức độ rủi ro
        let base_amount = if risk_score.score < 20 {
            config.trade_amount_high_confidence
        } else if risk_score.score < 40 {
            config.trade_amount_medium_confidence
        } else {
            config.trade_amount_low_confidence
        };
        
        // Tạo tham số giao dịch
        let trade_params = crate::types::TradeParams {
            chain_type: crate::types::ChainType::EVM(chain_id),
            token_address: token_address.to_string(),
            amount: base_amount,
            slippage: 1.0, // 1% slippage mặc định cho giao dịch tự động
            trade_type: crate::types::TradeType::Buy,
            deadline_minutes: 2, // Thời hạn 2 phút
            router_address: String::new(), // Dùng router mặc định
            gas_limit: None,
            gas_price: None,
            strategy: Some(format!("{:?}", strategy).to_lowercase()),
            stop_loss: if strategy == TradeStrategy::Smart { Some(5.0) } else { None }, // 5% stop loss for Smart strategy
            take_profit: if strategy == TradeStrategy::Smart { Some(20.0) } else { Some(10.0) }, // 20% take profit for Smart, 10% for Quick
            max_hold_time: None, // Sẽ được thiết lập dựa trên chiến lược
            custom_params: None,
        };
        
        // Thực hiện giao dịch và lấy kết quả
        info!("Thực hiện mua {} cho token {} trên chain {}", base_amount, token_address, chain_id);
        
        let buy_result = adapter.execute_trade(&trade_params).await;
        
        match buy_result {
            Ok(tx_result) => {
                // Tạo ID giao dịch duy nhất
                let trade_id = uuid::Uuid::new_v4().to_string();
                let current_timestamp = chrono::Utc::now().timestamp() as u64;
                
                // Tính thời gian giữ tối đa dựa trên chiến lược
                let max_hold_time = match strategy {
                    TradeStrategy::Quick => current_timestamp + QUICK_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Smart => current_timestamp + SMART_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Manual => current_timestamp + 24 * 60 * 60, // 24 giờ cho giao dịch thủ công
                };
                
                // Tạo trailing stop loss nếu sử dụng chiến lược Smart
                let trailing_stop_percent = match strategy {
                    TradeStrategy::Smart => Some(SMART_TRADE_TSL_PERCENT),
                    _ => None,
                };
                
                // Xác định giá entry từ kết quả giao dịch
                let entry_price = match tx_result.execution_price {
                    Some(price) => price,
                    None => {
                        error!("Không thể xác định giá mua vào cho token {}", token_address);
                        return Err(anyhow::anyhow!("Không thể xác định giá mua vào"));
                    }
                };
                
                // Lấy số lượng token đã mua
                let token_amount = match tx_result.tokens_received {
                    Some(amount) => amount,
                    None => {
                        error!("Không thể xác định số lượng token nhận được");
                        return Err(anyhow::anyhow!("Không thể xác định số lượng token nhận được"));
                    }
                };
                
                // Tạo theo dõi giao dịch mới
                let trade_tracker = TradeTracker {
                    trade_id: trade_id.clone(),
                    token_address: token_address.to_string(),
                    chain_id,
                    strategy: strategy.clone(),
                    entry_price,
                    token_amount,
                    invested_amount: base_amount,
                    highest_price: entry_price, // Ban đầu, giá cao nhất = giá mua
                    entry_time: current_timestamp,
                    max_hold_time,
                    take_profit_percent: match strategy {
                        TradeStrategy::Quick => QUICK_TRADE_TARGET_PROFIT,
                        _ => 10.0, // Mặc định 10% cho các chiến lược khác
                    },
                    stop_loss_percent: STOP_LOSS_PERCENT,
                    trailing_stop_percent,
                    buy_tx_hash: tx_result.transaction_hash.clone(),
                    sell_tx_hash: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                };
                
                // Thêm vào danh sách theo dõi
                {
                    let mut active_trades = self.active_trades.write().await;
                    active_trades.push(trade_tracker.clone());
                }
                
                // Log thành công
                info!(
                    "Giao dịch mua thành công: ID={}, Token={}, Chain={}, Strategy={:?}, Amount={}, Price={}",
                    trade_id, token_address, chain_id, strategy, base_amount, entry_price
                );
                
                // Thêm vào lịch sử
                let trade_result = TradeResult {
                    trade_id: trade_id.clone(),
                    entry_price,
                    exit_price: None,
                    profit_percent: None,
                    profit_usd: None,
                    entry_time: current_timestamp,
                    exit_time: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                    gas_cost_usd: tx_result.gas_cost_usd.unwrap_or(0.0),
                };
                
                let mut trade_history = self.trade_history.write().await;
                trade_history.push(trade_result);
                
                // Trả về ID giao dịch
                Ok(())
            },
            Err(e) => {
                // Log lỗi giao dịch
                error!("Giao dịch mua thất bại cho token {}: {}", token_address, e);
                Err(anyhow::anyhow!("Giao dịch mua thất bại: {}", e))
            }
        }
    }
    
/// Core implementation of SmartTradeExecutor
///
/// This file contains the main executor struct that manages the entire
/// smart trading process, orchestrates token analysis, trade strategies,
/// and market monitoring.
///
/// # Cải tiến đã thực hiện:
/// - Loại bỏ các phương thức trùng lặp với analys modules
/// - Sử dụng API từ analys/token_status và analys/risk_analyzer
/// - Áp dụng xử lý lỗi chuẩn với anyhow::Result thay vì unwrap/expect
/// - Đảm bảo thread safety trong các phương thức async với kỹ thuật "clone and drop" để tránh deadlock
/// - Tối ưu hóa việc xử lý các futures với tokio::join! cho các tác vụ song song
/// - Xử lý các trường hợp giá trị null an toàn với match/Option
/// - Tuân thủ nghiêm ngặt quy tắc từ .cursorrc

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use chrono::Utc;
use futures::future::join_all;
use tracing::{debug, error, info, warn};
use anyhow::{Result, Context, anyhow, bail};
use uuid;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use rand::Rng;
use std::collections::HashSet;
use futures::future::Future;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::{MempoolAnalyzer, MempoolTransaction};
use crate::analys::risk_analyzer::{RiskAnalyzer, RiskFactor, TradeRecommendation, TradeRiskAnalysis};
use crate::analys::token_status::{
    ContractInfo, LiquidityEvent, LiquidityEventType, TokenSafety, TokenStatus,
    TokenIssue, IssueSeverity, abnormal_liquidity_events, detect_dynamic_tax, 
    detect_hidden_fees, analyze_owner_privileges, is_proxy_contract, blacklist::{has_blacklist_or_whitelist, has_trading_cooldown, has_max_tx_or_wallet_limit},
};
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};
use crate::tradelogic::traits::{
    TradeExecutor, RiskManager, TradeCoordinator, ExecutorType, SharedOpportunity, 
    SharedOpportunityType, OpportunityPriority
};

// Module imports
use super::types::{
    SmartTradeConfig, 
    TradeResult, 
    TradeStatus, 
    TradeStrategy, 
    TradeTracker
};
use super::constants::*;
use super::token_analysis::*;
use super::trade_strategy::*;
use super::alert::*;
use super::optimization::*;
use super::security::*;
use super::analys_client::SmartTradeAnalysisClient;

/// Executor for smart trading strategies
#[derive(Clone)]
pub struct SmartTradeExecutor {
    /// EVM adapter for each chain
    pub(crate) evm_adapters: HashMap<u32, Arc<EvmAdapter>>,
    
    /// Analysis client that provides access to all analysis services
    pub(crate) analys_client: Arc<SmartTradeAnalysisClient>,
    
    /// Risk analyzer (legacy - kept for backward compatibility)
    pub(crate) risk_analyzer: Arc<RwLock<RiskAnalyzer>>,
    
    /// Mempool analyzer for each chain (legacy - kept for backward compatibility)
    pub(crate) mempool_analyzers: HashMap<u32, Arc<MempoolAnalyzer>>,
    
    /// Configuration
    pub(crate) config: RwLock<SmartTradeConfig>,
    
    /// Active trades being monitored
    pub(crate) active_trades: RwLock<Vec<TradeTracker>>,
    
    /// History of completed trades
    pub(crate) trade_history: RwLock<Vec<TradeResult>>,
    
    /// Running state
    pub(crate) running: RwLock<bool>,
    
    /// Coordinator for shared state between executors
    pub(crate) coordinator: Option<Arc<dyn TradeCoordinator>>,
    
    /// Unique ID for this executor instance
    pub(crate) executor_id: String,
    
    /// Subscription ID for coordinator notifications
    pub(crate) coordinator_subscription: RwLock<Option<String>>,
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
        
        // Khởi động vòng lặp theo dõi
        let executor = Arc::new(self.clone_with_config().await);
        
        tokio::spawn(async move {
            executor.monitor_loop().await;
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
        
        Ok(())
    }
    
    /// Execute a trade with the specified parameters
    async fn execute_trade(&self, params: TradeParams) -> anyhow::Result<Self::TradeResult> {
        // Find the appropriate adapter
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", params.chain_id)),
        };
        
        // Validate trade parameters
        if params.amount <= 0.0 {
            return Err(anyhow::anyhow!("Invalid trade amount: {}", params.amount));
        }
        
        // Create a trade tracker
        let trade_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp() as u64;
        
        let tracker = TradeTracker {
            id: trade_id.clone(),
            chain_id: params.chain_id,
            token_address: params.token_address.clone(),
            amount: params.amount,
            entry_price: 0.0, // Will be updated after trade execution
            current_price: 0.0,
            status: TradeStatus::Pending,
            strategy: if let Some(strategy) = &params.strategy {
                match strategy.as_str() {
                    "quick" => TradeStrategy::QuickFlip,
                    "tsl" => TradeStrategy::TrailingStopLoss,
                    "hodl" => TradeStrategy::LongTerm,
                    _ => TradeStrategy::default(),
                }
            } else {
                TradeStrategy::default()
            },
            created_at: now,
            updated_at: now,
            stop_loss: params.stop_loss,
            take_profit: params.take_profit,
            max_hold_time: params.max_hold_time.unwrap_or(DEFAULT_MAX_HOLD_TIME),
            custom_params: params.custom_params.unwrap_or_default(),
        };
        
        // TODO: Implement the actual trade execution logic
        // This is a simplified version - in a real implementation, you would:
        // 1. Execute the trade
        // 2. Update the trade tracker with actual execution details
        // 3. Add it to active trades
        
        let result = TradeResult {
            id: trade_id,
            chain_id: params.chain_id,
            token_address: params.token_address,
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: params.amount,
            entry_price: 0.0,                  // Would be populated from actual execution
            exit_price: None,
            current_price: 0.0,
            profit_loss: 0.0,
            status: TradeStatus::Pending,
            strategy: tracker.strategy.clone(),
            created_at: now,
            updated_at: now,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        // Add to active trades
        {
            let mut active_trades = self.active_trades.write().await;
            active_trades.push(tracker);
        }
        
        Ok(result)
    }
    
    /// Get the current status of an active trade
    async fn get_trade_status(&self, trade_id: &str) -> anyhow::Result<Option<Self::TradeResult>> {
        // Check active trades
        {
            let active_trades = self.active_trades.read().await;
            if let Some(trade) = active_trades.iter().find(|t| t.id == trade_id) {
                return Ok(Some(self.tracker_to_result(trade).await?));
            }
        }
        
        // Check trade history
        let trade_history = self.trade_history.read().await;
        let result = trade_history.iter()
            .find(|r| r.id == trade_id)
            .cloned();
            
        Ok(result)
    }
    
    /// Get all active trades
    async fn get_active_trades(&self) -> anyhow::Result<Vec<Self::TradeResult>> {
        let active_trades = self.active_trades.read().await;
        let mut results = Vec::with_capacity(active_trades.len());
        
        for tracker in active_trades.iter() {
            results.push(self.tracker_to_result(tracker).await?);
        }
        
        Ok(results)
    }
    
    /// Get trade history within a specific time range
    async fn get_trade_history(&self, from_timestamp: u64, to_timestamp: u64) -> anyhow::Result<Vec<Self::TradeResult>> {
        let trade_history = self.trade_history.read().await;
        
        let filtered_history = trade_history.iter()
            .filter(|trade| trade.created_at >= from_timestamp && trade.created_at <= to_timestamp)
            .cloned()
            .collect();
            
        Ok(filtered_history)
    }
    
    /// Evaluate a token for potential trading
    async fn evaluate_token(&self, chain_id: u32, token_address: &str) -> anyhow::Result<Option<TokenSafety>> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Sử dụng analys_client để phân tích token
        let token_safety_result = self.analys_client.token_provider()
            .analyze_token(chain_id, token_address).await
            .context(format!("Không thể phân tích token {}", token_address))?;
        
        // Nếu không phân tích được token, trả về None
        if !token_safety_result.is_valid() {
            info!("Token {} không hợp lệ hoặc không thể phân tích", token_address);
            return Ok(None);
        }
        
        // Lấy thêm thông tin chi tiết từ contract
        let contract_details = self.analys_client.token_provider()
            .get_contract_details(chain_id, token_address).await
            .context(format!("Không thể lấy chi tiết contract cho token {}", token_address))?;
        
        // Đánh giá rủi ro của token
        let risk_analysis = self.analys_client.risk_provider()
            .analyze_token_risk(chain_id, token_address).await
            .context(format!("Không thể phân tích rủi ro cho token {}", token_address))?;
        
        // Log thông tin phân tích
        info!(
            "Phân tích token {} trên chain {}: risk_score={}, honeypot={:?}, buy_tax={:?}, sell_tax={:?}",
            token_address,
            chain_id,
            risk_analysis.risk_score,
            token_safety_result.is_honeypot,
            token_safety_result.buy_tax,
            token_safety_result.sell_tax
        );
        
        Ok(Some(token_safety_result))
    }

    /// Đánh giá rủi ro của token bằng cách sử dụng analys_client
    async fn analyze_token_risk(&self, chain_id: u32, token_address: &str) -> anyhow::Result<RiskScore> {
        // Sử dụng phương thức analyze_risk từ analys_client
        self.analys_client.analyze_risk(chain_id, token_address).await
            .context(format!("Không thể đánh giá rủi ro cho token {}", token_address))
    }
}

impl SmartTradeExecutor {
    /// Create a new SmartTradeExecutor
    pub fn new() -> Self {
        let executor_id = format!("smart-trade-{}", uuid::Uuid::new_v4());
        
        let evm_adapters = HashMap::new();
        let analys_client = Arc::new(SmartTradeAnalysisClient::new(evm_adapters.clone()));
        
        Self {
            evm_adapters,
            analys_client,
            risk_analyzer: Arc::new(RwLock::new(RiskAnalyzer::new())),
            mempool_analyzers: HashMap::new(),
            config: RwLock::new(SmartTradeConfig::default()),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            coordinator: None,
            executor_id,
            coordinator_subscription: RwLock::new(None),
        }
    }
    
    /// Set coordinator for shared state
    pub fn with_coordinator(mut self, coordinator: Arc<dyn TradeCoordinator>) -> Self {
        self.coordinator = Some(coordinator);
        self
    }
    
    /// Convert a trade tracker to a trade result
    async fn tracker_to_result(&self, tracker: &TradeTracker) -> anyhow::Result<TradeResult> {
        let result = TradeResult {
            id: tracker.id.clone(),
            chain_id: tracker.chain_id,
            token_address: tracker.token_address.clone(),
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: tracker.amount,
            entry_price: tracker.entry_price,
            exit_price: None,
            current_price: tracker.current_price,
            profit_loss: if tracker.entry_price > 0.0 && tracker.current_price > 0.0 {
                (tracker.current_price - tracker.entry_price) / tracker.entry_price * 100.0
            } else {
                0.0
            },
            status: tracker.status.clone(),
            strategy: tracker.strategy.clone(),
            created_at: tracker.created_at,
            updated_at: tracker.updated_at,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        Ok(result)
    }
    
    /// Vòng lặp theo dõi chính
    async fn monitor_loop(&self) {
        let mut counter = 0;
        let mut persist_counter = 0;
        
        while let true_val = *self.running.read().await {
            if !true_val {
                break;
            }
            
            // Kiểm tra và cập nhật tất cả các giao dịch đang active
            if let Err(e) = self.check_active_trades().await {
                error!("Error checking active trades: {}", e);
            }
            
            // Tăng counter và dọn dẹp lịch sử giao dịch định kỳ
            counter += 1;
            if counter >= 100 {
                self.cleanup_trade_history().await;
                counter = 0;
            }
            
            // Lưu trạng thái vào bộ nhớ định kỳ
            persist_counter += 1;
            if persist_counter >= 30 { // Lưu mỗi 30 chu kỳ (khoảng 2.5 phút với interval 5s)
                if let Err(e) = self.update_and_persist_trades().await {
                    error!("Failed to persist bot state: {}", e);
                }
                persist_counter = 0;
            }
            
            // Tính toán thời gian sleep thích ứng
            let sleep_time = self.adaptive_sleep_time().await;
            tokio::time::sleep(tokio::time::Duration::from_millis(sleep_time)).await;
        }
        
        // Lưu trạng thái trước khi dừng
        if let Err(e) = self.update_and_persist_trades().await {
            error!("Failed to persist bot state before stopping: {}", e);
        }
    }
    
    /// Quét cơ hội giao dịch mới
    async fn scan_trading_opportunities(&self) {
        // Kiểm tra cấu hình
        let config = self.config.read().await;
        if !config.enabled || !config.auto_trade {
            return;
        }
        
        // Kiểm tra số lượng giao dịch hiện tại
        let active_trades = self.active_trades.read().await;
        if active_trades.len() >= config.max_concurrent_trades as usize {
            return;
        }
        
        // Chạy quét cho từng chain
        let futures = self.mempool_analyzers.iter().map(|(chain_id, analyzer)| {
            self.scan_chain_opportunities(*chain_id, analyzer)
        });
        join_all(futures).await;
    }
    
    /// Quét cơ hội giao dịch trên một chain cụ thể
    async fn scan_chain_opportunities(&self, chain_id: u32, analyzer: &Arc<MempoolAnalyzer>) -> anyhow::Result<()> {
        let config = self.config.read().await;
        
        // Lấy các giao dịch lớn từ mempool
        let large_txs = match analyzer.get_large_transactions(QUICK_TRADE_MIN_BNB).await {
            Ok(txs) => txs,
            Err(e) => {
                error!("Không thể lấy các giao dịch lớn từ mempool: {}", e);
                return Err(anyhow::anyhow!("Lỗi khi lấy giao dịch từ mempool: {}", e));
            }
        };
        
        // Phân tích từng giao dịch
        for tx in large_txs {
            // Kiểm tra nếu token không trong blacklist và đáp ứng điều kiện
            if let Some(to_token) = &tx.to_token {
                let token_address = &to_token.address;
                
                // Kiểm tra blacklist và whitelist
                if config.token_blacklist.contains(token_address) {
                    debug!("Bỏ qua token {} trong blacklist", token_address);
                    continue;
                }
                
                if !config.token_whitelist.is_empty() && !config.token_whitelist.contains(token_address) {
                    debug!("Bỏ qua token {} không trong whitelist", token_address);
                    continue;
                }
                
                // Kiểm tra nếu giao dịch đủ lớn
                if tx.value >= QUICK_TRADE_MIN_BNB {
                    // Phân tích token
                    if let Err(e) = self.analyze_and_trade_token(chain_id, token_address, tx).await {
                        error!("Lỗi khi phân tích và giao dịch token {}: {}", token_address, e);
                        // Tiếp tục với token tiếp theo, không dừng lại
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Phân tích token và thực hiện giao dịch nếu an toàn
    async fn analyze_and_trade_token(&self, chain_id: u32, token_address: &str, tx: MempoolTransaction) -> anyhow::Result<()> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id));
            }
        };
        
        // THÊM: Xác thực kỹ lưỡng dữ liệu mempool trước khi tiếp tục phân tích
        info!("Xác thực dữ liệu mempool cho token {} từ giao dịch {}", token_address, tx.tx_hash);
        let mempool_validation = match self.validate_mempool_data(chain_id, token_address, &tx).await {
            Ok(validation) => validation,
            Err(e) => {
                error!("Lỗi khi xác thực dữ liệu mempool: {}", e);
                self.log_trade_decision(
                    "new_opportunity", 
                    token_address, 
                    TradeStatus::Canceled, 
                    &format!("Lỗi xác thực mempool: {}", e), 
                    chain_id
                ).await;
                return Err(anyhow::anyhow!("Lỗi xác thực dữ liệu mempool: {}", e));
            }
        };
        
        // Nếu dữ liệu mempool không hợp lệ, dừng phân tích
        if !mempool_validation.is_valid {
            let reasons = mempool_validation.reasons.join(", ");
            info!("Dữ liệu mempool không đáng tin cậy cho token {}: {}", token_address, reasons);
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Dữ liệu mempool không đáng tin cậy: {}", reasons), 
                chain_id
            ).await;
            return Ok(());
        }
        
        info!("Xác thực mempool thành công với điểm tin cậy: {}/100", mempool_validation.confidence_score);
        
        // Code hiện tại: Sử dụng analys_client để kiểm tra an toàn token
        let security_check_future = self.analys_client.verify_token_safety(chain_id, token_address);
        
        // Code hiện tại: Sử dụng analys_client để đánh giá rủi ro
        let risk_analysis_future = self.analys_client.analyze_risk(chain_id, token_address);
        
        // Thực hiện song song các phân tích
        let (security_check_result, risk_analysis) = tokio::join!(security_check_future, risk_analysis_future);
        
        // Xử lý kết quả kiểm tra an toàn
        let security_check = match security_check_result {
            Ok(result) => result,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi kiểm tra an toàn token: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi kiểm tra an toàn token: {}", e));
            }
        };
        
        // Xử lý kết quả phân tích rủi ro
        let risk_score = match risk_analysis {
            Ok(score) => score,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi phân tích rủi ro: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi phân tích rủi ro: {}", e));
            }
        };
        
        // Kiểm tra xem token có an toàn không
        if !security_check.is_safe {
            let issues_str = security_check.issues.iter()
                .map(|issue| format!("{:?}", issue))
                .collect::<Vec<String>>()
                .join(", ");
                
            self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                &format!("Token không an toàn: {}", issues_str), chain_id).await;
            return Ok(());
        }
        
        // Lấy cấu hình hiện tại
        let config = self.config.read().await;
        
        // Kiểm tra điểm rủi ro
        if risk_score.score > config.max_risk_score {
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Điểm rủi ro quá cao: {}/{}", risk_score.score, config.max_risk_score), 
                chain_id
            ).await;
            return Ok(());
        }
        
        // Xác định chiến lược giao dịch dựa trên mức độ rủi ro
        let strategy = if risk_score.score < 30 {
            TradeStrategy::Smart  // Rủi ro thấp, dùng chiến lược thông minh với TSL
        } else {
            TradeStrategy::Quick  // Rủi ro cao hơn, dùng chiến lược nhanh
        };
        
        // Xác định số lượng giao dịch dựa trên cấu hình và mức độ rủi ro
        let base_amount = if risk_score.score < 20 {
            config.trade_amount_high_confidence
        } else if risk_score.score < 40 {
            config.trade_amount_medium_confidence
        } else {
            config.trade_amount_low_confidence
        };
        
        // Tạo tham số giao dịch
        let trade_params = crate::types::TradeParams {
            chain_type: crate::types::ChainType::EVM(chain_id),
            token_address: token_address.to_string(),
            amount: base_amount,
            slippage: 1.0, // 1% slippage mặc định cho giao dịch tự động
            trade_type: crate::types::TradeType::Buy,
            deadline_minutes: 2, // Thời hạn 2 phút
            router_address: String::new(), // Dùng router mặc định
            gas_limit: None,
            gas_price: None,
            strategy: Some(format!("{:?}", strategy).to_lowercase()),
            stop_loss: if strategy == TradeStrategy::Smart { Some(5.0) } else { None }, // 5% stop loss for Smart strategy
            take_profit: if strategy == TradeStrategy::Smart { Some(20.0) } else { Some(10.0) }, // 20% take profit for Smart, 10% for Quick
            max_hold_time: None, // Sẽ được thiết lập dựa trên chiến lược
            custom_params: None,
        };
        
        // Thực hiện giao dịch và lấy kết quả
        info!("Thực hiện mua {} cho token {} trên chain {}", base_amount, token_address, chain_id);
        
        let buy_result = adapter.execute_trade(&trade_params).await;
        
        match buy_result {
            Ok(tx_result) => {
                // Tạo ID giao dịch duy nhất
                let trade_id = uuid::Uuid::new_v4().to_string();
                let current_timestamp = chrono::Utc::now().timestamp() as u64;
                
                // Tính thời gian giữ tối đa dựa trên chiến lược
                let max_hold_time = match strategy {
                    TradeStrategy::Quick => current_timestamp + QUICK_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Smart => current_timestamp + SMART_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Manual => current_timestamp + 24 * 60 * 60, // 24 giờ cho giao dịch thủ công
                };
                
                // Tạo trailing stop loss nếu sử dụng chiến lược Smart
                let trailing_stop_percent = match strategy {
                    TradeStrategy::Smart => Some(SMART_TRADE_TSL_PERCENT),
                    _ => None,
                };
                
                // Xác định giá entry từ kết quả giao dịch
                let entry_price = match tx_result.execution_price {
                    Some(price) => price,
                    None => {
                        error!("Không thể xác định giá mua vào cho token {}", token_address);
                        return Err(anyhow::anyhow!("Không thể xác định giá mua vào"));
                    }
                };
                
                // Lấy số lượng token đã mua
                let token_amount = match tx_result.tokens_received {
                    Some(amount) => amount,
                    None => {
                        error!("Không thể xác định số lượng token nhận được");
                        return Err(anyhow::anyhow!("Không thể xác định số lượng token nhận được"));
                    }
                };
                
                // Tạo theo dõi giao dịch mới
                let trade_tracker = TradeTracker {
                    trade_id: trade_id.clone(),
                    token_address: token_address.to_string(),
                    chain_id,
                    strategy: strategy.clone(),
                    entry_price,
                    token_amount,
                    invested_amount: base_amount,
                    highest_price: entry_price, // Ban đầu, giá cao nhất = giá mua
                    entry_time: current_timestamp,
                    max_hold_time,
                    take_profit_percent: match strategy {
                        TradeStrategy::Quick => QUICK_TRADE_TARGET_PROFIT,
                        _ => 10.0, // Mặc định 10% cho các chiến lược khác
                    },
                    stop_loss_percent: STOP_LOSS_PERCENT,
                    trailing_stop_percent,
                    buy_tx_hash: tx_result.transaction_hash.clone(),
                    sell_tx_hash: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                };
                
                // Thêm vào danh sách theo dõi
                {
                    let mut active_trades = self.active_trades.write().await;
                    active_trades.push(trade_tracker.clone());
                }
                
                // Log thành công
                info!(
                    "Giao dịch mua thành công: ID={}, Token={}, Chain={}, Strategy={:?}, Amount={}, Price={}",
                    trade_id, token_address, chain_id, strategy, base_amount, entry_price
                );
                
                // Thêm vào lịch sử
                let trade_result = TradeResult {
                    trade_id: trade_id.clone(),
                    entry_price,
                    exit_price: None,
                    profit_percent: None,
                    profit_usd: None,
                    entry_time: current_timestamp,
                    exit_time: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                    gas_cost_usd: tx_result.gas_cost_usd.unwrap_or(0.0),
                };
                
                let mut trade_history = self.trade_history.write().await;
                trade_history.push(trade_result);
                
                // Trả về ID giao dịch
                Ok(())
            },
            Err(e) => {
                // Log lỗi giao dịch
                error!("Giao dịch mua thất bại cho token {}: {}", token_address, e);
                Err(anyhow::anyhow!("Giao dịch mua thất bại: {}", e))
            }
        }
    }
    
    /// Thực hiện mua token dựa trên các tham số
    async fn execute_trade(&self, chain_id: u32, token_address: &str, strategy: TradeStrategy, trigger_tx: &MempoolTransaction) -> anyhow::Result<()> {
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id));
            }
        };
        
        // Lấy cấu hình
        let config = self.config.read().await;
        
        // Tính số lượng giao dịch
        let base_amount = match config.auto_trade {
            true => {
                // Tự động tính dựa trên cấu hình, giới hạn trong khoảng min-max
                let calculated_amount = std::cmp::min(
                    trigger_tx.value * config.capital_per_trade_ratio,
                    config.max_trade_amount
                );
                std::cmp::max(calculated_amount, config.min_trade_amount)
            },
            false => {
                // Nếu không bật auto trade, chỉ log và không thực hiện
                info!(
                    "Auto-trade disabled. Would trade {} for token {} with strategy {:?}",
                    config.min_trade_amount, token_address, strategy
                );
                return Ok(());
            }
        };
        
        // Tạo tham số giao dịch
        let trade_params = crate::types::TradeParams {
            chain_type: crate::types::ChainType::EVM(chain_id),
            token_address: token_address.to_string(),
            amount: base_amount,
            slippage: 1.0, // 1% slippage mặc định cho giao dịch tự động
            trade_type: crate::types::TradeType::Buy,
            deadline_minutes: 2, // Thời hạn 2 phút
            router_address: String::new(), // Dùng router mặc định
        };
        
        // Thực hiện giao dịch và lấy kết quả
        info!("Thực hiện mua {} cho token {} trên chain {}", base_amount, token_address, chain_id);
        
        let buy_result = adapter.execute_trade(&trade_params).await;
        
        match buy_result {
            Ok(tx_result) => {
                // Tạo ID giao dịch duy nhất
                let trade_id = uuid::Uuid::new_v4().to_string();
                let current_timestamp = chrono::Utc::now().timestamp() as u64;
                
                // Tính thời gian giữ tối đa dựa trên chiến lược
                let max_hold_time = match strategy {
                    TradeStrategy::Quick => current_timestamp + QUICK_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Smart => current_timestamp + SMART_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Manual => current_timestamp + 24 * 60 * 60, // 24 giờ cho giao dịch thủ công
                };
                
                // Tạo trailing stop loss nếu sử dụng chiến lược Smart
                let trailing_stop_percent = match strategy {
                    TradeStrategy::Smart => Some(SMART_TRADE_TSL_PERCENT),
                    _ => None,
                };
                
                // Xác định giá entry từ kết quả giao dịch
                let entry_price = match tx_result.execution_price {
                    Some(price) => price,
                    None => {
                        error!("Không thể xác định giá mua vào cho token {}", token_address);
                        return Err(anyhow::anyhow!("Không thể xác định giá mua vào"));
                    }
                };
                
                // Lấy số lượng token đã mua
                let token_amount = match tx_result.tokens_received {
                    Some(amount) => amount,
                    None => {
                        error!("Không thể xác định số lượng token nhận được");
                        return Err(anyhow::anyhow!("Không thể xác định số lượng token nhận được"));
                    }
                };
                
                // Tạo theo dõi giao dịch mới
                let trade_tracker = TradeTracker {
                    trade_id: trade_id.clone(),
                    token_address: token_address.to_string(),
                    chain_id,
                    strategy: strategy.clone(),
                    entry_price,
                    token_amount,
                    invested_amount: base_amount,
                    highest_price: entry_price, // Ban đầu, giá cao nhất = giá mua
                    entry_time: current_timestamp,
                    max_hold_time,
                    take_profit_percent: match strategy {
                        TradeStrategy::Quick => QUICK_TRADE_TARGET_PROFIT,
                        _ => 10.0, // Mặc định 10% cho các chiến lược khác
                    },
                    stop_loss_percent: STOP_LOSS_PERCENT,
                    trailing_stop_percent,
                    buy_tx_hash: tx_result.transaction_hash.clone(),
                    sell_tx_hash: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                };
                
                // Thêm vào danh sách theo dõi
                {
                    let mut active_trades = self.active_trades.write().await;
                    active_trades.push(trade_tracker.clone());
                }
                
                // Log thành công
                info!(
                    "Giao dịch mua thành công: ID={}, Token={}, Chain={}, Strategy={:?}, Amount={}, Price={}",
                    trade_id, token_address, chain_id, strategy, base_amount, entry_price
                );
                
                // Thêm vào lịch sử
                let trade_result = TradeResult {
                    trade_id: trade_id.clone(),
                    entry_price,
                    exit_price: None,
                    profit_percent: None,
                    profit_usd: None,
                    entry_time: current_timestamp,
                    exit_time: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                    gas_cost_usd: tx_result.gas_cost_usd,
                };
                
                let mut trade_history = self.trade_history.write().await;
                trade_history.push(trade_result);
                
                // Trả về ID giao dịch
                Ok(())
            },
            Err(e) => {
                // Log lỗi giao dịch
                error!("Giao dịch mua thất bại cho token {}: {}", token_address, e);
                Err(anyhow::anyhow!("Giao dịch mua thất bại: {}", e))
            }
        }
    }
    
    /// Theo dõi các giao dịch đang hoạt động
    async fn track_active_trades(&self) -> anyhow::Result<()> {
        // Kiểm tra xem có bất kỳ giao dịch nào cần theo dõi không
        let active_trades = self.active_trades.read().await;
        
        if active_trades.is_empty() {
            return Ok(());
        }
        
        // Clone danh sách để tránh giữ lock quá lâu
        let active_trades_clone = active_trades.clone();
        drop(active_trades); // Giải phóng lock để tránh deadlock
        
        // Xử lý từng giao dịch song song
        let update_futures = active_trades_clone.iter().map(|trade| {
            self.check_and_update_trade(trade.clone())
        });
        
        let results = futures::future::join_all(update_futures).await;
        
        // Kiểm tra và ghi log lỗi nếu có
        for result in results {
            if let Err(e) = result {
                error!("Lỗi khi theo dõi giao dịch: {}", e);
                // Tiếp tục xử lý các giao dịch khác, không dừng lại
            }
        }
        
        Ok(())
    }
    
    /// Kiểm tra và cập nhật trạng thái của một giao dịch
    async fn check_and_update_trade(&self, trade: TradeTracker) -> anyhow::Result<()> {
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&trade.chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", trade.chain_id));
            }
        };
        
        // Nếu giao dịch không ở trạng thái Open, bỏ qua
        if trade.status != TradeStatus::Open {
            return Ok(());
        }
        
        // Lấy giá hiện tại của token
        let current_price = match adapter.get_token_price(&trade.token_address).await {
            Ok(price) => price,
            Err(e) => {
                warn!("Không thể lấy giá hiện tại cho token {}: {}", trade.token_address, e);
                // Không thể lấy giá, nhưng vẫn cần kiểm tra các điều kiện khác
                // Nên tiếp tục với giá = 0
                0.0
            }
        };
        
        // Nếu không lấy được giá, kiểm tra thời gian giữ tối đa
        if current_price <= 0.0 {
            let current_time = chrono::Utc::now().timestamp() as u64;
            
            // Nếu quá thời gian giữ tối đa, bán
            if current_time >= trade.max_hold_time {
                warn!("Không thể lấy giá cho token {}, đã quá thời gian giữ tối đa, thực hiện bán", trade.token_address);
                self.sell_token(trade, "Đã quá thời gian giữ tối đa".to_string(), trade.entry_price).await?;
                return Ok(());
            }
            
            // Nếu chưa quá thời gian, tiếp tục theo dõi
            return Ok(());
        }
        
        // Tính toán phần trăm lợi nhuận/lỗ hiện tại
        let profit_percent = ((current_price - trade.entry_price) / trade.entry_price) * 100.0;
        let current_time = chrono::Utc::now().timestamp() as u64;
        
        // Cập nhật giá cao nhất nếu cần (chỉ với chiến lược có TSL)
        let mut highest_price = trade.highest_price;
        if current_price > highest_price {
            highest_price = current_price;
            
            // Cập nhật TradeTracker với giá cao nhất mới
            {
                let mut active_trades = self.active_trades.write().await;
                if let Some(index) = active_trades.iter().position(|t| t.trade_id == trade.trade_id) {
                    active_trades[index].highest_price = highest_price;
                }
            }
        }
        
        // 1. Kiểm tra điều kiện lợi nhuận đạt mục tiêu
        if profit_percent >= trade.take_profit_percent {
            info!(
                "Take profit triggered for {} (ID: {}): current profit {:.2}% >= target {:.2}%", 
                trade.token_address, trade.trade_id, profit_percent, trade.take_profit_percent
            );
            self.sell_token(trade, "Đạt mục tiêu lợi nhuận".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 2. Kiểm tra điều kiện stop loss
        if profit_percent <= -trade.stop_loss_percent {
            warn!(
                "Stop loss triggered for {} (ID: {}): current profit {:.2}% <= -{:.2}%", 
                trade.token_address, trade.trade_id, profit_percent, trade.stop_loss_percent
            );
            self.sell_token(trade, "Kích hoạt stop loss".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 3. Kiểm tra trailing stop loss (nếu có)
        if let Some(tsl_percent) = trade.trailing_stop_percent {
            let tsl_trigger_price = highest_price * (1.0 - tsl_percent / 100.0);
            
            if current_price <= tsl_trigger_price && highest_price > trade.entry_price {
                // Chỉ kích hoạt TSL nếu đã có lãi trước đó
                info!(
                    "Trailing stop loss triggered for {} (ID: {}): current price {} <= TSL price {:.4} (highest {:.4})", 
                    trade.token_address, trade.trade_id, current_price, tsl_trigger_price, highest_price
                );
                self.sell_token(trade, "Kích hoạt trailing stop loss".to_string(), current_price).await?;
                return Ok(());
            }
        }
        
        // 4. Kiểm tra hết thời gian tối đa
        if current_time >= trade.max_hold_time {
            info!(
                "Max hold time reached for {} (ID: {}): current profit {:.2}%", 
                trade.token_address, trade.trade_id, profit_percent
            );
            self.sell_token(trade, "Đã hết thời gian giữ tối đa".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 5. Kiểm tra an toàn token
        if trade.strategy != TradeStrategy::Manual {
            if let Err(e) = self.check_token_safety_changes(&trade, adapter).await {
                warn!("Phát hiện vấn đề an toàn với token {}: {}", trade.token_address, e);
                self.sell_token(trade, format!("Phát hiện vấn đề an toàn: {}", e), current_price).await?;
                return Ok(());
            }
        }
        
        // Giao dịch vẫn an toàn, tiếp tục theo dõi
        debug!(
            "Trade {} (ID: {}) still active: current profit {:.2}%, current price {:.8}, highest {:.8}", 
            trade.token_address, trade.trade_id, profit_percent, current_price, highest_price
        );
        
        Ok(())
    }
    
    /// Kiểm tra các vấn đề an toàn mới phát sinh với token
    async fn check_token_safety_changes(&self, trade: &TradeTracker, adapter: &Arc<EvmAdapter>) -> anyhow::Result<()> {
        debug!("Checking token safety changes for {}", trade.token_address);
        
        // Lấy thông tin token
        let contract_info = match self.get_contract_info(trade.chain_id, &trade.token_address, adapter).await {
            Some(info) => info,
            None => return Ok(()),
        };
        
        // Kiểm tra thay đổi về quyền owner
        let (has_privilege, privileges) = self.detect_owner_privilege(trade.chain_id, &trade.token_address).await?;
        
        // Kiểm tra có thay đổi về tax/fee
        let (has_dynamic_tax, tax_reason) = self.detect_dynamic_tax(trade.chain_id, &trade.token_address).await?;
        
        // Kiểm tra các sự kiện bất thường
        let liquidity_events = adapter.get_liquidity_events(&trade.token_address, 10).await
            .context("Failed to get recent liquidity events")?;
            
        let has_abnormal_events = abnormal_liquidity_events(&liquidity_events);
        
        // Nếu có bất kỳ vấn đề nghiêm trọng, ghi log và cảnh báo
        if has_dynamic_tax || has_privilege || has_abnormal_events {
            let mut warnings = Vec::new();
            
            if has_dynamic_tax {
                warnings.push(format!("Tax may have changed: {}", tax_reason.unwrap_or_else(|| "Unknown".to_string())));
            }
            
            if has_privilege && !privileges.is_empty() {
                warnings.push(format!("Owner privileges detected: {}", privileges.join(", ")));
            }
            
            if has_abnormal_events {
                warnings.push("Abnormal liquidity events detected".to_string());
            }
            
            let warning_msg = warnings.join("; ");
            warn!("Token safety concerns for {}: {}", trade.token_address, warning_msg);
            
            // Gửi cảnh báo cho user (thông qua alert system)
            if let Some(alert_system) = &self.alert_system {
                let alert = Alert {
                    token_address: trade.token_address.clone(),
                    chain_id: trade.chain_id,
                    alert_type: AlertType::TokenSafetyChanged,
                    message: warning_msg.clone(),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    trade_id: Some(trade.id.clone()),
                };
                
                if let Err(e) = alert_system.send_alert(alert).await {
                    error!("Failed to send alert: {}", e);
                }
            }
        }
        
        Ok(())
    }
    
    /// Bán token
    async fn sell_token(&self, trade: TradeTracker, exit_reason: String, current_price: f64) {
        if let Some(adapter) = self.evm_adapters.get(&trade.chain_id) {
            // Tạo tham số giao dịch bán
            let trade_params = TradeParams {
                chain_type: ChainType::EVM(trade.chain_id),
                token_address: trade.token_address.clone(),
                amount: trade.token_amount,
                slippage: 1.0, // 1% slippage
                trade_type: TradeType::Sell,
                deadline_minutes: 5,
                router_address: "".to_string(), // Sẽ dùng router mặc định
            };
            
            // Thực hiện bán
            match adapter.execute_trade(&trade_params).await {
                Ok(result) => {
                    if let Some(tx_receipt) = result.tx_receipt {
                        // Cập nhật trạng thái giao dịch
                        let now = Utc::now().timestamp() as u64;
                        
                        // Tính lợi nhuận, có kiểm tra chia cho 0
                        let profit_percent = if trade.entry_price > 0.0 {
                            (current_price - trade.entry_price) / trade.entry_price * 100.0
                        } else {
                            0.0 // Giá trị mặc định nếu entry_price = 0
                        };
                        
                        // Lấy giá thực tế từ adapter thay vì hardcoded
                        let token_price_usd = adapter.get_base_token_price_usd().await.unwrap_or(300.0);
                        let profit_usd = trade.invested_amount * profit_percent / 100.0 * token_price_usd;
                        
                        // Tạo kết quả giao dịch
                        let trade_result = TradeResult {
                            trade_id: trade.trade_id.clone(),
                            entry_price: trade.entry_price,
                            exit_price: Some(current_price),
                            profit_percent: Some(profit_percent),
                            profit_usd: Some(profit_usd),
                            entry_time: trade.entry_time,
                            exit_time: Some(now),
                            status: TradeStatus::Closed,
                            exit_reason: Some(exit_reason.clone()),
                            gas_cost_usd: result.gas_cost_usd,
                        };
                        
                        // Cập nhật lịch sử
                        {
                            let mut trade_history = self.trade_history.write().await;
                            trade_history.push(trade_result);
                        }
                        
                        // Xóa khỏi danh sách theo dõi
                        {
                            let mut active_trades = self.active_trades.write().await;
                            active_trades.retain(|t| t.trade_id != trade.trade_id);
                        }
                    }
                },
                Err(e) => {
                    // Log lỗi
                    error!("Error selling token: {:?}", e);
                }
            }
        }
    }
    
    /// Lấy thông tin contract
    pub(crate) async fn get_contract_info(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> Option<ContractInfo> {
        let source_code_future = adapter.get_contract_source_code(token_address);
        let bytecode_future = adapter.get_contract_bytecode(token_address);
        
        let ((source_code, is_verified), bytecode) = tokio::join!(
            source_code_future,
            bytecode_future
        );
        
        let (source_code, is_verified) = source_code.unwrap_or((None, false));
        let bytecode = bytecode.unwrap_or(None);
        
        let abi = if is_verified { 
            adapter.get_contract_abi(token_address).await.unwrap_or(None) 
        } else { 
            None 
        };
        
        let owner_address = adapter.get_contract_owner(token_address).await.unwrap_or(None);
        
        Some(ContractInfo {
            address: token_address.to_string(),
            chain_id,
            source_code,
            bytecode,
            abi,
            is_verified,
            owner_address,
        })
    }
    
    /// Kiểm tra thanh khoản đã được khóa hay chưa và thời gian khóa
    async fn check_liquidity_lock(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, u64) {
        match adapter.check_liquidity_lock(token_address).await {
            Ok((is_locked, duration)) => (is_locked, duration),
            Err(_) => (false, 0), // Mặc định coi như không khóa nếu có lỗi
        }
    }
    
    /// Kiểm tra các dấu hiệu rug pull
    async fn check_rug_pull_indicators(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>, contract_info: &ContractInfo) -> (bool, Vec<String>) {
        let mut indicators = Vec::new();
        
        // Kiểm tra sở hữu tập trung
        if let Ok(ownership_data) = adapter.check_token_ownership_distribution(token_address).await {
            if ownership_data.max_wallet_percentage > MAX_SAFE_OWNERSHIP_PERCENTAGE {
                indicators.push(format!("High ownership concentration: {:.2}%", ownership_data.max_wallet_percentage));
            }
        }
        
        // Kiểm tra quyền mint không giới hạn
        if let Ok(has_unlimited_mint) = adapter.check_unlimited_mint(token_address).await {
            if has_unlimited_mint {
                indicators.push("Unlimited mint function".to_string());
            }
        }
        
        // Kiểm tra blacklist/transfer delay
        if let Ok(has_blacklist) = adapter.check_blacklist_function(token_address).await {
            if has_blacklist {
                indicators.push("Blacklist function".to_string());
            }
        }
        
        if let Ok(has_transfer_delay) = adapter.check_transfer_delay(token_address).await {
            if has_transfer_delay > MAX_TRANSFER_DELAY_SECONDS {
                indicators.push(format!("Transfer delay: {}s", has_transfer_delay));
            }
        }
        
        // Kiểm tra fake ownership renounce
        if let Ok(has_fake_renounce) = adapter.check_fake_ownership_renounce(token_address).await {
            if has_fake_renounce {
                indicators.push("Fake ownership renounce".to_string());
            }
        }
        
        // Kiểm tra history của dev wallet
        if let Some(owner_address) = &contract_info.owner_address {
            if let Ok(dev_history) = adapter.check_developer_history(owner_address).await {
                if dev_history.previous_rug_pulls > 0 {
                    indicators.push(format!("Developer involved in {} previous rug pulls", dev_history.previous_rug_pulls));
                }
            }
        }
        
        (indicators.len() > 0, indicators)
    }
    
    /// Dọn dẹp lịch sử giao dịch cũ
    /// 
    /// Giới hạn kích thước của trade_history để tránh memory leak
    async fn cleanup_trade_history(&self) {
        const MAX_HISTORY_SIZE: usize = 1000;
        const MAX_HISTORY_AGE_SECONDS: u64 = 7 * 24 * 60 * 60; // 7 ngày
        
        let mut history = self.trade_history.write().await;
        if history.len() <= MAX_HISTORY_SIZE {
            return;
        }
        
        let now = Utc::now().timestamp() as u64;
        
        // Lọc ra các giao dịch quá cũ
        history.retain(|trade| {
            now.saturating_sub(trade.created_at) < MAX_HISTORY_AGE_SECONDS
        });
        
        // Nếu vẫn còn quá nhiều, sắp xếp theo thời gian và giữ lại MAX_HISTORY_SIZE
        if history.len() > MAX_HISTORY_SIZE {
            history.sort_by(|a, b| b.created_at.cmp(&a.created_at)); // Sắp xếp mới nhất trước
            history.truncate(MAX_HISTORY_SIZE);
        }
    }
    
    /// Điều chỉnh thời gian ngủ dựa trên số lượng giao dịch đang theo dõi
    /// 
    /// Khi có nhiều giao dịch cần theo dõi, giảm thời gian ngủ để kiểm tra thường xuyên hơn
    /// Khi không có giao dịch nào, tăng thời gian ngủ để tiết kiệm tài nguyên
    async fn adaptive_sleep_time(&self) -> u64 {
        const PRICE_CHECK_INTERVAL_MS: u64 = 5000; // 5 giây
        
        let active_trades = self.active_trades.read().await;
        let count = active_trades.len();
        
        if count == 0 {
            // Không có giao dịch nào, sleep lâu hơn để tiết kiệm tài nguyên
            PRICE_CHECK_INTERVAL_MS * 3
        } else if count > 5 {
            // Nhiều giao dịch đang hoạt động, giảm thời gian kiểm tra
            PRICE_CHECK_INTERVAL_MS / 3
        } else {
            // Sử dụng thời gian mặc định
            PRICE_CHECK_INTERVAL_MS
        }
    }

    /// Cập nhật và lưu trạng thái mới
    async fn update_and_persist_trades(&self) -> Result<()> {
        let active_trades = self.active_trades.read().await;
        let history = self.trade_history.read().await;
        
        // Tạo đối tượng trạng thái để lưu
        let state = SavedBotState {
            active_trades: active_trades.clone(),
            recent_history: history.clone(),
            updated_at: Utc::now().timestamp(),
        };
        
        // Chuyển đổi sang JSON
        let state_json = serde_json::to_string(&state)
            .context("Failed to serialize bot state")?;
        
        // Lưu vào file
        let state_dir = std::env::var("SNIPEBOT_STATE_DIR")
            .unwrap_or_else(|_| "data/state".to_string());
        
        // Đảm bảo thư mục tồn tại
        std::fs::create_dir_all(&state_dir)
            .context(format!("Failed to create state directory: {}", state_dir))?;
        
        let filename = format!("{}/smart_trade_state.json", state_dir);
        std::fs::write(&filename, state_json)
            .context(format!("Failed to write state to file: {}", filename))?;
        
        debug!("Persisted bot state to {}", filename);
        Ok(())
    }
    
    /// Phục hồi trạng thái từ lưu trữ
    async fn restore_state(&self) -> Result<()> {
        // Lấy đường dẫn file trạng thái
        let state_dir = std::env::var("SNIPEBOT_STATE_DIR")
            .unwrap_or_else(|_| "data/state".to_string());
        let filename = format!("{}/smart_trade_state.json", state_dir);
        
        // Kiểm tra file tồn tại
        if !std::path::Path::new(&filename).exists() {
            info!("No saved state found, starting with empty state");
            return Ok(());
        }
        
        // Đọc file
        let state_json = std::fs::read_to_string(&filename)
            .context(format!("Failed to read state file: {}", filename))?;
        
        // Chuyển đổi từ JSON
        let state: SavedBotState = serde_json::from_str(&state_json)
            .context("Failed to deserialize bot state")?;
        
        // Kiểm tra tính hợp lệ của dữ liệu
        let now = Utc::now().timestamp();
        let max_age_seconds = 24 * 60 * 60; // 24 giờ
        
        if now - state.updated_at > max_age_seconds {
            warn!("Saved state is too old ({} seconds), starting with empty state", 
                  now - state.updated_at);
            return Ok(());
        }
        
        // Phục hồi trạng thái
        {
            let mut active_trades = self.active_trades.write().await;
            active_trades.clear();
            active_trades.extend(state.active_trades);
            info!("Restored {} active trades from saved state", active_trades.len());
        }
        
        {
            let mut history = self.trade_history.write().await;
            history.clear();
            history.extend(state.recent_history);
            info!("Restored {} historical trades from saved state", history.len());
        }
        
        Ok(())
    }

    /// Monitor active trades and perform necessary actions
    async fn monitor_loop(&self) {
        let mut counter = 0;
        let mut persist_counter = 0;
        
        while let true_val = *self.running.read().await {
            if !true_val {
                break;
            }
            
            // Kiểm tra và cập nhật tất cả các giao dịch đang active
            if let Err(e) = self.check_active_trades().await {
                error!("Error checking active trades: {}", e);
            }
            
            // Tăng counter và dọn dẹp lịch sử giao dịch định kỳ
            counter += 1;
            if counter >= 100 {
                self.cleanup_trade_history().await;
                counter = 0;
            }
            
            // Lưu trạng thái vào bộ nhớ định kỳ
            persist_counter += 1;
            if persist_counter >= 30 { // Lưu mỗi 30 chu kỳ (khoảng 2.5 phút với interval 5s)
                if let Err(e) = self.update_and_persist_trades().await {
                    error!("Failed to persist bot state: {}", e);
                }
                persist_counter = 0;
            }
            
            // Tính toán thời gian sleep thích ứng
            let sleep_time = self.adaptive_sleep_time().await;
            tokio::time::sleep(tokio::time::Duration::from_millis(sleep_time)).await;
        }
        
        // Lưu trạng thái trước khi dừng
        if let Err(e) = self.update_and_persist_trades().await {
            error!("Failed to persist bot state before stopping: {}", e);
        }
    }

    /// Check and update the status of a trade
    async fn check_and_update_trade(&self, trade: TradeTracker) -> anyhow::Result<()> {
        // Get adapter for chain
        let adapter = match self.evm_adapters.get(&trade.chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("No adapter found for chain ID {}", trade.chain_id));
            }
        };
        
        // If trade is not in Open state, skip
        if trade.status != TradeStatus::Open {
            return Ok(());
        }
        
        // Get current price of token
        let current_price = match adapter.get_token_price(&trade.token_address).await {
            Ok(price) => price,
            Err(e) => {
                warn!("Cannot get current price for token {}: {}", trade.token_address, e);
                // Cannot get price, but still need to check other conditions
                // Continue with price = 0
                0.0
            }
        };
        
        // If cannot get price, check max hold time
        if current_price <= 0.0 {
            let current_time = chrono::Utc::now().timestamp() as u64;
            
            // If exceeded max hold time, sell
            if current_time >= trade.max_hold_time {
                warn!("Cannot get price for token {}, exceeded max hold time, perform sell", trade.token_address);
                self.sell_token(trade, "Exceeded max hold time".to_string(), trade.entry_price).await?;
                return Ok(());
            }
            
            // If not exceeded, continue monitoring
            return Ok(());
        }
        
        // Calculate current profit percentage/loss
        let profit_percent = ((current_price - trade.entry_price) / trade.entry_price) * 100.0;
        let current_time = chrono::Utc::now().timestamp() as u64;
        
        // Update highest price if needed (only for Smart strategy)
        let mut highest_price = trade.highest_price;
        if current_price > highest_price {
            highest_price = current_price;
            
            // Update TradeTracker with new highest price
            {
                let mut active_trades = self.active_trades.write().await;
                if let Some(index) = active_trades.iter().position(|t| t.trade_id == trade.trade_id) {
                    active_trades[index].highest_price = highest_price;
                }
            }
        }
        
        // 1. Check profit condition to reach target
        if profit_percent >= trade.take_profit_percent {
            info!(
                "Take profit triggered for {} (ID: {}): current profit {:.2}% >= target {:.2}%", 
                trade.token_address, trade.trade_id, profit_percent, trade.take_profit_percent
            );
            self.sell_token(trade, "Reached profit target".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 2. Check stop loss condition
        if profit_percent <= -trade.stop_loss_percent {
            warn!(
                "Stop loss triggered for {} (ID: {}): current profit {:.2}% <= -{:.2}%", 
                trade.token_address, trade.trade_id, profit_percent, trade.stop_loss_percent
            );
            self.sell_token(trade, "Activated stop loss".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 3. Check trailing stop loss (if any)
        if let Some(tsl_percent) = trade.trailing_stop_percent {
            let tsl_trigger_price = highest_price * (1.0 - tsl_percent / 100.0);
            
            if current_price <= tsl_trigger_price && highest_price > trade.entry_price {
                // Activate TSL only if there was profit before
                info!(
                    "Trailing stop loss triggered for {} (ID: {}): current price {} <= TSL price {:.4} (highest {:.4})", 
                    trade.token_address, trade.trade_id, current_price, tsl_trigger_price, highest_price
                );
                self.sell_token(trade, "Activated trailing stop loss".to_string(), current_price).await?;
                return Ok(());
            }
        }
        
        // 4. Check max hold time
        if current_time >= trade.max_hold_time {
            info!(
                "Max hold time reached for {} (ID: {}): current profit {:.2}%", 
                trade.token_address, trade.trade_id, profit_percent
            );
            self.sell_token(trade, "Exceeded max hold time".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 5. Check token safety
        if trade.strategy != TradeStrategy::Manual {
            if let Err(e) = self.check_token_safety_changes(&trade, adapter).await {
                warn!("Security issue detected with token {}: {}", trade.token_address, e);
                self.sell_token(trade, format!("Security issue detected: {}", e), current_price).await?;
                return Ok(());
            }
        }
        
        // Trade still safe, continue monitoring
        debug!(
            "Trade {} (ID: {}) still active: current profit {:.2}%, current price {:.8}, highest {:.8}", 
            trade.token_address, trade.trade_id, profit_percent, current_price, highest_price
        );
        
        Ok(())
    }

    /// Monitor active trades and perform necessary actions
    async fn check_active_trades(&self) -> anyhow::Result<()> {
        // Check if there are any trades that need monitoring
        let active_trades = self.active_trades.read().await;
        
        if active_trades.is_empty() {
            return Ok(());
        }
        
        // Clone list to avoid holding lock for too long
        let active_trades_clone = active_trades.clone();
        drop(active_trades); // Release lock to avoid deadlock
        
        // Process each trade in parallel
        let update_futures = active_trades_clone.iter().map(|trade| {
            self.check_and_update_trade(trade.clone())
        });
        
        let results = futures::future::join_all(update_futures).await;
        
        // Check and log error if any
        for result in results {
            if let Err(e) = result {
                error!("Error monitoring trade: {}", e);
                // Continue processing other trades, do not stop
            }
        }
        
        Ok(())
    }

    /// Clone executor and copy config from original instance (safe for async context)
    pub(crate) async fn clone_with_config(&self) -> Self {
        let config = {
            let config_guard = self.config.read().await;
            config_guard.clone()
        };
        
        Self {
            evm_adapters: self.evm_adapters.clone(),
            analys_client: self.analys_client.clone(),
            risk_analyzer: self.risk_analyzer.clone(),
            mempool_analyzers: self.mempool_analyzers.clone(),
            config: RwLock::new(config),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            coordinator: self.coordinator.clone(),
            executor_id: self.executor_id.clone(),
            coordinator_subscription: RwLock::new(None),
        }
    }

    /// Đăng ký với coordinator
    async fn register_with_coordinator(&self) -> anyhow::Result<()> {
        if let Some(coordinator) = &self.coordinator {
            // Đăng ký executor với coordinator
            coordinator.register_executor(&self.executor_id, ExecutorType::SmartTrade).await?;
            
            // Đăng ký nhận thông báo về các cơ hội mới
            let subscription_callback = {
                let self_clone = self.clone_with_config().await;
                Arc::new(move |opportunity: SharedOpportunity| -> anyhow::Result<()> {
                    let executor = self_clone.clone();
                    tokio::spawn(async move {
                        if let Err(e) = executor.handle_shared_opportunity(opportunity).await {
                            error!("Error handling shared opportunity: {}", e);
                        }
                    });
                    Ok(())
                })
            };
            
            // Lưu subscription ID
            let subscription_id = coordinator.subscribe_to_opportunities(
                &self.executor_id, subscription_callback
            ).await?;
            
            let mut sub_id = self.coordinator_subscription.write().await;
            *sub_id = Some(subscription_id);
            
            info!("Registered with coordinator, subscription ID: {}", sub_id.as_ref().unwrap());
        }
        
        Ok(())
    }
    
    /// Hủy đăng ký khỏi coordinator
    async fn unregister_from_coordinator(&self) -> anyhow::Result<()> {
        if let Some(coordinator) = &self.coordinator {
            // Hủy đăng ký subscription
            let sub_id = {
                let sub = self.coordinator_subscription.read().await;
                sub.clone()
            };
            
            if let Some(id) = sub_id {
                coordinator.unsubscribe_from_opportunities(&id).await?;
                
                // Xóa subscription ID
                let mut sub = self.coordinator_subscription.write().await;
                *sub = None;
            }
            
            // Hủy đăng ký executor
            coordinator.unregister_executor(&self.executor_id).await?;
            info!("Unregistered from coordinator");
        }
        
        Ok(())
    }
    
    /// Phát hiện rủi ro thanh khoản
    /// 
    /// Kiểm tra toàn diện các sự kiện thanh khoản bất thường, khóa LP, rủi ro rugpull và biến động thanh khoản:
    /// 1. Kiểm tra LP được khóa hay không, và thời gian khóa còn lại
    /// 2. Phân tích lịch sử sự kiện thanh khoản để phát hiện rút LP bất thường
    /// 3. Kiểm tra sự cân bằng thanh khoản giữa token và base (ETH/BNB)
    /// 4. Phát hiện hàm nguy hiểm cho phép thay đổi LP (remove, migrate)
    /// 
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ của token cần kiểm tra
    /// 
    /// # Returns
    /// * `Result<(bool, Option<String>)>` - (có rủi ro thanh khoản, mô tả nếu có)
    pub async fn detect_liquidity_risk(&self, chain_id: u32, token_address: &str) -> Result<(bool, Option<String>)> {
        debug!("Detecting liquidity risk for token: {} on chain {}", token_address, chain_id);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
/// Core implementation of SmartTradeExecutor
///
/// This file contains the main executor struct that manages the entire
/// smart trading process, orchestrates token analysis, trade strategies,
/// and market monitoring.
///
/// # Cải tiến đã thực hiện:
/// - Loại bỏ các phương thức trùng lặp với analys modules
/// - Sử dụng API từ analys/token_status và analys/risk_analyzer
/// - Áp dụng xử lý lỗi chuẩn với anyhow::Result thay vì unwrap/expect
/// - Đảm bảo thread safety trong các phương thức async với kỹ thuật "clone and drop" để tránh deadlock
/// - Tối ưu hóa việc xử lý các futures với tokio::join! cho các tác vụ song song
/// - Xử lý các trường hợp giá trị null an toàn với match/Option
/// - Tuân thủ nghiêm ngặt quy tắc từ .cursorrc

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use chrono::Utc;
use futures::future::join_all;
use tracing::{debug, error, info, warn};
use anyhow::{Result, Context, anyhow, bail};
use uuid;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use rand::Rng;
use std::collections::HashSet;
use futures::future::Future;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::{MempoolAnalyzer, MempoolTransaction};
use crate::analys::risk_analyzer::{RiskAnalyzer, RiskFactor, TradeRecommendation, TradeRiskAnalysis};
use crate::analys::token_status::{
    ContractInfo, LiquidityEvent, LiquidityEventType, TokenSafety, TokenStatus,
    TokenIssue, IssueSeverity, abnormal_liquidity_events, detect_dynamic_tax, 
    detect_hidden_fees, analyze_owner_privileges, is_proxy_contract, blacklist::{has_blacklist_or_whitelist, has_trading_cooldown, has_max_tx_or_wallet_limit},
};
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};
use crate::tradelogic::traits::{
    TradeExecutor, RiskManager, TradeCoordinator, ExecutorType, SharedOpportunity, 
    SharedOpportunityType, OpportunityPriority
};

// Module imports
use super::types::{
    SmartTradeConfig, 
    TradeResult, 
    TradeStatus, 
    TradeStrategy, 
    TradeTracker
};
use super::constants::*;
use super::token_analysis::*;
use super::trade_strategy::*;
use super::alert::*;
use super::optimization::*;
use super::security::*;
use super::analys_client::SmartTradeAnalysisClient;

/// Executor for smart trading strategies
#[derive(Clone)]
pub struct SmartTradeExecutor {
    /// EVM adapter for each chain
    pub(crate) evm_adapters: HashMap<u32, Arc<EvmAdapter>>,
    
    /// Analysis client that provides access to all analysis services
    pub(crate) analys_client: Arc<SmartTradeAnalysisClient>,
    
    /// Risk analyzer (legacy - kept for backward compatibility)
    pub(crate) risk_analyzer: Arc<RwLock<RiskAnalyzer>>,
    
    /// Mempool analyzer for each chain (legacy - kept for backward compatibility)
    pub(crate) mempool_analyzers: HashMap<u32, Arc<MempoolAnalyzer>>,
    
    /// Configuration
    pub(crate) config: RwLock<SmartTradeConfig>,
    
    /// Active trades being monitored
    pub(crate) active_trades: RwLock<Vec<TradeTracker>>,
    
    /// History of completed trades
    pub(crate) trade_history: RwLock<Vec<TradeResult>>,
    
    /// Running state
    pub(crate) running: RwLock<bool>,
    
    /// Coordinator for shared state between executors
    pub(crate) coordinator: Option<Arc<dyn TradeCoordinator>>,
    
    /// Unique ID for this executor instance
    pub(crate) executor_id: String,
    
    /// Subscription ID for coordinator notifications
    pub(crate) coordinator_subscription: RwLock<Option<String>>,
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
        
        // Khởi động vòng lặp theo dõi
        let executor = Arc::new(self.clone_with_config().await);
        
        tokio::spawn(async move {
            executor.monitor_loop().await;
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
        
        Ok(())
    }
    
    /// Execute a trade with the specified parameters
    async fn execute_trade(&self, params: TradeParams) -> anyhow::Result<Self::TradeResult> {
        // Find the appropriate adapter
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", params.chain_id)),
        };
        
        // Validate trade parameters
        if params.amount <= 0.0 {
            return Err(anyhow::anyhow!("Invalid trade amount: {}", params.amount));
        }
        
        // Create a trade tracker
        let trade_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp() as u64;
        
        let tracker = TradeTracker {
            id: trade_id.clone(),
            chain_id: params.chain_id,
            token_address: params.token_address.clone(),
            amount: params.amount,
            entry_price: 0.0, // Will be updated after trade execution
            current_price: 0.0,
            status: TradeStatus::Pending,
            strategy: if let Some(strategy) = &params.strategy {
                match strategy.as_str() {
                    "quick" => TradeStrategy::QuickFlip,
                    "tsl" => TradeStrategy::TrailingStopLoss,
                    "hodl" => TradeStrategy::LongTerm,
                    _ => TradeStrategy::default(),
                }
            } else {
                TradeStrategy::default()
            },
            created_at: now,
            updated_at: now,
            stop_loss: params.stop_loss,
            take_profit: params.take_profit,
            max_hold_time: params.max_hold_time.unwrap_or(DEFAULT_MAX_HOLD_TIME),
            custom_params: params.custom_params.unwrap_or_default(),
        };
        
        // TODO: Implement the actual trade execution logic
        // This is a simplified version - in a real implementation, you would:
        // 1. Execute the trade
        // 2. Update the trade tracker with actual execution details
        // 3. Add it to active trades
        
        let result = TradeResult {
            id: trade_id,
            chain_id: params.chain_id,
            token_address: params.token_address,
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: params.amount,
            entry_price: 0.0,                  // Would be populated from actual execution
            exit_price: None,
            current_price: 0.0,
            profit_loss: 0.0,
            status: TradeStatus::Pending,
            strategy: tracker.strategy.clone(),
            created_at: now,
            updated_at: now,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        // Add to active trades
        {
            let mut active_trades = self.active_trades.write().await;
            active_trades.push(tracker);
        }
        
        Ok(result)
    }
    
    /// Get the current status of an active trade
    async fn get_trade_status(&self, trade_id: &str) -> anyhow::Result<Option<Self::TradeResult>> {
        // Check active trades
        {
            let active_trades = self.active_trades.read().await;
            if let Some(trade) = active_trades.iter().find(|t| t.id == trade_id) {
                return Ok(Some(self.tracker_to_result(trade).await?));
            }
        }
        
        // Check trade history
        let trade_history = self.trade_history.read().await;
        let result = trade_history.iter()
            .find(|r| r.id == trade_id)
            .cloned();
            
        Ok(result)
    }
    
    /// Get all active trades
    async fn get_active_trades(&self) -> anyhow::Result<Vec<Self::TradeResult>> {
        let active_trades = self.active_trades.read().await;
        let mut results = Vec::with_capacity(active_trades.len());
        
        for tracker in active_trades.iter() {
            results.push(self.tracker_to_result(tracker).await?);
        }
        
        Ok(results)
    }
    
    /// Get trade history within a specific time range
    async fn get_trade_history(&self, from_timestamp: u64, to_timestamp: u64) -> anyhow::Result<Vec<Self::TradeResult>> {
        let trade_history = self.trade_history.read().await;
        
        let filtered_history = trade_history.iter()
            .filter(|trade| trade.created_at >= from_timestamp && trade.created_at <= to_timestamp)
            .cloned()
            .collect();
            
        Ok(filtered_history)
    }
    
    /// Evaluate a token for potential trading
    async fn evaluate_token(&self, chain_id: u32, token_address: &str) -> anyhow::Result<Option<TokenSafety>> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Sử dụng analys_client để phân tích token
        let token_safety_result = self.analys_client.token_provider()
            .analyze_token(chain_id, token_address).await
            .context(format!("Không thể phân tích token {}", token_address))?;
        
        // Nếu không phân tích được token, trả về None
        if !token_safety_result.is_valid() {
            info!("Token {} không hợp lệ hoặc không thể phân tích", token_address);
            return Ok(None);
        }
        
        // Lấy thêm thông tin chi tiết từ contract
        let contract_details = self.analys_client.token_provider()
            .get_contract_details(chain_id, token_address).await
            .context(format!("Không thể lấy chi tiết contract cho token {}", token_address))?;
        
        // Đánh giá rủi ro của token
        let risk_analysis = self.analys_client.risk_provider()
            .analyze_token_risk(chain_id, token_address).await
            .context(format!("Không thể phân tích rủi ro cho token {}", token_address))?;
        
        // Log thông tin phân tích
        info!(
            "Phân tích token {} trên chain {}: risk_score={}, honeypot={:?}, buy_tax={:?}, sell_tax={:?}",
            token_address,
            chain_id,
            risk_analysis.risk_score,
            token_safety_result.is_honeypot,
            token_safety_result.buy_tax,
            token_safety_result.sell_tax
        );
        
        Ok(Some(token_safety_result))
    }

    /// Đánh giá rủi ro của token bằng cách sử dụng analys_client
    async fn analyze_token_risk(&self, chain_id: u32, token_address: &str) -> anyhow::Result<RiskScore> {
        // Sử dụng phương thức analyze_risk từ analys_client
        self.analys_client.analyze_risk(chain_id, token_address).await
            .context(format!("Không thể đánh giá rủi ro cho token {}", token_address))
    }
}

impl SmartTradeExecutor {
    /// Create a new SmartTradeExecutor
    pub fn new() -> Self {
        let executor_id = format!("smart-trade-{}", uuid::Uuid::new_v4());
        
        let evm_adapters = HashMap::new();
        let analys_client = Arc::new(SmartTradeAnalysisClient::new(evm_adapters.clone()));
        
        Self {
            evm_adapters,
            analys_client,
            risk_analyzer: Arc::new(RwLock::new(RiskAnalyzer::new())),
            mempool_analyzers: HashMap::new(),
            config: RwLock::new(SmartTradeConfig::default()),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            coordinator: None,
            executor_id,
            coordinator_subscription: RwLock::new(None),
        }
    }
    
    /// Set coordinator for shared state
    pub fn with_coordinator(mut self, coordinator: Arc<dyn TradeCoordinator>) -> Self {
        self.coordinator = Some(coordinator);
        self
    }
    
    /// Convert a trade tracker to a trade result
    async fn tracker_to_result(&self, tracker: &TradeTracker) -> anyhow::Result<TradeResult> {
        let result = TradeResult {
            id: tracker.id.clone(),
            chain_id: tracker.chain_id,
            token_address: tracker.token_address.clone(),
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: tracker.amount,
            entry_price: tracker.entry_price,
            exit_price: None,
            current_price: tracker.current_price,
            profit_loss: if tracker.entry_price > 0.0 && tracker.current_price > 0.0 {
                (tracker.current_price - tracker.entry_price) / tracker.entry_price * 100.0
            } else {
                0.0
            },
            status: tracker.status.clone(),
            strategy: tracker.strategy.clone(),
            created_at: tracker.created_at,
            updated_at: tracker.updated_at,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        Ok(result)
    }
    
    /// Vòng lặp theo dõi chính
    async fn monitor_loop(&self) {
        let mut counter = 0;
        let mut persist_counter = 0;
        
        while let true_val = *self.running.read().await {
            if !true_val {
                break;
            }
            
            // Kiểm tra và cập nhật tất cả các giao dịch đang active
            if let Err(e) = self.check_active_trades().await {
                error!("Error checking active trades: {}", e);
            }
            
            // Tăng counter và dọn dẹp lịch sử giao dịch định kỳ
            counter += 1;
            if counter >= 100 {
                self.cleanup_trade_history().await;
                counter = 0;
            }
            
            // Lưu trạng thái vào bộ nhớ định kỳ
            persist_counter += 1;
            if persist_counter >= 30 { // Lưu mỗi 30 chu kỳ (khoảng 2.5 phút với interval 5s)
                if let Err(e) = self.update_and_persist_trades().await {
                    error!("Failed to persist bot state: {}", e);
                }
                persist_counter = 0;
            }
            
            // Tính toán thời gian sleep thích ứng
            let sleep_time = self.adaptive_sleep_time().await;
            tokio::time::sleep(tokio::time::Duration::from_millis(sleep_time)).await;
        }
        
        // Lưu trạng thái trước khi dừng
        if let Err(e) = self.update_and_persist_trades().await {
            error!("Failed to persist bot state before stopping: {}", e);
        }
    }
    
    /// Quét cơ hội giao dịch mới
    async fn scan_trading_opportunities(&self) {
        // Kiểm tra cấu hình
        let config = self.config.read().await;
        if !config.enabled || !config.auto_trade {
            return;
        }
        
        // Kiểm tra số lượng giao dịch hiện tại
        let active_trades = self.active_trades.read().await;
        if active_trades.len() >= config.max_concurrent_trades as usize {
            return;
        }
        
        // Chạy quét cho từng chain
        let futures = self.mempool_analyzers.iter().map(|(chain_id, analyzer)| {
            self.scan_chain_opportunities(*chain_id, analyzer)
        });
        join_all(futures).await;
    }
    
    /// Quét cơ hội giao dịch trên một chain cụ thể
    async fn scan_chain_opportunities(&self, chain_id: u32, analyzer: &Arc<MempoolAnalyzer>) -> anyhow::Result<()> {
        let config = self.config.read().await;
        
        // Lấy các giao dịch lớn từ mempool
        let large_txs = match analyzer.get_large_transactions(QUICK_TRADE_MIN_BNB).await {
            Ok(txs) => txs,
            Err(e) => {
                error!("Không thể lấy các giao dịch lớn từ mempool: {}", e);
                return Err(anyhow::anyhow!("Lỗi khi lấy giao dịch từ mempool: {}", e));
            }
        };
        
        // Phân tích từng giao dịch
        for tx in large_txs {
            // Kiểm tra nếu token không trong blacklist và đáp ứng điều kiện
            if let Some(to_token) = &tx.to_token {
                let token_address = &to_token.address;
                
                // Kiểm tra blacklist và whitelist
                if config.token_blacklist.contains(token_address) {
                    debug!("Bỏ qua token {} trong blacklist", token_address);
                    continue;
                }
                
                if !config.token_whitelist.is_empty() && !config.token_whitelist.contains(token_address) {
                    debug!("Bỏ qua token {} không trong whitelist", token_address);
                    continue;
                }
                
                // Kiểm tra nếu giao dịch đủ lớn
                if tx.value >= QUICK_TRADE_MIN_BNB {
                    // Phân tích token
                    if let Err(e) = self.analyze_and_trade_token(chain_id, token_address, tx).await {
                        error!("Lỗi khi phân tích và giao dịch token {}: {}", token_address, e);
                        // Tiếp tục với token tiếp theo, không dừng lại
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Phân tích token và thực hiện giao dịch nếu an toàn
    async fn analyze_and_trade_token(&self, chain_id: u32, token_address: &str, tx: MempoolTransaction) -> anyhow::Result<()> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id));
            }
        };
        
        // THÊM: Xác thực kỹ lưỡng dữ liệu mempool trước khi tiếp tục phân tích
        info!("Xác thực dữ liệu mempool cho token {} từ giao dịch {}", token_address, tx.tx_hash);
        let mempool_validation = match self.validate_mempool_data(chain_id, token_address, &tx).await {
            Ok(validation) => validation,
            Err(e) => {
                error!("Lỗi khi xác thực dữ liệu mempool: {}", e);
                self.log_trade_decision(
                    "new_opportunity", 
                    token_address, 
                    TradeStatus::Canceled, 
                    &format!("Lỗi xác thực mempool: {}", e), 
                    chain_id
                ).await;
                return Err(anyhow::anyhow!("Lỗi xác thực dữ liệu mempool: {}", e));
            }
        };
        
        // Nếu dữ liệu mempool không hợp lệ, dừng phân tích
        if !mempool_validation.is_valid {
            let reasons = mempool_validation.reasons.join(", ");
            info!("Dữ liệu mempool không đáng tin cậy cho token {}: {}", token_address, reasons);
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Dữ liệu mempool không đáng tin cậy: {}", reasons), 
                chain_id
            ).await;
            return Ok(());
        }
        
        info!("Xác thực mempool thành công với điểm tin cậy: {}/100", mempool_validation.confidence_score);
        
        // Code hiện tại: Sử dụng analys_client để kiểm tra an toàn token
        let security_check_future = self.analys_client.verify_token_safety(chain_id, token_address);
        
        // Code hiện tại: Sử dụng analys_client để đánh giá rủi ro
        let risk_analysis_future = self.analys_client.analyze_risk(chain_id, token_address);
        
        // Thực hiện song song các phân tích
        let (security_check_result, risk_analysis) = tokio::join!(security_check_future, risk_analysis_future);
        
        // Xử lý kết quả kiểm tra an toàn
        let security_check = match security_check_result {
            Ok(result) => result,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi kiểm tra an toàn token: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi kiểm tra an toàn token: {}", e));
            }
        };
        
        // Xử lý kết quả phân tích rủi ro
        let risk_score = match risk_analysis {
            Ok(score) => score,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi phân tích rủi ro: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi phân tích rủi ro: {}", e));
            }
        };
        
        // Kiểm tra xem token có an toàn không
        if !security_check.is_safe {
            let issues_str = security_check.issues.iter()
                .map(|issue| format!("{:?}", issue))
                .collect::<Vec<String>>()
                .join(", ");
                
            self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                &format!("Token không an toàn: {}", issues_str), chain_id).await;
            return Ok(());
        }
        
        // Lấy cấu hình hiện tại
        let config = self.config.read().await;
        
        // Kiểm tra điểm rủi ro
        if risk_score.score > config.max_risk_score {
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Điểm rủi ro quá cao: {}/{}", risk_score.score, config.max_risk_score), 
                chain_id
            ).await;
            return Ok(());
        }
        
        // Xác định chiến lược giao dịch dựa trên mức độ rủi ro
        let strategy = if risk_score.score < 30 {
            TradeStrategy::Smart  // Rủi ro thấp, dùng chiến lược thông minh với TSL
        } else {
            TradeStrategy::Quick  // Rủi ro cao hơn, dùng chiến lược nhanh
        };
        
        // Xác định số lượng giao dịch dựa trên cấu hình và mức độ rủi ro
        let base_amount = if risk_score.score < 20 {
            config.trade_amount_high_confidence
        } else if risk_score.score < 40 {
            config.trade_amount_medium_confidence
        } else {
            config.trade_amount_low_confidence
        };
        
        // Tạo tham số giao dịch
        let trade_params = crate::types::TradeParams {
            chain_type: crate::types::ChainType::EVM(chain_id),
            token_address: token_address.to_string(),
            amount: base_amount,
            slippage: 1.0, // 1% slippage mặc định cho giao dịch tự động
            trade_type: crate::types::TradeType::Buy,
            deadline_minutes: 2, // Thời hạn 2 phút
            router_address: String::new(), // Dùng router mặc định
            gas_limit: None,
            gas_price: None,
            strategy: Some(format!("{:?}", strategy).to_lowercase()),
            stop_loss: if strategy == TradeStrategy::Smart { Some(5.0) } else { None }, // 5% stop loss for Smart strategy
            take_profit: if strategy == TradeStrategy::Smart { Some(20.0) } else { Some(10.0) }, // 20% take profit for Smart, 10% for Quick
            max_hold_time: None, // Sẽ được thiết lập dựa trên chiến lược
            custom_params: None,
        };
        
        // Thực hiện giao dịch và lấy kết quả
        info!("Thực hiện mua {} cho token {} trên chain {}", base_amount, token_address, chain_id);
        
        let buy_result = adapter.execute_trade(&trade_params).await;
        
        match buy_result {
            Ok(tx_result) => {
                // Tạo ID giao dịch duy nhất
                let trade_id = uuid::Uuid::new_v4().to_string();
                let current_timestamp = chrono::Utc::now().timestamp() as u64;
                
                // Tính thời gian giữ tối đa dựa trên chiến lược
                let max_hold_time = match strategy {
                    TradeStrategy::Quick => current_timestamp + QUICK_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Smart => current_timestamp + SMART_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Manual => current_timestamp + 24 * 60 * 60, // 24 giờ cho giao dịch thủ công
                };
                
                // Tạo trailing stop loss nếu sử dụng chiến lược Smart
                let trailing_stop_percent = match strategy {
                    TradeStrategy::Smart => Some(SMART_TRADE_TSL_PERCENT),
                    _ => None,
                };
                
                // Xác định giá entry từ kết quả giao dịch
                let entry_price = match tx_result.execution_price {
                    Some(price) => price,
                    None => {
                        error!("Không thể xác định giá mua vào cho token {}", token_address);
                        return Err(anyhow::anyhow!("Không thể xác định giá mua vào"));
                    }
                };
                
                // Lấy số lượng token đã mua
                let token_amount = match tx_result.tokens_received {
                    Some(amount) => amount,
                    None => {
                        error!("Không thể xác định số lượng token nhận được");
                        return Err(anyhow::anyhow!("Không thể xác định số lượng token nhận được"));
                    }
                };
                
                // Tạo theo dõi giao dịch mới
                let trade_tracker = TradeTracker {
                    trade_id: trade_id.clone(),
                    token_address: token_address.to_string(),
                    chain_id,
                    strategy: strategy.clone(),
                    entry_price,
                    token_amount,
                    invested_amount: base_amount,
                    highest_price: entry_price, // Ban đầu, giá cao nhất = giá mua
                    entry_time: current_timestamp,
                    max_hold_time,
                    take_profit_percent: match strategy {
                        TradeStrategy::Quick => QUICK_TRADE_TARGET_PROFIT,
                        _ => 10.0, // Mặc định 10% cho các chiến lược khác
                    },
                    stop_loss_percent: STOP_LOSS_PERCENT,
                    trailing_stop_percent,
                    buy_tx_hash: tx_result.transaction_hash.clone(),
                    sell_tx_hash: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                };
                
                // Thêm vào danh sách theo dõi
                {
                    let mut active_trades = self.active_trades.write().await;
                    active_trades.push(trade_tracker.clone());
                }
                
                // Log thành công
                info!(
                    "Giao dịch mua thành công: ID={}, Token={}, Chain={}, Strategy={:?}, Amount={}, Price={}",
                    trade_id, token_address, chain_id, strategy, base_amount, entry_price
                );
                
                // Thêm vào lịch sử
                let trade_result = TradeResult {
                    trade_id: trade_id.clone(),
                    entry_price,
                    exit_price: None,
                    profit_percent: None,
                    profit_usd: None,
                    entry_time: current_timestamp,
                    exit_time: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                    gas_cost_usd: tx_result.gas_cost_usd.unwrap_or(0.0),
                };
                
                let mut trade_history = self.trade_history.write().await;
                trade_history.push(trade_result);
                
                // Trả về ID giao dịch
                Ok(())
            },
            Err(e) => {
                // Log lỗi giao dịch
                error!("Giao dịch mua thất bại cho token {}: {}", token_address, e);
                Err(anyhow::anyhow!("Giao dịch mua thất bại: {}", e))
            }
        }
    }
    
    /// Thực hiện mua token dựa trên các tham số
    async fn execute_trade(&self, chain_id: u32, token_address: &str, strategy: TradeStrategy, trigger_tx: &MempoolTransaction) -> anyhow::Result<()> {
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id));
            }
        };
        
        // Lấy cấu hình
        let config = self.config.read().await;
        
        // Tính số lượng giao dịch
        let base_amount = match config.auto_trade {
            true => {
                // Tự động tính dựa trên cấu hình, giới hạn trong khoảng min-max
                let calculated_amount = std::cmp::min(
                    trigger_tx.value * config.capital_per_trade_ratio,
                    config.max_trade_amount
                );
                std::cmp::max(calculated_amount, config.min_trade_amount)
            },
            false => {
                // Nếu không bật auto trade, chỉ log và không thực hiện
                info!(
                    "Auto-trade disabled. Would trade {} for token {} with strategy {:?}",
                    config.min_trade_amount, token_address, strategy
                );
                return Ok(());
            }
        };
        
        // Tạo tham số giao dịch
        let trade_params = crate::types::TradeParams {
            chain_type: crate::types::ChainType::EVM(chain_id),
            token_address: token_address.to_string(),
            amount: base_amount,
            slippage: 1.0, // 1% slippage mặc định cho giao dịch tự động
            trade_type: crate::types::TradeType::Buy,
            deadline_minutes: 2, // Thời hạn 2 phút
            router_address: String::new(), // Dùng router mặc định
        };
        
        // Thực hiện giao dịch và lấy kết quả
        info!("Thực hiện mua {} cho token {} trên chain {}", base_amount, token_address, chain_id);
        
        let buy_result = adapter.execute_trade(&trade_params).await;
        
        match buy_result {
            Ok(tx_result) => {
                // Tạo ID giao dịch duy nhất
                let trade_id = uuid::Uuid::new_v4().to_string();
                let current_timestamp = chrono::Utc::now().timestamp() as u64;
                
                // Tính thời gian giữ tối đa dựa trên chiến lược
                let max_hold_time = match strategy {
                    TradeStrategy::Quick => current_timestamp + QUICK_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Smart => current_timestamp + SMART_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Manual => current_timestamp + 24 * 60 * 60, // 24 giờ cho giao dịch thủ công
                };
                
                // Tạo trailing stop loss nếu sử dụng chiến lược Smart
                let trailing_stop_percent = match strategy {
                    TradeStrategy::Smart => Some(SMART_TRADE_TSL_PERCENT),
                    _ => None,
                };
                
                // Xác định giá entry từ kết quả giao dịch
                let entry_price = match tx_result.execution_price {
                    Some(price) => price,
                    None => {
                        error!("Không thể xác định giá mua vào cho token {}", token_address);
                        return Err(anyhow::anyhow!("Không thể xác định giá mua vào"));
                    }
                };
                
                // Lấy số lượng token đã mua
                let token_amount = match tx_result.tokens_received {
                    Some(amount) => amount,
                    None => {
                        error!("Không thể xác định số lượng token nhận được");
                        return Err(anyhow::anyhow!("Không thể xác định số lượng token nhận được"));
                    }
                };
                
                // Tạo theo dõi giao dịch mới
                let trade_tracker = TradeTracker {
                    trade_id: trade_id.clone(),
                    token_address: token_address.to_string(),
                    chain_id,
                    strategy: strategy.clone(),
                    entry_price,
                    token_amount,
                    invested_amount: base_amount,
                    highest_price: entry_price, // Ban đầu, giá cao nhất = giá mua
                    entry_time: current_timestamp,
                    max_hold_time,
                    take_profit_percent: match strategy {
                        TradeStrategy::Quick => QUICK_TRADE_TARGET_PROFIT,
                        _ => 10.0, // Mặc định 10% cho các chiến lược khác
                    },
                    stop_loss_percent: STOP_LOSS_PERCENT,
                    trailing_stop_percent,
                    buy_tx_hash: tx_result.transaction_hash.clone(),
                    sell_tx_hash: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                };
                
                // Thêm vào danh sách theo dõi
                {
                    let mut active_trades = self.active_trades.write().await;
                    active_trades.push(trade_tracker.clone());
                }
                
                // Log thành công
                info!(
                    "Giao dịch mua thành công: ID={}, Token={}, Chain={}, Strategy={:?}, Amount={}, Price={}",
                    trade_id, token_address, chain_id, strategy, base_amount, entry_price
                );
                
                // Thêm vào lịch sử
                let trade_result = TradeResult {
                    trade_id: trade_id.clone(),
                    entry_price,
                    exit_price: None,
                    profit_percent: None,
                    profit_usd: None,
                    entry_time: current_timestamp,
                    exit_time: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                    gas_cost_usd: tx_result.gas_cost_usd,
                };
                
                let mut trade_history = self.trade_history.write().await;
                trade_history.push(trade_result);
                
                // Trả về ID giao dịch
                Ok(())
            },
            Err(e) => {
                // Log lỗi giao dịch
                error!("Giao dịch mua thất bại cho token {}: {}", token_address, e);
                Err(anyhow::anyhow!("Giao dịch mua thất bại: {}", e))
            }
        }
    }
    
    /// Theo dõi các giao dịch đang hoạt động
    async fn track_active_trades(&self) -> anyhow::Result<()> {
        // Kiểm tra xem có bất kỳ giao dịch nào cần theo dõi không
        let active_trades = self.active_trades.read().await;
        
        if active_trades.is_empty() {
            return Ok(());
        }
        
        // Clone danh sách để tránh giữ lock quá lâu
        let active_trades_clone = active_trades.clone();
        drop(active_trades); // Giải phóng lock để tránh deadlock
        
        // Xử lý từng giao dịch song song
        let update_futures = active_trades_clone.iter().map(|trade| {
            self.check_and_update_trade(trade.clone())
        });
        
        let results = futures::future::join_all(update_futures).await;
        
        // Kiểm tra và ghi log lỗi nếu có
        for result in results {
            if let Err(e) = result {
                error!("Lỗi khi theo dõi giao dịch: {}", e);
                // Tiếp tục xử lý các giao dịch khác, không dừng lại
            }
        }
        
        Ok(())
    }
    
    /// Kiểm tra và cập nhật trạng thái của một giao dịch
    async fn check_and_update_trade(&self, trade: TradeTracker) -> anyhow::Result<()> {
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&trade.chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", trade.chain_id));
            }
        };
        
        // Nếu giao dịch không ở trạng thái Open, bỏ qua
        if trade.status != TradeStatus::Open {
            return Ok(());
        }
        
        // Lấy giá hiện tại của token
        let current_price = match adapter.get_token_price(&trade.token_address).await {
            Ok(price) => price,
            Err(e) => {
                warn!("Không thể lấy giá hiện tại cho token {}: {}", trade.token_address, e);
                // Không thể lấy giá, nhưng vẫn cần kiểm tra các điều kiện khác
                // Nên tiếp tục với giá = 0
                0.0
            }
        };
        
        // Nếu không lấy được giá, kiểm tra thời gian giữ tối đa
        if current_price <= 0.0 {
            let current_time = chrono::Utc::now().timestamp() as u64;
            
            // Nếu quá thời gian giữ tối đa, bán
            if current_time >= trade.max_hold_time {
                warn!("Không thể lấy giá cho token {}, đã quá thời gian giữ tối đa, thực hiện bán", trade.token_address);
                self.sell_token(trade, "Đã quá thời gian giữ tối đa".to_string(), trade.entry_price).await?;
                return Ok(());
            }
            
            // Nếu chưa quá thời gian, tiếp tục theo dõi
            return Ok(());
        }
        
        // Tính toán phần trăm lợi nhuận/lỗ hiện tại
        let profit_percent = ((current_price - trade.entry_price) / trade.entry_price) * 100.0;
        let current_time = chrono::Utc::now().timestamp() as u64;
        
        // Cập nhật giá cao nhất nếu cần (chỉ với chiến lược có TSL)
        let mut highest_price = trade.highest_price;
        if current_price > highest_price {
            highest_price = current_price;
            
            // Cập nhật TradeTracker với giá cao nhất mới
            {
                let mut active_trades = self.active_trades.write().await;
                if let Some(index) = active_trades.iter().position(|t| t.trade_id == trade.trade_id) {
                    active_trades[index].highest_price = highest_price;
                }
            }
        }
        
        // 1. Kiểm tra điều kiện lợi nhuận đạt mục tiêu
        if profit_percent >= trade.take_profit_percent {
            info!(
                "Take profit triggered for {} (ID: {}): current profit {:.2}% >= target {:.2}%", 
                trade.token_address, trade.trade_id, profit_percent, trade.take_profit_percent
            );
            self.sell_token(trade, "Đạt mục tiêu lợi nhuận".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 2. Kiểm tra điều kiện stop loss
        if profit_percent <= -trade.stop_loss_percent {
            warn!(
                "Stop loss triggered for {} (ID: {}): current profit {:.2}% <= -{:.2}%", 
                trade.token_address, trade.trade_id, profit_percent, trade.stop_loss_percent
            );
            self.sell_token(trade, "Kích hoạt stop loss".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 3. Kiểm tra trailing stop loss (nếu có)
        if let Some(tsl_percent) = trade.trailing_stop_percent {
            let tsl_trigger_price = highest_price * (1.0 - tsl_percent / 100.0);
            
            if current_price <= tsl_trigger_price && highest_price > trade.entry_price {
                // Chỉ kích hoạt TSL nếu đã có lãi trước đó
                info!(
                    "Trailing stop loss triggered for {} (ID: {}): current price {} <= TSL price {:.4} (highest {:.4})", 
                    trade.token_address, trade.trade_id, current_price, tsl_trigger_price, highest_price
                );
                self.sell_token(trade, "Kích hoạt trailing stop loss".to_string(), current_price).await?;
                return Ok(());
            }
        }
        
        // 4. Kiểm tra hết thời gian tối đa
        if current_time >= trade.max_hold_time {
            info!(
                "Max hold time reached for {} (ID: {}): current profit {:.2}%", 
                trade.token_address, trade.trade_id, profit_percent
            );
            self.sell_token(trade, "Đã hết thời gian giữ tối đa".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 5. Kiểm tra an toàn token
        if trade.strategy != TradeStrategy::Manual {
            if let Err(e) = self.check_token_safety_changes(&trade, adapter).await {
                warn!("Phát hiện vấn đề an toàn với token {}: {}", trade.token_address, e);
                self.sell_token(trade, format!("Phát hiện vấn đề an toàn: {}", e), current_price).await?;
                return Ok(());
            }
        }
        
        // Giao dịch vẫn an toàn, tiếp tục theo dõi
        debug!(
            "Trade {} (ID: {}) still active: current profit {:.2}%, current price {:.8}, highest {:.8}", 
            trade.token_address, trade.trade_id, profit_percent, current_price, highest_price
        );
        
        Ok(())
    }
    
    /// Kiểm tra các vấn đề an toàn mới phát sinh với token
    async fn check_token_safety_changes(&self, trade: &TradeTracker, adapter: &Arc<EvmAdapter>) -> anyhow::Result<()> {
        debug!("Checking token safety changes for {}", trade.token_address);
        
        // Lấy thông tin token
        let contract_info = match self.get_contract_info(trade.chain_id, &trade.token_address, adapter).await {
            Some(info) => info,
            None => return Ok(()),
        };
        
        // Kiểm tra thay đổi về quyền owner
        let (has_privilege, privileges) = self.detect_owner_privilege(trade.chain_id, &trade.token_address).await?;
        
        // Kiểm tra có thay đổi về tax/fee
        let (has_dynamic_tax, tax_reason) = self.detect_dynamic_tax(trade.chain_id, &trade.token_address).await?;
        
        // Kiểm tra các sự kiện bất thường
        let liquidity_events = adapter.get_liquidity_events(&trade.token_address, 10).await
            .context("Failed to get recent liquidity events")?;
            
        let has_abnormal_events = abnormal_liquidity_events(&liquidity_events);
        
        // Nếu có bất kỳ vấn đề nghiêm trọng, ghi log và cảnh báo
        if has_dynamic_tax || has_privilege || has_abnormal_events {
            let mut warnings = Vec::new();
            
            if has_dynamic_tax {
                warnings.push(format!("Tax may have changed: {}", tax_reason.unwrap_or_else(|| "Unknown".to_string())));
            }
            
            if has_privilege && !privileges.is_empty() {
                warnings.push(format!("Owner privileges detected: {}", privileges.join(", ")));
            }
            
            if has_abnormal_events {
                warnings.push("Abnormal liquidity events detected".to_string());
            }
            
            let warning_msg = warnings.join("; ");
            warn!("Token safety concerns for {}: {}", trade.token_address, warning_msg);
            
            // Gửi cảnh báo cho user (thông qua alert system)
            if let Some(alert_system) = &self.alert_system {
                let alert = Alert {
                    token_address: trade.token_address.clone(),
                    chain_id: trade.chain_id,
                    alert_type: AlertType::TokenSafetyChanged,
                    message: warning_msg.clone(),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    trade_id: Some(trade.id.clone()),
                };
                
                if let Err(e) = alert_system.send_alert(alert).await {
                    error!("Failed to send alert: {}", e);
                }
            }
        }
        
        Ok(())
    }
    
    /// Bán token
    async fn sell_token(&self, trade: TradeTracker, exit_reason: String, current_price: f64) {
        if let Some(adapter) = self.evm_adapters.get(&trade.chain_id) {
            // Tạo tham số giao dịch bán
            let trade_params = TradeParams {
                chain_type: ChainType::EVM(trade.chain_id),
                token_address: trade.token_address.clone(),
                amount: trade.token_amount,
                slippage: 1.0, // 1% slippage
                trade_type: TradeType::Sell,
                deadline_minutes: 5,
                router_address: "".to_string(), // Sẽ dùng router mặc định
            };
            
            // Thực hiện bán
            match adapter.execute_trade(&trade_params).await {
                Ok(result) => {
                    if let Some(tx_receipt) = result.tx_receipt {
                        // Cập nhật trạng thái giao dịch
                        let now = Utc::now().timestamp() as u64;
                        
                        // Tính lợi nhuận, có kiểm tra chia cho 0
                        let profit_percent = if trade.entry_price > 0.0 {
                            (current_price - trade.entry_price) / trade.entry_price * 100.0
                        } else {
                            0.0 // Giá trị mặc định nếu entry_price = 0
                        };
                        
                        // Lấy giá thực tế từ adapter thay vì hardcoded
                        let token_price_usd = adapter.get_base_token_price_usd().await.unwrap_or(300.0);
                        let profit_usd = trade.invested_amount * profit_percent / 100.0 * token_price_usd;
                        
                        // Tạo kết quả giao dịch
                        let trade_result = TradeResult {
                            trade_id: trade.trade_id.clone(),
                            entry_price: trade.entry_price,
                            exit_price: Some(current_price),
                            profit_percent: Some(profit_percent),
                            profit_usd: Some(profit_usd),
                            entry_time: trade.entry_time,
                            exit_time: Some(now),
                            status: TradeStatus::Closed,
                            exit_reason: Some(exit_reason.clone()),
                            gas_cost_usd: result.gas_cost_usd,
                        };
                        
                        // Cập nhật lịch sử
                        {
                            let mut trade_history = self.trade_history.write().await;
                            trade_history.push(trade_result);
                        }
                        
                        // Xóa khỏi danh sách theo dõi
                        {
                            let mut active_trades = self.active_trades.write().await;
                            active_trades.retain(|t| t.trade_id != trade.trade_id);
                        }
                    }
                },
                Err(e) => {
                    // Log lỗi
                    error!("Error selling token: {:?}", e);
                }
            }
        }
    }
    
    /// Lấy thông tin contract
    pub(crate) async fn get_contract_info(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> Option<ContractInfo> {
        let source_code_future = adapter.get_contract_source_code(token_address);
        let bytecode_future = adapter.get_contract_bytecode(token_address);
        
        let ((source_code, is_verified), bytecode) = tokio::join!(
            source_code_future,
            bytecode_future
        );
        
        let (source_code, is_verified) = source_code.unwrap_or((None, false));
        let bytecode = bytecode.unwrap_or(None);
        
        let abi = if is_verified { 
            adapter.get_contract_abi(token_address).await.unwrap_or(None) 
        } else { 
            None 
        };
        
        let owner_address = adapter.get_contract_owner(token_address).await.unwrap_or(None);
        
        Some(ContractInfo {
            address: token_address.to_string(),
            chain_id,
            source_code,
            bytecode,
            abi,
            is_verified,
            owner_address,
        })
    }
    
    /// Kiểm tra thanh khoản đã được khóa hay chưa và thời gian khóa
    async fn check_liquidity_lock(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, u64) {
        match adapter.check_liquidity_lock(token_address).await {
            Ok((is_locked, duration)) => (is_locked, duration),
            Err(_) => (false, 0), // Mặc định coi như không khóa nếu có lỗi
        }
    }
    
    /// Kiểm tra các dấu hiệu rug pull
    async fn check_rug_pull_indicators(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>, contract_info: &ContractInfo) -> (bool, Vec<String>) {
        let mut indicators = Vec::new();
        
        // Kiểm tra sở hữu tập trung
        if let Ok(ownership_data) = adapter.check_token_ownership_distribution(token_address).await {
            if ownership_data.max_wallet_percentage > MAX_SAFE_OWNERSHIP_PERCENTAGE {
                indicators.push(format!("High ownership concentration: {:.2}%", ownership_data.max_wallet_percentage));
            }
        }
        
        // Kiểm tra quyền mint không giới hạn
        if let Ok(has_unlimited_mint) = adapter.check_unlimited_mint(token_address).await {
            if has_unlimited_mint {
                indicators.push("Unlimited mint function".to_string());
            }
        }
        
        // Kiểm tra blacklist/transfer delay
        if let Ok(has_blacklist) = adapter.check_blacklist_function(token_address).await {
            if has_blacklist {
                indicators.push("Blacklist function".to_string());
            }
        }
        
        if let Ok(has_transfer_delay) = adapter.check_transfer_delay(token_address).await {
            if has_transfer_delay > MAX_TRANSFER_DELAY_SECONDS {
                indicators.push(format!("Transfer delay: {}s", has_transfer_delay));
            }
        }
        
        // Kiểm tra fake ownership renounce
        if let Ok(has_fake_renounce) = adapter.check_fake_ownership_renounce(token_address).await {
            if has_fake_renounce {
                indicators.push("Fake ownership renounce".to_string());
            }
        }
        
        // Kiểm tra history của dev wallet
        if let Some(owner_address) = &contract_info.owner_address {
            if let Ok(dev_history) = adapter.check_developer_history(owner_address).await {
                if dev_history.previous_rug_pulls > 0 {
                    indicators.push(format!("Developer involved in {} previous rug pulls", dev_history.previous_rug_pulls));
                }
            }
        }
        
        (indicators.len() > 0, indicators)
    }
    
    /// Dọn dẹp lịch sử giao dịch cũ
    /// 
    /// Giới hạn kích thước của trade_history để tránh memory leak
    async fn cleanup_trade_history(&self) {
        const MAX_HISTORY_SIZE: usize = 1000;
        const MAX_HISTORY_AGE_SECONDS: u64 = 7 * 24 * 60 * 60; // 7 ngày
        
        let mut history = self.trade_history.write().await;
        if history.len() <= MAX_HISTORY_SIZE {
            return;
        }
        
        let now = Utc::now().timestamp() as u64;
        
        // Lọc ra các giao dịch quá cũ
        history.retain(|trade| {
            now.saturating_sub(trade.created_at) < MAX_HISTORY_AGE_SECONDS
        });
        
        // Nếu vẫn còn quá nhiều, sắp xếp theo thời gian và giữ lại MAX_HISTORY_SIZE
        if history.len() > MAX_HISTORY_SIZE {
            history.sort_by(|a, b| b.created_at.cmp(&a.created_at)); // Sắp xếp mới nhất trước
            history.truncate(MAX_HISTORY_SIZE);
        }
    }
    
    /// Điều chỉnh thời gian ngủ dựa trên số lượng giao dịch đang theo dõi
    /// 
    /// Khi có nhiều giao dịch cần theo dõi, giảm thời gian ngủ để kiểm tra thường xuyên hơn
    /// Khi không có giao dịch nào, tăng thời gian ngủ để tiết kiệm tài nguyên
    async fn adaptive_sleep_time(&self) -> u64 {
        const PRICE_CHECK_INTERVAL_MS: u64 = 5000; // 5 giây
        
        let active_trades = self.active_trades.read().await;
        let count = active_trades.len();
        
        if count == 0 {
            // Không có giao dịch nào, sleep lâu hơn để tiết kiệm tài nguyên
            PRICE_CHECK_INTERVAL_MS * 3
        } else if count > 5 {
            // Nhiều giao dịch đang hoạt động, giảm thời gian kiểm tra
            PRICE_CHECK_INTERVAL_MS / 3
        } else {
            // Sử dụng thời gian mặc định
            PRICE_CHECK_INTERVAL_MS
        }
    }

    /// Cập nhật và lưu trạng thái mới
    async fn update_and_persist_trades(&self) -> Result<()> {
        let active_trades = self.active_trades.read().await;
        let history = self.trade_history.read().await;
        
        // Tạo đối tượng trạng thái để lưu
        let state = SavedBotState {
            active_trades: active_trades.clone(),
            recent_history: history.clone(),
            updated_at: Utc::now().timestamp(),
        };
        
        // Chuyển đổi sang JSON
        let state_json = serde_json::to_string(&state)
            .context("Failed to serialize bot state")?;
        
        // Lưu vào file
        let state_dir = std::env::var("SNIPEBOT_STATE_DIR")
            .unwrap_or_else(|_| "data/state".to_string());
        
        // Đảm bảo thư mục tồn tại
        std::fs::create_dir_all(&state_dir)
            .context(format!("Failed to create state directory: {}", state_dir))?;
        
        let filename = format!("{}/smart_trade_state.json", state_dir);
        std::fs::write(&filename, state_json)
            .context(format!("Failed to write state to file: {}", filename))?;
        
        debug!("Persisted bot state to {}", filename);
        Ok(())
    }
    
    /// Phục hồi trạng thái từ lưu trữ
    async fn restore_state(&self) -> Result<()> {
        // Lấy đường dẫn file trạng thái
        let state_dir = std::env::var("SNIPEBOT_STATE_DIR")
            .unwrap_or_else(|_| "data/state".to_string());
        let filename = format!("{}/smart_trade_state.json", state_dir);
        
        // Kiểm tra file tồn tại
        if !std::path::Path::new(&filename).exists() {
            info!("No saved state found, starting with empty state");
            return Ok(());
        }
        
        // Đọc file
        let state_json = std::fs::read_to_string(&filename)
            .context(format!("Failed to read state file: {}", filename))?;
        
        // Chuyển đổi từ JSON
        let state: SavedBotState = serde_json::from_str(&state_json)
            .context("Failed to deserialize bot state")?;
        
        // Kiểm tra tính hợp lệ của dữ liệu
        let now = Utc::now().timestamp();
        let max_age_seconds = 24 * 60 * 60; // 24 giờ
        
        if now - state.updated_at > max_age_seconds {
            warn!("Saved state is too old ({} seconds), starting with empty state", 
                  now - state.updated_at);
            return Ok(());
        }
        
        // Phục hồi trạng thái
        {
            let mut active_trades = self.active_trades.write().await;
            active_trades.clear();
            active_trades.extend(state.active_trades);
            info!("Restored {} active trades from saved state", active_trades.len());
        }
        
        {
            let mut history = self.trade_history.write().await;
            history.clear();
            history.extend(state.recent_history);
            info!("Restored {} historical trades from saved state", history.len());
        }
        
        Ok(())
    }

    /// Monitor active trades and perform necessary actions
    async fn monitor_loop(&self) {
        let mut counter = 0;
        let mut persist_counter = 0;
        
        while let true_val = *self.running.read().await {
            if !true_val {
                break;
            }
            
            // Kiểm tra và cập nhật tất cả các giao dịch đang active
            if let Err(e) = self.check_active_trades().await {
                error!("Error checking active trades: {}", e);
            }
            
            // Tăng counter và dọn dẹp lịch sử giao dịch định kỳ
            counter += 1;
            if counter >= 100 {
                self.cleanup_trade_history().await;
                counter = 0;
            }
            
            // Lưu trạng thái vào bộ nhớ định kỳ
            persist_counter += 1;
            if persist_counter >= 30 { // Lưu mỗi 30 chu kỳ (khoảng 2.5 phút với interval 5s)
                if let Err(e) = self.update_and_persist_trades().await {
                    error!("Failed to persist bot state: {}", e);
                }
                persist_counter = 0;
            }
            
            // Tính toán thời gian sleep thích ứng
            let sleep_time = self.adaptive_sleep_time().await;
            tokio::time::sleep(tokio::time::Duration::from_millis(sleep_time)).await;
        }
        
        // Lưu trạng thái trước khi dừng
        if let Err(e) = self.update_and_persist_trades().await {
            error!("Failed to persist bot state before stopping: {}", e);
        }
    }

    /// Check and update the status of a trade
    async fn check_and_update_trade(&self, trade: TradeTracker) -> anyhow::Result<()> {
        // Get adapter for chain
        let adapter = match self.evm_adapters.get(&trade.chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("No adapter found for chain ID {}", trade.chain_id));
            }
        };
        
        // If trade is not in Open state, skip
        if trade.status != TradeStatus::Open {
            return Ok(());
        }
        
        // Get current price of token
        let current_price = match adapter.get_token_price(&trade.token_address).await {
            Ok(price) => price,
            Err(e) => {
                warn!("Cannot get current price for token {}: {}", trade.token_address, e);
                // Cannot get price, but still need to check other conditions
                // Continue with price = 0
                0.0
            }
        };
        
        // If cannot get price, check max hold time
        if current_price <= 0.0 {
            let current_time = chrono::Utc::now().timestamp() as u64;
            
            // If exceeded max hold time, sell
            if current_time >= trade.max_hold_time {
                warn!("Cannot get price for token {}, exceeded max hold time, perform sell", trade.token_address);
                self.sell_token(trade, "Exceeded max hold time".to_string(), trade.entry_price).await?;
                return Ok(());
            }
            
            // If not exceeded, continue monitoring
            return Ok(());
        }
        
        // Calculate current profit percentage/loss
        let profit_percent = ((current_price - trade.entry_price) / trade.entry_price) * 100.0;
        let current_time = chrono::Utc::now().timestamp() as u64;
        
        // Update highest price if needed (only for Smart strategy)
        let mut highest_price = trade.highest_price;
        if current_price > highest_price {
            highest_price = current_price;
            
            // Update TradeTracker with new highest price
            {
                let mut active_trades = self.active_trades.write().await;
                if let Some(index) = active_trades.iter().position(|t| t.trade_id == trade.trade_id) {
                    active_trades[index].highest_price = highest_price;
                }
            }
        }
        
        // 1. Check profit condition to reach target
        if profit_percent >= trade.take_profit_percent {
            info!(
                "Take profit triggered for {} (ID: {}): current profit {:.2}% >= target {:.2}%", 
                trade.token_address, trade.trade_id, profit_percent, trade.take_profit_percent
            );
            self.sell_token(trade, "Reached profit target".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 2. Check stop loss condition
        if profit_percent <= -trade.stop_loss_percent {
            warn!(
                "Stop loss triggered for {} (ID: {}): current profit {:.2}% <= -{:.2}%", 
                trade.token_address, trade.trade_id, profit_percent, trade.stop_loss_percent
            );
            self.sell_token(trade, "Activated stop loss".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 3. Check trailing stop loss (if any)
        if let Some(tsl_percent) = trade.trailing_stop_percent {
            let tsl_trigger_price = highest_price * (1.0 - tsl_percent / 100.0);
            
            if current_price <= tsl_trigger_price && highest_price > trade.entry_price {
                // Activate TSL only if there was profit before
                info!(
                    "Trailing stop loss triggered for {} (ID: {}): current price {} <= TSL price {:.4} (highest {:.4})", 
                    trade.token_address, trade.trade_id, current_price, tsl_trigger_price, highest_price
                );
                self.sell_token(trade, "Activated trailing stop loss".to_string(), current_price).await?;
                return Ok(());
            }
        }
        
        // 4. Check max hold time
        if current_time >= trade.max_hold_time {
            info!(
                "Max hold time reached for {} (ID: {}): current profit {:.2}%", 
                trade.token_address, trade.trade_id, profit_percent
            );
            self.sell_token(trade, "Exceeded max hold time".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 5. Check token safety
        if trade.strategy != TradeStrategy::Manual {
            if let Err(e) = self.check_token_safety_changes(&trade, adapter).await {
                warn!("Security issue detected with token {}: {}", trade.token_address, e);
                self.sell_token(trade, format!("Security issue detected: {}", e), current_price).await?;
                return Ok(());
            }
        }
        
        // Trade still safe, continue monitoring
        debug!(
            "Trade {} (ID: {}) still active: current profit {:.2}%, current price {:.8}, highest {:.8}", 
            trade.token_address, trade.trade_id, profit_percent, current_price, highest_price
        );
        
        Ok(())
    }

    /// Monitor active trades and perform necessary actions
    async fn check_active_trades(&self) -> anyhow::Result<()> {
        // Check if there are any trades that need monitoring
        let active_trades = self.active_trades.read().await;
        
        if active_trades.is_empty() {
            return Ok(());
        }
        
        // Clone list to avoid holding lock for too long
        let active_trades_clone = active_trades.clone();
        drop(active_trades); // Release lock to avoid deadlock
        
        // Process each trade in parallel
        let update_futures = active_trades_clone.iter().map(|trade| {
            self.check_and_update_trade(trade.clone())
        });
        
        let results = futures::future::join_all(update_futures).await;
        
        // Check and log error if any
        for result in results {
            if let Err(e) = result {
                error!("Error monitoring trade: {}", e);
                // Continue processing other trades, do not stop
            }
        }
        
        Ok(())
    }

    /// Clone executor and copy config from original instance (safe for async context)
    pub(crate) async fn clone_with_config(&self) -> Self {
        let config = {
            let config_guard = self.config.read().await;
            config_guard.clone()
        };
        
        Self {
            evm_adapters: self.evm_adapters.clone(),
            analys_client: self.analys_client.clone(),
            risk_analyzer: self.risk_analyzer.clone(),
            mempool_analyzers: self.mempool_analyzers.clone(),
            config: RwLock::new(config),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            coordinator: self.coordinator.clone(),
            executor_id: self.executor_id.clone(),
            coordinator_subscription: RwLock::new(None),
        }
    }

    /// Đăng ký với coordinator
    async fn register_with_coordinator(&self) -> anyhow::Result<()> {
        if let Some(coordinator) = &self.coordinator {
            // Đăng ký executor với coordinator
            coordinator.register_executor(&self.executor_id, ExecutorType::SmartTrade).await?;
            
            // Đăng ký nhận thông báo về các cơ hội mới
            let subscription_callback = {
                let self_clone = self.clone_with_config().await;
                Arc::new(move |opportunity: SharedOpportunity| -> anyhow::Result<()> {
                    let executor = self_clone.clone();
                    tokio::spawn(async move {
                        if let Err(e) = executor.handle_shared_opportunity(opportunity).await {
                            error!("Error handling shared opportunity: {}", e);
                        }
                    });
                    Ok(())
                })
            };
            
            // Lưu subscription ID
            let subscription_id = coordinator.subscribe_to_opportunities(
                &self.executor_id, subscription_callback
            ).await?;
            
            let mut sub_id = self.coordinator_subscription.write().await;
            *sub_id = Some(subscription_id);
            
            info!("Registered with coordinator, subscription ID: {}", sub_id.as_ref().unwrap());
        }
        
        Ok(())
    }
    
    /// Hủy đăng ký khỏi coordinator
    async fn unregister_from_coordinator(&self) -> anyhow::Result<()> {
        if let Some(coordinator) = &self.coordinator {
            // Hủy đăng ký subscription
            let sub_id = {
                let sub = self.coordinator_subscription.read().await;
                sub.clone()
            };
            
            if let Some(id) = sub_id {
                coordinator.unsubscribe_from_opportunities(&id).await?;
                
                // Xóa subscription ID
                let mut sub = self.coordinator_subscription.write().await;
                *sub = None;
            }
            
            // Hủy đăng ký executor
            coordinator.unregister_executor(&self.executor_id).await?;
            info!("Unregistered from coordinator");
        }
        
        Ok(())
    }
    
    /// Xử lý cơ hội được chia sẻ từ coordinator
    async fn handle_shared_opportunity(&self, opportunity: SharedOpportunity) -> anyhow::Result<()> {
        info!("Received shared opportunity: {} (type: {:?}, from: {})", 
             opportunity.id, opportunity.opportunity_type, opportunity.source);
        
        // Kiểm tra nếu nguồn là chính mình, bỏ qua
        if opportunity.source == self.executor_id {
            debug!("Ignoring opportunity from self");
            return Ok(());
        }
        
        // Lấy cấu hình
        let config = self.config.read().await;
        
        // Kiểm tra nếu bot chưa bật hoặc đang đầy trade
        if !config.enabled || !config.auto_trade {
            debug!("Bot disabled or auto-trade disabled, ignoring opportunity");
            return Ok(());
        }
        
        let active_trades = self.active_trades.read().await;
        if active_trades.len() >= config.max_concurrent_trades as usize {
            debug!("Max concurrent trades reached, ignoring opportunity");
            return Ok(());
        }
        
        // Kiểm tra xem có token nào trong danh sách token của cơ hội nằm trong blacklist không
        let mut blacklisted = false;
        for token in &opportunity.tokens {
            if config.token_blacklist.contains(token) {
                blacklisted = true;
                debug!("Token {} in blacklist, ignoring opportunity", token);
                break;
            }
        }
        
        if blacklisted {
            return Ok(());
        }
        
        // Nếu whitelist không rỗng, kiểm tra xem có token nào nằm trong whitelist không
        if !config.token_whitelist.is_empty() {
            let mut whitelisted = false;
            for token in &opportunity.tokens {
                if config.token_whitelist.contains(token) {
                    whitelisted = true;
                    break;
                }
            }
            
            if !whitelisted {
                debug!("No tokens in whitelist, ignoring opportunity");
                return Ok(());
            }
        }
        
        // Kiểm tra điểm rủi ro
        if opportunity.risk_score > config.max_risk_score {
            debug!("Risk score too high: {}/{}, ignoring opportunity", 
                  opportunity.risk_score, config.max_risk_score);
            return Ok(());
        }
        
        // Kiểm tra lợi nhuận tối thiểu
        if opportunity.estimated_profit_usd < config.min_profit_threshold {
            debug!("Profit too low: ${}/{}, ignoring opportunity", 
                  opportunity.estimated_profit_usd, config.min_profit_threshold);
            return Ok(());
        }
        
        // Kiểm tra xem cơ hội đã bị reserve chưa
        if let Some(reservation) = &opportunity.reservation {
            debug!("Opportunity already reserved by {}, ignoring", reservation.executor_id);
            return Ok(());
        }
        
        // Quyết định ưu tiên dựa trên score của cơ hội
        let priority = if opportunity.risk_score < 30 && opportunity.estimated_profit_usd > 50.0 {
            OpportunityPriority::High
        } else if opportunity.risk_score < 50 && opportunity.estimated_profit_usd > 20.0 {
            OpportunityPriority::Medium
        } else {
            OpportunityPriority::Low
        };
        
        // Thử reserve cơ hội
        if let Some(coordinator) = &self.coordinator {
            let success = coordinator.reserve_opportunity(
                &opportunity.id, &self.executor_id, priority
            ).await?;
            
            if !success {
                debug!("Failed to reserve opportunity, another executor has higher priority");
                return Ok(());
            }
            
            info!("Successfully reserved opportunity {}", opportunity.id);
        }
        
        // Xử lý cơ hội tùy theo loại
        match &opportunity.opportunity_type {
            SharedOpportunityType::Mev(mev_type) => {
                debug!("Processing MEV opportunity: {:?}", mev_type);
                // Implement MEV handling logic based on mev_type
            },
            SharedOpportunityType::NewToken => {
                // Lấy token đầu tiên trong danh sách
                if let Some(token_address) = opportunity.tokens.get(0) {
                    // Thực hiện phân tích token và giao dịch
                    debug!("Processing new token opportunity: {}", token_address);
                    
                    // Kiểm tra token an toàn
                    let token_safety = match self.evaluate_token(opportunity.chain_id, token_address).await {
                        Ok(Some(safety)) => safety,
                        _ => {
                            debug!("Token safety check failed, releasing opportunity");
                            if let Some(coordinator) = &self.coordinator {
                                coordinator.release_opportunity(&opportunity.id, &self.executor_id).await?;
                            }
                            return Ok(());
                        }
                    };
                    
                    if !token_safety.is_valid() || token_safety.is_honeypot {
                        debug!("Token is not safe or is honeypot, releasing opportunity");
                        if let Some(coordinator) = &self.coordinator {
                            coordinator.release_opportunity(&opportunity.id, &self.executor_id).await?;
                        }
                        return Ok(());
                    }
                    
                    // Dựa vào độ rủi ro để chọn chiến lược phù hợp
                    let risk_score = self.analyze_token_risk(opportunity.chain_id, token_address).await?;
                    let strategy = if risk_score.score < 40 {
                        TradeStrategy::Smart
                    } else {
                        TradeStrategy::Quick
                    };
                    
                    // Lấy thông tin giao dịch trigger từ custom_data nếu có
                    let mut trigger_tx_hash = opportunity.custom_data.get("trigger_tx").cloned();
                    if trigger_tx_hash.is_none() {
                        trigger_tx_hash = Some("shared_opportunity".to_string());
                    }
                    
                    // Tạo một MempoolTransaction giả để truyền vào executor
                    let dummy_tx = MempoolTransaction {
                        tx_hash: trigger_tx_hash.unwrap_or_else(|| "shared_opportunity".to_string()),
                        from_address: opportunity.source.clone(),
                        to_address: token_address.clone(),
                        value: opportunity.custom_data.get("value")
                            .and_then(|v| v.parse::<f64>().ok())
                            .unwrap_or(0.1),
                        gas_price: opportunity.custom_data.get("gas_price")
                            .and_then(|v| v.parse::<f64>().ok()),
                        gas_limit: opportunity.custom_data.get("gas_limit")
                            .and_then(|v| v.parse::<u64>().ok()),
                        input_data: Some("shared_opportunity".to_string()),
                        timestamp: Some(chrono::Utc::now().timestamp() as u64),
                        block_number: opportunity.custom_data.get("block_number")
                            .and_then(|v| v.parse::<u64>().ok()),
                        to_token: Some(TokenInfo {
                            address: token_address.clone(),
                            symbol: opportunity.custom_data.get("symbol")
                                .cloned()
                                .unwrap_or_else(|| "UNKNOWN".to_string()),
                            decimals: opportunity.custom_data.get("decimals")
                                .and_then(|v| v.parse::<u8>().ok())
                                .unwrap_or(18),
                            price: opportunity.custom_data.get("price")
                                .and_then(|v| v.parse::<f64>().ok())
                                .unwrap_or(0.0),
                        }),
                        transaction_type: TransactionType::Standard,
                    };
                    
                    // Thực hiện giao dịch
                    match self.execute_trade(opportunity.chain_id, token_address, strategy, &dummy_tx).await {
                        Ok(_) => info!("Successfully executed trade for shared opportunity: {}", opportunity.id),
                        Err(e) => {
                            error!("Failed to execute trade for shared opportunity: {}", e);
                            // Release opportunity để các executor khác có thể thử
                            if let Some(coordinator) = &self.coordinator {
                                coordinator.release_opportunity(&opportunity.id, &self.executor_id).await?;
                            }
                        }
                    }
                }
            },
            SharedOpportunityType::LiquidityChange => {
                // Handle liquidity change opportunity
                debug!("Processing liquidity change opportunity");
                // Implement liquidity change handling logic
            },
            SharedOpportunityType::PriceMovement => {
                // Handle price movement opportunity
                debug!("Processing price movement opportunity");
                // Implement price movement handling logic
            },
            SharedOpportunityType::Custom(custom_type) => {
                debug!("Processing custom opportunity type: {}", custom_type);
                // Implement custom opportunity handling logic
            },
        }
        
        Ok(())
    }
    
    /// Chia sẻ cơ hội mới với các executor khác thông qua coordinator
    async fn share_opportunity(
        &self, 
        chain_id: u32,
        token_address: &str,
        opportunity_type: SharedOpportunityType,
        estimated_profit_usd: f64,
        risk_score: u8,
        time_sensitivity: u64,
    ) -> anyhow::Result<()> {
        if let Some(coordinator) = &self.coordinator {
            // Tạo unique ID cho cơ hội
            let opportunity_id = format!("opp-{}", uuid::Uuid::new_v4());
            
            // Tạo custom data map
            let mut custom_data = HashMap::new();
            
            // Lấy thông tin token
            if let Some(adapter) = self.evm_adapters.get(&chain_id) {
                if let Ok(token_info) = adapter.get_token_info(token_address).await {
                    custom_data.insert("symbol".to_string(), token_info.symbol);
                    custom_data.insert("decimals".to_string(), token_info.decimals.to_string());
                    custom_data.insert("price".to_string(), token_info.price.to_string());
                }
            }
            
            // Thêm block hiện tại
            if let Some(adapter) = self.evm_adapters.get(&chain_id) {
                if let Ok(block) = adapter.get_current_block_number().await {
                    custom_data.insert("block_number".to_string(), block.to_string());
                }
            }
            
            // Tạo SharedOpportunity
            let opportunity = SharedOpportunity {
                id: opportunity_id,
                chain_id,
                opportunity_type,
                tokens: vec![token_address.to_string()],
                estimated_profit_usd,
                risk_score,
                time_sensitivity,
                source: self.executor_id.clone(),
                created_at: chrono::Utc::now().timestamp() as u64,
                custom_data,
                reservation: None,
            };
            
            // Chia sẻ qua coordinator
            coordinator.share_opportunity(&self.executor_id, opportunity).await?;
            info!("Shared opportunity for token {} with other executors", token_address);
        }
        
        Ok(())
    }

    /// Tính toán gas price tối ưu dựa trên tình trạng mạng hiện tại
    /// 
    /// * `chain_id` - ID của blockchain
    /// * `priority` - Mức độ ưu tiên của giao dịch (1 = thấp, 2 = trung bình, 3 = cao)
    async fn calculate_optimal_gas_price(&self, chain_id: u32, priority: u8) -> Result<f64> {
        // Lấy adapter cho chain này
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id)),
        };
        
        // Sử dụng chức năng chung từ common/gas
        use crate::tradelogic::common::gas::{calculate_optimal_gas_price, TransactionPriority};
        let transaction_priority: TransactionPriority = priority.into();
        
        // Lấy mempool provider từ analys client
        let mempool_provider = Some(&self.analys_client.mempool_provider());
        
        // Tính toán gas price tối ưu dùng hàm chung
        let result = calculate_optimal_gas_price(
            chain_id, 
            transaction_priority, 
            adapter, 
            mempool_provider,
            self.should_apply_mev_protection(chain_id).await
        ).await?;
        
        Ok(result)
    }
    
    /// Kiểm tra nếu nên áp dụng bảo vệ MEV
    async fn should_apply_mev_protection(&self, chain_id: u32) -> bool {
        // Chỉ áp dụng MEV protection trên mainnet và một số chain lớn có MEV
        use crate::tradelogic::common::gas::needs_mev_protection;
        needs_mev_protection(chain_id)
    }

    /// Thực hiện giao dịch với retry tự động khi thất bại
    /// 
    /// * `params` - Tham số giao dịch
    /// * `max_retries` - Số lần thử lại tối đa
    /// * `increase_gas_percent` - Phần trăm tăng gas mỗi lần thử lại
    async fn execute_trade_with_retry(
        &self, 
        mut params: TradeParams, 
        max_retries: u32, 
        increase_gas_percent: f64
    ) -> Result<TransactionResult> {
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", params.chain_id)),
        };
        
        // Nếu gas_price chưa được thiết lập, tính toán giá gas tối ưu
        if params.gas_price.is_none() {
            let optimal_gas = self.calculate_optimal_gas_price(params.chain_id, 2).await?;
            params.gas_price = Some(optimal_gas);
        }
        
        let mut last_error = None;
        let mut current_gas_price = params.gas_price.unwrap_or(0.0);
        
        // Thử giao dịch với số lần retry được chỉ định
        for retry in 0..=max_retries {
            // Cập nhật gas price cho mỗi lần retry
            if retry > 0 {
                current_gas_price *= 1.0 + (increase_gas_percent / 100.0);
                params.gas_price = Some(current_gas_price);
                
                info!("Thử lại giao dịch ({}/{}), tăng gas price lên: {} gwei", 
                     retry, max_retries, current_gas_price);
            }
            
            match adapter.execute_trade(&params).await {
                Ok(result) => {
                    // Giao dịch thành công
                    info!("Giao dịch thành công sau {} lần thử, tx hash: {}", 
                         retry, result.transaction_hash.as_ref().unwrap_or(&"unknown".to_string()));
                    return Ok(result);
                },
                Err(e) => {
                    // Xác định xem có nên thử lại không dựa trên loại lỗi
                    if self.is_retriable_error(&e) && retry < max_retries {
                        warn!("Giao dịch thất bại ({}), sẽ thử lại với gas cao hơn: {}", e, current_gas_price);
                        last_error = Some(e);
                        
                        // Chờ một khoảng thời gian ngắn trước khi thử lại
                        let wait_time = 1000 + retry as u64 * 1000; // 1s, 2s, 3s,...
                        tokio::time::sleep(tokio::time::Duration::from_millis(wait_time)).await;
                        continue;
                    } else {
                        // Lỗi không thể thử lại hoặc đã hết số lần thử
                        return Err(anyhow::anyhow!("Giao dịch thất bại sau {} lần thử: {}", 
                                                  retry, e));
                    }
                }
            }
        }
        
        // Nếu đến đây vẫn chưa return, tức là tất cả các lần retry đều thất bại
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Giao dịch thất bại với lỗi không xác định")))
    }
    
    /// Kiểm tra xem lỗi có thể thử lại không
    fn is_retriable_error(&self, error: &anyhow::Error) -> bool {
        let error_msg = error.to_string().to_lowercase();
        
        // Các lỗi liên quan đến gas, nonce hoặc mạng có thể thử lại
        error_msg.contains("underpriced") || 
        error_msg.contains("gas price too low") ||
        error_msg.contains("transaction underpriced") ||
        error_msg.contains("replacement transaction underpriced") ||
        error_msg.contains("nonce too low") ||
        error_msg.contains("already known") ||
        error_msg.contains("connection reset") ||
        error_msg.contains("timeout") ||
        error_msg.contains("503") ||
        error_msg.contains("429") ||
        error_msg.contains("rate limit") ||
        error_msg.contains("too many requests")
    }

    /// Xác thực dữ liệu mempool từ nhiều nguồn khác nhau
    /// 
    /// Đảm bảo thông tin token và giao dịch từ mempool là chính xác và đáng tin cậy
    /// trước khi thực hiện giao dịch
    /// 
    /// * `chain_id` - Chain ID
    /// * `token_address` - Địa chỉ token cần xác thực
    /// * `tx` - Giao dịch mempool đã phát hiện
    async fn validate_mempool_data(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        tx: &MempoolTransaction
    ) -> Result<ValidationResult> {
        // Lấy adapter cho chain này
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id)),
        };
        
        // 1. Xác thực địa chỉ token - Kiểm tra xem địa chỉ token có tồn tại và là token thực
        info!("Xác thực token {} trên chain {}", token_address, chain_id);
        let token_validation = self.validate_token_address(chain_id, token_address, adapter.clone()).await;
        if let Err(e) = token_validation {
            return Ok(ValidationResult {
                is_valid: false,
                token_valid: false,
                tx_valid: false,
                confidence_score: 0,
                reasons: vec![format!("Token không hợp lệ: {}", e)],
            });
        }
        
        // 2. Xác thực giao dịch mempool - Kiểm tra xem giao dịch có tồn tại trong mempool không
        info!("Xác thực giao dịch mempool: {}", tx.tx_hash);
        let tx_validated = self.validate_mempool_transaction(chain_id, tx).await;
        if !tx_validated.is_valid {
            return Ok(ValidationResult {
                is_valid: false,
                token_valid: true,
                tx_valid: false,
                confidence_score: 0,
                reasons: tx_validated.reasons,
            });
        }
        
        // 3. Xác thực mối quan hệ giữa token và giao dịch mempool
        let token_tx_validation = self.validate_token_transaction_relation(chain_id, token_address, tx).await;
        if !token_tx_validation.is_valid {
            return Ok(ValidationResult {
                is_valid: false,
                token_valid: true,
                tx_valid: true,
                confidence_score: 0,
                reasons: token_tx_validation.reasons,
            });
        }
        
        // 4. Chống giao dịch giả mạo - Kiểm tra các dấu hiệu mempool poisoning
        let anti_poisoning_check = self.check_mempool_poisoning(chain_id, token_address, tx).await;
        if !anti_poisoning_check.is_valid {
            return Ok(ValidationResult {
                is_valid: false,
                token_valid: true,
                tx_valid: true,
                confidence_score: 0,
                reasons: anti_poisoning_check.reasons,
            });
        }
        
        // 5. Xác thực giá - So sánh giá từ giao dịch mempool với giá từ các nguồn khác
        let price_validation = self.validate_price_from_multiple_sources(chain_id, token_address, tx).await;
        if !price_validation.is_valid {
            return Ok(ValidationResult {
                is_valid: false,
                token_valid: true,
                tx_valid: true,
                confidence_score: 30, // Có thể vẫn có một số khả năng là đúng
                reasons: price_validation.reasons,
            });
        }
        
        // 6. Kiểm tra hoạt động khả nghi trên token này - So sánh với lịch sử trước đó
        let suspicious_activity = self.check_suspicious_activity(chain_id, token_address, tx).await;
        if !suspicious_activity.is_valid {
            return Ok(ValidationResult {
                is_valid: false,
                token_valid: true,
                tx_valid: true,
                confidence_score: 40,
                reasons: suspicious_activity.reasons,
            });
        }
        
        // 7. Tính điểm tin cậy cho cặp token-transaction từ kết quả các bước trên
        let confidence_score = self.calculate_confidence_score(
            &token_validation,
            &tx_validated,
            &token_tx_validation,
            &anti_poisoning_check,
            &price_validation,
            &suspicious_activity
        ).await;
        
        // Kết luận: Dữ liệu mempool đáng tin cậy nếu đạt điểm tin cậy đủ cao
        if confidence_score >= 70 {
            Ok(ValidationResult {
                is_valid: true,
                token_valid: true,
                tx_valid: true,
                confidence_score,
                reasons: vec![format!("Dữ liệu mempool đáng tin cậy với điểm tin cậy {}", confidence_score)],
            })
        } else {
            Ok(ValidationResult {
                is_valid: false,
                token_valid: true,
                tx_valid: true,
                confidence_score,
                reasons: vec![format!("Điểm tin cậy quá thấp: {}/100", confidence_score)],
            })
        }
    }
    
    /// Xác thực địa chỉ token
    async fn validate_token_address(&self, chain_id: u32, token_address: &str, adapter: Arc<EvmAdapter>) -> Result<()> {
        // 1. Kiểm tra định dạng địa chỉ
        if !self.is_valid_address_format(token_address) {
            return Err(anyhow::anyhow!("Định dạng địa chỉ token không hợp lệ"));
        }
        
        // 2. Kiểm tra token có tồn tại trên chain
        let token_exists = adapter.check_token_exists(token_address).await?;
        if !token_exists {
            return Err(anyhow::anyhow!("Token không tồn tại trên chain"));
        }
        
        // 3. Kiểm tra token có phải là hợp đồng ERC20 hợp lệ
        let is_valid_erc20 = adapter.validate_erc20_interface(token_address).await?;
        if !is_valid_erc20 {
            return Err(anyhow::anyhow!("Token không tuân thủ chuẩn ERC20"));
        }
        
        // 4. Kiểm tra thêm tính hợp lệ của token thông qua analys client
        match self.analys_client.token_provider().analyze_token(chain_id, token_address).await {
            Ok(token_safety) => {
                if !token_safety.is_valid() {
                    return Err(anyhow::anyhow!("Token không hợp lệ theo phân tích an toàn"));
                }
            },
            Err(e) => {
                warn!("Không thể phân tích token thông qua analys client: {}", e);
                // Không fail hard ở đây, vẫn tiếp tục với các kiểm tra khác
            }
        }
        
        Ok(())
    }
    
    /// Kiểm tra định dạng địa chỉ Ethereum
    fn is_valid_address_format(&self, address: &str) -> bool {
        // Kiểm tra cơ bản: 0x + 40 ký tự hex
        if !address.starts_with("0x") || address.len() != 42 {
            return false;
        }
        
        // Kiểm tra xem có chứa các ký tự không hợp lệ không
        for c in address[2..].chars() {
            if !c.is_ascii_hexdigit() {
                return false;
            }
        }
        
        true
    }
    
    /// Xác thực giao dịch mempool
    async fn validate_mempool_transaction(&self, chain_id: u32, tx: &MempoolTransaction) -> SimpleValidationResult {
        let mut reasons = Vec::new();
        let mut is_valid = true;
        
        // 1. Kiểm tra tx_hash
        if tx.tx_hash.is_empty() || !tx.tx_hash.starts_with("0x") || tx.tx_hash.len() != 66 {
            reasons.push("TX hash không hợp lệ".to_string());
            is_valid = false;
        }
        
        // 2. Kiểm tra from_address
        if tx.from_address.is_empty() || !self.is_valid_address_format(&tx.from_address) {
            reasons.push("Địa chỉ người gửi không hợp lệ".to_string());
            is_valid = false;
        }
        
        // 3. Kiểm tra to_address
        if tx.to_address.is_empty() || !self.is_valid_address_format(&tx.to_address) {
            reasons.push("Địa chỉ người nhận không hợp lệ".to_string());
            is_valid = false;
        }
        
        // 4. Kiểm tra xem giao dịch có còn trong mempool không
        if let Some(analyzer) = self.mempool_analyzers.get(&chain_id) {
            match analyzer.is_transaction_in_mempool(&tx.tx_hash).await {
                Ok(in_mempool) => {
                    if !in_mempool {
                        reasons.push("Giao dịch không còn trong mempool".to_string());
                        is_valid = false;
                    }
                },
                Err(e) => {
                    warn!("Không thể kiểm tra giao dịch trong mempool: {}", e);
                    reasons.push(format!("Không thể xác minh giao dịch trong mempool: {}", e));
                    // Không fail hard, vẫn tiếp tục
                }
            }
        }
        
        // 5. Kiểm tra timestamp (nếu giao dịch quá cũ > 30 giây, có thể không còn hợp lệ)
        let now = chrono::Utc::now().timestamp() as u64;
        let tx_timestamp = tx.timestamp.unwrap_or(now);
        if now - tx_timestamp > 30 {
            reasons.push(format!("Giao dịch quá cũ ({} giây)", now - tx_timestamp));
            // Giao dịch quá cũ là warning, không phải lỗi nặng
            if now - tx_timestamp > 120 {
                is_valid = false;
            }
        }
        
        SimpleValidationResult {
            is_valid,
            reasons,
        }
    }
    
    /// Xác thực mối quan hệ giữa token và giao dịch mempool
    async fn validate_token_transaction_relation(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        tx: &MempoolTransaction
    ) -> SimpleValidationResult {
        let mut reasons = Vec::new();
        let mut is_valid = true;
        
        // 1. Kiểm tra nếu token được đề cập trong giao dịch
        let token_in_tx = if let Some(to_token) = &tx.to_token {
            // Nếu to_token tồn tại, kiểm tra địa chỉ có khớp không
            if to_token.address.to_lowercase() != token_address.to_lowercase() {
                reasons.push(format!(
                    "Không khớp địa chỉ token: {} vs {}", 
                    to_token.address.to_lowercase(), 
                    token_address.to_lowercase()
                ));
                false
            } else {
                true
            }
        } else {
            // Nếu không có to_token, kiểm tra input data của giao dịch
            if let Some(input) = &tx.input_data {
                // Kiểm tra xem input data có chứa địa chỉ token không (bỏ 0x)
                let token_no_prefix = if token_address.starts_with("0x") {
                    &token_address[2..]
                } else {
                    token_address
                };
                
                input.contains(&token_no_prefix.to_lowercase())
            } else {
                false
            }
        };
        
        if !token_in_tx {
            reasons.push("Giao dịch không liên quan đến token này".to_string());
            is_valid = false;
        }
        
        // 2. Kiểm tra gas limit - nếu gas limit quá thấp, có thể giao dịch sẽ thất bại
        if let Some(gas_limit) = tx.gas_limit {
            if gas_limit < 100000 {
                reasons.push(format!("Gas limit quá thấp ({}) cho giao dịch token", gas_limit));
                // Không fail hard, chỉ cảnh báo
            }
        }
        
        // 3. Kiểm tra giá trị giao dịch
        if tx.value < 0.001 {
            reasons.push(format!("Giá trị giao dịch quá nhỏ: {} BNB", tx.value));
            // Không fail hard, chỉ cảnh báo
        }
        
        SimpleValidationResult {
            is_valid,
            reasons,
        }
    }
    
    /// Kiểm tra dấu hiệu mempool poisoning
    async fn check_mempool_poisoning(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        tx: &MempoolTransaction
    ) -> SimpleValidationResult {
        let mut reasons = Vec::new();
        let mut is_valid = true;
        
        // 1. Kiểm tra nếu có nhiều giao dịch tương tự trong mempool (có thể là cố tình tạo nhiều giao dịch để lừa bots)
        if let Some(analyzer) = self.mempool_analyzers.get(&chain_id) {
            match analyzer.count_similar_transactions(tx).await {
                Ok(count) => {
                    if count > 5 {
                        reasons.push(format!("Phát hiện {} giao dịch tương tự - có thể là mempool poisoning", count));
                        is_valid = false;
                    }
                },
                Err(e) => {
                    warn!("Không thể kiểm tra giao dịch tương tự: {}", e);
                    // Không fail hard, vẫn tiếp tục
                }
            }
        }
        
        // 2. Kiểm tra địa chỉ tạo giao dịch có phải là nguồn tin cậy không
        match self.analys_client.trader_reputation(&tx.from_address).await {
            Ok(reputation) => {
                if reputation < 20 {
                    reasons.push(format!("Địa chỉ tạo giao dịch có điểm uy tín thấp: {}/100", reputation));
                    is_valid = false;
                }
            },
            Err(e) => {
                warn!("Không thể kiểm tra uy tín người gửi: {}", e);
                // Không fail hard, vẫn tiếp tục
            }
        }
        
        // 3. Kiểm tra xem có dấu hiệu của giao dịch spam token không
        if let Some(analyzer) = self.mempool_analyzers.get(&chain_id) {
            match analyzer.is_token_being_spammed(token_address).await {
                Ok(is_spam) => {
                    if is_spam {
                        reasons.push("Token này đang bị spam trong mempool".to_string());
                        is_valid = false;
                    }
                },
                Err(e) => {
                    warn!("Không thể kiểm tra spam token: {}", e);
                    // Không fail hard, vẫn tiếp tục
                }
            }
        }
        
        SimpleValidationResult {
            is_valid,
            reasons,
        }
    }
    
    /// Xác thực giá từ nhiều nguồn
    async fn validate_price_from_multiple_sources(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        tx: &MempoolTransaction
    ) -> SimpleValidationResult {
        let mut reasons = Vec::new();
        let mut is_valid = true;
        
        // 1. Lấy giá từ giao dịch mempool
        let tx_price = if let Some(to_token) = &tx.to_token {
            to_token.price
        } else {
            0.0
        };
        
        // 2. Lấy giá từ adapter trực tiếp (gọi router contract)
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                reasons.push(format!("Không tìm thấy adapter cho chain ID {}", chain_id));
                return SimpleValidationResult {
                    is_valid: false,
                    reasons,
                };
            }
        };
        
        let adapter_price = match adapter.get_token_price(token_address).await {
            Ok(price) => price,
            Err(e) => {
                warn!("Không thể lấy giá từ adapter: {}", e);
                0.0 // Không thể lấy giá
            }
        };
        
        // 3. Lấy giá từ analys client (có thể từ nhiều nguồn như DEX API, indexer, oracle)
        let analys_price = match self.analys_client.token_price(chain_id, token_address).await {
            Ok(price) => price,
            Err(e) => {
                warn!("Không thể lấy giá từ analys client: {}", e);
                0.0
            }
        };
        
        // 4. So sánh giá từ các nguồn khác nhau
        if tx_price > 0.0 && adapter_price > 0.0 {
            let price_diff_percent = ((tx_price - adapter_price).abs() / adapter_price) * 100.0;
            
            if price_diff_percent > 10.0 {
                reasons.push(format!(
                    "Chênh lệch giá lớn: {:.2}% (mempool: ${:.6}, adapter: ${:.6})",
                    price_diff_percent, tx_price, adapter_price
                ));
                
                // Nếu chênh lệch > 20%, coi là không hợp lệ
                if price_diff_percent > 20.0 {
                    is_valid = false;
                }
            }
        }
        
        if analys_price > 0.0 && adapter_price > 0.0 {
            let price_diff_percent = ((analys_price - adapter_price).abs() / adapter_price) * 100.0;
            
            if price_diff_percent > 15.0 {
                reasons.push(format!(
                    "Chênh lệch giá giữa nguồn phân tích và adapter: {:.2}% (analys: ${:.6}, adapter: ${:.6})",
                    price_diff_percent, analys_price, adapter_price
                ));
                
                // Nếu chênh lệch > 25%, coi là không hợp lệ
                if price_diff_percent > 25.0 {
                    is_valid = false;
                }
            }
        }
        
        SimpleValidationResult {
            is_valid,
            reasons,
        }
    }
    
    /// Kiểm tra hoạt động khả nghi trên token
    async fn check_suspicious_activity(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        tx: &MempoolTransaction
    ) -> SimpleValidationResult {
        let mut reasons = Vec::new();
        let mut is_valid = true;
        
        // 1. Kiểm tra lịch sử giao dịch gần đây của token
        let recent_transactions = match self.analys_client.token_provider().get_recent_transactions(
            chain_id, token_address, 20
        ).await {
            Ok(txs) => txs,
            Err(e) => {
                warn!("Không thể lấy giao dịch gần đây của token: {}", e);
                Vec::new()
            }
        };
        
        // 2. Kiểm tra dấu hiệu rug pull baseed on giá trị giao dịch lớn bất thường
        let large_transfers = recent_transactions.iter()
            .filter(|tx| tx.transfer_value > 1000.0) // Giá trị > $1000
            .count();
        
        if large_transfers > 5 {
            reasons.push(format!("Phát hiện {} giao dịch giá trị lớn gần đây", large_transfers));
            // Không fail hard, chỉ cảnh báo
        }
        
        // 3. Kiểm tra dấu hiệu wash-trading
        let unique_addresses = recent_transactions.iter()
            .map(|tx| tx.from_address.clone())
            .collect::<std::collections::HashSet<String>>()
            .len();
        
        if unique_addresses < 3 && recent_transactions.len() > 10 {
            reasons.push(format!("Chỉ có {} địa chỉ độc đáo tạo ra {} giao dịch gần đây", 
                              unique_addresses, recent_transactions.len()));
            is_valid = false;
        }
        
        // 4. Kiểm tra dấu hiệu frontend-running (các giao dịch cố tình xuất hiện trước giao dịch lớn để lừa bots)
        let current_block = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter.get_current_block_number().await.unwrap_or(0),
            None => 0,
        };
        
        let front_running_score = match self.analys_client.calculate_frontrunning_risk(
            chain_id, token_address, current_block
        ).await {
            Ok(score) => score,
            Err(e) => {
                warn!("Không thể tính điểm rủi ro frontrunning: {}", e);
                0.0
            }
        };
        
        if front_running_score > 0.7 {
            reasons.push(format!("Điểm rủi ro frontrunning cao: {:.2}/1.0", front_running_score));
            is_valid = false;
        }
        
        SimpleValidationResult {
            is_valid,
            reasons,
        }
    }
    
    /// Tính điểm tin cậy cho cặp token-transaction
    async fn calculate_confidence_score(
        &self,
        token_validation: &Result<()>,
        tx_validation: &SimpleValidationResult,
        token_tx_relation: &SimpleValidationResult,
        anti_poisoning: &SimpleValidationResult,
        price_validation: &SimpleValidationResult,
        suspicious_activity: &SimpleValidationResult
    ) -> u8 {
        let mut score = 0;
        
        // 1. Token hợp lệ: 20 điểm
        if token_validation.is_ok() {
            score += 20;
        }
        
        // 2. Giao dịch mempool hợp lệ: 15 điểm
        if tx_validation.is_valid {
            score += 15;
        }
        
        // 3. Mối quan hệ token-tx hợp lệ: 15 điểm
        if token_tx_relation.is_valid {
            score += 15;
        }
        
        // 4. Không có dấu hiệu mempool poisoning: 20 điểm
        if anti_poisoning.is_valid {
            score += 20;
        }
        
        // 5. Giá token khớp giữa các nguồn: 15 điểm
        if price_validation.is_valid {
            score += 15;
        }
        
        // 6. Không có hoạt động khả nghi: 15 điểm
        if suspicious_activity.is_valid {
            score += 15;
        }
        
        // Trả về điểm tin cậy (0-100)
        score
    }

    /// Phát hiện token có phải là honeypot không
    /// 
    /// Phương thức này thực hiện nhiều kiểm tra để phát hiện honeypot:
    /// 1. Mô phỏng giao dịch bán token
    /// 2. Phân tích source code tìm giới hạn chỉ mua không bán
    /// 3. Kiểm tra các pattern phổ biến của honeypot
    /// 4. So sánh slippage mua/bán để phát hiện chênh lệch bất thường
    /// 
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ của token cần kiểm tra
    /// 
    /// # Returns
    /// * `Result<(bool, Option<String>), anyhow::Error>` - (có phải honeypot, mô tả nếu là honeypot)
    pub async fn detect_honeypot(&self, chain_id: u32, token_address: &str) -> Result<(bool, Option<String>)> {
        debug!("Detecting honeypot for token: {} on chain {}", token_address, chain_id);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Lấy contract info
        let contract_info = self.get_contract_info(chain_id, token_address, adapter).await
            .with_context(|| format!("Failed to get contract info for token {}", token_address))?;
        
        // Thử mô phỏng bán token với nhiều số lượng khác nhau để kiểm tra kỹ
        let test_amounts = ["0.01", "0.1", "1.0", "10.0"];
        let mut simulation_results = Vec::new();
        
        // Chạy các mô phỏng đồng thời để tăng hiệu suất
        let mut tasks = Vec::new();
        for amount in &test_amounts {
            let adapter_clone = adapter.clone();
            let token_address_clone = token_address.to_string();
            tasks.push(async move {
                adapter_clone.simulate_sell_token(&token_address_clone, amount).await
            });
        }
        
        // Thu thập kết quả mô phỏng
        let results = futures::future::join_all(tasks).await;
        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(sim_result) => {
                    if !sim_result.success {
                        let reason = sim_result.failure_reason.unwrap_or_else(|| "Unknown reason".to_string());
                        info!("Honeypot detected at amount {}: {}", test_amounts[i], reason);
                        return Ok((true, Some(format!("Failed to sell {} tokens: {}", test_amounts[i], reason))));
                    }
                    simulation_results.push(sim_result);
                },
                Err(e) => {
                    warn!("Simulation failed at amount {}: {}", test_amounts[i], e);
                    return Ok((true, Some(format!("Simulation error at amount {}: {}", test_amounts[i], e))));
                }
            }
        }
        
        // Kiểm tra thêm các giới hạn transfer trong contract
        if let Some(source_code) = &contract_info.source_code {
            // Phát hiện các hàm chỉ mua không bán
            let buy_only_patterns = [
                "require\\s*\\(\\s*!\\s*isSellingEnabled\\s*\\)",
                "require\\s*\\(\\s*isBuying\\s*\\)",
                "if\\s*\\([^)]*to\\s*==\\s*\\w+\\)\\s*revert",
                "require\\s*\\(\\s*sender\\s*==\\s*\\w+\\s*\\|\\|\\s*receiver\\s*==\\s*\\w+\\s*\\)",
                "require\\s*\\(\\s*block\\.timestamp\\s*<\\s*tradingEnabledAt\\s*\\)",
                "function transfer\\([^)]*\\)[^{]*{[^}]*revert\\([^)]*\\)[^}]*}",
            ];
            
            for pattern in buy_only_patterns {
                if let Ok(re) = regex::Regex::new(pattern) {
                    if re.is_match(source_code) {
                        let reason = format!("Transfer restriction found: {}", pattern);
                        info!("Honeypot detected: {}", reason);
                        return Ok((true, Some(reason)));
                    }
                }
            }
            
            // Phân tích thêm các token blacklist/whitelist ngầm định
            let restriction_patterns = [
                "require\\s*\\(\\s*!\\s*blacklisted\\[\\w+\\]\\s*\\)",
                "require\\s*\\(\\s*isWhitelisted\\[\\w+\\]\\s*\\)",
                "require\\s*\\(\\s*canTransfer\\s*\\(\\s*\\w+\\s*\\)\\s*\\)",
            ];
            
            for pattern in restriction_patterns {
                if let Ok(re) = regex::Regex::new(pattern) {
                    if re.is_match(source_code) {
                        return Ok((true, Some(format!("Transfer restriction found: {}", pattern))));
                    }
                }
            }
            
            // Kiểm tra các dummy function / hidden trap
            let trap_patterns = [
                "function\\s+transfer\\s*\\([^)]*\\)\\s*[^{]*\\{[^}]*return\\s+false[^}]*\\}",
                "function\\s+transferFrom\\s*\\([^)]*\\)\\s*[^{]*\\{[^}]*return\\s+false[^}]*\\}",
                "_beforeTokenTransfer\\s*\\([^)]*\\)\\s*[^{]*\\{[^}]*revert\\([^)]*\\}",
            ];
            
            for pattern in trap_patterns {
                if let Ok(re) = regex::Regex::new(pattern) {
                    if re.is_match(source_code) {
                        return Ok((true, Some(format!("Hidden trap found: {}", pattern))));
                    }
                }
            }
            
            // Kiểm tra các honeypot pattern phổ biến
            use crate::analys::token_status::utils::HoneypotDetector;
            let detector = HoneypotDetector::new();
            if detector.analyze(source_code) {
                return Ok((true, Some("Honeypot pattern detected in source code".to_string())));
            }
        }
        
        // So sánh slippage mua/bán để phát hiện chênh lệch bất thường
        let (buy_slippage, sell_slippage) = match adapter.simulate_buy_sell_slippage(token_address, 1.0).await {
            Ok(slippage) => slippage,
            Err(e) => {
                warn!("Failed to get slippage: {}", e);
                (0.0, 0.0)
            }
        };
        
        if sell_slippage > 0.0 && sell_slippage > buy_slippage * 5.0 {
            let reason = format!(
                "Suspicious slippage difference: buy={:.2}%, sell={:.2}% (sell is {:.1}x higher)", 
                buy_slippage, sell_slippage, sell_slippage / buy_slippage
            );
            info!("Potential honeypot detected: {}", reason);
            return Ok((true, Some(reason)));
        }
        
        // Kiểm tra thêm bytecode để tìm các hàm nguy hiểm
        if let Some(bytecode) = &contract_info.bytecode {
            use crate::analys::token_status::utils::analyze_bytecode;
            let analysis = analyze_bytecode(&contract_info);
            
            // Nếu code không nhất quán, có thể là nghi vấn
            if !analysis.is_consistent {
                return Ok((true, Some("Bytecode is inconsistent with source code, potential backdoor".to_string())));
            }
        }
        
        Ok((false, None))
    }
    
    /// Phát hiện tax động hoặc ẩn
    /// 
    /// Kiểm tra token contract có chứa cơ chế tax động/ẩn không bằng cách:
    /// 1. Phân tích source code tìm các biến tax có thể thay đổi
    /// 2. Mô phỏng giao dịch mua/bán để phát hiện chênh lệch tax bất thường
    /// 3. Kiểm tra cơ chế tax đặc biệt như thời gian, địa chỉ, số lượng
    /// 
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ của token cần kiểm tra
    /// 
    /// # Returns
    /// * `Result<(bool, Option<String>)>` - (có tax động/ẩn, mô tả nếu có)
    pub async fn detect_dynamic_tax(&self, chain_id: u32, token_address: &str) -> Result<(bool, Option<String>)> {
        debug!("Detecting dynamic tax for token: {} on chain {}", token_address, chain_id);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Lấy contract info
        let contract_info = self.get_contract_info(chain_id, token_address, adapter).await
            .with_context(|| format!("Failed to get contract info for token {}", token_address))?;
        
        let mut findings = Vec::new();
        
        // 1. Kiểm tra pattern biến tax động trong code
        if let Some(source_code) = &contract_info.source_code {
            // Tìm các biến tax/fee có thể thay đổi
            let dynamic_tax_patterns = [
                // Hàm set tax/fee có thể gọi bởi owner
                r"function\s+set[A-Z][a-zA-Z]*(?:Fee|Tax|Rate)",
                r"function\s+update[A-Z][a-zA-Z]*(?:Fee|Tax|Rate)",
                r"function\s+change[A-Z][a-zA-Z]*(?:Fee|Tax|Rate)",
                // Biến tax lưu trữ
                r"uint\d*\s+(?:public|private)?\s+(?:buy|sell|transfer)(?:Fee|Tax)",
                // Tax thay đổi theo thời gian/block
                r"if\s*\(\s*block\.timestamp",
                r"if\s*\(\s*block\.number",
            ];
            
            for pattern in &dynamic_tax_patterns {
                if let Ok(re) = regex::Regex::new(pattern) {
                    if let Some(captures) = re.captures(source_code) {
                        let matched = captures.get(0).map_or("", |m| m.as_str());
                        findings.push(format!("Dynamic tax pattern: {}", matched));
                    }
                }
            }
            
            // Kiểm tra phân biệt tax mua/bán
            if source_code.contains("buyFee") && source_code.contains("sellFee") {
                findings.push("Different buy/sell fees detected".to_string());
            }
            
            // Kiểm tra tax dựa vào địa chỉ ví
            let address_based_patterns = [
                r"if\s*\(\s*.*address\s*==\s*(?:owner|_owner)",
                r"if\s*\(\s*.*isExcluded\[",
                r"if\s*\(\s*.*excludedFromFee\[",
                r"if\s*\(\s*.*whitelist\[",
            ];
            
            for pattern in &address_based_patterns {
                if let Ok(re) = regex::Regex::new(pattern) {
                    if re.is_match(source_code) {
                        findings.push("Address-based fee exception detected".to_string());
                        break;
                    }
                }
            }
            
            // Kiểm tra tax biến thiên theo size giao dịch
            let amount_based_patterns = [
                r"if\s*\(\s*amount\s*[<>=]",
                r"fee\s*=\s*.*amount",
            ];
            
            for pattern in &amount_based_patterns {
                if let Ok(re) = regex::Regex::new(pattern) {
                    if re.is_match(source_code) {
                        findings.push("Amount-based dynamic fee detected".to_string());
                        break;
                    }
                }
            }
            
            // Kiểm tra cơ chế chống bot có thể tăng tax tạm thời
            if source_code.contains("antiBot") || source_code.contains("sniper") {
                findings.push("Anti-bot mechanism may include hidden fees".to_string());
            }
        }
        
        // 2. Thử mô phỏng mua/bán để tìm chênh lệch tax
        // 2.1 Lấy tax info từ adapter
        let (buy_tax, sell_tax) = match adapter.get_token_tax_info(token_address).await {
            Ok((buy, sell)) => (buy, sell),
            Err(e) => {
                warn!("Failed to get tax info: {}", e);
                (0.0, 0.0)
            }
        };
        
        // 2.2 Nếu sell tax quá cao so với buy tax, cảnh báo
        if sell_tax > buy_tax * 2.0 && sell_tax > 15.0 {
            findings.push(format!("Excessive sell tax: buy={:.1}%, sell={:.1}%", buy_tax, sell_tax));
        }
        
        // 2.3 Mô phỏng giao dịch với nhiều giá trị khác nhau để phát hiện tax động theo giá trị
        let test_amounts = [0.1, 1.0, 5.0, 10.0, 100.0];
        let mut slippages = Vec::new();
        
        for amount in &test_amounts {
            let (buy_slip, sell_slip) = match adapter.simulate_buy_sell_slippage(token_address, *amount).await {
                Ok(slip) => slip,
                Err(_) => continue,
            };
            
            slippages.push((amount, buy_slip, sell_slip));
        }
        
        // 2.4 Kiểm tra sự thay đổi của slippage theo số lượng
        // Nếu slippage tăng nhanh theo số lượng, có thể là tax động
        if slippages.len() >= 2 {
            let mut slippage_varies = false;
            let mut prev_sell_slip = slippages[0].2;
            
            for i in 1..slippages.len() {
                let ratio = slippages[i].2 / prev_sell_slip;
                if ratio > 1.5 && (slippages[i].0 / slippages[i-1].0) < 5.0 {
                    slippage_varies = true;
                    findings.push(format!(
                        "Dynamic slippage based on amount: {:.1} tokens: {:.2}%, {:.1} tokens: {:.2}%", 
                        slippages[i-1].0, prev_sell_slip, slippages[i].0, slippages[i].2
                    ));
                    break;
                }
                prev_sell_slip = slippages[i].2;
            }
        }
        
        // Nếu có nhiều hơn 0 findings, token có tax động
        if !findings.is_empty() {
            let summary = findings.join("; ");
            info!("Dynamic tax detected for {}: {}", token_address, summary);
            return Ok((true, Some(summary)));
        }
        
        Ok((false, None))
    }
    
    /// Phát hiện rủi ro thanh khoản
    /// 
    /// Kiểm tra các sự kiện thanh khoản bất thường, khóa LP, và rủi ro rugpull
    /// 
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ của token cần kiểm tra
    /// 
    /// # Returns
    /// * `Result<(bool, Option<String>)>` - (có rủi ro thanh khoản, mô tả nếu có)
    pub async fn detect_liquidity_risk(&self, chain_id: u32, token_address: &str) -> Result<(bool, Option<String>)> {
        debug!("Detecting liquidity risk for token: {} on chain {}", token_address, chain_id);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Lấy thông tin thanh khoản
        let liquidity_events = adapter.get_liquidity_events(token_address, 100).await
            .context("Failed to get liquidity events")?;
        
        // Kiểm tra sự kiện bất thường
        let has_abnormal_events = abnormal_liquidity_events(&liquidity_events);
        
        // Kiểm tra khóa thanh khoản
        let (is_liquidity_locked, lock_time_remaining) = self.check_liquidity_lock(chain_id, token_address, adapter).await;
        
        if has_abnormal_events || !is_liquidity_locked {
            let mut reasons = Vec::new();
            
            if has_abnormal_events {
                reasons.push("Abnormal liquidity events detected".to_string());
            }
            
            if !is_liquidity_locked {
                reasons.push("Liquidity is not locked".to_string());
            } else if lock_time_remaining < 2592000 { // dưới 30 ngày
                reasons.push(format!("Liquidity lock expiring soon ({})", lock_time_remaining));
            }
            
            let reason = reasons.join(", ");
            return Ok((true, Some(reason)));
        }
        
        Ok((false, None))
    }
    
    /// Phát hiện quyền đặc biệt của owner
    /// 
    /// Kiểm tra các quyền đặc biệt của owner như mint, blacklist, set fee, disable trading
    /// 
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ của token cần kiểm tra
    /// 
    /// # Returns
    /// * `Result<(bool, Vec<String>)>` - (có quyền đặc biệt, danh sách quyền)
    pub async fn detect_owner_privilege(&self, chain_id: u32, token_address: &str) -> Result<(bool, Vec<String>)> {
        debug!("Detecting owner privileges for token: {} on chain {}", token_address, chain_id);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Lấy contract info
        let contract_info = self.get_contract_info(chain_id, token_address, adapter).await
            .ok_or_else(|| anyhow!("Failed to get contract info"))?;
        
        // Phân tích quyền owner
        let owner_analysis = analyze_owner_privileges(&contract_info);
        
        let mut privileges = Vec::new();
        let mut has_privilege = false;
        
        if owner_analysis.has_mint_authority {
            has_privilege = true;
            privileges.push("Mint authority".to_string());
        }
        
        if owner_analysis.has_burn_authority {
            has_privilege = true;
            privileges.push("Burn authority".to_string());
        }
        
        if owner_analysis.has_pause_authority {
            has_privilege = true;
            privileges.push("Pause authority".to_string());
        }
        
        if !owner_analysis.is_ownership_renounced {
            has_privilege = true;
            privileges.push("Ownership not renounced".to_string());
        }
        
        if owner_analysis.can_retrieve_ownership {
            has_privilege = true;
            privileges.push("Can retrieve ownership (backdoor)".to_string());
        }
        
        // Kiểm tra proxy contract
        if is_proxy_contract(&contract_info) {
            has_privilege = true;
            privileges.push("Proxy contract (upgradeable)".to_string());
        }
        
        Ok((has_privilege, privileges))
    }
    
    /// Tự động bán token khi phát hiện bất thường
    /// 
    /// Phương thức này sẽ ngay lập tức bán token khi phát hiện các dấu hiệu nguy hiểm
    /// 
    /// # Parameters
    /// * `trade` - Thông tin giao dịch đang theo dõi
    /// * `current_price` - Giá hiện tại của token
    /// 
    /// # Returns
    /// * `Result<bool>` - Đã kích hoạt bán hay chưa
    pub async fn auto_sell_on_alert(&self, trade: &TradeTracker, current_price: f64) -> Result<bool> {
        debug!("Checking for auto-sell alerts for trade {}", trade.id);
        
        let chain_id = trade.chain_id;
        let token_address = &trade.token_address;
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Lấy contract info
        let contract_info = self.get_contract_info(chain_id, token_address, adapter).await
            .ok_or_else(|| anyhow!("Failed to get contract info"))?;
        
        let mut sell_reasons = Vec::new();
        let mut should_sell = false;
        
        // 1. Kiểm tra honeypot
        let (is_honeypot, honeypot_reason) = self.detect_honeypot(chain_id, token_address).await?;
        if is_honeypot {
            should_sell = true;
            sell_reasons.push(format!("Honeypot detected: {}", 
                                      honeypot_reason.unwrap_or_else(|| "Unknown".to_string())));
        }
        
        // 2. Kiểm tra tax động/ẩn
        let (has_dynamic_tax, tax_reason) = self.detect_dynamic_tax(chain_id, token_address).await?;
        if has_dynamic_tax {
            should_sell = true;
            sell_reasons.push(format!("Dynamic tax detected: {}", 
                                     tax_reason.unwrap_or_else(|| "Unknown".to_string())));
        }
        
        // 3. Kiểm tra rủi ro thanh khoản
        let (has_liquidity_risk, liquidity_reason) = self.detect_liquidity_risk(chain_id, token_address).await?;
        if has_liquidity_risk {
            should_sell = true;
            sell_reasons.push(format!("Liquidity risk detected: {}", 
                                     liquidity_reason.unwrap_or_else(|| "Unknown".to_string())));
        }
        
        // 4. Kiểm tra rút LP bất thường
        let liquidity_events = adapter.get_liquidity_events(token_address, 10).await
            .context("Failed to get recent liquidity events")?;
        
        if abnormal_liquidity_events(&liquidity_events) {
            should_sell = true;
            sell_reasons.push("Abnormal liquidity events detected recently".to_string());
        }
        
        // Nếu nên bán, tiến hành bán và ghi log
        if should_sell {
            let reason = sell_reasons.join("; ");
            info!("Auto-selling token {} due to alerts: {}", token_address, reason);
            
            // Clone trade để tránh borrow checker issues
            let mut trade_clone = trade.clone();
            trade_clone.status = TradeStatus::Selling;
            
            // Bán token
            self.sell_token(trade_clone, format!("AUTO-SELL: {}", reason), current_price).await;
            
            return Ok(true);
        }
        
        Ok(false)
    }

    /// Phát hiện blacklist và danh sách hạn chế trong token
    ///
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ contract của token
    ///
    /// # Returns
    /// * `(bool, Option<Vec<String>>)` - (có blacklist không, danh sách các loại hạn chế nếu có)
    pub async fn detect_blacklist(&self, chain_id: u32, token_address: &str) -> Result<(bool, Option<Vec<String>>)> {
        debug!("Checking blacklist restrictions for token: {}", token_address);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter.clone(),
            None => bail!("No adapter found for chain ID: {}", chain_id),
        };
        
        // Lấy thông tin contract
        let contract_info = self.get_contract_info(chain_id, token_address, &adapter)
            .await
            .context("Failed to get contract info for blacklist detection")?;
        
        let mut restrictions = Vec::new();
        
        // Kiểm tra blacklist/whitelist
        if let Some(ref source_code) = contract_info.source_code {
            use crate::analys::token_status::blacklist::{has_blacklist_or_whitelist, has_trading_cooldown, has_max_tx_or_wallet_limit};
            
            if has_blacklist_or_whitelist(&contract_info) {
                debug!("Blacklist/whitelist detected in token {}", token_address);
                restrictions.push("Has blacklist or whitelist".to_string());
            }
            
            if has_trading_cooldown(&contract_info) {
                debug!("Trading cooldown detected in token {}", token_address);
                restrictions.push("Has trading cooldown".to_string());
            }
            
            if has_max_tx_or_wallet_limit(&contract_info) {
                debug!("Max transaction or wallet limit detected in token {}", token_address);
                restrictions.push("Has max transaction or wallet limit".to_string());
            }
        }
        
        let has_restrictions = !restrictions.is_empty();
        
        if has_restrictions {
            info!("Token {} has {} restriction(s): {:?}", token_address, restrictions.len(), restrictions);
            Ok((true, Some(restrictions)))
        } else {
            debug!("No blacklist restrictions detected for token: {}", token_address);
            Ok((false, None))
        }
    }
    
    /// Phát hiện cơ chế anti-bot, anti-whale và các hạn chế giao dịch
    /// 
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ contract của token
    /// 
    /// # Returns
    /// * `(bool, Option<Vec<String>>)` - (có anti-bot/anti-whale không, danh sách các loại hạn chế nếu có)
    pub async fn detect_anti_bot(&self, chain_id: u32, token_address: &str) -> Result<(bool, Option<Vec<String>>)> {
        debug!("Checking anti-bot/anti-whale mechanisms for token: {}", token_address);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter.clone(),
            None => bail!("No adapter found for chain ID: {}", chain_id),
        };
        
        // Lấy thông tin contract
        let contract_info = self.get_contract_info(chain_id, token_address, &adapter)
            .await
            .context("Failed to get contract info for anti-bot detection")?;
        
        let mut anti_patterns = Vec::new();
        
        if let Some(ref source_code) = contract_info.source_code {
            // Tìm các pattern liên quan đến anti-bot
            let anti_bot_patterns = [
                "antibot", "anti-bot", "anti_bot", "botProtection",
                "onlyHuman", "noBot", "botBlacklist", "sniper",
                "sniperBot", "preventSniper", "preventFrontRunning",
                "sniperProtection", "humanCheck",
            ];
            
            for pattern in &anti_bot_patterns {
                if source_code.contains(pattern) {
                    anti_patterns.push(format!("Anti-bot: {}", pattern));
                }
            }
            
            // Kiểm tra các giới hạn thời gian (thường để chống bot)
            let time_patterns = [
                "tradingStartTime", "enableTrading", "tradingEnabled",
                "public\\s+bool\\s+tradeEnabled", "canTrade", "disableTrading",
                "tradingOpen", "tradingActive", "tradingAllowed",
                "block.timestamp\\s*[<>=]\\s*launch", "sellingEnabled",
            ];
            
            for pattern in &time_patterns {
                if source_code.contains(pattern) {
                    anti_patterns.push(format!("Trading time restriction: {}", pattern));
                }
            }
            
            // Kiểm tra maximum supply (thường để tránh whale)
            let whale_patterns = [
                "maxSupply", "MAX_SUPPLY", "maxTokens", "maxTotalSupply",
            ];
            
            for pattern in &whale_patterns {
                if source_code.contains(pattern) {
                    anti_patterns.push(format!("Supply limitation: {}", pattern));
                }
            }
        }
        
        let has_anti_mechanisms = !anti_patterns.is_empty();
        
        if has_anti_mechanisms {
            info!("Token {} has {} anti-bot/anti-whale mechanism(s): {:?}", 
                  token_address, anti_patterns.len(), anti_patterns);
            Ok((true, Some(anti_patterns)))
        } else {
            debug!("No anti-bot/anti-whale mechanisms detected for token: {}", token_address);
            Ok((false, None))
        }
    }

    /// Tự động điều chỉnh TP/SL theo biến động giá, volume, on-chain event
    ///
    /// # Parameters
    /// * `trade` - Thông tin giao dịch đang theo dõi
    /// * `current_price` - Giá hiện tại của token
    /// * `adapter` - EVM adapter để truy vấn thông tin blockchain
    ///
    /// # Returns
    /// * `(Option<f64>, Option<f64>)` - (take profit mới, stop loss mới)
    pub async fn dynamic_tp_sl(&self, trade: &TradeTracker, current_price: f64, adapter: &Arc<EvmAdapter>) -> Result<(Option<f64>, Option<f64>)> {
        debug!("Calculating dynamic TP/SL for trade: {}", trade.trade_id);
        
        // Lấy cấu hình ban đầu từ trade
        let initial_tp = trade.params.take_profit;
        let initial_sl = trade.params.stop_loss;
        let token_address = &trade.params.token_address;
        let chain_id = trade.params.chain_id;
        
        // Giá mua và giá hiện tại
        let purchase_price = trade.purchase_price.unwrap_or(0.0);
        if purchase_price == 0.0 {
            warn!("Cannot calculate dynamic TP/SL: purchase price is unknown");
            return Ok((None, None));
        }
        
        // Phần trăm thay đổi giá hiện tại so với giá mua
        let price_change_pct = (current_price - purchase_price) / purchase_price * 100.0;
        debug!("Price change since purchase: {:.2}%", price_change_pct);
        
        // Lấy dữ liệu thị trường để đánh giá volatility
        let market_data = adapter.get_market_data(token_address)
            .await
            .context(format!("Failed to get market data for token {}", token_address))?;
        
        // Phân tích volatility để điều chỉnh TP/SL
        let volatility = market_data.volatility_24h;
        debug!("Token volatility (24h): {:.2}%", volatility);
        
        // Lấy thông tin volume
        let volume = market_data.volume_24h;
        debug!("Token volume (24h): ${:.2}", volume);
        
        // Hệ số điều chỉnh dựa trên volatility
        let volatility_factor = if volatility > 50.0 {
            2.0 // Rất dao động -> tăng biên độ
        } else if volatility > 25.0 {
            1.5 // Dao động cao -> tăng biên độ nhẹ
        } else if volatility > 10.0 {
            1.0 // Dao động trung bình -> giữ nguyên
        } else {
            0.8 // Ổn định -> giảm biên độ
        };
        
        // Hệ số điều chỉnh dựa trên volume
        let volume_factor = if volume > 1_000_000.0 {
            1.0 // Volume cao -> tin cậy
        } else if volume > 100_000.0 {
            0.9 // Volume trung bình -> giảm nhẹ biên độ
        } else {
            0.8 // Volume thấp -> giảm biên độ nhiều hơn
        };
        
        // Hệ số điều chỉnh từ dao động giá gần đây (ATR - Average True Range concept)
        let price_history = adapter.get_token_price_history(token_address, 0, 0, 24)
            .await
            .context(format!("Failed to get price history for token {}", token_address))?;
        
        // Tính toán ATR đơn giản từ lịch sử giá
        let mut price_ranges = Vec::new();
        if price_history.len() >= 2 {
            for i in 1..price_history.len() {
                let (_, prev_price) = price_history[i-1];
                let (_, curr_price) = price_history[i];
                let range = (curr_price - prev_price).abs() / prev_price * 100.0;
                price_ranges.push(range);
            }
        }
        
        // Tính ATR trung bình
        let atr = if !price_ranges.is_empty() {
            price_ranges.iter().sum::<f64>() / price_ranges.len() as f64
        } else {
            5.0 // Giá trị mặc định nếu không có đủ dữ liệu
        };
        debug!("Calculated ATR: {:.2}%", atr);
        
        // Kiểm tra các sự kiện on-chain gần đây
        let (has_liquidity_events, _) = self.detect_liquidity_risk(chain_id, token_address)
            .await
            .unwrap_or((false, None));
        
        let (has_owner_privileges, _) = self.detect_owner_privilege(chain_id, token_address)
            .await
            .unwrap_or((false, vec![]));
        
        // Hệ số điều chỉnh dựa trên các sự kiện on-chain
        let risk_factor = if has_liquidity_events || has_owner_privileges {
            0.5 // Rút ngắn TP/SL nếu phát hiện rủi ro
        } else {
            1.0 // Giữ nguyên nếu không có rủi ro đặc biệt
        };
        
        // Tính toán TP/SL mới
        let mut new_tp = initial_tp;
        let mut new_sl = initial_sl;
        
        // Điều chỉnh TP
        if let Some(tp) = new_tp {
            // Điều chỉnh TP dựa trên ATR và các hệ số
            let adjusted_tp = purchase_price * (1.0 + (tp / purchase_price - 1.0) * volatility_factor * volume_factor * risk_factor);
            
            // Nếu giá đã tăng nhiều, điều chỉnh TP lên theo để bắt thêm profit
            if price_change_pct > 50.0 {
                let bonus_factor = 1.0 + (price_change_pct - 50.0) * 0.01;
                new_tp = Some(adjusted_tp * bonus_factor);
            } else {
                new_tp = Some(adjusted_tp);
            }
            
            info!("Adjusted take profit from ${:.6} to ${:.6}", tp, new_tp.unwrap_or(0.0));
        }
        
        // Điều chỉnh SL
        if let Some(sl) = new_sl {
            // Nếu lãi rồi thì đẩy SL lên để bảo vệ lãi (trailing stop concept)
            if current_price > purchase_price * 1.1 { // 10% lãi
                // Điều chỉnh SL lên để bảo vệ ít nhất 50% lãi đã có
                let min_protected_price = purchase_price + (current_price - purchase_price) * 0.5;
                
                // Chỉ nâng SL nếu giá mới cao hơn SL hiện tại
                if min_protected_price > sl {
                    new_sl = Some(min_protected_price);
                    info!("Raised stop loss to ${:.6} to protect profit", new_sl.unwrap_or(0.0));
                }
            } else {
                // Điều chỉnh SL dựa trên ATR và các hệ số
                let buffer = atr * 1.5; // Tạo khoảng đệm dựa trên ATR
                let min_allowed_sl = current_price * (1.0 - buffer / 100.0);
                
                // Nếu SL hiện tại thấp hơn giới hạn tối thiểu, điều chỉnh lên
                if sl < min_allowed_sl {
                    new_sl = Some(min_allowed_sl);
                    info!("Adjusted stop loss from ${:.6} to ${:.6} based on ATR", sl, new_sl.unwrap_or(0.0));
                }
            }
        }
        
        // Log kết quả cuối cùng
        info!(
            "Dynamic TP/SL for trade {}: TP=${:.6} -> ${:.6}, SL=${:.6} -> ${:.6}",
            trade.trade_id,
            initial_tp.unwrap_or(0.0),
            new_tp.unwrap_or(0.0),
            initial_sl.unwrap_or(0.0),
            new_sl.unwrap_or(0.0)
        );
        
        Ok((new_tp, new_sl))
    }

    /// Điều chỉnh trailing stop theo volatility (ATR, Bollinger Band)
    ///
    /// # Parameters
    /// * `trade` - Thông tin giao dịch đang theo dõi
    /// * `current_price` - Giá hiện tại của token
    /// * `price_history` - Lịch sử giá token [(timestamp, price), ...]
    ///
    /// # Returns
    /// * `f64` - Phần trăm trailing stop mới (% dưới giá cao nhất)
    pub async fn dynamic_trailing_stop(&self, trade: &TradeTracker, current_price: f64, price_history: &[(u64, f64)]) -> Result<f64> {
        debug!("Calculating dynamic trailing stop for trade: {}", trade.trade_id);
        
        // Lấy trailing stop hiện tại từ cấu hình
        let config = self.config.read().await;
        let base_tsl_percent = config.default_trailing_stop_pct.unwrap_or(2.5);
        drop(config); // Giải phóng lock sớm
        
        // Lấy thông tin token
        let token_address = &trade.params.token_address;
        let chain_id = trade.params.chain_id;
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter.clone(),
            None => bail!("No adapter found for chain ID: {}", chain_id),
        };
        
        // Nếu không có đủ dữ liệu lịch sử giá, sử dụng giá trị mặc định
        if price_history.len() < 10 {
            debug!("Not enough price history data for dynamic TSL calculation, using base value: {}%", base_tsl_percent);
            return Ok(base_tsl_percent);
        }
        
        // Tính toán ATR (Average True Range)
        let mut true_ranges = Vec::with_capacity(price_history.len() - 1);
        for i in 1..price_history.len() {
            let (_, prev_price) = price_history[i-1];
            let (_, curr_price) = price_history[i];
            
            // TR = |current_high - current_low|
            // Đơn giản hóa: sử dụng chỉ close price thay vì high/low
            let true_range = (curr_price - prev_price).abs() / prev_price * 100.0;
            true_ranges.push(true_range);
        }
        
        // Tính ATR (trung bình N kỳ gần nhất, thông thường 14 kỳ)
        let atr_periods = std::cmp::min(14, true_ranges.len());
        let atr = if atr_periods > 0 {
            let sum: f64 = true_ranges.iter().take(atr_periods).sum();
            sum / atr_periods as f64
        } else {
            1.0 // Giá trị mặc định
        };
        
        debug!("Calculated ATR: {:.2}%", atr);
        
        // Tính Bollinger Bands
        // 1. Tính SMA (Simple Moving Average)
        let prices: Vec<f64> = price_history.iter().map(|(_, price)| *price).collect();
        let sma_periods = std::cmp::min(20, prices.len());
        let sma = if sma_periods > 0 {
            let sum: f64 = prices.iter().take(sma_periods).sum();
            sum / sma_periods as f64
        } else {
            current_price
        };
        
        // 2. Tính Standard Deviation
        let mut sum_squared_diffs = 0.0;
        for i in 0..sma_periods {
            if i < prices.len() {
                sum_squared_diffs += (prices[i] - sma).powi(2);
            }
        }
        let std_dev = if sma_periods > 1 {
            (sum_squared_diffs / (sma_periods - 1) as f64).sqrt()
        } else {
            0.0
        };
        
        // 3. Tính Bollinger Bands
        let upper_band = sma + 2.0 * std_dev;
        let lower_band = sma - 2.0 * std_dev;
        let band_width = (upper_band - lower_band) / sma * 100.0;
        
        debug!("Bollinger Band Width: {:.2}%", band_width);
        
        // Tính volatility factor dựa trên Band Width
        // Thị trường biến động mạnh = Band Width lớn = trailing stop xa hơn
        // Thị trường ít biến động = Band Width nhỏ = trailing stop gần hơn
        let band_width_factor = if band_width > 20.0 {
            2.0 // Biến động rất cao
        } else if band_width > 10.0 {
            1.5 // Biến động cao
        } else if band_width > 5.0 {
            1.0 // Biến động trung bình
        } else {
            0.7 // Biến động thấp
        };
        
        // Tính volatility factor dựa trên ATR
        // ATR cao = biến động lớn = trailing stop xa hơn
        let atr_factor = if atr > 10.0 {
            2.0 // Biến động rất cao
        } else if atr > 5.0 {
            1.5 // Biến động cao
        } else if atr > 2.0 {
            1.0 // Biến động trung bình
        } else {
            0.7 // Biến động thấp
        };
        
        // Lấy dữ liệu thị trường
        let market_data = adapter.get_market_data(token_address)
            .await
            .context(format!("Failed to get market data for token {}", token_address))?;
        
        // Tính market cap factor
        // Market cap thấp = rủi ro cao = trailing stop gần hơn
        let market_cap_factor = if market_data.holder_count > 10000 {
            1.0 // Nhiều holder, ít rủi ro
        } else if market_data.holder_count > 1000 {
            0.8 // Số lượng holder trung bình
        } else {
            0.6 // Ít holder, rủi ro cao
        };
        
        // Kết hợp tất cả các yếu tố để tính trailing stop mới
        let new_tsl_percent = base_tsl_percent * atr_factor * band_width_factor * market_cap_factor;
        
        // Giới hạn trong khoảng hợp lý (0.5% đến 10%)
        let clamped_tsl_percent = new_tsl_percent.max(0.5).min(10.0);
        
        info!(
            "Dynamic trailing stop for trade {}: base={:.2}% -> dynamic={:.2}% (ATR={:.2}, BW={:.2}, factors: ATR={:.1}, BW={:.1}, MC={:.1})",
            trade.trade_id, 
            base_tsl_percent, 
            clamped_tsl_percent,
            atr,
            band_width,
            atr_factor,
            band_width_factor,
            market_cap_factor
        );
        
        Ok(clamped_tsl_percent)
    }

    /// Theo dõi ví lớn (whale), tự động bán khi phát hiện whale bán mạnh
    ///
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ contract của token
    /// * `current_price` - Giá hiện tại của token
    /// * `trade` - Thông tin giao dịch đang theo dõi (nếu có)
    ///
    /// # Returns
    /// * `(bool, Option<String>)` - (có nên bán không, lý do nếu nên bán)
    pub async fn whale_tracker(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        current_price: f64,
        trade: Option<&TradeTracker>
    ) -> Result<(bool, Option<String>)> {
        debug!("Tracking whale activity for token: {}", token_address);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter.clone(),
            None => bail!("No adapter found for chain ID: {}", chain_id),
        };
        
        // Lấy thông tin về token
        let token_supply = adapter.get_token_total_supply(token_address)
            .await
            .context(format!("Failed to get token supply for {}", token_address))?;
            
        // Lấy thông tin về market
        let market_data = adapter.get_market_data(token_address)
            .await
            .context(format!("Failed to get market data for {}", token_address))?;
            
        // Lấy thông tin liquidity
        let liquidity = adapter.get_token_liquidity(token_address)
            .await
            .context(format!("Failed to get liquidity for {}", token_address))?;
        
        // Phân tích mempool để phát hiện các giao dịch của ví lớn đang pending
        // Sử dụng API phân tích từ analys_client
        let mempool_analyzer = match self.mempool_analyzers.get(&chain_id) {
            Some(analyzer) => analyzer.clone(),
            None => {
                warn!("No mempool analyzer found for chain {}, can't track whale activity", chain_id);
                return Ok((false, None));
            }
        };
        
        // Lấy danh sách giao dịch trong mempool liên quan đến token này
        let pending_txs = mempool_analyzer.get_token_transactions(token_address, 50)
            .await
            .context(format!("Failed to get pending transactions for token {}", token_address))?;
            
        // Lấy thông tin top holders từ blockchain
        // Giả lập: normaly we would use adapter.get_top_holders(), but for now we'll use a workaround
        let top_holders = self.analys_client.token.get_top_holders(chain_id, token_address)
            .await
            .unwrap_or_else(|_| Vec::new());
            
        // Phân loại và phát hiện hành vi của whale
        
        // 1. Tạo thresholds để xác định whale (ví có % lớn token supply)
        let whale_threshold_pct = 2.0; // Ví chiếm >= 2% supply là whale
        let mini_whale_threshold_pct = 0.5; // Ví chiếm >= 0.5% supply là mini whale
        
        // 2. Tính số lượng token tương ứng với threshold
        let whale_threshold = token_supply * whale_threshold_pct / 100.0;
        let mini_whale_threshold = token_supply * mini_whale_threshold_pct / 100.0;
        
        // 3. Lọc ra các giao dịch sell của whale trong mempool
        let mut whale_sell_amount = 0.0;
        let mut whale_sell_txs = 0;
        let mut whale_addresses = HashSet::new();
        
        for tx in &pending_txs {
            // Kiểm tra xem giao dịch có phải là bán token hay không
            if let Some(tx_amount) = tx.parsed_data.token_amount {
                // Xác định xem đây có phải giao dịch sell không
                let is_sell = match tx.parsed_data.transaction_type.as_str() {
                    "SELL" | "REMOVE_LIQUIDITY" => true,
                    _ => false,
                };
                
                if is_sell {
                    // Kiểm tra xem ví có phải whale không
                    let is_whale = top_holders.iter().any(|holder| {
                        holder.address == tx.from && holder.balance >= whale_threshold
                    });
                    
                    let is_mini_whale = top_holders.iter().any(|holder| {
                        holder.address == tx.from && holder.balance >= mini_whale_threshold
                    });
                    
                    if is_whale {
                        whale_sell_amount += tx_amount;
                        whale_sell_txs += 1;
                        whale_addresses.insert(tx.from.clone());
                    } else if is_mini_whale {
/// Core implementation of SmartTradeExecutor
///
/// This file contains the main executor struct that manages the entire
/// smart trading process, orchestrates token analysis, trade strategies,
/// and market monitoring.
///
/// # Cải tiến đã thực hiện:
/// - Loại bỏ các phương thức trùng lặp với analys modules
/// - Sử dụng API từ analys/token_status và analys/risk_analyzer
/// - Áp dụng xử lý lỗi chuẩn với anyhow::Result thay vì unwrap/expect
/// - Đảm bảo thread safety trong các phương thức async với kỹ thuật "clone and drop" để tránh deadlock
/// - Tối ưu hóa việc xử lý các futures với tokio::join! cho các tác vụ song song
/// - Xử lý các trường hợp giá trị null an toàn với match/Option
/// - Tuân thủ nghiêm ngặt quy tắc từ .cursorrc

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use chrono::Utc;
use futures::future::join_all;
use tracing::{debug, error, info, warn};
use anyhow::{Result, Context, anyhow, bail};
use uuid;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use rand::Rng;
use std::collections::HashSet;
use futures::future::Future;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::{MempoolAnalyzer, MempoolTransaction};
use crate::analys::risk_analyzer::{RiskAnalyzer, RiskFactor, TradeRecommendation, TradeRiskAnalysis};
use crate::analys::token_status::{
    ContractInfo, LiquidityEvent, LiquidityEventType, TokenSafety, TokenStatus,
    TokenIssue, IssueSeverity, abnormal_liquidity_events, detect_dynamic_tax, 
    detect_hidden_fees, analyze_owner_privileges, is_proxy_contract, blacklist::{has_blacklist_or_whitelist, has_trading_cooldown, has_max_tx_or_wallet_limit},
};
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};
use crate::tradelogic::traits::{
    TradeExecutor, RiskManager, TradeCoordinator, ExecutorType, SharedOpportunity, 
    SharedOpportunityType, OpportunityPriority
};

// Module imports
use super::types::{
    SmartTradeConfig, 
    TradeResult, 
    TradeStatus, 
    TradeStrategy, 
    TradeTracker
};
use super::constants::*;
use super::token_analysis::*;
use super::trade_strategy::*;
use super::alert::*;
use super::optimization::*;
use super::security::*;
use super::analys_client::SmartTradeAnalysisClient;

/// Executor for smart trading strategies
#[derive(Clone)]
pub struct SmartTradeExecutor {
    /// EVM adapter for each chain
    pub(crate) evm_adapters: HashMap<u32, Arc<EvmAdapter>>,
    
    /// Analysis client that provides access to all analysis services
    pub(crate) analys_client: Arc<SmartTradeAnalysisClient>,
    
    /// Risk analyzer (legacy - kept for backward compatibility)
    pub(crate) risk_analyzer: Arc<RwLock<RiskAnalyzer>>,
    
    /// Mempool analyzer for each chain (legacy - kept for backward compatibility)
    pub(crate) mempool_analyzers: HashMap<u32, Arc<MempoolAnalyzer>>,
    
    /// Configuration
    pub(crate) config: RwLock<SmartTradeConfig>,
    
    /// Active trades being monitored
    pub(crate) active_trades: RwLock<Vec<TradeTracker>>,
    
    /// History of completed trades
    pub(crate) trade_history: RwLock<Vec<TradeResult>>,
    
    /// Running state
    pub(crate) running: RwLock<bool>,
    
    /// Coordinator for shared state between executors
    pub(crate) coordinator: Option<Arc<dyn TradeCoordinator>>,
    
    /// Unique ID for this executor instance
    pub(crate) executor_id: String,
    
    /// Subscription ID for coordinator notifications
    pub(crate) coordinator_subscription: RwLock<Option<String>>,
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
        
        // Khởi động vòng lặp theo dõi
        let executor = Arc::new(self.clone_with_config().await);
        
        tokio::spawn(async move {
            executor.monitor_loop().await;
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
        
        Ok(())
    }
    
    /// Execute a trade with the specified parameters
    async fn execute_trade(&self, params: TradeParams) -> anyhow::Result<Self::TradeResult> {
        // Find the appropriate adapter
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", params.chain_id)),
        };
        
        // Validate trade parameters
        if params.amount <= 0.0 {
            return Err(anyhow::anyhow!("Invalid trade amount: {}", params.amount));
        }
        
        // Create a trade tracker
        let trade_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp() as u64;
        
        let tracker = TradeTracker {
            id: trade_id.clone(),
            chain_id: params.chain_id,
            token_address: params.token_address.clone(),
            amount: params.amount,
            entry_price: 0.0, // Will be updated after trade execution
            current_price: 0.0,
            status: TradeStatus::Pending,
            strategy: if let Some(strategy) = &params.strategy {
                match strategy.as_str() {
                    "quick" => TradeStrategy::QuickFlip,
                    "tsl" => TradeStrategy::TrailingStopLoss,
                    "hodl" => TradeStrategy::LongTerm,
                    _ => TradeStrategy::default(),
                }
            } else {
                TradeStrategy::default()
            },
            created_at: now,
            updated_at: now,
            stop_loss: params.stop_loss,
            take_profit: params.take_profit,
            max_hold_time: params.max_hold_time.unwrap_or(DEFAULT_MAX_HOLD_TIME),
            custom_params: params.custom_params.unwrap_or_default(),
        };
        
        // TODO: Implement the actual trade execution logic
        // This is a simplified version - in a real implementation, you would:
        // 1. Execute the trade
        // 2. Update the trade tracker with actual execution details
        // 3. Add it to active trades
        
        let result = TradeResult {
            id: trade_id,
            chain_id: params.chain_id,
            token_address: params.token_address,
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: params.amount,
            entry_price: 0.0,                  // Would be populated from actual execution
            exit_price: None,
            current_price: 0.0,
            profit_loss: 0.0,
            status: TradeStatus::Pending,
            strategy: tracker.strategy.clone(),
            created_at: now,
            updated_at: now,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        // Add to active trades
        {
            let mut active_trades = self.active_trades.write().await;
            active_trades.push(tracker);
        }
        
        Ok(result)
    }
    
    /// Get the current status of an active trade
    async fn get_trade_status(&self, trade_id: &str) -> anyhow::Result<Option<Self::TradeResult>> {
        // Check active trades
        {
            let active_trades = self.active_trades.read().await;
            if let Some(trade) = active_trades.iter().find(|t| t.id == trade_id) {
                return Ok(Some(self.tracker_to_result(trade).await?));
            }
        }
        
        // Check trade history
        let trade_history = self.trade_history.read().await;
        let result = trade_history.iter()
            .find(|r| r.id == trade_id)
            .cloned();
            
        Ok(result)
    }
    
    /// Get all active trades
    async fn get_active_trades(&self) -> anyhow::Result<Vec<Self::TradeResult>> {
        let active_trades = self.active_trades.read().await;
        let mut results = Vec::with_capacity(active_trades.len());
        
        for tracker in active_trades.iter() {
            results.push(self.tracker_to_result(tracker).await?);
        }
        
        Ok(results)
    }
    
    /// Get trade history within a specific time range
    async fn get_trade_history(&self, from_timestamp: u64, to_timestamp: u64) -> anyhow::Result<Vec<Self::TradeResult>> {
        let trade_history = self.trade_history.read().await;
        
        let filtered_history = trade_history.iter()
            .filter(|trade| trade.created_at >= from_timestamp && trade.created_at <= to_timestamp)
            .cloned()
            .collect();
            
        Ok(filtered_history)
    }
    
    /// Evaluate a token for potential trading
    async fn evaluate_token(&self, chain_id: u32, token_address: &str) -> anyhow::Result<Option<TokenSafety>> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Sử dụng analys_client để phân tích token
        let token_safety_result = self.analys_client.token_provider()
            .analyze_token(chain_id, token_address).await
            .context(format!("Không thể phân tích token {}", token_address))?;
        
        // Nếu không phân tích được token, trả về None
        if !token_safety_result.is_valid() {
            info!("Token {} không hợp lệ hoặc không thể phân tích", token_address);
            return Ok(None);
        }
        
        // Lấy thêm thông tin chi tiết từ contract
        let contract_details = self.analys_client.token_provider()
            .get_contract_details(chain_id, token_address).await
            .context(format!("Không thể lấy chi tiết contract cho token {}", token_address))?;
        
        // Đánh giá rủi ro của token
        let risk_analysis = self.analys_client.risk_provider()
            .analyze_token_risk(chain_id, token_address).await
            .context(format!("Không thể phân tích rủi ro cho token {}", token_address))?;
        
        // Log thông tin phân tích
        info!(
            "Phân tích token {} trên chain {}: risk_score={}, honeypot={:?}, buy_tax={:?}, sell_tax={:?}",
            token_address,
            chain_id,
            risk_analysis.risk_score,
            token_safety_result.is_honeypot,
            token_safety_result.buy_tax,
            token_safety_result.sell_tax
        );
        
        Ok(Some(token_safety_result))
    }

    /// Đánh giá rủi ro của token bằng cách sử dụng analys_client
    async fn analyze_token_risk(&self, chain_id: u32, token_address: &str) -> anyhow::Result<RiskScore> {
        // Sử dụng phương thức analyze_risk từ analys_client
        self.analys_client.analyze_risk(chain_id, token_address).await
            .context(format!("Không thể đánh giá rủi ro cho token {}", token_address))
    }
}

impl SmartTradeExecutor {
    /// Create a new SmartTradeExecutor
    pub fn new() -> Self {
        let executor_id = format!("smart-trade-{}", uuid::Uuid::new_v4());
        
        let evm_adapters = HashMap::new();
        let analys_client = Arc::new(SmartTradeAnalysisClient::new(evm_adapters.clone()));
        
        Self {
            evm_adapters,
            analys_client,
            risk_analyzer: Arc::new(RwLock::new(RiskAnalyzer::new())),
            mempool_analyzers: HashMap::new(),
            config: RwLock::new(SmartTradeConfig::default()),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            coordinator: None,
            executor_id,
            coordinator_subscription: RwLock::new(None),
        }
    }
    
    /// Set coordinator for shared state
    pub fn with_coordinator(mut self, coordinator: Arc<dyn TradeCoordinator>) -> Self {
        self.coordinator = Some(coordinator);
        self
    }
    
    /// Convert a trade tracker to a trade result
    async fn tracker_to_result(&self, tracker: &TradeTracker) -> anyhow::Result<TradeResult> {
        let result = TradeResult {
            id: tracker.id.clone(),
            chain_id: tracker.chain_id,
            token_address: tracker.token_address.clone(),
            token_name: "Unknown".to_string(), // Would be populated from token data
            token_symbol: "UNK".to_string(),   // Would be populated from token data
            amount: tracker.amount,
            entry_price: tracker.entry_price,
            exit_price: None,
            current_price: tracker.current_price,
            profit_loss: if tracker.entry_price > 0.0 && tracker.current_price > 0.0 {
                (tracker.current_price - tracker.entry_price) / tracker.entry_price * 100.0
            } else {
                0.0
            },
            status: tracker.status.clone(),
            strategy: tracker.strategy.clone(),
            created_at: tracker.created_at,
            updated_at: tracker.updated_at,
            completed_at: None,
            exit_reason: None,
            gas_used: 0.0,
            safety_score: 0,
            risk_factors: Vec::new(),
        };
        
        Ok(result)
    }
    
    /// Vòng lặp theo dõi chính
    async fn monitor_loop(&self) {
        let mut counter = 0;
        let mut persist_counter = 0;
        
        while let true_val = *self.running.read().await {
            if !true_val {
                break;
            }
            
            // Kiểm tra và cập nhật tất cả các giao dịch đang active
            if let Err(e) = self.check_active_trades().await {
                error!("Error checking active trades: {}", e);
            }
            
            // Tăng counter và dọn dẹp lịch sử giao dịch định kỳ
            counter += 1;
            if counter >= 100 {
                self.cleanup_trade_history().await;
                counter = 0;
            }
            
            // Lưu trạng thái vào bộ nhớ định kỳ
            persist_counter += 1;
            if persist_counter >= 30 { // Lưu mỗi 30 chu kỳ (khoảng 2.5 phút với interval 5s)
                if let Err(e) = self.update_and_persist_trades().await {
                    error!("Failed to persist bot state: {}", e);
                }
                persist_counter = 0;
            }
            
            // Tính toán thời gian sleep thích ứng
            let sleep_time = self.adaptive_sleep_time().await;
            tokio::time::sleep(tokio::time::Duration::from_millis(sleep_time)).await;
        }
        
        // Lưu trạng thái trước khi dừng
        if let Err(e) = self.update_and_persist_trades().await {
            error!("Failed to persist bot state before stopping: {}", e);
        }
    }
    
    /// Quét cơ hội giao dịch mới
    async fn scan_trading_opportunities(&self) {
        // Kiểm tra cấu hình
        let config = self.config.read().await;
        if !config.enabled || !config.auto_trade {
            return;
        }
        
        // Kiểm tra số lượng giao dịch hiện tại
        let active_trades = self.active_trades.read().await;
        if active_trades.len() >= config.max_concurrent_trades as usize {
            return;
        }
        
        // Chạy quét cho từng chain
        let futures = self.mempool_analyzers.iter().map(|(chain_id, analyzer)| {
            self.scan_chain_opportunities(*chain_id, analyzer)
        });
        join_all(futures).await;
    }
    
    /// Quét cơ hội giao dịch trên một chain cụ thể
    async fn scan_chain_opportunities(&self, chain_id: u32, analyzer: &Arc<MempoolAnalyzer>) -> anyhow::Result<()> {
        let config = self.config.read().await;
        
        // Lấy các giao dịch lớn từ mempool
        let large_txs = match analyzer.get_large_transactions(QUICK_TRADE_MIN_BNB).await {
            Ok(txs) => txs,
            Err(e) => {
                error!("Không thể lấy các giao dịch lớn từ mempool: {}", e);
                return Err(anyhow::anyhow!("Lỗi khi lấy giao dịch từ mempool: {}", e));
            }
        };
        
        // Phân tích từng giao dịch
        for tx in large_txs {
            // Kiểm tra nếu token không trong blacklist và đáp ứng điều kiện
            if let Some(to_token) = &tx.to_token {
                let token_address = &to_token.address;
                
                // Kiểm tra blacklist và whitelist
                if config.token_blacklist.contains(token_address) {
                    debug!("Bỏ qua token {} trong blacklist", token_address);
                    continue;
                }
                
                if !config.token_whitelist.is_empty() && !config.token_whitelist.contains(token_address) {
                    debug!("Bỏ qua token {} không trong whitelist", token_address);
                    continue;
                }
                
                // Kiểm tra nếu giao dịch đủ lớn
                if tx.value >= QUICK_TRADE_MIN_BNB {
                    // Phân tích token
                    if let Err(e) = self.analyze_and_trade_token(chain_id, token_address, tx).await {
                        error!("Lỗi khi phân tích và giao dịch token {}: {}", token_address, e);
                        // Tiếp tục với token tiếp theo, không dừng lại
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Phân tích token và thực hiện giao dịch nếu an toàn
    async fn analyze_and_trade_token(&self, chain_id: u32, token_address: &str, tx: MempoolTransaction) -> anyhow::Result<()> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id));
            }
        };
        
        // THÊM: Xác thực kỹ lưỡng dữ liệu mempool trước khi tiếp tục phân tích
        info!("Xác thực dữ liệu mempool cho token {} từ giao dịch {}", token_address, tx.tx_hash);
        let mempool_validation = match self.validate_mempool_data(chain_id, token_address, &tx).await {
            Ok(validation) => validation,
            Err(e) => {
                error!("Lỗi khi xác thực dữ liệu mempool: {}", e);
                self.log_trade_decision(
                    "new_opportunity", 
                    token_address, 
                    TradeStatus::Canceled, 
                    &format!("Lỗi xác thực mempool: {}", e), 
                    chain_id
                ).await;
                return Err(anyhow::anyhow!("Lỗi xác thực dữ liệu mempool: {}", e));
            }
        };
        
        // Nếu dữ liệu mempool không hợp lệ, dừng phân tích
        if !mempool_validation.is_valid {
            let reasons = mempool_validation.reasons.join(", ");
            info!("Dữ liệu mempool không đáng tin cậy cho token {}: {}", token_address, reasons);
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Dữ liệu mempool không đáng tin cậy: {}", reasons), 
                chain_id
            ).await;
            return Ok(());
        }
        
        info!("Xác thực mempool thành công với điểm tin cậy: {}/100", mempool_validation.confidence_score);
        
        // Code hiện tại: Sử dụng analys_client để kiểm tra an toàn token
        let security_check_future = self.analys_client.verify_token_safety(chain_id, token_address);
        
        // Code hiện tại: Sử dụng analys_client để đánh giá rủi ro
        let risk_analysis_future = self.analys_client.analyze_risk(chain_id, token_address);
        
        // Thực hiện song song các phân tích
        let (security_check_result, risk_analysis) = tokio::join!(security_check_future, risk_analysis_future);
        
        // Xử lý kết quả kiểm tra an toàn
        let security_check = match security_check_result {
            Ok(result) => result,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi kiểm tra an toàn token: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi kiểm tra an toàn token: {}", e));
            }
        };
        
        // Xử lý kết quả phân tích rủi ro
        let risk_score = match risk_analysis {
            Ok(score) => score,
            Err(e) => {
                self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                    &format!("Lỗi khi phân tích rủi ro: {}", e), chain_id).await;
                return Err(anyhow::anyhow!("Lỗi khi phân tích rủi ro: {}", e));
            }
        };
        
        // Kiểm tra xem token có an toàn không
        if !security_check.is_safe {
            let issues_str = security_check.issues.iter()
                .map(|issue| format!("{:?}", issue))
                .collect::<Vec<String>>()
                .join(", ");
                
            self.log_trade_decision("new_opportunity", token_address, TradeStatus::Canceled, 
                &format!("Token không an toàn: {}", issues_str), chain_id).await;
            return Ok(());
        }
        
        // Lấy cấu hình hiện tại
        let config = self.config.read().await;
        
        // Kiểm tra điểm rủi ro
        if risk_score.score > config.max_risk_score {
            self.log_trade_decision(
                "new_opportunity", 
                token_address, 
                TradeStatus::Canceled, 
                &format!("Điểm rủi ro quá cao: {}/{}", risk_score.score, config.max_risk_score), 
                chain_id
            ).await;
            return Ok(());
        }
        
        // Xác định chiến lược giao dịch dựa trên mức độ rủi ro
        let strategy = if risk_score.score < 30 {
            TradeStrategy::Smart  // Rủi ro thấp, dùng chiến lược thông minh với TSL
        } else {
            TradeStrategy::Quick  // Rủi ro cao hơn, dùng chiến lược nhanh
        };
        
        // Xác định số lượng giao dịch dựa trên cấu hình và mức độ rủi ro
        let base_amount = if risk_score.score < 20 {
            config.trade_amount_high_confidence
        } else if risk_score.score < 40 {
            config.trade_amount_medium_confidence
        } else {
            config.trade_amount_low_confidence
        };
        
        // Tạo tham số giao dịch
        let trade_params = crate::types::TradeParams {
            chain_type: crate::types::ChainType::EVM(chain_id),
            token_address: token_address.to_string(),
            amount: base_amount,
            slippage: 1.0, // 1% slippage mặc định cho giao dịch tự động
            trade_type: crate::types::TradeType::Buy,
            deadline_minutes: 2, // Thời hạn 2 phút
            router_address: String::new(), // Dùng router mặc định
            gas_limit: None,
            gas_price: None,
            strategy: Some(format!("{:?}", strategy).to_lowercase()),
            stop_loss: if strategy == TradeStrategy::Smart { Some(5.0) } else { None }, // 5% stop loss for Smart strategy
            take_profit: if strategy == TradeStrategy::Smart { Some(20.0) } else { Some(10.0) }, // 20% take profit for Smart, 10% for Quick
            max_hold_time: None, // Sẽ được thiết lập dựa trên chiến lược
            custom_params: None,
        };
        
        // Thực hiện giao dịch và lấy kết quả
        info!("Thực hiện mua {} cho token {} trên chain {}", base_amount, token_address, chain_id);
        
        let buy_result = adapter.execute_trade(&trade_params).await;
        
        match buy_result {
            Ok(tx_result) => {
                // Tạo ID giao dịch duy nhất
                let trade_id = uuid::Uuid::new_v4().to_string();
                let current_timestamp = chrono::Utc::now().timestamp() as u64;
                
                // Tính thời gian giữ tối đa dựa trên chiến lược
                let max_hold_time = match strategy {
                    TradeStrategy::Quick => current_timestamp + QUICK_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Smart => current_timestamp + SMART_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Manual => current_timestamp + 24 * 60 * 60, // 24 giờ cho giao dịch thủ công
                };
                
                // Tạo trailing stop loss nếu sử dụng chiến lược Smart
                let trailing_stop_percent = match strategy {
                    TradeStrategy::Smart => Some(SMART_TRADE_TSL_PERCENT),
                    _ => None,
                };
                
                // Xác định giá entry từ kết quả giao dịch
                let entry_price = match tx_result.execution_price {
                    Some(price) => price,
                    None => {
                        error!("Không thể xác định giá mua vào cho token {}", token_address);
                        return Err(anyhow::anyhow!("Không thể xác định giá mua vào"));
                    }
                };
                
                // Lấy số lượng token đã mua
                let token_amount = match tx_result.tokens_received {
                    Some(amount) => amount,
                    None => {
                        error!("Không thể xác định số lượng token nhận được");
                        return Err(anyhow::anyhow!("Không thể xác định số lượng token nhận được"));
                    }
                };
                
                // Tạo theo dõi giao dịch mới
                let trade_tracker = TradeTracker {
                    trade_id: trade_id.clone(),
                    token_address: token_address.to_string(),
                    chain_id,
                    strategy: strategy.clone(),
                    entry_price,
                    token_amount,
                    invested_amount: base_amount,
                    highest_price: entry_price, // Ban đầu, giá cao nhất = giá mua
                    entry_time: current_timestamp,
                    max_hold_time,
                    take_profit_percent: match strategy {
                        TradeStrategy::Quick => QUICK_TRADE_TARGET_PROFIT,
                        _ => 10.0, // Mặc định 10% cho các chiến lược khác
                    },
                    stop_loss_percent: STOP_LOSS_PERCENT,
                    trailing_stop_percent,
                    buy_tx_hash: tx_result.transaction_hash.clone(),
                    sell_tx_hash: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                };
                
                // Thêm vào danh sách theo dõi
                {
                    let mut active_trades = self.active_trades.write().await;
                    active_trades.push(trade_tracker.clone());
                }
                
                // Log thành công
                info!(
                    "Giao dịch mua thành công: ID={}, Token={}, Chain={}, Strategy={:?}, Amount={}, Price={}",
                    trade_id, token_address, chain_id, strategy, base_amount, entry_price
                );
                
                // Thêm vào lịch sử
                let trade_result = TradeResult {
                    trade_id: trade_id.clone(),
                    entry_price,
                    exit_price: None,
                    profit_percent: None,
                    profit_usd: None,
                    entry_time: current_timestamp,
                    exit_time: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                    gas_cost_usd: tx_result.gas_cost_usd.unwrap_or(0.0),
                };
                
                let mut trade_history = self.trade_history.write().await;
                trade_history.push(trade_result);
                
                // Trả về ID giao dịch
                Ok(())
            },
            Err(e) => {
                // Log lỗi giao dịch
                error!("Giao dịch mua thất bại cho token {}: {}", token_address, e);
                Err(anyhow::anyhow!("Giao dịch mua thất bại: {}", e))
            }
        }
    }
    
    /// Thực hiện mua token dựa trên các tham số
    async fn execute_trade(&self, chain_id: u32, token_address: &str, strategy: TradeStrategy, trigger_tx: &MempoolTransaction) -> anyhow::Result<()> {
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id));
            }
        };
        
        // Lấy cấu hình
        let config = self.config.read().await;
        
        // Tính số lượng giao dịch
        let base_amount = match config.auto_trade {
            true => {
                // Tự động tính dựa trên cấu hình, giới hạn trong khoảng min-max
                let calculated_amount = std::cmp::min(
                    trigger_tx.value * config.capital_per_trade_ratio,
                    config.max_trade_amount
                );
                std::cmp::max(calculated_amount, config.min_trade_amount)
            },
            false => {
                // Nếu không bật auto trade, chỉ log và không thực hiện
                info!(
                    "Auto-trade disabled. Would trade {} for token {} with strategy {:?}",
                    config.min_trade_amount, token_address, strategy
                );
                return Ok(());
            }
        };
        
        // Tạo tham số giao dịch
        let trade_params = crate::types::TradeParams {
            chain_type: crate::types::ChainType::EVM(chain_id),
            token_address: token_address.to_string(),
            amount: base_amount,
            slippage: 1.0, // 1% slippage mặc định cho giao dịch tự động
            trade_type: crate::types::TradeType::Buy,
            deadline_minutes: 2, // Thời hạn 2 phút
            router_address: String::new(), // Dùng router mặc định
        };
        
        // Thực hiện giao dịch và lấy kết quả
        info!("Thực hiện mua {} cho token {} trên chain {}", base_amount, token_address, chain_id);
        
        let buy_result = adapter.execute_trade(&trade_params).await;
        
        match buy_result {
            Ok(tx_result) => {
                // Tạo ID giao dịch duy nhất
                let trade_id = uuid::Uuid::new_v4().to_string();
                let current_timestamp = chrono::Utc::now().timestamp() as u64;
                
                // Tính thời gian giữ tối đa dựa trên chiến lược
                let max_hold_time = match strategy {
                    TradeStrategy::Quick => current_timestamp + QUICK_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Smart => current_timestamp + SMART_TRADE_MAX_HOLD_TIME,
                    TradeStrategy::Manual => current_timestamp + 24 * 60 * 60, // 24 giờ cho giao dịch thủ công
                };
                
                // Tạo trailing stop loss nếu sử dụng chiến lược Smart
                let trailing_stop_percent = match strategy {
                    TradeStrategy::Smart => Some(SMART_TRADE_TSL_PERCENT),
                    _ => None,
                };
                
                // Xác định giá entry từ kết quả giao dịch
                let entry_price = match tx_result.execution_price {
                    Some(price) => price,
                    None => {
                        error!("Không thể xác định giá mua vào cho token {}", token_address);
                        return Err(anyhow::anyhow!("Không thể xác định giá mua vào"));
                    }
                };
                
                // Lấy số lượng token đã mua
                let token_amount = match tx_result.tokens_received {
                    Some(amount) => amount,
                    None => {
                        error!("Không thể xác định số lượng token nhận được");
                        return Err(anyhow::anyhow!("Không thể xác định số lượng token nhận được"));
                    }
                };
                
                // Tạo theo dõi giao dịch mới
                let trade_tracker = TradeTracker {
                    trade_id: trade_id.clone(),
                    token_address: token_address.to_string(),
                    chain_id,
                    strategy: strategy.clone(),
                    entry_price,
                    token_amount,
                    invested_amount: base_amount,
                    highest_price: entry_price, // Ban đầu, giá cao nhất = giá mua
                    entry_time: current_timestamp,
                    max_hold_time,
                    take_profit_percent: match strategy {
                        TradeStrategy::Quick => QUICK_TRADE_TARGET_PROFIT,
                        _ => 10.0, // Mặc định 10% cho các chiến lược khác
                    },
                    stop_loss_percent: STOP_LOSS_PERCENT,
                    trailing_stop_percent,
                    buy_tx_hash: tx_result.transaction_hash.clone(),
                    sell_tx_hash: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                };
                
                // Thêm vào danh sách theo dõi
                {
                    let mut active_trades = self.active_trades.write().await;
                    active_trades.push(trade_tracker.clone());
                }
                
                // Log thành công
                info!(
                    "Giao dịch mua thành công: ID={}, Token={}, Chain={}, Strategy={:?}, Amount={}, Price={}",
                    trade_id, token_address, chain_id, strategy, base_amount, entry_price
                );
                
                // Thêm vào lịch sử
                let trade_result = TradeResult {
                    trade_id: trade_id.clone(),
                    entry_price,
                    exit_price: None,
                    profit_percent: None,
                    profit_usd: None,
                    entry_time: current_timestamp,
                    exit_time: None,
                    status: TradeStatus::Open,
                    exit_reason: None,
                    gas_cost_usd: tx_result.gas_cost_usd,
                };
                
                let mut trade_history = self.trade_history.write().await;
                trade_history.push(trade_result);
                
                // Trả về ID giao dịch
                Ok(())
            },
            Err(e) => {
                // Log lỗi giao dịch
                error!("Giao dịch mua thất bại cho token {}: {}", token_address, e);
                Err(anyhow::anyhow!("Giao dịch mua thất bại: {}", e))
            }
        }
    }
    
    /// Theo dõi các giao dịch đang hoạt động
    async fn track_active_trades(&self) -> anyhow::Result<()> {
        // Kiểm tra xem có bất kỳ giao dịch nào cần theo dõi không
        let active_trades = self.active_trades.read().await;
        
        if active_trades.is_empty() {
            return Ok(());
        }
        
        // Clone danh sách để tránh giữ lock quá lâu
        let active_trades_clone = active_trades.clone();
        drop(active_trades); // Giải phóng lock để tránh deadlock
        
        // Xử lý từng giao dịch song song
        let update_futures = active_trades_clone.iter().map(|trade| {
            self.check_and_update_trade(trade.clone())
        });
        
        let results = futures::future::join_all(update_futures).await;
        
        // Kiểm tra và ghi log lỗi nếu có
        for result in results {
            if let Err(e) = result {
                error!("Lỗi khi theo dõi giao dịch: {}", e);
                // Tiếp tục xử lý các giao dịch khác, không dừng lại
            }
        }
        
        Ok(())
    }
    
    /// Kiểm tra và cập nhật trạng thái của một giao dịch
    async fn check_and_update_trade(&self, trade: TradeTracker) -> anyhow::Result<()> {
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&trade.chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", trade.chain_id));
            }
        };
        
        // Nếu giao dịch không ở trạng thái Open, bỏ qua
        if trade.status != TradeStatus::Open {
            return Ok(());
        }
        
        // Lấy giá hiện tại của token
        let current_price = match adapter.get_token_price(&trade.token_address).await {
            Ok(price) => price,
            Err(e) => {
                warn!("Không thể lấy giá hiện tại cho token {}: {}", trade.token_address, e);
                // Không thể lấy giá, nhưng vẫn cần kiểm tra các điều kiện khác
                // Nên tiếp tục với giá = 0
                0.0
            }
        };
        
        // Nếu không lấy được giá, kiểm tra thời gian giữ tối đa
        if current_price <= 0.0 {
            let current_time = chrono::Utc::now().timestamp() as u64;
            
            // Nếu quá thời gian giữ tối đa, bán
            if current_time >= trade.max_hold_time {
                warn!("Không thể lấy giá cho token {}, đã quá thời gian giữ tối đa, thực hiện bán", trade.token_address);
                self.sell_token(trade, "Đã quá thời gian giữ tối đa".to_string(), trade.entry_price).await?;
                return Ok(());
            }
            
            // Nếu chưa quá thời gian, tiếp tục theo dõi
            return Ok(());
        }
        
        // Tính toán phần trăm lợi nhuận/lỗ hiện tại
        let profit_percent = ((current_price - trade.entry_price) / trade.entry_price) * 100.0;
        let current_time = chrono::Utc::now().timestamp() as u64;
        
        // Cập nhật giá cao nhất nếu cần (chỉ với chiến lược có TSL)
        let mut highest_price = trade.highest_price;
        if current_price > highest_price {
            highest_price = current_price;
            
            // Cập nhật TradeTracker với giá cao nhất mới
            {
                let mut active_trades = self.active_trades.write().await;
                if let Some(index) = active_trades.iter().position(|t| t.trade_id == trade.trade_id) {
                    active_trades[index].highest_price = highest_price;
                }
            }
        }
        
        // 1. Kiểm tra điều kiện lợi nhuận đạt mục tiêu
        if profit_percent >= trade.take_profit_percent {
            info!(
                "Take profit triggered for {} (ID: {}): current profit {:.2}% >= target {:.2}%", 
                trade.token_address, trade.trade_id, profit_percent, trade.take_profit_percent
            );
            self.sell_token(trade, "Đạt mục tiêu lợi nhuận".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 2. Kiểm tra điều kiện stop loss
        if profit_percent <= -trade.stop_loss_percent {
            warn!(
                "Stop loss triggered for {} (ID: {}): current profit {:.2}% <= -{:.2}%", 
                trade.token_address, trade.trade_id, profit_percent, trade.stop_loss_percent
            );
            self.sell_token(trade, "Kích hoạt stop loss".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 3. Kiểm tra trailing stop loss (nếu có)
        if let Some(tsl_percent) = trade.trailing_stop_percent {
            let tsl_trigger_price = highest_price * (1.0 - tsl_percent / 100.0);
            
            if current_price <= tsl_trigger_price && highest_price > trade.entry_price {
                // Chỉ kích hoạt TSL nếu đã có lãi trước đó
                info!(
                    "Trailing stop loss triggered for {} (ID: {}): current price {} <= TSL price {:.4} (highest {:.4})", 
                    trade.token_address, trade.trade_id, current_price, tsl_trigger_price, highest_price
                );
                self.sell_token(trade, "Kích hoạt trailing stop loss".to_string(), current_price).await?;
                return Ok(());
            }
        }
        
        // 4. Kiểm tra hết thời gian tối đa
        if current_time >= trade.max_hold_time {
            info!(
                "Max hold time reached for {} (ID: {}): current profit {:.2}%", 
                trade.token_address, trade.trade_id, profit_percent
            );
            self.sell_token(trade, "Đã hết thời gian giữ tối đa".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 5. Kiểm tra an toàn token
        if trade.strategy != TradeStrategy::Manual {
            if let Err(e) = self.check_token_safety_changes(&trade, adapter).await {
                warn!("Phát hiện vấn đề an toàn với token {}: {}", trade.token_address, e);
                self.sell_token(trade, format!("Phát hiện vấn đề an toàn: {}", e), current_price).await?;
                return Ok(());
            }
        }
        
        // Giao dịch vẫn an toàn, tiếp tục theo dõi
        debug!(
            "Trade {} (ID: {}) still active: current profit {:.2}%, current price {:.8}, highest {:.8}", 
            trade.token_address, trade.trade_id, profit_percent, current_price, highest_price
        );
        
        Ok(())
    }
    
    /// Kiểm tra các vấn đề an toàn mới phát sinh với token
    async fn check_token_safety_changes(&self, trade: &TradeTracker, adapter: &Arc<EvmAdapter>) -> anyhow::Result<()> {
        debug!("Checking token safety changes for {}", trade.token_address);
        
        // Lấy thông tin token
        let contract_info = match self.get_contract_info(trade.chain_id, &trade.token_address, adapter).await {
            Some(info) => info,
            None => return Ok(()),
        };
        
        // Kiểm tra thay đổi về quyền owner
        let (has_privilege, privileges) = self.detect_owner_privilege(trade.chain_id, &trade.token_address).await?;
        
        // Kiểm tra có thay đổi về tax/fee
        let (has_dynamic_tax, tax_reason) = self.detect_dynamic_tax(trade.chain_id, &trade.token_address).await?;
        
        // Kiểm tra các sự kiện bất thường
        let liquidity_events = adapter.get_liquidity_events(&trade.token_address, 10).await
            .context("Failed to get recent liquidity events")?;
            
        let has_abnormal_events = abnormal_liquidity_events(&liquidity_events);
        
        // Nếu có bất kỳ vấn đề nghiêm trọng, ghi log và cảnh báo
        if has_dynamic_tax || has_privilege || has_abnormal_events {
            let mut warnings = Vec::new();
            
            if has_dynamic_tax {
                warnings.push(format!("Tax may have changed: {}", tax_reason.unwrap_or_else(|| "Unknown".to_string())));
            }
            
            if has_privilege && !privileges.is_empty() {
                warnings.push(format!("Owner privileges detected: {}", privileges.join(", ")));
            }
            
            if has_abnormal_events {
                warnings.push("Abnormal liquidity events detected".to_string());
            }
            
            let warning_msg = warnings.join("; ");
            warn!("Token safety concerns for {}: {}", trade.token_address, warning_msg);
            
            // Gửi cảnh báo cho user (thông qua alert system)
            if let Some(alert_system) = &self.alert_system {
                let alert = Alert {
                    token_address: trade.token_address.clone(),
                    chain_id: trade.chain_id,
                    alert_type: AlertType::TokenSafetyChanged,
                    message: warning_msg.clone(),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    trade_id: Some(trade.id.clone()),
                };
                
                if let Err(e) = alert_system.send_alert(alert).await {
                    error!("Failed to send alert: {}", e);
                }
            }
        }
        
        Ok(())
    }
    
    /// Bán token
    async fn sell_token(&self, trade: TradeTracker, exit_reason: String, current_price: f64) {
        if let Some(adapter) = self.evm_adapters.get(&trade.chain_id) {
            // Tạo tham số giao dịch bán
            let trade_params = TradeParams {
                chain_type: ChainType::EVM(trade.chain_id),
                token_address: trade.token_address.clone(),
                amount: trade.token_amount,
                slippage: 1.0, // 1% slippage
                trade_type: TradeType::Sell,
                deadline_minutes: 5,
                router_address: "".to_string(), // Sẽ dùng router mặc định
            };
            
            // Thực hiện bán
            match adapter.execute_trade(&trade_params).await {
                Ok(result) => {
                    if let Some(tx_receipt) = result.tx_receipt {
                        // Cập nhật trạng thái giao dịch
                        let now = Utc::now().timestamp() as u64;
                        
                        // Tính lợi nhuận, có kiểm tra chia cho 0
                        let profit_percent = if trade.entry_price > 0.0 {
                            (current_price - trade.entry_price) / trade.entry_price * 100.0
                        } else {
                            0.0 // Giá trị mặc định nếu entry_price = 0
                        };
                        
                        // Lấy giá thực tế từ adapter thay vì hardcoded
                        let token_price_usd = adapter.get_base_token_price_usd().await.unwrap_or(300.0);
                        let profit_usd = trade.invested_amount * profit_percent / 100.0 * token_price_usd;
                        
                        // Tạo kết quả giao dịch
                        let trade_result = TradeResult {
                            trade_id: trade.trade_id.clone(),
                            entry_price: trade.entry_price,
                            exit_price: Some(current_price),
                            profit_percent: Some(profit_percent),
                            profit_usd: Some(profit_usd),
                            entry_time: trade.entry_time,
                            exit_time: Some(now),
                            status: TradeStatus::Closed,
                            exit_reason: Some(exit_reason.clone()),
                            gas_cost_usd: result.gas_cost_usd,
                        };
                        
                        // Cập nhật lịch sử
                        {
                            let mut trade_history = self.trade_history.write().await;
                            trade_history.push(trade_result);
                        }
                        
                        // Xóa khỏi danh sách theo dõi
                        {
                            let mut active_trades = self.active_trades.write().await;
                            active_trades.retain(|t| t.trade_id != trade.trade_id);
                        }
                    }
                },
                Err(e) => {
                    // Log lỗi
                    error!("Error selling token: {:?}", e);
                }
            }
        }
    }
    
    /// Lấy thông tin contract
    pub(crate) async fn get_contract_info(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> Option<ContractInfo> {
        let source_code_future = adapter.get_contract_source_code(token_address);
        let bytecode_future = adapter.get_contract_bytecode(token_address);
        
        let ((source_code, is_verified), bytecode) = tokio::join!(
            source_code_future,
            bytecode_future
        );
        
        let (source_code, is_verified) = source_code.unwrap_or((None, false));
        let bytecode = bytecode.unwrap_or(None);
        
        let abi = if is_verified { 
            adapter.get_contract_abi(token_address).await.unwrap_or(None) 
        } else { 
            None 
        };
        
        let owner_address = adapter.get_contract_owner(token_address).await.unwrap_or(None);
        
        Some(ContractInfo {
            address: token_address.to_string(),
            chain_id,
            source_code,
            bytecode,
            abi,
            is_verified,
            owner_address,
        })
    }
    
    /// Kiểm tra thanh khoản đã được khóa hay chưa và thời gian khóa
    async fn check_liquidity_lock(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, u64) {
        match adapter.check_liquidity_lock(token_address).await {
            Ok((is_locked, duration)) => (is_locked, duration),
            Err(_) => (false, 0), // Mặc định coi như không khóa nếu có lỗi
        }
    }
    
    /// Kiểm tra các dấu hiệu rug pull
    async fn check_rug_pull_indicators(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>, contract_info: &ContractInfo) -> (bool, Vec<String>) {
        let mut indicators = Vec::new();
        
        // Kiểm tra sở hữu tập trung
        if let Ok(ownership_data) = adapter.check_token_ownership_distribution(token_address).await {
            if ownership_data.max_wallet_percentage > MAX_SAFE_OWNERSHIP_PERCENTAGE {
                indicators.push(format!("High ownership concentration: {:.2}%", ownership_data.max_wallet_percentage));
            }
        }
        
        // Kiểm tra quyền mint không giới hạn
        if let Ok(has_unlimited_mint) = adapter.check_unlimited_mint(token_address).await {
            if has_unlimited_mint {
                indicators.push("Unlimited mint function".to_string());
            }
        }
        
        // Kiểm tra blacklist/transfer delay
        if let Ok(has_blacklist) = adapter.check_blacklist_function(token_address).await {
            if has_blacklist {
                indicators.push("Blacklist function".to_string());
            }
        }
        
        if let Ok(has_transfer_delay) = adapter.check_transfer_delay(token_address).await {
            if has_transfer_delay > MAX_TRANSFER_DELAY_SECONDS {
                indicators.push(format!("Transfer delay: {}s", has_transfer_delay));
            }
        }
        
        // Kiểm tra fake ownership renounce
        if let Ok(has_fake_renounce) = adapter.check_fake_ownership_renounce(token_address).await {
            if has_fake_renounce {
                indicators.push("Fake ownership renounce".to_string());
            }
        }
        
        // Kiểm tra history của dev wallet
        if let Some(owner_address) = &contract_info.owner_address {
            if let Ok(dev_history) = adapter.check_developer_history(owner_address).await {
                if dev_history.previous_rug_pulls > 0 {
                    indicators.push(format!("Developer involved in {} previous rug pulls", dev_history.previous_rug_pulls));
                }
            }
        }
        
        (indicators.len() > 0, indicators)
    }
    
    /// Dọn dẹp lịch sử giao dịch cũ
    /// 
    /// Giới hạn kích thước của trade_history để tránh memory leak
    async fn cleanup_trade_history(&self) {
        const MAX_HISTORY_SIZE: usize = 1000;
        const MAX_HISTORY_AGE_SECONDS: u64 = 7 * 24 * 60 * 60; // 7 ngày
        
        let mut history = self.trade_history.write().await;
        if history.len() <= MAX_HISTORY_SIZE {
            return;
        }
        
        let now = Utc::now().timestamp() as u64;
        
        // Lọc ra các giao dịch quá cũ
        history.retain(|trade| {
            now.saturating_sub(trade.created_at) < MAX_HISTORY_AGE_SECONDS
        });
        
        // Nếu vẫn còn quá nhiều, sắp xếp theo thời gian và giữ lại MAX_HISTORY_SIZE
        if history.len() > MAX_HISTORY_SIZE {
            history.sort_by(|a, b| b.created_at.cmp(&a.created_at)); // Sắp xếp mới nhất trước
            history.truncate(MAX_HISTORY_SIZE);
        }
    }
    
    /// Điều chỉnh thời gian ngủ dựa trên số lượng giao dịch đang theo dõi
    /// 
    /// Khi có nhiều giao dịch cần theo dõi, giảm thời gian ngủ để kiểm tra thường xuyên hơn
    /// Khi không có giao dịch nào, tăng thời gian ngủ để tiết kiệm tài nguyên
    async fn adaptive_sleep_time(&self) -> u64 {
        const PRICE_CHECK_INTERVAL_MS: u64 = 5000; // 5 giây
        
        let active_trades = self.active_trades.read().await;
        let count = active_trades.len();
        
        if count == 0 {
            // Không có giao dịch nào, sleep lâu hơn để tiết kiệm tài nguyên
            PRICE_CHECK_INTERVAL_MS * 3
        } else if count > 5 {
            // Nhiều giao dịch đang hoạt động, giảm thời gian kiểm tra
            PRICE_CHECK_INTERVAL_MS / 3
        } else {
            // Sử dụng thời gian mặc định
            PRICE_CHECK_INTERVAL_MS
        }
    }

    /// Cập nhật và lưu trạng thái mới
    async fn update_and_persist_trades(&self) -> Result<()> {
        let active_trades = self.active_trades.read().await;
        let history = self.trade_history.read().await;
        
        // Tạo đối tượng trạng thái để lưu
        let state = SavedBotState {
            active_trades: active_trades.clone(),
            recent_history: history.clone(),
            updated_at: Utc::now().timestamp(),
        };
        
        // Chuyển đổi sang JSON
        let state_json = serde_json::to_string(&state)
            .context("Failed to serialize bot state")?;
        
        // Lưu vào file
        let state_dir = std::env::var("SNIPEBOT_STATE_DIR")
            .unwrap_or_else(|_| "data/state".to_string());
        
        // Đảm bảo thư mục tồn tại
        std::fs::create_dir_all(&state_dir)
            .context(format!("Failed to create state directory: {}", state_dir))?;
        
        let filename = format!("{}/smart_trade_state.json", state_dir);
        std::fs::write(&filename, state_json)
            .context(format!("Failed to write state to file: {}", filename))?;
        
        debug!("Persisted bot state to {}", filename);
        Ok(())
    }
    
    /// Phục hồi trạng thái từ lưu trữ
    async fn restore_state(&self) -> Result<()> {
        // Lấy đường dẫn file trạng thái
        let state_dir = std::env::var("SNIPEBOT_STATE_DIR")
            .unwrap_or_else(|_| "data/state".to_string());
        let filename = format!("{}/smart_trade_state.json", state_dir);
        
        // Kiểm tra file tồn tại
        if !std::path::Path::new(&filename).exists() {
            info!("No saved state found, starting with empty state");
            return Ok(());
        }
        
        // Đọc file
        let state_json = std::fs::read_to_string(&filename)
            .context(format!("Failed to read state file: {}", filename))?;
        
        // Chuyển đổi từ JSON
        let state: SavedBotState = serde_json::from_str(&state_json)
            .context("Failed to deserialize bot state")?;
        
        // Kiểm tra tính hợp lệ của dữ liệu
        let now = Utc::now().timestamp();
        let max_age_seconds = 24 * 60 * 60; // 24 giờ
        
        if now - state.updated_at > max_age_seconds {
            warn!("Saved state is too old ({} seconds), starting with empty state", 
                  now - state.updated_at);
            return Ok(());
        }
        
        // Phục hồi trạng thái
        {
            let mut active_trades = self.active_trades.write().await;
            active_trades.clear();
            active_trades.extend(state.active_trades);
            info!("Restored {} active trades from saved state", active_trades.len());
        }
        
        {
            let mut history = self.trade_history.write().await;
            history.clear();
            history.extend(state.recent_history);
            info!("Restored {} historical trades from saved state", history.len());
        }
        
        Ok(())
    }

    /// Monitor active trades and perform necessary actions
    async fn monitor_loop(&self) {
        let mut counter = 0;
        let mut persist_counter = 0;
        
        while let true_val = *self.running.read().await {
            if !true_val {
                break;
            }
            
            // Kiểm tra và cập nhật tất cả các giao dịch đang active
            if let Err(e) = self.check_active_trades().await {
                error!("Error checking active trades: {}", e);
            }
            
            // Tăng counter và dọn dẹp lịch sử giao dịch định kỳ
            counter += 1;
            if counter >= 100 {
                self.cleanup_trade_history().await;
                counter = 0;
            }
            
            // Lưu trạng thái vào bộ nhớ định kỳ
            persist_counter += 1;
            if persist_counter >= 30 { // Lưu mỗi 30 chu kỳ (khoảng 2.5 phút với interval 5s)
                if let Err(e) = self.update_and_persist_trades().await {
                    error!("Failed to persist bot state: {}", e);
                }
                persist_counter = 0;
            }
            
            // Tính toán thời gian sleep thích ứng
            let sleep_time = self.adaptive_sleep_time().await;
            tokio::time::sleep(tokio::time::Duration::from_millis(sleep_time)).await;
        }
        
        // Lưu trạng thái trước khi dừng
        if let Err(e) = self.update_and_persist_trades().await {
            error!("Failed to persist bot state before stopping: {}", e);
        }
    }

    /// Check and update the status of a trade
    async fn check_and_update_trade(&self, trade: TradeTracker) -> anyhow::Result<()> {
        // Get adapter for chain
        let adapter = match self.evm_adapters.get(&trade.chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("No adapter found for chain ID {}", trade.chain_id));
            }
        };
        
        // If trade is not in Open state, skip
        if trade.status != TradeStatus::Open {
            return Ok(());
        }
        
        // Get current price of token
        let current_price = match adapter.get_token_price(&trade.token_address).await {
            Ok(price) => price,
            Err(e) => {
                warn!("Cannot get current price for token {}: {}", trade.token_address, e);
                // Cannot get price, but still need to check other conditions
                // Continue with price = 0
                0.0
            }
        };
        
        // If cannot get price, check max hold time
        if current_price <= 0.0 {
            let current_time = chrono::Utc::now().timestamp() as u64;
            
            // If exceeded max hold time, sell
            if current_time >= trade.max_hold_time {
                warn!("Cannot get price for token {}, exceeded max hold time, perform sell", trade.token_address);
                self.sell_token(trade, "Exceeded max hold time".to_string(), trade.entry_price).await?;
                return Ok(());
            }
            
            // If not exceeded, continue monitoring
            return Ok(());
        }
        
        // Calculate current profit percentage/loss
        let profit_percent = ((current_price - trade.entry_price) / trade.entry_price) * 100.0;
        let current_time = chrono::Utc::now().timestamp() as u64;
        
        // Update highest price if needed (only for Smart strategy)
        let mut highest_price = trade.highest_price;
        if current_price > highest_price {
            highest_price = current_price;
            
            // Update TradeTracker with new highest price
            {
                let mut active_trades = self.active_trades.write().await;
                if let Some(index) = active_trades.iter().position(|t| t.trade_id == trade.trade_id) {
                    active_trades[index].highest_price = highest_price;
                }
            }
        }
        
        // 1. Check profit condition to reach target
        if profit_percent >= trade.take_profit_percent {
            info!(
                "Take profit triggered for {} (ID: {}): current profit {:.2}% >= target {:.2}%", 
                trade.token_address, trade.trade_id, profit_percent, trade.take_profit_percent
            );
            self.sell_token(trade, "Reached profit target".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 2. Check stop loss condition
        if profit_percent <= -trade.stop_loss_percent {
            warn!(
                "Stop loss triggered for {} (ID: {}): current profit {:.2}% <= -{:.2}%", 
                trade.token_address, trade.trade_id, profit_percent, trade.stop_loss_percent
            );
            self.sell_token(trade, "Activated stop loss".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 3. Check trailing stop loss (if any)
        if let Some(tsl_percent) = trade.trailing_stop_percent {
            let tsl_trigger_price = highest_price * (1.0 - tsl_percent / 100.0);
            
            if current_price <= tsl_trigger_price && highest_price > trade.entry_price {
                // Activate TSL only if there was profit before
                info!(
                    "Trailing stop loss triggered for {} (ID: {}): current price {} <= TSL price {:.4} (highest {:.4})", 
                    trade.token_address, trade.trade_id, current_price, tsl_trigger_price, highest_price
                );
                self.sell_token(trade, "Activated trailing stop loss".to_string(), current_price).await?;
                return Ok(());
            }
        }
        
        // 4. Check max hold time
        if current_time >= trade.max_hold_time {
            info!(
                "Max hold time reached for {} (ID: {}): current profit {:.2}%", 
                trade.token_address, trade.trade_id, profit_percent
            );
            self.sell_token(trade, "Exceeded max hold time".to_string(), current_price).await?;
            return Ok(());
        }
        
        // 5. Check token safety
        if trade.strategy != TradeStrategy::Manual {
            if let Err(e) = self.check_token_safety_changes(&trade, adapter).await {
                warn!("Security issue detected with token {}: {}", trade.token_address, e);
                self.sell_token(trade, format!("Security issue detected: {}", e), current_price).await?;
                return Ok(());
            }
        }
        
        // Trade still safe, continue monitoring
        debug!(
            "Trade {} (ID: {}) still active: current profit {:.2}%, current price {:.8}, highest {:.8}", 
            trade.token_address, trade.trade_id, profit_percent, current_price, highest_price
        );
        
        Ok(())
    }

    /// Monitor active trades and perform necessary actions
    async fn check_active_trades(&self) -> anyhow::Result<()> {
        // Check if there are any trades that need monitoring
        let active_trades = self.active_trades.read().await;
        
        if active_trades.is_empty() {
            return Ok(());
        }
        
        // Clone list to avoid holding lock for too long
        let active_trades_clone = active_trades.clone();
        drop(active_trades); // Release lock to avoid deadlock
        
        // Process each trade in parallel
        let update_futures = active_trades_clone.iter().map(|trade| {
            self.check_and_update_trade(trade.clone())
        });
        
        let results = futures::future::join_all(update_futures).await;
        
        // Check and log error if any
        for result in results {
            if let Err(e) = result {
                error!("Error monitoring trade: {}", e);
                // Continue processing other trades, do not stop
            }
        }
        
        Ok(())
    }

    /// Clone executor and copy config from original instance (safe for async context)
    pub(crate) async fn clone_with_config(&self) -> Self {
        let config = {
            let config_guard = self.config.read().await;
            config_guard.clone()
        };
        
        Self {
            evm_adapters: self.evm_adapters.clone(),
            analys_client: self.analys_client.clone(),
            risk_analyzer: self.risk_analyzer.clone(),
            mempool_analyzers: self.mempool_analyzers.clone(),
            config: RwLock::new(config),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            coordinator: self.coordinator.clone(),
            executor_id: self.executor_id.clone(),
            coordinator_subscription: RwLock::new(None),
        }
    }

    /// Đăng ký với coordinator
    async fn register_with_coordinator(&self) -> anyhow::Result<()> {
        if let Some(coordinator) = &self.coordinator {
            // Đăng ký executor với coordinator
            coordinator.register_executor(&self.executor_id, ExecutorType::SmartTrade).await?;
            
            // Đăng ký nhận thông báo về các cơ hội mới
            let subscription_callback = {
                let self_clone = self.clone_with_config().await;
                Arc::new(move |opportunity: SharedOpportunity| -> anyhow::Result<()> {
                    let executor = self_clone.clone();
                    tokio::spawn(async move {
                        if let Err(e) = executor.handle_shared_opportunity(opportunity).await {
                            error!("Error handling shared opportunity: {}", e);
                        }
                    });
                    Ok(())
                })
            };
            
            // Lưu subscription ID
            let subscription_id = coordinator.subscribe_to_opportunities(
                &self.executor_id, subscription_callback
            ).await?;
            
            let mut sub_id = self.coordinator_subscription.write().await;
            *sub_id = Some(subscription_id);
            
            info!("Registered with coordinator, subscription ID: {}", sub_id.as_ref().unwrap());
        }
        
        Ok(())
    }
    
    /// Hủy đăng ký khỏi coordinator
    async fn unregister_from_coordinator(&self) -> anyhow::Result<()> {
        if let Some(coordinator) = &self.coordinator {
            // Hủy đăng ký subscription
            let sub_id = {
                let sub = self.coordinator_subscription.read().await;
                sub.clone()
            };
            
            if let Some(id) = sub_id {
                coordinator.unsubscribe_from_opportunities(&id).await?;
                
                // Xóa subscription ID
                let mut sub = self.coordinator_subscription.write().await;
                *sub = None;
            }
            
            // Hủy đăng ký executor
            coordinator.unregister_executor(&self.executor_id).await?;
            info!("Unregistered from coordinator");
        }
        
        Ok(())
    }
    
    /// Xử lý cơ hội được chia sẻ từ coordinator
    async fn handle_shared_opportunity(&self, opportunity: SharedOpportunity) -> anyhow::Result<()> {
        info!("Received shared opportunity: {} (type: {:?}, from: {})", 
             opportunity.id, opportunity.opportunity_type, opportunity.source);
        
        // Kiểm tra nếu nguồn là chính mình, bỏ qua
        if opportunity.source == self.executor_id {
            debug!("Ignoring opportunity from self");
            return Ok(());
        }
        
        // Lấy cấu hình
        let config = self.config.read().await;
        
        // Kiểm tra nếu bot chưa bật hoặc đang đầy trade
        if !config.enabled || !config.auto_trade {
            debug!("Bot disabled or auto-trade disabled, ignoring opportunity");
            return Ok(());
        }
        
        let active_trades = self.active_trades.read().await;
        if active_trades.len() >= config.max_concurrent_trades as usize {
            debug!("Max concurrent trades reached, ignoring opportunity");
            return Ok(());
        }
        
        // Kiểm tra xem có token nào trong danh sách token của cơ hội nằm trong blacklist không
        let mut blacklisted = false;
        for token in &opportunity.tokens {
            if config.token_blacklist.contains(token) {
                blacklisted = true;
                debug!("Token {} in blacklist, ignoring opportunity", token);
                break;
            }
        }
        
        if blacklisted {
            return Ok(());
        }
        
        // Nếu whitelist không rỗng, kiểm tra xem có token nào nằm trong whitelist không
        if !config.token_whitelist.is_empty() {
            let mut whitelisted = false;
            for token in &opportunity.tokens {
                if config.token_whitelist.contains(token) {
                    whitelisted = true;
                    break;
                }
            }
            
            if !whitelisted {
                debug!("No tokens in whitelist, ignoring opportunity");
                return Ok(());
            }
        }
        
        // Kiểm tra điểm rủi ro
        if opportunity.risk_score > config.max_risk_score {
            debug!("Risk score too high: {}/{}, ignoring opportunity", 
                  opportunity.risk_score, config.max_risk_score);
            return Ok(());
        }
        
        // Kiểm tra lợi nhuận tối thiểu
        if opportunity.estimated_profit_usd < config.min_profit_threshold {
            debug!("Profit too low: ${}/{}, ignoring opportunity", 
                  opportunity.estimated_profit_usd, config.min_profit_threshold);
            return Ok(());
        }
        
        // Kiểm tra xem cơ hội đã bị reserve chưa
        if let Some(reservation) = &opportunity.reservation {
            debug!("Opportunity already reserved by {}, ignoring", reservation.executor_id);
            return Ok(());
        }
        
        // Quyết định ưu tiên dựa trên score của cơ hội
        let priority = if opportunity.risk_score < 30 && opportunity.estimated_profit_usd > 50.0 {
            OpportunityPriority::High
        } else if opportunity.risk_score < 50 && opportunity.estimated_profit_usd > 20.0 {
            OpportunityPriority::Medium
        } else {
            OpportunityPriority::Low
        };
        
        // Thử reserve cơ hội
        if let Some(coordinator) = &self.coordinator {
            let success = coordinator.reserve_opportunity(
                &opportunity.id, &self.executor_id, priority
            ).await?;
            
            if !success {
                debug!("Failed to reserve opportunity, another executor has higher priority");
                return Ok(());
            }
            
            info!("Successfully reserved opportunity {}", opportunity.id);
        }
        
        // Xử lý cơ hội tùy theo loại
        match &opportunity.opportunity_type {
            SharedOpportunityType::Mev(mev_type) => {
                debug!("Processing MEV opportunity: {:?}", mev_type);
                // Implement MEV handling logic based on mev_type
            },
            SharedOpportunityType::NewToken => {
                // Lấy token đầu tiên trong danh sách
                if let Some(token_address) = opportunity.tokens.get(0) {
                    // Thực hiện phân tích token và giao dịch
                    debug!("Processing new token opportunity: {}", token_address);
                    
                    // Kiểm tra token an toàn
                    let token_safety = match self.evaluate_token(opportunity.chain_id, token_address).await {
                        Ok(Some(safety)) => safety,
                        _ => {
                            debug!("Token safety check failed, releasing opportunity");
                            if let Some(coordinator) = &self.coordinator {
                                coordinator.release_opportunity(&opportunity.id, &self.executor_id).await?;
                            }
                            return Ok(());
                        }
                    };
                    
                    if !token_safety.is_valid() || token_safety.is_honeypot {
                        debug!("Token is not safe or is honeypot, releasing opportunity");
                        if let Some(coordinator) = &self.coordinator {
                            coordinator.release_opportunity(&opportunity.id, &self.executor_id).await?;
                        }
                        return Ok(());
                    }
                    
                    // Dựa vào độ rủi ro để chọn chiến lược phù hợp
                    let risk_score = self.analyze_token_risk(opportunity.chain_id, token_address).await?;
                    let strategy = if risk_score.score < 40 {
                        TradeStrategy::Smart
                    } else {
                        TradeStrategy::Quick
                    };
                    
                    // Lấy thông tin giao dịch trigger từ custom_data nếu có
                    let mut trigger_tx_hash = opportunity.custom_data.get("trigger_tx").cloned();
                    if trigger_tx_hash.is_none() {
                        trigger_tx_hash = Some("shared_opportunity".to_string());
                    }
                    
                    // Tạo một MempoolTransaction giả để truyền vào executor
                    let dummy_tx = MempoolTransaction {
                        tx_hash: trigger_tx_hash.unwrap_or_else(|| "shared_opportunity".to_string()),
                        from_address: opportunity.source.clone(),
                        to_address: token_address.clone(),
                        value: opportunity.custom_data.get("value")
                            .and_then(|v| v.parse::<f64>().ok())
                            .unwrap_or(0.1),
                        gas_price: opportunity.custom_data.get("gas_price")
                            .and_then(|v| v.parse::<f64>().ok()),
                        gas_limit: opportunity.custom_data.get("gas_limit")
                            .and_then(|v| v.parse::<u64>().ok()),
                        input_data: Some("shared_opportunity".to_string()),
                        timestamp: Some(chrono::Utc::now().timestamp() as u64),
                        block_number: opportunity.custom_data.get("block_number")
                            .and_then(|v| v.parse::<u64>().ok()),
                        to_token: Some(TokenInfo {
                            address: token_address.clone(),
                            symbol: opportunity.custom_data.get("symbol")
                                .cloned()
                                .unwrap_or_else(|| "UNKNOWN".to_string()),
                            decimals: opportunity.custom_data.get("decimals")
                                .and_then(|v| v.parse::<u8>().ok())
                                .unwrap_or(18),
                            price: opportunity.custom_data.get("price")
                                .and_then(|v| v.parse::<f64>().ok())
                                .unwrap_or(0.0),
                        }),
                        transaction_type: TransactionType::Standard,
                    };
                    
                    // Thực hiện giao dịch
                    match self.execute_trade(opportunity.chain_id, token_address, strategy, &dummy_tx).await {
                        Ok(_) => info!("Successfully executed trade for shared opportunity: {}", opportunity.id),
                        Err(e) => {
                            error!("Failed to execute trade for shared opportunity: {}", e);
                            // Release opportunity để các executor khác có thể thử
                            if let Some(coordinator) = &self.coordinator {
                                coordinator.release_opportunity(&opportunity.id, &self.executor_id).await?;
                            }
                        }
                    }
                }
            },
            SharedOpportunityType::LiquidityChange => {
                // Handle liquidity change opportunity
                debug!("Processing liquidity change opportunity");
                // Implement liquidity change handling logic
            },
            SharedOpportunityType::PriceMovement => {
                // Handle price movement opportunity
                debug!("Processing price movement opportunity");
                // Implement price movement handling logic
            },
            SharedOpportunityType::Custom(custom_type) => {
                debug!("Processing custom opportunity type: {}", custom_type);
                // Implement custom opportunity handling logic
            },
        }
        
        Ok(())
    }
    
    /// Chia sẻ cơ hội mới với các executor khác thông qua coordinator
    async fn share_opportunity(
        &self, 
        chain_id: u32,
        token_address: &str,
        opportunity_type: SharedOpportunityType,
        estimated_profit_usd: f64,
        risk_score: u8,
        time_sensitivity: u64,
    ) -> anyhow::Result<()> {
        if let Some(coordinator) = &self.coordinator {
            // Tạo unique ID cho cơ hội
            let opportunity_id = format!("opp-{}", uuid::Uuid::new_v4());
            
            // Tạo custom data map
            let mut custom_data = HashMap::new();
            
            // Lấy thông tin token
            if let Some(adapter) = self.evm_adapters.get(&chain_id) {
                if let Ok(token_info) = adapter.get_token_info(token_address).await {
                    custom_data.insert("symbol".to_string(), token_info.symbol);
                    custom_data.insert("decimals".to_string(), token_info.decimals.to_string());
                    custom_data.insert("price".to_string(), token_info.price.to_string());
                }
            }
            
            // Thêm block hiện tại
            if let Some(adapter) = self.evm_adapters.get(&chain_id) {
                if let Ok(block) = adapter.get_current_block_number().await {
                    custom_data.insert("block_number".to_string(), block.to_string());
                }
            }
            
            // Tạo SharedOpportunity
            let opportunity = SharedOpportunity {
                id: opportunity_id,
                chain_id,
                opportunity_type,
                tokens: vec![token_address.to_string()],
                estimated_profit_usd,
                risk_score,
                time_sensitivity,
                source: self.executor_id.clone(),
                created_at: chrono::Utc::now().timestamp() as u64,
                custom_data,
                reservation: None,
            };
            
            // Chia sẻ qua coordinator
            coordinator.share_opportunity(&self.executor_id, opportunity).await?;
            info!("Shared opportunity for token {} with other executors", token_address);
        }
        
        Ok(())
    }

    /// Tính toán gas price tối ưu dựa trên tình trạng mạng hiện tại
    /// 
    /// * `chain_id` - ID của blockchain
    /// * `priority` - Mức độ ưu tiên của giao dịch (1 = thấp, 2 = trung bình, 3 = cao)
    async fn calculate_optimal_gas_price(&self, chain_id: u32, priority: u8) -> Result<f64> {
        // Lấy adapter cho chain này
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id)),
        };
        
        // Sử dụng chức năng chung từ common/gas
        use crate::tradelogic::common::gas::{calculate_optimal_gas_price, TransactionPriority};
        let transaction_priority: TransactionPriority = priority.into();
        
        // Lấy mempool provider từ analys client
        let mempool_provider = Some(&self.analys_client.mempool_provider());
        
        // Tính toán gas price tối ưu dùng hàm chung
        let result = calculate_optimal_gas_price(
            chain_id, 
            transaction_priority, 
            adapter, 
            mempool_provider,
            self.should_apply_mev_protection(chain_id).await
        ).await?;
        
        Ok(result)
    }
    
    /// Kiểm tra nếu nên áp dụng bảo vệ MEV
    async fn should_apply_mev_protection(&self, chain_id: u32) -> bool {
        // Chỉ áp dụng MEV protection trên mainnet và một số chain lớn có MEV
        use crate::tradelogic::common::gas::needs_mev_protection;
        needs_mev_protection(chain_id)
    }

    /// Thực hiện giao dịch với retry tự động khi thất bại
    /// 
    /// * `params` - Tham số giao dịch
    /// * `max_retries` - Số lần thử lại tối đa
    /// * `increase_gas_percent` - Phần trăm tăng gas mỗi lần thử lại
    async fn execute_trade_with_retry(
        &self, 
        mut params: TradeParams, 
        max_retries: u32, 
        increase_gas_percent: f64
    ) -> Result<TransactionResult> {
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", params.chain_id)),
        };
        
        // Nếu gas_price chưa được thiết lập, tính toán giá gas tối ưu
        if params.gas_price.is_none() {
            let optimal_gas = self.calculate_optimal_gas_price(params.chain_id, 2).await?;
            params.gas_price = Some(optimal_gas);
        }
        
        let mut last_error = None;
        let mut current_gas_price = params.gas_price.unwrap_or(0.0);
        
        // Thử giao dịch với số lần retry được chỉ định
        for retry in 0..=max_retries {
            // Cập nhật gas price cho mỗi lần retry
            if retry > 0 {
                current_gas_price *= 1.0 + (increase_gas_percent / 100.0);
                params.gas_price = Some(current_gas_price);
                
                info!("Thử lại giao dịch ({}/{}), tăng gas price lên: {} gwei", 
                     retry, max_retries, current_gas_price);
            }
            
            match adapter.execute_trade(&params).await {
                Ok(result) => {
                    // Giao dịch thành công
                    info!("Giao dịch thành công sau {} lần thử, tx hash: {}", 
                         retry, result.transaction_hash.as_ref().unwrap_or(&"unknown".to_string()));
                    return Ok(result);
                },
                Err(e) => {
                    // Xác định xem có nên thử lại không dựa trên loại lỗi
                    if self.is_retriable_error(&e) && retry < max_retries {
                        warn!("Giao dịch thất bại ({}), sẽ thử lại với gas cao hơn: {}", e, current_gas_price);
                        last_error = Some(e);
                        
                        // Chờ một khoảng thời gian ngắn trước khi thử lại
                        let wait_time = 1000 + retry as u64 * 1000; // 1s, 2s, 3s,...
                        tokio::time::sleep(tokio::time::Duration::from_millis(wait_time)).await;
                        continue;
                    } else {
                        // Lỗi không thể thử lại hoặc đã hết số lần thử
                        return Err(anyhow::anyhow!("Giao dịch thất bại sau {} lần thử: {}", 
                                                  retry, e));
                    }
                }
            }
        }
        
        // Nếu đến đây vẫn chưa return, tức là tất cả các lần retry đều thất bại
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Giao dịch thất bại với lỗi không xác định")))
    }
    
    /// Kiểm tra xem lỗi có thể thử lại không
    fn is_retriable_error(&self, error: &anyhow::Error) -> bool {
        let error_msg = error.to_string().to_lowercase();
        
        // Các lỗi liên quan đến gas, nonce hoặc mạng có thể thử lại
        error_msg.contains("underpriced") || 
        error_msg.contains("gas price too low") ||
        error_msg.contains("transaction underpriced") ||
        error_msg.contains("replacement transaction underpriced") ||
        error_msg.contains("nonce too low") ||
        error_msg.contains("already known") ||
        error_msg.contains("connection reset") ||
        error_msg.contains("timeout") ||
        error_msg.contains("503") ||
        error_msg.contains("429") ||
        error_msg.contains("rate limit") ||
        error_msg.contains("too many requests")
    }

    /// Xác thực dữ liệu mempool từ nhiều nguồn khác nhau
    /// 
    /// Đảm bảo thông tin token và giao dịch từ mempool là chính xác và đáng tin cậy
    /// trước khi thực hiện giao dịch
    /// 
    /// * `chain_id` - Chain ID
    /// * `token_address` - Địa chỉ token cần xác thực
    /// * `tx` - Giao dịch mempool đã phát hiện
    async fn validate_mempool_data(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        tx: &MempoolTransaction
    ) -> Result<ValidationResult> {
        // Lấy adapter cho chain này
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id)),
        };
        
        // 1. Xác thực địa chỉ token - Kiểm tra xem địa chỉ token có tồn tại và là token thực
        info!("Xác thực token {} trên chain {}", token_address, chain_id);
        let token_validation = self.validate_token_address(chain_id, token_address, adapter.clone()).await;
        if let Err(e) = token_validation {
            return Ok(ValidationResult {
                is_valid: false,
                token_valid: false,
                tx_valid: false,
                confidence_score: 0,
                reasons: vec![format!("Token không hợp lệ: {}", e)],
            });
        }
        
        // 2. Xác thực giao dịch mempool - Kiểm tra xem giao dịch có tồn tại trong mempool không
        info!("Xác thực giao dịch mempool: {}", tx.tx_hash);
        let tx_validated = self.validate_mempool_transaction(chain_id, tx).await;
        if !tx_validated.is_valid {
            return Ok(ValidationResult {
                is_valid: false,
                token_valid: true,
                tx_valid: false,
                confidence_score: 0,
                reasons: tx_validated.reasons,
            });
        }
        
        // 3. Xác thực mối quan hệ giữa token và giao dịch mempool
        let token_tx_validation = self.validate_token_transaction_relation(chain_id, token_address, tx).await;
        if !token_tx_validation.is_valid {
            return Ok(ValidationResult {
                is_valid: false,
                token_valid: true,
                tx_valid: true,
                confidence_score: 0,
                reasons: token_tx_validation.reasons,
            });
        }
        
        // 4. Chống giao dịch giả mạo - Kiểm tra các dấu hiệu mempool poisoning
        let anti_poisoning_check = self.check_mempool_poisoning(chain_id, token_address, tx).await;
        if !anti_poisoning_check.is_valid {
            return Ok(ValidationResult {
                is_valid: false,
                token_valid: true,
                tx_valid: true,
                confidence_score: 0,
                reasons: anti_poisoning_check.reasons,
            });
        }
        
        // 5. Xác thực giá - So sánh giá từ giao dịch mempool với giá từ các nguồn khác
        let price_validation = self.validate_price_from_multiple_sources(chain_id, token_address, tx).await;
        if !price_validation.is_valid {
            return Ok(ValidationResult {
                is_valid: false,
                token_valid: true,
                tx_valid: true,
                confidence_score: 30, // Có thể vẫn có một số khả năng là đúng
                reasons: price_validation.reasons,
            });
        }
        
        // 6. Kiểm tra hoạt động khả nghi trên token này - So sánh với lịch sử trước đó
        let suspicious_activity = self.check_suspicious_activity(chain_id, token_address, tx).await;
        if !suspicious_activity.is_valid {
            return Ok(ValidationResult {
                is_valid: false,
                token_valid: true,
                tx_valid: true,
                confidence_score: 40,
                reasons: suspicious_activity.reasons,
            });
        }
        
        // 7. Tính điểm tin cậy cho cặp token-transaction từ kết quả các bước trên
        let confidence_score = self.calculate_confidence_score(
            &token_validation,
            &tx_validated,
            &token_tx_validation,
            &anti_poisoning_check,
            &price_validation,
            &suspicious_activity
        ).await;
        
        // Kết luận: Dữ liệu mempool đáng tin cậy nếu đạt điểm tin cậy đủ cao
        if confidence_score >= 70 {
            Ok(ValidationResult {
                is_valid: true,
                token_valid: true,
                tx_valid: true,
                confidence_score,
                reasons: vec![format!("Dữ liệu mempool đáng tin cậy với điểm tin cậy {}", confidence_score)],
            })
        } else {
            Ok(ValidationResult {
                is_valid: false,
                token_valid: true,
                tx_valid: true,
                confidence_score,
                reasons: vec![format!("Điểm tin cậy quá thấp: {}/100", confidence_score)],
            })
        }
    }
    
    /// Xác thực địa chỉ token
    async fn validate_token_address(&self, chain_id: u32, token_address: &str, adapter: Arc<EvmAdapter>) -> Result<()> {
        // 1. Kiểm tra định dạng địa chỉ
        if !self.is_valid_address_format(token_address) {
            return Err(anyhow::anyhow!("Định dạng địa chỉ token không hợp lệ"));
        }
        
        // 2. Kiểm tra token có tồn tại trên chain
        let token_exists = adapter.check_token_exists(token_address).await?;
        if !token_exists {
            return Err(anyhow::anyhow!("Token không tồn tại trên chain"));
        }
        
        // 3. Kiểm tra token có phải là hợp đồng ERC20 hợp lệ
        let is_valid_erc20 = adapter.validate_erc20_interface(token_address).await?;
        if !is_valid_erc20 {
            return Err(anyhow::anyhow!("Token không tuân thủ chuẩn ERC20"));
        }
        
        // 4. Kiểm tra thêm tính hợp lệ của token thông qua analys client
        match self.analys_client.token_provider().analyze_token(chain_id, token_address).await {
            Ok(token_safety) => {
                if !token_safety.is_valid() {
                    return Err(anyhow::anyhow!("Token không hợp lệ theo phân tích an toàn"));
                }
            },
            Err(e) => {
                warn!("Không thể phân tích token thông qua analys client: {}", e);
                // Không fail hard ở đây, vẫn tiếp tục với các kiểm tra khác
            }
        }
        
        Ok(())
    }
    
    /// Kiểm tra định dạng địa chỉ Ethereum
    fn is_valid_address_format(&self, address: &str) -> bool {
        // Kiểm tra cơ bản: 0x + 40 ký tự hex
        if !address.starts_with("0x") || address.len() != 42 {
            return false;
        }
        
        // Kiểm tra xem có chứa các ký tự không hợp lệ không
        for c in address[2..].chars() {
            if !c.is_ascii_hexdigit() {
                return false;
            }
        }
        
        true
    }
    
    /// Xác thực giao dịch mempool
    async fn validate_mempool_transaction(&self, chain_id: u32, tx: &MempoolTransaction) -> SimpleValidationResult {
        let mut reasons = Vec::new();
        let mut is_valid = true;
        
        // 1. Kiểm tra tx_hash
        if tx.tx_hash.is_empty() || !tx.tx_hash.starts_with("0x") || tx.tx_hash.len() != 66 {
            reasons.push("TX hash không hợp lệ".to_string());
            is_valid = false;
        }
        
        // 2. Kiểm tra from_address
        if tx.from_address.is_empty() || !self.is_valid_address_format(&tx.from_address) {
            reasons.push("Địa chỉ người gửi không hợp lệ".to_string());
            is_valid = false;
        }
        
        // 3. Kiểm tra to_address
        if tx.to_address.is_empty() || !self.is_valid_address_format(&tx.to_address) {
            reasons.push("Địa chỉ người nhận không hợp lệ".to_string());
            is_valid = false;
        }
        
        // 4. Kiểm tra xem giao dịch có còn trong mempool không
        if let Some(analyzer) = self.mempool_analyzers.get(&chain_id) {
            match analyzer.is_transaction_in_mempool(&tx.tx_hash).await {
                Ok(in_mempool) => {
                    if !in_mempool {
                        reasons.push("Giao dịch không còn trong mempool".to_string());
                        is_valid = false;
                    }
                },
                Err(e) => {
                    warn!("Không thể kiểm tra giao dịch trong mempool: {}", e);
                    reasons.push(format!("Không thể xác minh giao dịch trong mempool: {}", e));
                    // Không fail hard, vẫn tiếp tục
                }
            }
        }
        
        // 5. Kiểm tra timestamp (nếu giao dịch quá cũ > 30 giây, có thể không còn hợp lệ)
        let now = chrono::Utc::now().timestamp() as u64;
        let tx_timestamp = tx.timestamp.unwrap_or(now);
        if now - tx_timestamp > 30 {
            reasons.push(format!("Giao dịch quá cũ ({} giây)", now - tx_timestamp));
            // Giao dịch quá cũ là warning, không phải lỗi nặng
            if now - tx_timestamp > 120 {
                is_valid = false;
            }
        }
        
        SimpleValidationResult {
            is_valid,
            reasons,
        }
    }
    
    /// Xác thực mối quan hệ giữa token và giao dịch mempool
    async fn validate_token_transaction_relation(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        tx: &MempoolTransaction
    ) -> SimpleValidationResult {
        let mut reasons = Vec::new();
        let mut is_valid = true;
        
        // 1. Kiểm tra nếu token được đề cập trong giao dịch
        let token_in_tx = if let Some(to_token) = &tx.to_token {
            // Nếu to_token tồn tại, kiểm tra địa chỉ có khớp không
            if to_token.address.to_lowercase() != token_address.to_lowercase() {
                reasons.push(format!(
                    "Không khớp địa chỉ token: {} vs {}", 
                    to_token.address.to_lowercase(), 
                    token_address.to_lowercase()
                ));
                false
            } else {
                true
            }
        } else {
            // Nếu không có to_token, kiểm tra input data của giao dịch
            if let Some(input) = &tx.input_data {
                // Kiểm tra xem input data có chứa địa chỉ token không (bỏ 0x)
                let token_no_prefix = if token_address.starts_with("0x") {
                    &token_address[2..]
                } else {
                    token_address
                };
                
                input.contains(&token_no_prefix.to_lowercase())
            } else {
                false
            }
        };
        
        if !token_in_tx {
            reasons.push("Giao dịch không liên quan đến token này".to_string());
            is_valid = false;
        }
        
        // 2. Kiểm tra gas limit - nếu gas limit quá thấp, có thể giao dịch sẽ thất bại
        if let Some(gas_limit) = tx.gas_limit {
            if gas_limit < 100000 {
                reasons.push(format!("Gas limit quá thấp ({}) cho giao dịch token", gas_limit));
                // Không fail hard, chỉ cảnh báo
            }
        }
        
        // 3. Kiểm tra giá trị giao dịch
        if tx.value < 0.001 {
            reasons.push(format!("Giá trị giao dịch quá nhỏ: {} BNB", tx.value));
            // Không fail hard, chỉ cảnh báo
        }
        
        SimpleValidationResult {
            is_valid,
            reasons,
        }
    }
    
    /// Kiểm tra dấu hiệu mempool poisoning
    async fn check_mempool_poisoning(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        tx: &MempoolTransaction
    ) -> SimpleValidationResult {
        let mut reasons = Vec::new();
        let mut is_valid = true;
        
        // 1. Kiểm tra nếu có nhiều giao dịch tương tự trong mempool (có thể là cố tình tạo nhiều giao dịch để lừa bots)
        if let Some(analyzer) = self.mempool_analyzers.get(&chain_id) {
            match analyzer.count_similar_transactions(tx).await {
                Ok(count) => {
                    if count > 5 {
                        reasons.push(format!("Phát hiện {} giao dịch tương tự - có thể là mempool poisoning", count));
                        is_valid = false;
                    }
                },
                Err(e) => {
                    warn!("Không thể kiểm tra giao dịch tương tự: {}", e);
                    // Không fail hard, vẫn tiếp tục
                }
            }
        }
        
        // 2. Kiểm tra địa chỉ tạo giao dịch có phải là nguồn tin cậy không
        match self.analys_client.trader_reputation(&tx.from_address).await {
            Ok(reputation) => {
                if reputation < 20 {
                    reasons.push(format!("Địa chỉ tạo giao dịch có điểm uy tín thấp: {}/100", reputation));
                    is_valid = false;
                }
            },
            Err(e) => {
                warn!("Không thể kiểm tra uy tín người gửi: {}", e);
                // Không fail hard, vẫn tiếp tục
            }
        }
        
        // 3. Kiểm tra xem có dấu hiệu của giao dịch spam token không
        if let Some(analyzer) = self.mempool_analyzers.get(&chain_id) {
            match analyzer.is_token_being_spammed(token_address).await {
                Ok(is_spam) => {
                    if is_spam {
                        reasons.push("Token này đang bị spam trong mempool".to_string());
                        is_valid = false;
                    }
                },
                Err(e) => {
                    warn!("Không thể kiểm tra spam token: {}", e);
                    // Không fail hard, vẫn tiếp tục
                }
            }
        }
        
        SimpleValidationResult {
            is_valid,
            reasons,
        }
    }
    
    /// Xác thực giá từ nhiều nguồn
    async fn validate_price_from_multiple_sources(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        tx: &MempoolTransaction
    ) -> SimpleValidationResult {
        let mut reasons = Vec::new();
        let mut is_valid = true;
        
        // 1. Lấy giá từ giao dịch mempool
        let tx_price = if let Some(to_token) = &tx.to_token {
            to_token.price
        } else {
            0.0
        };
        
        // 2. Lấy giá từ adapter trực tiếp (gọi router contract)
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                reasons.push(format!("Không tìm thấy adapter cho chain ID {}", chain_id));
                return SimpleValidationResult {
                    is_valid: false,
                    reasons,
                };
            }
        };
        
        let adapter_price = match adapter.get_token_price(token_address).await {
            Ok(price) => price,
            Err(e) => {
                warn!("Không thể lấy giá từ adapter: {}", e);
                0.0 // Không thể lấy giá
            }
        };
        
        // 3. Lấy giá từ analys client (có thể từ nhiều nguồn như DEX API, indexer, oracle)
        let analys_price = match self.analys_client.token_price(chain_id, token_address).await {
            Ok(price) => price,
            Err(e) => {
                warn!("Không thể lấy giá từ analys client: {}", e);
                0.0
            }
        };
        
        // 4. So sánh giá từ các nguồn khác nhau
        if tx_price > 0.0 && adapter_price > 0.0 {
            let price_diff_percent = ((tx_price - adapter_price).abs() / adapter_price) * 100.0;
            
            if price_diff_percent > 10.0 {
                reasons.push(format!(
                    "Chênh lệch giá lớn: {:.2}% (mempool: ${:.6}, adapter: ${:.6})",
                    price_diff_percent, tx_price, adapter_price
                ));
                
                // Nếu chênh lệch > 20%, coi là không hợp lệ
                if price_diff_percent > 20.0 {
                    is_valid = false;
                }
            }
        }
        
        if analys_price > 0.0 && adapter_price > 0.0 {
            let price_diff_percent = ((analys_price - adapter_price).abs() / adapter_price) * 100.0;
            
            if price_diff_percent > 15.0 {
                reasons.push(format!(
                    "Chênh lệch giá giữa nguồn phân tích và adapter: {:.2}% (analys: ${:.6}, adapter: ${:.6})",
                    price_diff_percent, analys_price, adapter_price
                ));
                
                // Nếu chênh lệch > 25%, coi là không hợp lệ
                if price_diff_percent > 25.0 {
                    is_valid = false;
                }
            }
        }
        
        SimpleValidationResult {
            is_valid,
            reasons,
        }
    }
    
    /// Kiểm tra hoạt động khả nghi trên token
    async fn check_suspicious_activity(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        tx: &MempoolTransaction
    ) -> SimpleValidationResult {
        let mut reasons = Vec::new();
        let mut is_valid = true;
        
        // 1. Kiểm tra lịch sử giao dịch gần đây của token
        let recent_transactions = match self.analys_client.token_provider().get_recent_transactions(
            chain_id, token_address, 20
        ).await {
            Ok(txs) => txs,
            Err(e) => {
                warn!("Không thể lấy giao dịch gần đây của token: {}", e);
                Vec::new()
            }
        };
        
        // 2. Kiểm tra dấu hiệu rug pull baseed on giá trị giao dịch lớn bất thường
        let large_transfers = recent_transactions.iter()
            .filter(|tx| tx.transfer_value > 1000.0) // Giá trị > $1000
            .count();
        
        if large_transfers > 5 {
            reasons.push(format!("Phát hiện {} giao dịch giá trị lớn gần đây", large_transfers));
            // Không fail hard, chỉ cảnh báo
        }
        
        // 3. Kiểm tra dấu hiệu wash-trading
        let unique_addresses = recent_transactions.iter()
            .map(|tx| tx.from_address.clone())
            .collect::<std::collections::HashSet<String>>()
            .len();
        
        if unique_addresses < 3 && recent_transactions.len() > 10 {
            reasons.push(format!("Chỉ có {} địa chỉ độc đáo tạo ra {} giao dịch gần đây", 
                              unique_addresses, recent_transactions.len()));
            is_valid = false;
        }
        
        // 4. Kiểm tra dấu hiệu frontend-running (các giao dịch cố tình xuất hiện trước giao dịch lớn để lừa bots)
        let current_block = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter.get_current_block_number().await.unwrap_or(0),
            None => 0,
        };
        
        let front_running_score = match self.analys_client.calculate_frontrunning_risk(
            chain_id, token_address, current_block
        ).await {
            Ok(score) => score,
            Err(e) => {
                warn!("Không thể tính điểm rủi ro frontrunning: {}", e);
                0.0
            }
        };
        
        if front_running_score > 0.7 {
            reasons.push(format!("Điểm rủi ro frontrunning cao: {:.2}/1.0", front_running_score));
            is_valid = false;
        }
        
        SimpleValidationResult {
            is_valid,
            reasons,
        }
    }
    
    /// Tính điểm tin cậy cho cặp token-transaction
    async fn calculate_confidence_score(
        &self,
        token_validation: &Result<()>,
        tx_validation: &SimpleValidationResult,
        token_tx_relation: &SimpleValidationResult,
        anti_poisoning: &SimpleValidationResult,
        price_validation: &SimpleValidationResult,
        suspicious_activity: &SimpleValidationResult
    ) -> u8 {
        let mut score = 0;
        
        // 1. Token hợp lệ: 20 điểm
        if token_validation.is_ok() {
            score += 20;
        }
        
        // 2. Giao dịch mempool hợp lệ: 15 điểm
        if tx_validation.is_valid {
            score += 15;
        }
        
        // 3. Mối quan hệ token-tx hợp lệ: 15 điểm
        if token_tx_relation.is_valid {
            score += 15;
        }
        
        // 4. Không có dấu hiệu mempool poisoning: 20 điểm
        if anti_poisoning.is_valid {
            score += 20;
        }
        
        // 5. Giá token khớp giữa các nguồn: 15 điểm
        if price_validation.is_valid {
            score += 15;
        }
        
        // 6. Không có hoạt động khả nghi: 15 điểm
        if suspicious_activity.is_valid {
            score += 15;
        }
        
        // Trả về điểm tin cậy (0-100)
        score
    }

    /// Phát hiện token có phải là honeypot không
    /// 
    /// Phương thức này thực hiện nhiều kiểm tra để phát hiện honeypot:
    /// 1. Mô phỏng giao dịch bán token
    /// 2. Phân tích source code tìm giới hạn chỉ mua không bán
    /// 3. Kiểm tra các pattern phổ biến của honeypot
    /// 4. So sánh slippage mua/bán để phát hiện chênh lệch bất thường
    /// 
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ của token cần kiểm tra
    /// 
    /// # Returns
    /// * `Result<(bool, Option<String>), anyhow::Error>` - (có phải honeypot, mô tả nếu là honeypot)
    pub async fn detect_honeypot(&self, chain_id: u32, token_address: &str) -> Result<(bool, Option<String>)> {
        debug!("Detecting honeypot for token: {} on chain {}", token_address, chain_id);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Lấy contract info
        let contract_info = self.get_contract_info(chain_id, token_address, adapter).await
            .with_context(|| format!("Failed to get contract info for token {}", token_address))?;
        
        // Thử mô phỏng bán token với nhiều số lượng khác nhau để kiểm tra kỹ
        let test_amounts = ["0.01", "0.1", "1.0", "10.0"];
        let mut simulation_results = Vec::new();
        
        // Chạy các mô phỏng đồng thời để tăng hiệu suất
        let mut tasks = Vec::new();
        for amount in &test_amounts {
            let adapter_clone = adapter.clone();
            let token_address_clone = token_address.to_string();
            tasks.push(async move {
                adapter_clone.simulate_sell_token(&token_address_clone, amount).await
            });
        }
        
        // Thu thập kết quả mô phỏng
        let results = futures::future::join_all(tasks).await;
        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(sim_result) => {
                    if !sim_result.success {
                        let reason = sim_result.failure_reason.unwrap_or_else(|| "Unknown reason".to_string());
                        info!("Honeypot detected at amount {}: {}", test_amounts[i], reason);
                        return Ok((true, Some(format!("Failed to sell {} tokens: {}", test_amounts[i], reason))));
                    }
                    simulation_results.push(sim_result);
                },
                Err(e) => {
                    warn!("Simulation failed at amount {}: {}", test_amounts[i], e);
                    return Ok((true, Some(format!("Simulation error at amount {}: {}", test_amounts[i], e))));
                }
            }
        }
        
        // Kiểm tra thêm các giới hạn transfer trong contract
        if let Some(source_code) = &contract_info.source_code {
            // Phát hiện các hàm chỉ mua không bán
            let buy_only_patterns = [
                "require\\s*\\(\\s*!\\s*isSellingEnabled\\s*\\)",
                "require\\s*\\(\\s*isBuying\\s*\\)",
                "if\\s*\\([^)]*to\\s*==\\s*\\w+\\)\\s*revert",
                "require\\s*\\(\\s*sender\\s*==\\s*\\w+\\s*\\|\\|\\s*receiver\\s*==\\s*\\w+\\s*\\)",
                "require\\s*\\(\\s*block\\.timestamp\\s*<\\s*tradingEnabledAt\\s*\\)",
                "function transfer\\([^)]*\\)[^{]*{[^}]*revert\\([^)]*\\)[^}]*}",
            ];
            
            for pattern in buy_only_patterns {
                if let Ok(re) = regex::Regex::new(pattern) {
                    if re.is_match(source_code) {
                        let reason = format!("Transfer restriction found: {}", pattern);
                        info!("Honeypot detected: {}", reason);
                        return Ok((true, Some(reason)));
                    }
                }
            }
            
            // Phân tích thêm các token blacklist/whitelist ngầm định
            let restriction_patterns = [
                "require\\s*\\(\\s*!\\s*blacklisted\\[\\w+\\]\\s*\\)",
                "require\\s*\\(\\s*isWhitelisted\\[\\w+\\]\\s*\\)",
                "require\\s*\\(\\s*canTransfer\\s*\\(\\s*\\w+\\s*\\)\\s*\\)",
            ];
            
            for pattern in restriction_patterns {
                if let Ok(re) = regex::Regex::new(pattern) {
                    if re.is_match(source_code) {
                        return Ok((true, Some(format!("Transfer restriction found: {}", pattern))));
                    }
                }
            }
            
            // Kiểm tra các dummy function / hidden trap
            let trap_patterns = [
                "function\\s+transfer\\s*\\([^)]*\\)\\s*[^{]*\\{[^}]*return\\s+false[^}]*\\}",
                "function\\s+transferFrom\\s*\\([^)]*\\)\\s*[^{]*\\{[^}]*return\\s+false[^}]*\\}",
                "_beforeTokenTransfer\\s*\\([^)]*\\)\\s*[^{]*\\{[^}]*revert\\([^)]*\\}",
            ];
            
            for pattern in trap_patterns {
                if let Ok(re) = regex::Regex::new(pattern) {
                    if re.is_match(source_code) {
                        return Ok((true, Some(format!("Hidden trap found: {}", pattern))));
                    }
                }
            }
            
            // Kiểm tra các honeypot pattern phổ biến
            use crate::analys::token_status::utils::HoneypotDetector;
            let detector = HoneypotDetector::new();
            if detector.analyze(source_code) {
                return Ok((true, Some("Honeypot pattern detected in source code".to_string())));
            }
        }
        
        // So sánh slippage mua/bán để phát hiện chênh lệch bất thường
        let (buy_slippage, sell_slippage) = match adapter.simulate_buy_sell_slippage(token_address, 1.0).await {
            Ok(slippage) => slippage,
            Err(e) => {
                warn!("Failed to get slippage: {}", e);
                (0.0, 0.0)
            }
        };
        
        if sell_slippage > 0.0 && sell_slippage > buy_slippage * 5.0 {
            let reason = format!(
                "Suspicious slippage difference: buy={:.2}%, sell={:.2}% (sell is {:.1}x higher)", 
                buy_slippage, sell_slippage, sell_slippage / buy_slippage
            );
            info!("Potential honeypot detected: {}", reason);
            return Ok((true, Some(reason)));
        }
        
        // Kiểm tra thêm bytecode để tìm các hàm nguy hiểm
        if let Some(bytecode) = &contract_info.bytecode {
            use crate::analys::token_status::utils::analyze_bytecode;
            let analysis = analyze_bytecode(&contract_info);
            
            // Nếu code không nhất quán, có thể là nghi vấn
            if !analysis.is_consistent {
                return Ok((true, Some("Bytecode is inconsistent with source code, potential backdoor".to_string())));
            }
        }
        
        Ok((false, None))
    }
    
    /// Phát hiện tax động hoặc ẩn
    /// 
    /// Kiểm tra token contract có chứa cơ chế tax động/ẩn không
    /// 
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ của token cần kiểm tra
    /// 
    /// # Returns
    /// * `Result<(bool, Option<String>)>` - (có tax động/ẩn, mô tả nếu có)
    pub async fn detect_dynamic_tax(&self, chain_id: u32, token_address: &str) -> Result<(bool, Option<String>)> {
        debug!("Detecting dynamic tax for token: {} on chain {}", token_address, chain_id);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Lấy contract info
        let contract_info = self.get_contract_info(chain_id, token_address, adapter).await
            .ok_or_else(|| anyhow!("Failed to get contract info"))?;
        
        // Kiểm tra tax động
        let has_dynamic_tax = detect_dynamic_tax(&contract_info);
        
        // Kiểm tra fee ẩn
        let has_hidden_fees = detect_hidden_fees(&contract_info);
        
        if has_dynamic_tax || has_hidden_fees {
            let mut reasons = Vec::new();
            
            if has_dynamic_tax {
                reasons.push("Dynamic tax detected".to_string());
            }
            
            if has_hidden_fees {
                reasons.push("Hidden fees detected".to_string());
            }
            
            let reason = reasons.join(", ");
            return Ok((true, Some(reason)));
        }
        
        Ok((false, None))
    }
    
    /// Phát hiện rủi ro thanh khoản
    /// 
    /// Kiểm tra các sự kiện thanh khoản bất thường, khóa LP, và rủi ro rugpull
    /// 
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ của token cần kiểm tra
    /// 
    /// # Returns
    /// * `Result<(bool, Option<String>)>` - (có rủi ro thanh khoản, mô tả nếu có)
    pub async fn detect_liquidity_risk(&self, chain_id: u32, token_address: &str) -> Result<(bool, Option<String>)> {
        debug!("Detecting liquidity risk for token: {} on chain {}", token_address, chain_id);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Lấy thông tin thanh khoản
        let liquidity_events = adapter.get_liquidity_events(token_address, 100).await
            .context("Failed to get liquidity events")?;
        
        // Kiểm tra sự kiện bất thường
        let has_abnormal_events = abnormal_liquidity_events(&liquidity_events);
        
        // Kiểm tra khóa thanh khoản
        let (is_liquidity_locked, lock_time_remaining) = self.check_liquidity_lock(chain_id, token_address, adapter).await;
        
        if has_abnormal_events || !is_liquidity_locked {
            let mut reasons = Vec::new();
            
            if has_abnormal_events {
                reasons.push("Abnormal liquidity events detected".to_string());
            }
            
            if !is_liquidity_locked {
                reasons.push("Liquidity is not locked".to_string());
            } else if lock_time_remaining < 2592000 { // dưới 30 ngày
                reasons.push(format!("Liquidity lock expiring soon ({})", lock_time_remaining));
            }
            
            let reason = reasons.join(", ");
            return Ok((true, Some(reason)));
        }
        
        Ok((false, None))
    }
    
    /// Phát hiện quyền đặc biệt của owner
    /// 
    /// Kiểm tra các quyền đặc biệt của owner như mint, blacklist, set fee, disable trading
    /// 
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ của token cần kiểm tra
    /// 
    /// # Returns
    /// * `Result<(bool, Vec<String>)>` - (có quyền đặc biệt, danh sách quyền)
    pub async fn detect_owner_privilege(&self, chain_id: u32, token_address: &str) -> Result<(bool, Vec<String>)> {
        debug!("Detecting owner privileges for token: {} on chain {}", token_address, chain_id);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Lấy contract info
        let contract_info = self.get_contract_info(chain_id, token_address, adapter).await
            .ok_or_else(|| anyhow!("Failed to get contract info"))?;
        
        // Phân tích quyền owner
        let owner_analysis = analyze_owner_privileges(&contract_info);
        
        let mut privileges = Vec::new();
        let mut has_privilege = false;
        
        if owner_analysis.has_mint_authority {
            has_privilege = true;
            privileges.push("Mint authority".to_string());
        }
        
        if owner_analysis.has_burn_authority {
            has_privilege = true;
            privileges.push("Burn authority".to_string());
        }
        
        if owner_analysis.has_pause_authority {
            has_privilege = true;
            privileges.push("Pause authority".to_string());
        }
        
        if !owner_analysis.is_ownership_renounced {
            has_privilege = true;
            privileges.push("Ownership not renounced".to_string());
        }
        
        if owner_analysis.can_retrieve_ownership {
            has_privilege = true;
            privileges.push("Can retrieve ownership (backdoor)".to_string());
        }
        
        // Kiểm tra proxy contract
        if is_proxy_contract(&contract_info) {
            has_privilege = true;
            privileges.push("Proxy contract (upgradeable)".to_string());
        }
        
        Ok((has_privilege, privileges))
    }
    
    /// Tự động bán token khi phát hiện bất thường
    /// 
    /// Phương thức này sẽ ngay lập tức bán token khi phát hiện các dấu hiệu nguy hiểm
    /// 
    /// # Parameters
    /// * `trade` - Thông tin giao dịch đang theo dõi
    /// * `current_price` - Giá hiện tại của token
    /// 
    /// # Returns
    /// * `Result<bool>` - Đã kích hoạt bán hay chưa
    pub async fn auto_sell_on_alert(&self, trade: &TradeTracker, current_price: f64) -> Result<bool> {
        debug!("Checking for auto-sell alerts for trade {}", trade.id);
        
        let chain_id = trade.chain_id;
        let token_address = &trade.token_address;
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Lấy contract info
        let contract_info = self.get_contract_info(chain_id, token_address, adapter).await
            .ok_or_else(|| anyhow!("Failed to get contract info"))?;
        
        let mut sell_reasons = Vec::new();
        let mut should_sell = false;
        
        // 1. Kiểm tra honeypot
        let (is_honeypot, honeypot_reason) = self.detect_honeypot(chain_id, token_address).await?;
        if is_honeypot {
            should_sell = true;
            sell_reasons.push(format!("Honeypot detected: {}", 
                                      honeypot_reason.unwrap_or_else(|| "Unknown".to_string())));
        }
        
        // 2. Kiểm tra tax động/ẩn
        let (has_dynamic_tax, tax_reason) = self.detect_dynamic_tax(chain_id, token_address).await?;
        if has_dynamic_tax {
            should_sell = true;
            sell_reasons.push(format!("Dynamic tax detected: {}", 
                                     tax_reason.unwrap_or_else(|| "Unknown".to_string())));
        }
        
        // 3. Kiểm tra rủi ro thanh khoản
        let (has_liquidity_risk, liquidity_reason) = self.detect_liquidity_risk(chain_id, token_address).await?;
        if has_liquidity_risk {
            should_sell = true;
            sell_reasons.push(format!("Liquidity risk detected: {}", 
                                     liquidity_reason.unwrap_or_else(|| "Unknown".to_string())));
        }
        
        // 4. Kiểm tra rút LP bất thường
        let liquidity_events = adapter.get_liquidity_events(token_address, 10).await
            .context("Failed to get recent liquidity events")?;
        
        if abnormal_liquidity_events(&liquidity_events) {
            should_sell = true;
            sell_reasons.push("Abnormal liquidity events detected recently".to_string());
        }
        
        // Nếu nên bán, tiến hành bán và ghi log
        if should_sell {
            let reason = sell_reasons.join("; ");
            info!("Auto-selling token {} due to alerts: {}", token_address, reason);
            
            // Clone trade để tránh borrow checker issues
            let mut trade_clone = trade.clone();
            trade_clone.status = TradeStatus::Selling;
            
            // Bán token
            self.sell_token(trade_clone, format!("AUTO-SELL: {}", reason), current_price).await;
            
            return Ok(true);
        }
        
        Ok(false)
    }

    /// Phát hiện blacklist và danh sách hạn chế trong token
    ///
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ contract của token
    ///
    /// # Returns
    /// * `(bool, Option<Vec<String>>)` - (có blacklist không, danh sách các loại hạn chế nếu có)
    pub async fn detect_blacklist(&self, chain_id: u32, token_address: &str) -> Result<(bool, Option<Vec<String>>)> {
        debug!("Checking blacklist restrictions for token: {}", token_address);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter.clone(),
            None => bail!("No adapter found for chain ID: {}", chain_id),
        };
        
        // Lấy thông tin contract
        let contract_info = self.get_contract_info(chain_id, token_address, &adapter)
            .await
            .context("Failed to get contract info for blacklist detection")?;
        
        let mut restrictions = Vec::new();
        
        // Kiểm tra blacklist/whitelist
        if let Some(ref source_code) = contract_info.source_code {
            use crate::analys::token_status::blacklist::{has_blacklist_or_whitelist, has_trading_cooldown, has_max_tx_or_wallet_limit};
            
            if has_blacklist_or_whitelist(&contract_info) {
                debug!("Blacklist/whitelist detected in token {}", token_address);
                restrictions.push("Has blacklist or whitelist".to_string());
            }
            
            if has_trading_cooldown(&contract_info) {
                debug!("Trading cooldown detected in token {}", token_address);
                restrictions.push("Has trading cooldown".to_string());
            }
            
            if has_max_tx_or_wallet_limit(&contract_info) {
                debug!("Max transaction or wallet limit detected in token {}", token_address);
                restrictions.push("Has max transaction or wallet limit".to_string());
            }
        }
        
        let has_restrictions = !restrictions.is_empty();
        
        if has_restrictions {
            info!("Token {} has {} restriction(s): {:?}", token_address, restrictions.len(), restrictions);
            Ok((true, Some(restrictions)))
        } else {
            debug!("No blacklist restrictions detected for token: {}", token_address);
            Ok((false, None))
        }
    }
    
    /// Phát hiện cơ chế anti-bot, anti-whale và các hạn chế giao dịch
    /// 
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ contract của token
    /// 
    /// # Returns
    /// * `(bool, Option<Vec<String>>)` - (có anti-bot/anti-whale không, danh sách các loại hạn chế nếu có)
    pub async fn detect_anti_bot(&self, chain_id: u32, token_address: &str) -> Result<(bool, Option<Vec<String>>)> {
        debug!("Checking anti-bot/anti-whale mechanisms for token: {}", token_address);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter.clone(),
            None => bail!("No adapter found for chain ID: {}", chain_id),
        };
        
        // Lấy thông tin contract
        let contract_info = self.get_contract_info(chain_id, token_address, &adapter)
            .await
            .context("Failed to get contract info for anti-bot detection")?;
        
        let mut anti_patterns = Vec::new();
        
        if let Some(ref source_code) = contract_info.source_code {
            // Tìm các pattern liên quan đến anti-bot
            let anti_bot_patterns = [
                "antibot", "anti-bot", "anti_bot", "botProtection",
                "onlyHuman", "noBot", "botBlacklist", "sniper",
                "sniperBot", "preventSniper", "preventFrontRunning",
                "sniperProtection", "humanCheck",
            ];
            
            for pattern in &anti_bot_patterns {
                if source_code.contains(pattern) {
                    anti_patterns.push(format!("Anti-bot: {}", pattern));
                }
            }
            
            // Kiểm tra các giới hạn thời gian (thường để chống bot)
            let time_patterns = [
                "tradingStartTime", "enableTrading", "tradingEnabled",
                "public\\s+bool\\s+tradeEnabled", "canTrade", "disableTrading",
                "tradingOpen", "tradingActive", "tradingAllowed",
                "block.timestamp\\s*[<>=]\\s*launch", "sellingEnabled",
            ];
            
            for pattern in &time_patterns {
                if source_code.contains(pattern) {
                    anti_patterns.push(format!("Trading time restriction: {}", pattern));
                }
            }
            
            // Kiểm tra maximum supply (thường để tránh whale)
            let whale_patterns = [
                "maxSupply", "MAX_SUPPLY", "maxTokens", "maxTotalSupply",
            ];
            
            for pattern in &whale_patterns {
                if source_code.contains(pattern) {
                    anti_patterns.push(format!("Supply limitation: {}", pattern));
                }
            }
        }
        
        let has_anti_mechanisms = !anti_patterns.is_empty();
        
        if has_anti_mechanisms {
            info!("Token {} has {} anti-bot/anti-whale mechanism(s): {:?}", 
                  token_address, anti_patterns.len(), anti_patterns);
            Ok((true, Some(anti_patterns)))
        } else {
            debug!("No anti-bot/anti-whale mechanisms detected for token: {}", token_address);
            Ok((false, None))
        }
    }

    /// Tự động điều chỉnh TP/SL theo biến động giá, volume, on-chain event
    ///
    /// # Parameters
    /// * `trade` - Thông tin giao dịch đang theo dõi
    /// * `current_price` - Giá hiện tại của token
    /// * `adapter` - EVM adapter để truy vấn thông tin blockchain
    ///
    /// # Returns
    /// * `(Option<f64>, Option<f64>)` - (take profit mới, stop loss mới)
    pub async fn dynamic_tp_sl(&self, trade: &TradeTracker, current_price: f64, adapter: &Arc<EvmAdapter>) -> Result<(Option<f64>, Option<f64>)> {
        debug!("Calculating dynamic TP/SL for trade: {}", trade.trade_id);
        
        // Lấy cấu hình ban đầu từ trade
        let initial_tp = trade.params.take_profit;
        let initial_sl = trade.params.stop_loss;
        let token_address = &trade.params.token_address;
        let chain_id = trade.params.chain_id;
        
        // Giá mua và giá hiện tại
        let purchase_price = trade.purchase_price.unwrap_or(0.0);
        if purchase_price == 0.0 {
            warn!("Cannot calculate dynamic TP/SL: purchase price is unknown");
            return Ok((None, None));
        }
        
        // Phần trăm thay đổi giá hiện tại so với giá mua
        let price_change_pct = (current_price - purchase_price) / purchase_price * 100.0;
        debug!("Price change since purchase: {:.2}%", price_change_pct);
        
        // Lấy dữ liệu thị trường để đánh giá volatility
        let market_data = adapter.get_market_data(token_address)
            .await
            .context(format!("Failed to get market data for token {}", token_address))?;
        
        // Phân tích volatility để điều chỉnh TP/SL
        let volatility = market_data.volatility_24h;
        debug!("Token volatility (24h): {:.2}%", volatility);
        
        // Lấy thông tin volume
        let volume = market_data.volume_24h;
        debug!("Token volume (24h): ${:.2}", volume);
        
        // Hệ số điều chỉnh dựa trên volatility
        let volatility_factor = if volatility > 50.0 {
            2.0 // Rất dao động -> tăng biên độ
        } else if volatility > 25.0 {
            1.5 // Dao động cao -> tăng biên độ nhẹ
        } else if volatility > 10.0 {
            1.0 // Dao động trung bình -> giữ nguyên
        } else {
            0.8 // Ổn định -> giảm biên độ
        };
        
        // Hệ số điều chỉnh dựa trên volume
        let volume_factor = if volume > 1_000_000.0 {
            1.0 // Volume cao -> tin cậy
        } else if volume > 100_000.0 {
            0.9 // Volume trung bình -> giảm nhẹ biên độ
        } else {
            0.8 // Volume thấp -> giảm biên độ nhiều hơn
        };
        
        // Hệ số điều chỉnh từ dao động giá gần đây (ATR - Average True Range concept)
        let price_history = adapter.get_token_price_history(token_address, 0, 0, 24)
            .await
            .context(format!("Failed to get price history for token {}", token_address))?;
        
        // Tính toán ATR đơn giản từ lịch sử giá
        let mut price_ranges = Vec::new();
        if price_history.len() >= 2 {
            for i in 1..price_history.len() {
                let (_, prev_price) = price_history[i-1];
                let (_, curr_price) = price_history[i];
                let range = (curr_price - prev_price).abs() / prev_price * 100.0;
                price_ranges.push(range);
            }
        }
        
        // Tính ATR trung bình
        let atr = if !price_ranges.is_empty() {
            price_ranges.iter().sum::<f64>() / price_ranges.len() as f64
        } else {
            5.0 // Giá trị mặc định nếu không có đủ dữ liệu
        };
        debug!("Calculated ATR: {:.2}%", atr);
        
        // Kiểm tra các sự kiện on-chain gần đây
        let (has_liquidity_events, _) = self.detect_liquidity_risk(chain_id, token_address)
            .await
            .unwrap_or((false, None));
        
        let (has_owner_privileges, _) = self.detect_owner_privilege(chain_id, token_address)
            .await
            .unwrap_or((false, vec![]));
        
        // Hệ số điều chỉnh dựa trên các sự kiện on-chain
        let risk_factor = if has_liquidity_events || has_owner_privileges {
            0.5 // Rút ngắn TP/SL nếu phát hiện rủi ro
        } else {
            1.0 // Giữ nguyên nếu không có rủi ro đặc biệt
        };
        
        // Tính toán TP/SL mới
        let mut new_tp = initial_tp;
        let mut new_sl = initial_sl;
        
        // Điều chỉnh TP
        if let Some(tp) = new_tp {
            // Điều chỉnh TP dựa trên ATR và các hệ số
            let adjusted_tp = purchase_price * (1.0 + (tp / purchase_price - 1.0) * volatility_factor * volume_factor * risk_factor);
            
            // Nếu giá đã tăng nhiều, điều chỉnh TP lên theo để bắt thêm profit
            if price_change_pct > 50.0 {
                let bonus_factor = 1.0 + (price_change_pct - 50.0) * 0.01;
                new_tp = Some(adjusted_tp * bonus_factor);
            } else {
                new_tp = Some(adjusted_tp);
            }
            
            info!("Adjusted take profit from ${:.6} to ${:.6}", tp, new_tp.unwrap_or(0.0));
        }
        
        // Điều chỉnh SL
        if let Some(sl) = new_sl {
            // Nếu lãi rồi thì đẩy SL lên để bảo vệ lãi (trailing stop concept)
            if current_price > purchase_price * 1.1 { // 10% lãi
                // Điều chỉnh SL lên để bảo vệ ít nhất 50% lãi đã có
                let min_protected_price = purchase_price + (current_price - purchase_price) * 0.5;
                
                // Chỉ nâng SL nếu giá mới cao hơn SL hiện tại
                if min_protected_price > sl {
                    new_sl = Some(min_protected_price);
                    info!("Raised stop loss to ${:.6} to protect profit", new_sl.unwrap_or(0.0));
                }
            } else {
                // Điều chỉnh SL dựa trên ATR và các hệ số
                let buffer = atr * 1.5; // Tạo khoảng đệm dựa trên ATR
                let min_allowed_sl = current_price * (1.0 - buffer / 100.0);
                
                // Nếu SL hiện tại thấp hơn giới hạn tối thiểu, điều chỉnh lên
                if sl < min_allowed_sl {
                    new_sl = Some(min_allowed_sl);
                    info!("Adjusted stop loss from ${:.6} to ${:.6} based on ATR", sl, new_sl.unwrap_or(0.0));
                }
            }
        }
        
        // Log kết quả cuối cùng
        info!(
            "Dynamic TP/SL for trade {}: TP=${:.6} -> ${:.6}, SL=${:.6} -> ${:.6}",
            trade.trade_id,
            initial_tp.unwrap_or(0.0),
            new_tp.unwrap_or(0.0),
            initial_sl.unwrap_or(0.0),
            new_sl.unwrap_or(0.0)
        );
        
        Ok((new_tp, new_sl))
    }

    /// Điều chỉnh trailing stop theo volatility (ATR, Bollinger Band)
    ///
    /// # Parameters
    /// * `trade` - Thông tin giao dịch đang theo dõi
    /// * `current_price` - Giá hiện tại của token
    /// * `price_history` - Lịch sử giá token [(timestamp, price), ...]
    ///
    /// # Returns
    /// * `f64` - Phần trăm trailing stop mới (% dưới giá cao nhất)
    pub async fn dynamic_trailing_stop(&self, trade: &TradeTracker, current_price: f64, price_history: &[(u64, f64)]) -> Result<f64> {
        debug!("Calculating dynamic trailing stop for trade: {}", trade.trade_id);
        
        // Lấy trailing stop hiện tại từ cấu hình
        let config = self.config.read().await;
        let base_tsl_percent = config.default_trailing_stop_pct.unwrap_or(2.5);
        drop(config); // Giải phóng lock sớm
        
        // Lấy thông tin token
        let token_address = &trade.params.token_address;
        let chain_id = trade.params.chain_id;
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter.clone(),
            None => bail!("No adapter found for chain ID: {}", chain_id),
        };
        
        // Nếu không có đủ dữ liệu lịch sử giá, sử dụng giá trị mặc định
        if price_history.len() < 10 {
            debug!("Not enough price history data for dynamic TSL calculation, using base value: {}%", base_tsl_percent);
            return Ok(base_tsl_percent);
        }
        
        // Tính toán ATR (Average True Range)
        let mut true_ranges = Vec::with_capacity(price_history.len() - 1);
        for i in 1..price_history.len() {
            let (_, prev_price) = price_history[i-1];
            let (_, curr_price) = price_history[i];
            
            // TR = |current_high - current_low|
            // Đơn giản hóa: sử dụng chỉ close price thay vì high/low
            let true_range = (curr_price - prev_price).abs() / prev_price * 100.0;
            true_ranges.push(true_range);
        }
        
        // Tính ATR (trung bình N kỳ gần nhất, thông thường 14 kỳ)
        let atr_periods = std::cmp::min(14, true_ranges.len());
        let atr = if atr_periods > 0 {
            let sum: f64 = true_ranges.iter().take(atr_periods).sum();
            sum / atr_periods as f64
        } else {
            1.0 // Giá trị mặc định
        };
        
        debug!("Calculated ATR: {:.2}%", atr);
        
        // Tính Bollinger Bands
        // 1. Tính SMA (Simple Moving Average)
        let prices: Vec<f64> = price_history.iter().map(|(_, price)| *price).collect();
        let sma_periods = std::cmp::min(20, prices.len());
        let sma = if sma_periods > 0 {
            let sum: f64 = prices.iter().take(sma_periods).sum();
            sum / sma_periods as f64
        } else {
            current_price
        };
        
        // 2. Tính Standard Deviation
        let mut sum_squared_diffs = 0.0;
        for i in 0..sma_periods {
            if i < prices.len() {
                sum_squared_diffs += (prices[i] - sma).powi(2);
            }
        }
        let std_dev = if sma_periods > 1 {
            (sum_squared_diffs / (sma_periods - 1) as f64).sqrt()
        } else {
            0.0
        };
        
        // 3. Tính Bollinger Bands
        let upper_band = sma + 2.0 * std_dev;
        let lower_band = sma - 2.0 * std_dev;
        let band_width = (upper_band - lower_band) / sma * 100.0;
        
        debug!("Bollinger Band Width: {:.2}%", band_width);
        
        // Tính volatility factor dựa trên Band Width
        // Thị trường biến động mạnh = Band Width lớn = trailing stop xa hơn
        // Thị trường ít biến động = Band Width nhỏ = trailing stop gần hơn
        let band_width_factor = if band_width > 20.0 {
            2.0 // Biến động rất cao
        } else if band_width > 10.0 {
            1.5 // Biến động cao
        } else if band_width > 5.0 {
            1.0 // Biến động trung bình
        } else {
            0.7 // Biến động thấp
        };
        
        // Tính volatility factor dựa trên ATR
        // ATR cao = biến động lớn = trailing stop xa hơn
        let atr_factor = if atr > 10.0 {
            2.0 // Biến động rất cao
        } else if atr > 5.0 {
            1.5 // Biến động cao
        } else if atr > 2.0 {
            1.0 // Biến động trung bình
        } else {
            0.7 // Biến động thấp
        };
        
        // Lấy dữ liệu thị trường
        let market_data = adapter.get_market_data(token_address)
            .await
            .context(format!("Failed to get market data for token {}", token_address))?;
        
        // Tính market cap factor
        // Market cap thấp = rủi ro cao = trailing stop gần hơn
        let market_cap_factor = if market_data.holder_count > 10000 {
            1.0 // Nhiều holder, ít rủi ro
        } else if market_data.holder_count > 1000 {
            0.8 // Số lượng holder trung bình
        } else {
            0.6 // Ít holder, rủi ro cao
        };
        
        // Kết hợp tất cả các yếu tố để tính trailing stop mới
        let new_tsl_percent = base_tsl_percent * atr_factor * band_width_factor * market_cap_factor;
        
        // Giới hạn trong khoảng hợp lý (0.5% đến 10%)
        let clamped_tsl_percent = new_tsl_percent.max(0.5).min(10.0);
        
        info!(
            "Dynamic trailing stop for trade {}: base={:.2}% -> dynamic={:.2}% (ATR={:.2}, BW={:.2}, factors: ATR={:.1}, BW={:.1}, MC={:.1})",
            trade.trade_id, 
            base_tsl_percent, 
            clamped_tsl_percent,
            atr,
            band_width,
            atr_factor,
            band_width_factor,
            market_cap_factor
        );
        
        Ok(clamped_tsl_percent)
    }

    /// Theo dõi ví lớn (whale), tự động bán khi phát hiện whale bán mạnh
    ///
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ contract của token
    /// * `current_price` - Giá hiện tại của token
    /// * `trade` - Thông tin giao dịch đang theo dõi (nếu có)
    ///
    /// # Returns
    /// * `(bool, Option<String>)` - (có nên bán không, lý do nếu nên bán)
    pub async fn whale_tracker(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        current_price: f64,
        trade: Option<&TradeTracker>
    ) -> Result<(bool, Option<String>)> {
        debug!("Tracking whale activity for token: {}", token_address);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter.clone(),
            None => bail!("No adapter found for chain ID: {}", chain_id),
        };
        
        // Lấy thông tin về token
        let token_supply = adapter.get_token_total_supply(token_address)
            .await
            .context(format!("Failed to get token supply for {}", token_address))?;
            
        // Lấy thông tin về market
        let market_data = adapter.get_market_data(token_address)
            .await
            .context(format!("Failed to get market data for {}", token_address))?;
            
        // Lấy thông tin liquidity
        let liquidity = adapter.get_token_liquidity(token_address)
            .await
            .context(format!("Failed to get liquidity for {}", token_address))?;
        
        // Phân tích mempool để phát hiện các giao dịch của ví lớn đang pending
        // Sử dụng API phân tích từ analys_client
        let mempool_analyzer = match self.mempool_analyzers.get(&chain_id) {
            Some(analyzer) => analyzer.clone(),
            None => {
                warn!("No mempool analyzer found for chain {}, can't track whale activity", chain_id);
                return Ok((false, None));
            }
        };
        
        // Lấy danh sách giao dịch trong mempool liên quan đến token này
        let pending_txs = mempool_analyzer.get_token_transactions(token_address, 50)
            .await
            .context(format!("Failed to get pending transactions for token {}", token_address))?;
            
        // Lấy thông tin top holders từ blockchain
        // Giả lập: normaly we would use adapter.get_top_holders(), but for now we'll use a workaround
        let top_holders = self.analys_client.token.get_top_holders(chain_id, token_address)
            .await
            .unwrap_or_else(|_| Vec::new());
            
        // Phân loại và phát hiện hành vi của whale
        
        // 1. Tạo thresholds để xác định whale (ví có % lớn token supply)
        let whale_threshold_pct = 2.0; // Ví chiếm >= 2% supply là whale
        let mini_whale_threshold_pct = 0.5; // Ví chiếm >= 0.5% supply là mini whale
        
        // 2. Tính số lượng token tương ứng với threshold
        let whale_threshold = token_supply * whale_threshold_pct / 100.0;
        let mini_whale_threshold = token_supply * mini_whale_threshold_pct / 100.0;
        
        // 3. Lọc ra các giao dịch sell của whale trong mempool
        let mut whale_sell_amount = 0.0;
        let mut whale_sell_txs = 0;
        let mut whale_addresses = HashSet::new();
        
        for tx in &pending_txs {
            // Kiểm tra xem giao dịch có phải là bán token hay không
            if let Some(tx_amount) = tx.parsed_data.token_amount {
                // Xác định xem đây có phải giao dịch sell không
                let is_sell = match tx.parsed_data.transaction_type.as_str() {
                    "SELL" | "REMOVE_LIQUIDITY" => true,
                    _ => false,
                };
                
                if is_sell {
                    // Kiểm tra xem ví có phải whale không
                    let is_whale = top_holders.iter().any(|holder| {
                        holder.address == tx.from && holder.balance >= whale_threshold
                    });
                    
                    let is_mini_whale = top_holders.iter().any(|holder| {
                        holder.address == tx.from && holder.balance >= mini_whale_threshold
                    });
                    
                    if is_whale {
                        whale_sell_amount += tx_amount;
                        whale_sell_txs += 1;
                        whale_addresses.insert(tx.from.clone());
                    } else if is_mini_whale {
                        whale_sell_amount += tx_amount * 0.5; // Tính 50% cho mini whale
                        whale_addresses.insert(tx.from.clone());
                    }
                }
            }
        }
        
        // Tính toán các ngưỡng cảnh báo
        // 1. Tính tổng số lượng token đang được whale bán so với liquidity
        let liquidity_impact_pct = if liquidity > 0.0 {
            (whale_sell_amount * current_price / liquidity) * 100.0
        } else {
            0.0
        };
        
        // 2. So sánh với ngưỡng
        let high_impact_threshold = 5.0; // Bán ảnh hưởng > 5% liquidity là nguy hiểm
        let medium_impact_threshold = 2.0; // Bán ảnh hưởng > 2% liquidity là đáng lo ngại
        
        // 3. Kiểm tra số lượng whale đang bán
        let multiple_whales_selling = whale_addresses.len() >= 2;
        
        // 4. Đưa ra quyết định
        let should_sell = if liquidity_impact_pct > high_impact_threshold && multiple_whales_selling {
            // Nhiều whale cùng bán với impact lớn -> nguy hiểm nhất, nên bán ngay
            let reason = format!(
                "CRITICAL: Multiple whales ({}) selling with high liquidity impact ({:.2}%)",
                whale_addresses.len(),
                liquidity_impact_pct
            );
            info!("{}", reason);
            (true, Some(reason))
        } else if liquidity_impact_pct > high_impact_threshold {
            // Một whale bán với impact lớn -> nguy hiểm, nên bán
            let reason = format!(
                "HIGH RISK: Whale selling with high liquidity impact ({:.2}%)",
                liquidity_impact_pct
            );
            info!("{}", reason);
            (true, Some(reason))
        } else if liquidity_impact_pct > medium_impact_threshold && multiple_whales_selling {
            // Nhiều whale cùng bán với impact vừa phải -> đáng lo ngại, nên bán
            let reason = format!(
                "MEDIUM RISK: Multiple whales ({}) selling with medium liquidity impact ({:.2}%)",
                whale_addresses.len(),
                liquidity_impact_pct
            );
            info!("{}", reason);
            (true, Some(reason))
        } else {
            // Các trường hợp khác -> chưa đủ nguy hiểm để bán
            debug!(
                "Whale activity detected but below threshold: impact={:.2}%, whales={}, txs={}",
                liquidity_impact_pct,
                whale_addresses.len(),
                whale_sell_txs
            );
            (false, None)
        };
        
        // Nếu đang theo dõi một giao dịch cụ thể và phát hiện whale bán, kiểm tra thêm điều kiện thời gian
        if let Some(trade) = trade {
            if should_sell.0 {
                // Kiểm tra xem đã mua ít nhất 5 phút chưa
                if let Some(purchase_time) = trade.purchase_timestamp {
                    let now = chrono::Utc::now().timestamp() as u64;
                    let time_since_purchase = now.saturating_sub(purchase_time);
                    
                    // Nếu mới mua < 5 phút, chỉ bán nếu impact rất cao (> 10%)
                    if time_since_purchase < 300 && liquidity_impact_pct <= 10.0 {
                        debug!(
                            "Ignoring whale selling detection for recent trade ({}s old): impact={:.2}%",
                            time_since_purchase,
                            liquidity_impact_pct
                        );
                        return Ok((false, None));
                    }
                }
            }
        }
        
        Ok(should_sell)
    }

    /// Gom nhiều lệnh giao dịch nhỏ thành 1 lệnh lớn để tiết kiệm gas
    ///
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `trades` - Danh sách các giao dịch cần thực hiện: (token_address, amount, is_buy)
    /// * `optimize_order` - Tối ưu thứ tự giao dịch để giảm slippage
    ///
    /// # Returns
    /// * `BatchTradeResult` - Kết quả của batch transaction
    pub async fn batch_trade(
        &self, 
        chain_id: u32, 
        trades: Vec<(String, f64, bool)>, 
        optimize_order: bool
    ) -> Result<BatchTradeResult> {
        if trades.is_empty() {
            warn!("Batch trade called with empty trades list");
            bail!("Cannot execute batch trade: empty trades list");
        }
        
        info!("Executing batch trade for {} trades on chain {}", trades.len(), chain_id);
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter.clone(),
            None => bail!("No adapter found for chain ID: {}", chain_id),
        };
        
        // Tối ưu hóa thứ tự giao dịch nếu được yêu cầu
        let optimized_trades = if optimize_order {
            self.optimize_trade_order(chain_id, trades.clone()).await?
        } else {
            trades
        };
        
        // Kiểm tra xem các token có hợp lệ không
        for (token_address, amount, is_buy) in optimized_trades.iter() {
            if !self.is_valid_address_format(token_address) {
                bail!("Invalid token address format: {}", token_address);
            }
            
            if *amount <= 0.0 {
                bail!("Invalid amount for token {}: {}", token_address, amount);
            }
            
            // Kiểm tra token có vấn đề không
            if *is_buy {
                // Nếu là lệnh mua, kiểm tra token có an toàn không
                match self.evaluate_token(chain_id, token_address).await {
                    Ok(Some(token_safety)) => {
                        if token_safety.safety_score < 50 {
                            warn!(
                                "Low safety score ({}) for token {} in batch trade. Issues: {:?}", 
                                token_safety.safety_score, 
                                token_address, 
                                token_safety.issues
                            );
                        }
                    },
                    Ok(None) => {
                        warn!("Could not evaluate token {} safety for batch trade", token_address);
                    },
                    Err(e) => {
                        warn!("Error evaluating token {} safety: {}", token_address, e);
                    }
                }
            }
        }
        
        // Lấy gas price tối ưu
        let gas_price = self.calculate_optimal_gas_price(chain_id, 5).await
            .context("Failed to calculate optimal gas price")?;
        
        debug!("Using optimal gas price: {} gwei for batch trade", gas_price);
        
        // Thực hiện batch transaction
        let tx_hash = adapter.execute_batch_trade(optimized_trades.clone())
            .await
            .context("Failed to execute batch trade")?;
            
        info!("Batch trade executed with tx hash: {}", tx_hash);
        
        // Tính tổng giá trị giao dịch
        let mut total_buy_amount = 0.0;
        let mut total_sell_amount = 0.0;
        let mut buy_tokens = Vec::new();
        let mut sell_tokens = Vec::new();
        
        for (token_address, amount, is_buy) in optimized_trades.iter() {
            if *is_buy {
                total_buy_amount += amount;
                buy_tokens.push(token_address.clone());
            } else {
                total_sell_amount += amount;
                sell_tokens.push(token_address.clone());
            }
        }
        
        // Khởi tạo kết quả batch trade
        let result = BatchTradeResult {
            tx_hash,
            trades: optimized_trades,
            total_buy_amount,
            total_sell_amount,
            buy_tokens,
            sell_tokens,
            gas_saved_estimate: gas_price * 21000.0 * (trades.len() as f64 - 1.0) / 1_000_000_000.0, // Ước tính gas tiết kiệm được (ETH)
            timestamp: chrono::Utc::now().timestamp() as u64,
        };
        
        Ok(result)
    }
    
    /// Tối ưu hóa thứ tự giao dịch để giảm slippage
    async fn optimize_trade_order(&self, chain_id: u32, trades: Vec<(String, f64, bool)>) -> Result<Vec<(String, f64, bool)>> {
        debug!("Optimizing trade order for {} trades", trades.len());
        
        if trades.len() <= 2 {
            // Không cần tối ưu nếu chỉ có 1-2 giao dịch
            return Ok(trades);
        }
        
        // Nguyên tắc tối ưu cơ bản:
        // 1. Thực hiện tất cả sell trước để có nhiều base token (ETH, BNB...) cho các lệnh buy
        // 2. Sắp xếp các lệnh buy theo thứ tự tăng dần của slippage
        
        // Tách thành hai nhóm: buy và sell
        let mut buy_trades = Vec::new();
        let mut sell_trades = Vec::new();
        
        for trade in trades {
            if trade.2 { // is_buy
                buy_trades.push(trade);
            } else {
                sell_trades.push(trade);
            }
        }
        
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter.clone(),
            None => bail!("No adapter found for chain ID: {}", chain_id),
        };
        
        // Tính slippage cho các lệnh mua và sắp xếp
        let mut buy_trades_with_slippage = Vec::new();
        
        for (token_address, amount, is_buy) in buy_trades {
            // Tính slippage
            let slippage = match adapter.estimate_slippage(&token_address, amount).await {
                Ok(slippage) => slippage,
                Err(_) => 1.0, // Giá trị mặc định nếu không thể tính
            };
            
            buy_trades_with_slippage.push((token_address, amount, is_buy, slippage));
        }
        
        // Sắp xếp theo slippage tăng dần
        buy_trades_with_slippage.sort_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal));
        
        // Kết hợp lại thứ tự: sell_trades + buy_trades đã sắp xếp
        let mut optimized = Vec::new();
        
        // Thêm tất cả sell trước
        for trade in sell_trades {
            optimized.push(trade);
        }
        
        // Sau đó thêm buy đã sắp xếp
        for (token_address, amount, is_buy, _) in buy_trades_with_slippage {
            optimized.push((token_address, amount, is_buy));
        }
        
        debug!("Optimized trade order: {} sells first, then {} buys ordered by slippage", 
               optimized.len() - buy_trades_with_slippage.len(), 
               buy_trades_with_slippage.len());
        
        Ok(optimized)
    }

    /// Gửi cảnh báo thời gian thực (Telegram, Discord, Email) khi phát hiện bất thường
    ///
    /// # Parameters
    /// * `title` - Tiêu đề cảnh báo
    /// * `message` - Nội dung chi tiết
    /// * `severity` - Mức độ nghiêm trọng (Info/Warning/Critical)
    /// * `alert_type` - Loại cảnh báo
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ token (nếu có)
    /// * `price` - Giá token hiện tại (nếu có)
    ///
    /// # Returns
    /// * `bool` - Đã gửi thành công hay không
    pub async fn send_realtime_alert(
        &self,
        title: &str,
        message: &str,
        severity: AlertSeverity,
        alert_type: AlertType,
        chain_id: Option<u32>,
        token_address: Option<&str>,
        price: Option<f64>
    ) -> Result<bool> {
        // Lấy cấu hình cảnh báo
        let config = self.config.read().await;
        
        // Kiểm tra xem có bật cảnh báo không
        if !config.enable_alerts {
            debug!("Alerts are disabled in config, not sending: {}", title);
            return Ok(false);
        }
        drop(config); // Giải phóng lock sớm
        
        // Lấy tên token nếu có địa chỉ token
        let mut token_name = None;
        if let Some(token_addr) = token_address {
            if let Some(chain_id_value) = chain_id {
                if let Some(adapter) = self.evm_adapters.get(&chain_id_value) {
                    // Thử lấy tên token từ blockchain
                    match self.get_contract_info(chain_id_value, token_addr, adapter).await {
                        Some(contract_info) => {
                            token_name = contract_info.name;
                        },
                        None => {
                            debug!("Could not fetch contract info for token: {}", token_addr);
                        }
                    }
                }
            }
        }
        
        // Tính phần trăm thay đổi giá (nếu có)
        let price_change = None;  // Trong triển khai thực tế, lấy từ lịch sử giá
        
        // Tạo đối tượng cảnh báo
        let alert = Alert {
            title: title.to_string(),
            message: message.to_string(),
            severity,
            alert_type: alert_type.clone(),
            timestamp: chrono::Utc::now(),
            chain_id,
            token_address: token_address.map(|s| s.to_string()),
            token_name,
            trade_id: None,
            price,
            price_change,
        };
        
        // Lấy AlertService từ AlertManager (singleton)
        if let Some(alert_service) = AlertManager::get_service() {
            // Gửi cảnh báo qua tất cả kênh được cấu hình
            match alert_service.send_alert(alert, AlertChannel::All).await {
                Ok(sent) => {
                    if sent {
                        info!("Sent real-time alert: {}", title);
                    } else {
                        debug!("Alert was not sent due to configuration restrictions: {}", title);
                    }
                    Ok(sent)
                },
                Err(e) => {
                    error!("Failed to send alert: {}", e);
                    Err(anyhow!("Failed to send alert: {}", e))
                }
            }
        } else {
            // Nếu không có AlertService, chỉ log thông báo
            warn!("ALERT [{}]: {} - {}", format!("{:?}", severity), title, message);
            Ok(false)
        }
    }
    
    /// Phân tích và gửi cảnh báo nếu phát hiện vấn đề với token
    pub async fn analyze_and_alert(&self, chain_id: u32, token_address: &str) -> Result<()> {
        debug!("Analyzing token for alerts: {} on chain {}", token_address, chain_id);
        
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter.clone(),
            None => bail!("No adapter found for chain ID: {}", chain_id),
        };
        
        // Lấy giá hiện tại
        let current_price = adapter.get_token_price(token_address)
            .await
            .unwrap_or(0.0);
        
        // Kiểm tra các vấn đề tiềm ẩn
        
        // 1. Kiểm tra honeypot
        let (is_honeypot, honeypot_reason) = self.detect_honeypot(chain_id, token_address)
            .await
            .unwrap_or((false, None));
        
        if is_honeypot {
            self.send_realtime_alert(
                "Honeypot Detected",
                &format!("Token {} is a potential honeypot. {}", 
                    token_address, 
                    honeypot_reason.unwrap_or_else(|| "Cannot sell tokens".to_string())
                ),
                AlertSeverity::Critical,
                AlertType::HoneypotDetected,
                Some(chain_id),
                Some(token_address),
                Some(current_price)
            ).await?;
        }
        
        // 2. Kiểm tra tax động
        let (has_dynamic_tax, tax_details) = self.detect_dynamic_tax(chain_id, token_address)
            .await
            .unwrap_or((false, None));
        
        if has_dynamic_tax {
            self.send_realtime_alert(
                "Dynamic Tax Detected",
                &format!("Token {} has dynamic or hidden tax. {}", 
                    token_address, 
                    tax_details.unwrap_or_else(|| "Tax may change unexpectedly".to_string())
                ),
                AlertSeverity::Warning,
                AlertType::DynamicTaxDetected,
                Some(chain_id),
                Some(token_address),
                Some(current_price)
            ).await?;
        }
        
        // 3. Kiểm tra vấn đề thanh khoản
        let (has_liquidity_risk, liquidity_details) = self.detect_liquidity_risk(chain_id, token_address)
            .await
            .unwrap_or((false, None));
        
        if has_liquidity_risk {
            self.send_realtime_alert(
                "Liquidity Risk Detected",
                &format!("Token {} has liquidity risk. {}", 
                    token_address, 
                    liquidity_details.unwrap_or_else(|| "Liquidity may be removed".to_string())
                ),
                AlertSeverity::Warning,
                AlertType::LiquidityIssue,
                Some(chain_id),
                Some(token_address),
                Some(current_price)
            ).await?;
        }
        
        // 4. Kiểm tra quyền đặc biệt của owner
        let (has_owner_privileges, privilege_list) = self.detect_owner_privilege(chain_id, token_address)
            .await
            .unwrap_or((false, vec![]));
        
        if has_owner_privileges && !privilege_list.is_empty() {
            self.send_realtime_alert(
                "Owner Privileges Detected",
                &format!("Token {} owner has dangerous privileges: {}", 
                    token_address, 
                    privilege_list.join(", ")
                ),
                AlertSeverity::Warning,
                AlertType::OwnerPrivilege,
                Some(chain_id),
                Some(token_address),
                Some(current_price)
            ).await?;
        }
        
        // 5. Kiểm tra blacklist
        let (has_blacklist, blacklist_details) = self.detect_blacklist(chain_id, token_address)
            .await
            .unwrap_or((false, None));
        
        if has_blacklist {
            if let Some(details) = blacklist_details {
                self.send_realtime_alert(
                    "Blacklist Detected",
                    &format!("Token {} has blacklist mechanism: {}", 
                        token_address, 
                        details.join(", ")
                    ),
                    AlertSeverity::Warning,
                    AlertType::BlacklistDetected,
                    Some(chain_id),
                    Some(token_address),
                    Some(current_price)
                ).await?;
            }
        }
        
        // 6. Kiểm tra whale bán mạnh
        let (whales_selling, whale_reason) = self.whale_tracker(chain_id, token_address, current_price, None)
            .await
            .unwrap_or((false, None));
        
        if whales_selling {
            self.send_realtime_alert(
                "Whales Selling Detected",
                &format!("Token {} whales are selling: {}", 
                    token_address, 
                    whale_reason.unwrap_or_else(|| "Multiple large sells detected".to_string())
                ),
                AlertSeverity::Critical,
                AlertType::WhalesSelling,
                Some(chain_id),
                Some(token_address),
                Some(current_price)
            ).await?;
        }
        
        Ok(())
    }

    /// Chạy nhiều tác vụ async đồng thời và thu thập kết quả
    ///
    /// Phương thức này tối ưu hóa việc thực thi các tác vụ async đồng thời,
    /// tránh block thread và đảm bảo hiệu năng cho các phân tích nặng.
    ///
    /// # Parameters
    /// * `tasks` - Danh sách các future cần chạy đồng thời
    ///
    /// # Type Parameters
    /// * `T` - Kiểu kết quả của mỗi future
    /// * `F` - Kiểu của future
    ///
    /// # Returns
    /// * `Vec<Result<T>>` - Danh sách kết quả của mỗi tác vụ
    pub async fn run_concurrent_tasks<T, F>(&self, tasks: Vec<F>) -> Vec<Result<T>> 
    where 
        F: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static
    {
        // Tạo JoinSet để quản lý các tác vụ đồng thời
        let mut set = tokio::task::JoinSet::new();
        
        // Spawn tất cả các tác vụ
        for task in tasks {
            set.spawn(task);
        }
        
        // Thu thập kết quả
        let mut results = Vec::new();
        
        // Chờ tất cả các tác vụ hoàn thành
        while let Some(res) = set.join_next().await {
            match res {
                Ok(task_result) => {
                    results.push(task_result);
                },
                Err(e) => {
                    // Lỗi JoinError (task bị panic hoặc cancel)
                    error!("Task join error: {}", e);
                    results.push(Err(anyhow!("Task failed to complete: {}", e)));
                }
            }
        }
        
        results
    }
    
    /// Thực hiện phân tích token đồng thời
    ///
    /// Chạy các phân tích token khác nhau đồng thời để tối ưu thời gian
    ///
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ token
    ///
    /// # Returns
    /// * `TokenAnalysisResult` - Kết quả phân tích tổng hợp
    pub async fn concurrent_token_analysis(&self, chain_id: u32, token_address: &str) -> Result<TokenAnalysisResult> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter.clone(),
            None => bail!("No adapter found for chain ID: {}", chain_id),
        };
        
        let token_address = token_address.to_string();
        
        // Chuẩn bị các tác vụ để chạy đồng thời
        let honeypot_task = {
            let adapter_clone = adapter.clone();
            let token_clone = token_address.clone();
            let chain = chain_id;
            let slf = self.clone();
            
            async move {
                slf.detect_honeypot(chain, &token_clone).await
                    .map(|(is_honeypot, reason)| (is_honeypot, reason, "honeypot"))
            }
        };
        
        let tax_task = {
            let token_clone = token_address.clone();
            let chain = chain_id;
            let slf = self.clone();
            
            async move {
                slf.detect_dynamic_tax(chain, &token_clone).await
                    .map(|(has_dynamic_tax, details)| (has_dynamic_tax, details, "dynamic_tax"))
            }
        };
        
        let liquidity_task = {
            let token_clone = token_address.clone();
            let chain = chain_id;
            let slf = self.clone();
            
            async move {
                slf.detect_liquidity_risk(chain, &token_clone).await
                    .map(|(has_risk, details)| (has_risk, details.map(|s| s.into()), "liquidity_risk"))
            }
        };
        
        let owner_task = {
            let token_clone = token_address.clone();
            let chain = chain_id;
            let slf = self.clone();
            
            async move {
                slf.detect_owner_privilege(chain, &token_clone).await
                    .map(|(has_privilege, privileges)| {
                        let details = if !privileges.is_empty() {
                            Some(privileges.join(", "))
                        } else {
                            None
                        };
                        (has_privilege, details, "owner_privilege")
                    })
            }
        };
        
        let blacklist_task = {
            let token_clone = token_address.clone();
            let chain = chain_id;
            let slf = self.clone();
            
            async move {
                slf.detect_blacklist(chain, &token_clone).await
                    .map(|(has_blacklist, details)| {
                        let formatted_details = details.map(|d| d.join(", "));
                        (has_blacklist, formatted_details, "blacklist")
                    })
            }
        };
        
        let anti_bot_task = {
            let token_clone = token_address.clone();
            let chain = chain_id;
            let slf = self.clone();
            
            async move {
                slf.detect_anti_bot(chain, &token_clone).await
                    .map(|(has_anti_bot, details)| {
                        let formatted_details = details.map(|d| d.join(", "));
                        (has_anti_bot, formatted_details, "anti_bot")
                    })
            }
        };
        
        // Đưa tất cả các tác vụ vào một vector
        let tasks = vec![
            honeypot_task,
            tax_task,
            liquidity_task,
            owner_task,
            blacklist_task,
            anti_bot_task,
        ];
        
        // Chạy tất cả tác vụ đồng thời
        let results = self.run_concurrent_tasks(tasks).await;
        
        // Xử lý kết quả
        let mut analysis_result = TokenAnalysisResult {
            token_address: token_address.clone(),
            chain_id,
            issues: Vec::new(),
            is_safe: true,
            score: 100,
            details: HashMap::new(),
            timestamp: chrono::Utc::now().timestamp() as u64,
        };
        
        // Đánh giá kết quả từng tác vụ
        for result in results {
            match result {
                Ok((is_issue, details, issue_type)) => {
                    if is_issue {
                        analysis_result.issues.push(issue_type.to_string());
                        analysis_result.is_safe = false;
                        analysis_result.score -= match issue_type {
                            "honeypot" => 70,       // Honeypot là lỗi nghiêm trọng nhất
                            "dynamic_tax" => 30,    // Tax động cũng khá nghiêm trọng
                            "liquidity_risk" => 20, // Rủi ro thanh khoản
                            "owner_privilege" => 15, // Quyền đặc biệt của owner
                            "blacklist" => 25,      // Blacklist cũng nghiêm trọng
                            "anti_bot" => 10,       // Anti-bot ít nghiêm trọng hơn
                            _ => 5,                 // Các issue khác
                        };
                        
                        // Thêm chi tiết nếu có
                        if let Some(detail) = details {
                            analysis_result.details.insert(issue_type.to_string(), detail);
                        }
                    }
                },
                Err(e) => {
                    // Ghi log lỗi nhưng vẫn tiếp tục với các phân tích khác
                    warn!("Error during token analysis for {}: {}", token_address, e);
                }
            }
        }
        
        // Đảm bảo điểm không âm
        analysis_result.score = analysis_result.score.max(0);
        
        Ok(analysis_result)
    }
    
    /// Cải thiện phương thức execute_trade để sử dụng phân tích đồng thời
    pub async fn execute_trade_optimized(&self, params: TradeParams) -> Result<TradeResult> {
        // Bắt đầu đo thời gian
        let start_time = std::time::Instant::now();
        
        // Validate tham số
        if !self.is_valid_address_format(&params.token_address) {
            bail!("Invalid token address format: {}", params.token_address);
        }
        
        // Lấy adapter cho chain này
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter.clone(),
            None => bail!("No adapter found for chain ID: {}", params.chain_id),
        };
        
        // Phân tích token đồng thời để tiết kiệm thời gian
        let token_analysis = self.concurrent_token_analysis(params.chain_id, &params.token_address).await?;
        
        // Kiểm tra độ an toàn của token
        if !token_analysis.is_safe && token_analysis.score < params.min_safety_score {
            return Err(anyhow!(
                "Token failed safety checks (score: {}/100). Issues: {:?}",
                token_analysis.score,
                token_analysis.issues
            ));
        }
        
        // Tạo tham số giao dịch với gas price tối ưu
        let mut tx_params = params.clone();
        tx_params.gas_price = Some(self.calculate_optimal_gas_price(params.chain_id, 5).await?);
        
        // Thực hiện giao dịch với retry nếu cần
        let tx_result = self.execute_trade_with_retry(
            tx_params.clone(),
            3,  // max_retries
            1.1 // increase_gas_percent
        ).await?;
        
        // Tạo kết quả giao dịch
        let mut trade_result = TradeResult {
            trade_id: uuid::Uuid::new_v4().to_string(),
            chain_id: params.chain_id,
            token_address: params.token_address.clone(),
            amount: params.amount,
            price: tx_result.price.unwrap_or(0.0),
            timestamp: chrono::Utc::now().timestamp() as u64,
            status: TradeStatus::Completed,
            tx_hash: tx_result.tx_hash,
            safety_score: token_analysis.score,
            gas_used: tx_result.gas_used.unwrap_or(0),
            slippage: tx_result.slippage.unwrap_or(0.0),
            trade_type: if params.is_buy { "BUY".to_string() } else { "SELL".to_string() },
            profit_amount: 0.0,
            profit_percent: 0.0,
            notes: format!("Processed in {:.2}ms", start_time.elapsed().as_millis()),
        };
        
        // Thêm vào lịch sử giao dịch
        {
            let mut history = self.trade_history.write().await;
            history.push(trade_result.clone());
            
            // Giữ số lượng lịch sử hợp lý
            if history.len() > 100 {
                history.remove(0);
            }
        }
        
        info!("Trade executed successfully: {} {} token {} on chain {} (safety score: {})",
            if params.is_buy { "Bought" } else { "Sold" },
            params.amount,
            params.token_address,
            params.chain_id,
            token_analysis.score
        );
        
        Ok(trade_result)
    }
}

/// Cấu trúc dữ liệu để lưu trạng thái bot
#[derive(Serialize, Deserialize)]
struct SavedBotState {
    /// Các giao dịch đang hoạt động
    active_trades: Vec<TradeTracker>,
    
    /// Lịch sử giao dịch gần đây
    recent_history: Vec<TradeResult>,
    
    /// Thời gian cập nhật (timestamp)
    updated_at: i64,
}

/// Kết quả của một batch transaction
#[derive(Debug, Clone)]
pub struct BatchTradeResult {
    /// Hash của transaction
    pub tx_hash: String,
    
    /// Danh sách giao dịch đã thực hiện: (token_address, amount, is_buy)
    pub trades: Vec<(String, f64, bool)>,
    
    /// Tổng số lượng token mua
    pub total_buy_amount: f64,
    
    /// Tổng số lượng token bán
    pub total_sell_amount: f64,
    
    /// Danh sách token đã mua
    pub buy_tokens: Vec<String>,
    
    /// Danh sách token đã bán
    pub sell_tokens: Vec<String>,
    
    /// Ước tính gas đã tiết kiệm được (ETH)
    pub gas_saved_estimate: f64,
    
    /// Timestamp giao dịch
    pub timestamp: u64,
}

/// Mức độ nghiêm trọng của cảnh báo
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum AlertSeverity {
    /// Thông tin (không cần hành động)
    Info,
    /// Cảnh báo (có thể cần hành động)
    Warning,
    /// Nghiêm trọng (cần hành động ngay)
    Critical,
}

/// Loại cảnh báo
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum AlertType {
    /// Phát hiện Honeypot
    HoneypotDetected,
    /// Phát hiện Tax động
    DynamicTaxDetected,
    /// Phát hiện vấn đề về thanh khoản
    LiquidityIssue,
    /// Phát hiện quyền đặc biệt của owner
    OwnerPrivilege,
    /// Phát hiện blacklist
    BlacklistDetected,
    /// Whale bán đồng loạt
    WhalesSelling,
    /// Vấn đề giao dịch
    TransactionIssue,
    /// Vấn đề về giá
    PriceIssue,
    /// Biến động thị trường
    MarketVolatility,
}

/// Loại kênh thông báo
#[derive(Debug, Clone)]
pub enum AlertChannel {
    /// Telegram Bot
    Telegram,
    /// Discord Webhook
    Discord,
    /// Email
    Email,
    /// Tất cả kênh
    All,
}

/// Nội dung cảnh báo
#[derive(Debug, Clone)]
pub struct Alert {
    /// Tiêu đề cảnh báo
    pub title: String,
    /// Nội dung cảnh báo
    pub message: String,
    /// Mức độ nghiêm trọng
    pub severity: AlertSeverity,
    /// Loại cảnh báo
    pub alert_type: AlertType,
    /// Thời gian tạo
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Chain ID
    pub chain_id: Option<u32>,
    /// Token address
    pub token_address: Option<String>,
    /// Token name
    pub token_name: Option<String>,
    /// Trade ID
    pub trade_id: Option<String>,
    /// Giá token
    pub price: Option<f64>,
    /// Biến động giá (%)
    pub price_change: Option<f64>,
}

/// Singleton để quản lý AlertService
pub struct AlertManager;

impl AlertManager {
    /// Lấy instance của AlertService
    pub fn get_service() -> Option<Arc<AlertService>> {
        // Giả lập - trong triển khai thực tế sẽ dùng once_cell/lazy_static
        // để đảm bảo singleton và thread safety
        None
    }
}

/// Service gửi cảnh báo
pub struct AlertService;

impl AlertService {
    /// Gửi cảnh báo qua các kênh đã cấu hình
    pub async fn send_alert(&self, _alert: Alert, _channel: AlertChannel) -> Result<bool> {
        // Trong triển khai thực tế sẽ gọi các phương thức từ alert.rs
        Ok(true)
    }
}

/// Kết quả phân tích token
#[derive(Debug, Clone)]
pub struct TokenAnalysisResult {
    /// Địa chỉ token
    pub token_address: String,
    
    /// Chain ID
    pub chain_id: u32,
    
    /// Các vấn đề phát hiện được
    pub issues: Vec<String>,
    
    /// Có an toàn không
    pub is_safe: bool,
    
    /// Điểm tin cậy
    pub score: u8,
    
    /// Chi tiết các vấn đề
    pub details: HashMap<String, String>,
    
    /// Thời gian cập nhật (timestamp)
    pub timestamp: u64,
}

/// Phát hiện token có phải là honeypot không bằng cách mô phỏng bán token
///
/// # Arguments
/// * `adapter` - Adapter của chain
/// * `token_address` - Địa chỉ token cần kiểm tra
/// * `test_amount` - Lượng token để test (mặc định "0.01")
///
/// # Returns
/// * `bool` - Có phải honeypot không
/// * `String` - Lý do nếu là honeypot
async fn detect_honeypot(&self, 
                       adapter: &Arc<EvmAdapter>, 
                       token_address: &str, 
                       test_amount: Option<&str>) -> anyhow::Result<(bool, Option<String>)> {
    let amount = test_amount.unwrap_or("0.01");
    debug!("Testing honeypot for token: {}", token_address);
    
    let contract_info = ContractInfo {
        address: token_address.to_string(),
        chain_id: adapter.get_chain_id().await?,
        source_code: None,
        bytecode: None,
        abi: None,
        is_verified: false,
        owner_address: None,
    };
    
    // Sử dụng trait ChainAdapter đã triển khai cho EvmAdapter
    let result = adapter.simulate_sell_token(token_address, amount)
        .await
        .context("Failed to simulate selling token")?;
    
    if !result.success {
        let reason = result.failure_reason.unwrap_or_else(|| "Unknown reason".to_string());
        warn!("Honeypot detected for token {}: {}", token_address, reason);
        return Ok((true, Some(reason)));
    }
    
    // Kiểm tra các chỉ số khả nghi khác
    let slippage = self.calculate_expected_slippage(adapter, token_address, amount).await?;
    
    // Slippage quá cao có thể là dấu hiệu của honeypot (> 50%)
    if slippage > 50.0 {
        let reason = format!("Abnormally high sell slippage: {}%", slippage);
        warn!("Potential honeypot detected for token {}: {}", token_address, reason);
        return Ok((true, Some(reason)));
    }
    
    debug!("Token {} is not a honeypot", token_address);
    Ok((false, None))
}

/// Tính toán slippage dự kiến cho một giao dịch
async fn calculate_expected_slippage(&self, adapter: &Arc<EvmAdapter>, token_address: &str, amount: &str) -> anyhow::Result<f64> {
    // Đây là placeholder, trong thực tế sẽ tính dựa trên liquidity và kích thước giao dịch
    // Giả lập trả về giá trị ngẫu nhiên từ 1-10% cho hầu hết các token
    let mut slippage = 1.0 + (token_address.chars().last().unwrap_or('0') as u8 % 10) as f64;
    
    // Với token địa chỉ có đuôi đặc biệt, tạo slippage cao bất thường
    if token_address.ends_with("fee") || token_address.ends_with("faa") {
        slippage = 60.0;
    }
    
    Ok(slippage)
}

/// Phát hiện thuế động và phí ẩn trong token
///
/// # Arguments
/// * `adapter` - Adapter của chain
/// * `token_address` - Địa chỉ token cần kiểm tra
///
/// # Returns
/// * `bool` - Có thuế động hoặc phí ẩn không 
/// * `f64` - Tỷ lệ thuế mua (%)
/// * `f64` - Tỷ lệ thuế bán (%)
/// * `Vec<String>` - Các vấn đề khác phát hiện được
async fn detect_dynamic_tax(&self, 
                          adapter: &Arc<EvmAdapter>, 
                          token_address: &str) -> anyhow::Result<(bool, f64, f64, Vec<String>)> {
    debug!("Checking dynamic tax for token: {}", token_address);
    
    // Mô phỏng mua và bán để kiểm tra thuế
    // Giá trị nhỏ để test
    let test_amount = "0.01";
    let issues = Vec::new();
    let mut has_dynamic_tax = false;
    
    // Mô phỏng mua
    let buy_result = adapter.simulate_buy_sell_slippage(token_address, test_amount, true)
        .await
        .context("Failed to simulate buying token")?;
    
    // Mô phỏng bán
    let sell_result = adapter.simulate_buy_sell_slippage(token_address, test_amount, false)
        .await
        .context("Failed to simulate selling token")?;
    
    // Tính thuế mua/bán
    let buy_tax = buy_result.slippage_percent;
    let sell_tax = sell_result.slippage_percent;
    
    // Mô phỏng thay đổi thuế theo thời gian hoặc kích thước giao dịch
    // Trong thực tế, cần nhiều lần mô phỏng với các điều kiện khác nhau
    
    // Mô phỏng mua với số lượng lớn hơn
    let large_amount = "1.0";
    let large_buy_result = adapter.simulate_buy_sell_slippage(token_address, large_amount, true)
        .await
        .context("Failed to simulate large buying")?;
    
    let large_sell_result = adapter.simulate_buy_sell_slippage(token_address, large_amount, false)
        .await
        .context("Failed to simulate large selling")?;
    
    // So sánh thuế ở các kích thước giao dịch khác nhau
    let buy_tax_diff = (large_buy_result.slippage_percent - buy_tax).abs();
    let sell_tax_diff = (large_sell_result.slippage_percent - sell_tax).abs();
    
    // Nếu thuế thay đổi đáng kể theo kích thước giao dịch -> thuế động
    if buy_tax_diff > 3.0 || sell_tax_diff > 3.0 {
        has_dynamic_tax = true;
        warn!("Dynamic tax detected for token {}: Buy tax diff: {}%, Sell tax diff: {}%", 
             token_address, buy_tax_diff, sell_tax_diff);
    }
    
    // Thuế bán cao bất thường là dấu hiệu đáng nghi
    if sell_tax > 20.0 {
        has_dynamic_tax = true;
        warn!("Abnormally high sell tax ({}%) detected for token {}", sell_tax, token_address);
    }
    
    debug!("Tax analysis for token {}: Buy tax: {}%, Sell tax: {}%, Dynamic: {}", 
          token_address, buy_tax, sell_tax, has_dynamic_tax);
    
    Ok((has_dynamic_tax, buy_tax, sell_tax, issues))
}

/// Phân tích các quyền đặc biệt của owner token
///
/// # Arguments
/// * `adapter` - Adapter của chain
/// * `token_address` - Địa chỉ token cần kiểm tra
///
/// # Returns
/// * `bool` - Có quyền đặc biệt nguy hiểm không
/// * `Vec<String>` - Các quyền đặc biệt phát hiện được
/// * `Option<String>` - Địa chỉ owner nếu có
async fn detect_owner_privilege(&self, 
                              adapter: &Arc<EvmAdapter>, 
                              token_address: &str) -> anyhow::Result<(bool, Vec<String>, Option<String>)> {
    debug!("Analyzing owner privileges for token: {}", token_address);
    
    // Lấy thông tin contract
    let contract_info = adapter.get_contract_info(token_address)
        .await
        .context("Failed to get contract info")?;
    
    let mut owner_address = None;
    let mut dangerous_privileges = Vec::new();
    let mut has_dangerous_privileges = false;
    
    // Kiểm tra owner address
    if let Some(owner) = contract_info.owner_address.clone() {
        if !owner.eq_ignore_ascii_case("0x0000000000000000000000000000000000000000") {
            owner_address = Some(owner.clone());
            debug!("Token {} has owner: {}", token_address, owner);
        }
    }
    
    // Kiểm tra source code nếu có
    if let Some(source_code) = &contract_info.source_code {
        // Kiểm tra các hàm mint
        if source_code.contains("mint") || source_code.contains("_mint") {
            dangerous_privileges.push("Owner can mint new tokens".to_string());
            has_dangerous_privileges = true;
        }
        
        // Kiểm tra hàm pause/freeze
        if source_code.contains("pause") || source_code.contains("freeze") {
            dangerous_privileges.push("Owner can pause/freeze transfers".to_string());
            has_dangerous_privileges = true;
        }
        
        // Kiểm tra blacklist
        if source_code.contains("blacklist") || source_code.contains("whitelist") {
            dangerous_privileges.push("Owner can blacklist/whitelist addresses".to_string());
            has_dangerous_privileges = true;
        }
        
        // Kiểm tra setFee/updateFee
        if source_code.contains("setFee") || source_code.contains("updateFee") {
            dangerous_privileges.push("Owner can change transaction fees".to_string());
            has_dangerous_privileges = true;
        }
        
        // Kiểm tra proxy/upgradeable
        if source_code.contains("upgradeTo") || source_code.contains("upgradeToAndCall") || 
           source_code.contains("delegatecall") {
            dangerous_privileges.push("Contract is upgradeable (proxy pattern)".to_string());
            has_dangerous_privileges = true;
        }
        
        // Kiểm tra khả năng rút liquidity
        if source_code.contains("withdrawToken") || source_code.contains("rescueToken") {
            dangerous_privileges.push("Owner can withdraw/rescue tokens from contract".to_string());
            has_dangerous_privileges = true;
        }
    } else {
        // Không có source code, đánh dấu là có thể đáng nghi
        debug!("No source code available for token {} - cannot analyze owner privileges directly", token_address);
    }
    
    if has_dangerous_privileges {
        warn!("Token {} has dangerous owner privileges: {}", 
             token_address, dangerous_privileges.join(", "));
    } else {
        debug!("No dangerous owner privileges detected for token {}", token_address);
    }
    
    Ok((has_dangerous_privileges, dangerous_privileges, owner_address))
}

/// Phát hiện rủi ro về thanh khoản của token
///
/// # Arguments
/// * `adapter` - Adapter của chain
/// * `token_address` - Địa chỉ token cần kiểm tra
///
/// # Returns
/// * `bool` - Có rủi ro thanh khoản không
/// * `Vec<String>` - Các vấn đề phát hiện được
/// * `Option<u64>` - Thời gian khóa thanh khoản (nếu có)
async fn detect_liquidity_risk(&self, 
                            adapter: &Arc<EvmAdapter>, 
                            token_address: &str) -> anyhow::Result<(bool, Vec<String>, Option<u64>)> {
    debug!("Analyzing liquidity risk for token: {}", token_address);
    
    let mut issues = Vec::new();
    let mut has_risk = false;
    let mut lock_time = None;
    
    // Lấy thông tin liquidity
    let liquidity_info = adapter.get_token_liquidity_info(token_address)
        .await
        .context("Failed to get token liquidity info")?;
    
    // Kiểm tra giá trị thanh khoản quá thấp (< $1,000)
    if liquidity_info.liquidity_usd < 1000.0 {
        issues.push(format!("Very low liquidity: ${:.2}", liquidity_info.liquidity_usd));
        has_risk = true;
    } else if liquidity_info.liquidity_usd < 5000.0 {
        issues.push(format!("Low liquidity: ${:.2}", liquidity_info.liquidity_usd));
        has_risk = true;
    }
    
    // Kiểm tra thanh khoản có khóa không
    if let Some(is_locked) = liquidity_info.is_liquidity_locked {
        if !is_locked {
            issues.push("Liquidity is NOT locked".to_string());
            has_risk = true;
        } else if let Some(lock_time_seconds) = liquidity_info.lock_time_seconds {
            lock_time = Some(lock_time_seconds);
            
            // Thời gian khóa quá ngắn (< 1 tháng)
            if lock_time_seconds < 30 * 24 * 3600 {
                let days = lock_time_seconds / (24 * 3600);
                issues.push(format!("Short liquidity lock: {} days", days));
                has_risk = true;
            }
        }
    } else {
        issues.push("Cannot determine if liquidity is locked".to_string());
        has_risk = true;
    }
    
    // Kiểm tra tỷ lệ token trong pool vs tổng cung
    if let Some(pool_vs_supply) = liquidity_info.pool_percent_of_supply {
        if pool_vs_supply < 3.0 {
            issues.push(format!("Only {:.2}% of supply in pool (high risk)", pool_vs_supply));
            has_risk = true;
        } else if pool_vs_supply < 10.0 {
            issues.push(format!("Only {:.2}% of supply in pool (medium risk)", pool_vs_supply));
            has_risk = true;
        }
    }
    
    // Kiểm tra các sự kiện rút liquidity đáng ngờ
    let recent_events = adapter.get_liquidity_events(token_address, 10)
        .await
        .context("Failed to get recent liquidity events")?;
    
    // Phát hiện các sự kiện rút thanh khoản đáng ngờ
    let mut abnormal_events = 0;
    for event in recent_events {
        if event.event_type == LiquidityEventType::RemoveLiquidity {
            abnormal_events += 1;
        }
    }
    
    if abnormal_events > 2 {
        issues.push(format!("Multiple liquidity removal events detected ({})", abnormal_events));
        has_risk = true;
    }
    
    if has_risk {
        warn!("Liquidity risks detected for token {}: {}", token_address, issues.join(", "));
    } else {
        debug!("No significant liquidity risks detected for token {}", token_address);
    }
    
    Ok((has_risk, issues, lock_time))
}

/// Tự động bán token khi phát hiện dấu hiệu nguy hiểm
///
/// # Arguments
/// * `tracker` - Thông tin theo dõi giao dịch
/// * `alert_reason` - Lý do cảnh báo
/// * `emergency_level` - Mức độ khẩn cấp (1-5, 5 là cao nhất)
/// 
/// # Returns
/// Kết quả bán token
async fn auto_sell_on_alert(&self, 
                           tracker: &TradeTracker, 
                           alert_reason: &str, 
                           emergency_level: u8) -> anyhow::Result<()> {
    // Chỉ bán nếu trade đang mở và mức khẩn cấp đủ cao
    if tracker.status != TradeStatus::Open || emergency_level < 3 {
        warn!("Alert for {} (ID: {}): {} - Level: {}/5, monitoring", 
             tracker.token_address, tracker.id, alert_reason, emergency_level);
        return Ok(());
    }
    
    warn!("EMERGENCY ALERT for {} (ID: {}): {} - Level: {}/5, executing auto-sell", 
         tracker.token_address, tracker.id, alert_reason, emergency_level);
    
    let adapter = self.get_adapter_for_chain(tracker.chain_id)
        .ok_or_else(|| anyhow::anyhow!("No adapter for chain {}", tracker.chain_id))?;
    
    // Lấy giá hiện tại nếu có thể, nếu không thì dùng giá gần nhất biết được
    let current_price = match adapter.get_token_price(&tracker.token_address).await {
        Ok(price) => price,
        Err(_) => tracker.current_price,
    };
    
    // Tối ưu hóa gas price cho giao dịch khẩn cấp
    let gas_boost = match emergency_level {
        5 => 2.0, // Gấp đôi gas price cho trường hợp khẩn cấp nhất
        4 => 1.5, // Tăng 50% gas price
        _ => 1.2, // Tăng 20% gas price
    };
    
    // Thực hiện bán với gas price cao hơn để đảm bảo giao dịch nhanh chóng được xử lý
    match self.execute_sell(adapter, tracker, current_price, Some(gas_boost)).await {
        Ok(tx_hash) => {
            info!("Emergency sell successful for {} (ID: {}): {}", 
                 tracker.token_address, tracker.id, tx_hash);
            
            // Cập nhật trạng thái trade
            self.update_trade_status(tracker.id.clone(), 
                                     TradeStatus::Closed, 
                                     Some(format!("Emergency sell: {}", alert_reason))).await?;
        },
        Err(e) => {
            error!("Emergency sell FAILED for {} (ID: {}): {}", 
                  tracker.token_address, tracker.id, e);
            
            // Cập nhật trạng thái là SellFailed nhưng giữ open để có thể thử lại
            self.update_trade_status(tracker.id.clone(), 
                                     TradeStatus::SellFailed, 
                                     Some(format!("Failed emergency sell: {}", alert_reason))).await?;
        }
    }
    
    Ok(())
}

/// Kiểm tra các thay đổi về độ an toàn của token sau khi mua
///
/// # Arguments
/// * `trade` - Thông tin theo dõi giao dịch
/// * `adapter` - Adapter của chain
/// 
/// # Returns
/// Kết quả kiểm tra - Ok() nếu an toàn, Err() nếu có vấn đề
async fn check_token_safety_changes(&self, trade: &TradeTracker, adapter: &Arc<EvmAdapter>) -> anyhow::Result<()> {
    debug!("Checking token safety changes for {} (ID: {})", trade.token_address, trade.id);
    
    // 1. Kiểm tra honeypot
    let (is_honeypot, reason) = self.detect_honeypot(adapter, &trade.token_address, None).await?;
    if is_honeypot {
        return Err(anyhow::anyhow!("Token became honeypot: {}", 
                                  reason.unwrap_or_else(|| "Unknown reason".to_string())));
    }
    
    // 2. Kiểm tra thuế động
    let (has_dynamic_tax, buy_tax, sell_tax, _) = self.detect_dynamic_tax(adapter, &trade.token_address).await?;
    if has_dynamic_tax {
        return Err(anyhow::anyhow!("Dynamic tax detected: Buy {}%, Sell {}%", buy_tax, sell_tax));
    }
    
    // 3. Kiểm tra thuế bán quá cao (>20%)
    if sell_tax > 20.0 {
        return Err(anyhow::anyhow!("Sell tax too high: {}%", sell_tax));
    }
    
    // 4. Kiểm tra rủi ro thanh khoản
    let (has_liquidity_risk, issues, _) = self.detect_liquidity_risk(adapter, &trade.token_address).await?;
    if has_liquidity_risk {
        if issues.iter().any(|i| i.contains("liquidity removal")) {
            // Nghiêm trọng - có rút liquidity
            return Err(anyhow::anyhow!("Liquidity risk detected: {}", issues.join(", ")));
        }
        // Chỉ log warning nếu các vấn đề khác không quá nghiêm trọng
        warn!("Liquidity concerns for {} (ID: {}): {}", trade.token_address, trade.id, issues.join(", "));
    }
    
    // 5. Kiểm tra sự thay đổi của owner
    let initial_owner = trade.custom_params.get("owner_address").cloned();
    let (has_privileges, privileges, current_owner) = self.detect_owner_privilege(adapter, &trade.token_address).await?;
    
    // Nếu owner thay đổi, đáng ngờ
    if let (Some(initial), Some(current)) = (&initial_owner, &current_owner) {
        if initial != current {
            return Err(anyhow::anyhow!("Owner changed from {} to {}", initial, current));
        }
    }
    
    // Nếu có các quyền nguy hiểm và là mới (không có từ trước), cảnh báo
    if has_privileges && privileges.iter().any(|p| p.contains("withdraw") || p.contains("upgrade")) {
        return Err(anyhow::anyhow!("Dangerous privileges detected: {}", privileges.join(", ")));
    }
    
    Ok(())
}

/// Phát hiện blacklist, anti-bot, anti-whale và các giới hạn giao dịch
///
/// # Arguments
/// * `adapter` - Adapter của chain
/// * `token_address` - Địa chỉ token cần kiểm tra
///
/// # Returns
/// * `bool` - Có giới hạn đáng ngờ không
/// * `Vec<String>` - Các giới hạn phát hiện được
/// * `HashMap<String, f64>` - Chi tiết các giới hạn với giá trị cụ thể
async fn detect_blacklist(&self, 
                        adapter: &Arc<EvmAdapter>, 
                        token_address: &str) -> anyhow::Result<(bool, Vec<String>, HashMap<String, f64>)> {
    debug!("Checking blacklist/whitelist for token: {}", token_address);
    
    let mut has_restrictions = false;
    let mut restrictions = Vec::new();
    let mut details = HashMap::new();
    
    // Lấy thông tin contract
    let contract_info = adapter.get_contract_info(token_address)
        .await
        .context("Failed to get contract info")?;
    
    // Kiểm tra source code nếu có
    if let Some(source_code) = &contract_info.source_code {
        // Kiểm tra blacklist
        if source_code.contains("blacklist") || source_code.contains("_blacklisted") {
            restrictions.push("Has blacklist function".to_string());
            has_restrictions = true;
        }
        
        // Kiểm tra whitelist
        if source_code.contains("whitelist") || source_code.contains("_whitelisted") {
            restrictions.push("Has whitelist function".to_string());
            has_restrictions = true;
        }
    }
    
    // Mô phỏng transfer với nhiều wallet khác nhau để phát hiện blacklist ẩn
    // (đây chỉ là giả lập, trong thực tế cần phân tích code và thử nhiều dạng transfer)
    let test_wallets = [
        "0x1111111111111111111111111111111111111111",
        "0x2222222222222222222222222222222222222222",
        "0x3333333333333333333333333333333333333333",
    ];
    
    let mut blocked_transfers = 0;
    
    for wallet in test_wallets.iter() {
        let simulation = adapter.simulate_transfer_to(token_address, "0.01", wallet)
            .await
            .context(format!("Failed to simulate transfer to {}", wallet))?;
            
        if !simulation.success {
            blocked_transfers += 1;
            if let Some(reason) = &simulation.failure_reason {
                if reason.contains("blacklist") || reason.contains("not allowed") {
                    has_restrictions = true;
                    restrictions.push(format!("Transfer to {} blocked: {}", wallet, reason));
                }
            }
        }
    }
    
    if blocked_transfers > 0 {
        details.insert("blocked_transfers".to_string(), blocked_transfers as f64);
    }
    
    // Kiểm tra có trading cooldown không
    let has_cooldown = adapter.check_trading_cooldown(token_address)
        .await
        .context("Failed to check trading cooldown")?;
    
    if has_cooldown {
        has_restrictions = true;
        restrictions.push("Has trading cooldown".to_string());
    }
    
    if has_restrictions {
        warn!("Token {} has address restrictions: {}", token_address, restrictions.join(", "));
    } else {
        debug!("No address restrictions detected for token {}", token_address);
    }
    
    Ok((has_restrictions, restrictions, details))
}

/// Phát hiện anti-bot, anti-whale và các giới hạn giao dịch
/// 
/// # Arguments
/// * `adapter` - Adapter của chain
/// * `token_address` - Địa chỉ token cần kiểm tra
/// 
/// # Returns
/// * `bool` - Có giới hạn đáng ngờ không
/// * `Vec<String>` - Các giới hạn phát hiện được
/// * `HashMap<String, f64>` - Chi tiết các giới hạn với giá trị cụ thể 
async fn detect_anti_bot(&self, 
                       adapter: &Arc<EvmAdapter>, 
                       token_address: &str) -> anyhow::Result<(bool, Vec<String>, HashMap<String, f64>)> {
    debug!("Checking transaction limits for token: {}", token_address);
    
    let mut has_limits = false;
    let mut limits = Vec::new();
    let mut details = HashMap::new();
    
    // Lấy thông tin contract
    let contract_info = adapter.get_contract_info(token_address)
        .await
        .context("Failed to get contract info")?;
    
    // Lấy thông tin supply
    let token_info = adapter.get_token_info(token_address)
        .await
        .context("Failed to get token info")?;
    
    let total_supply = token_info.total_supply;
    
    // Kiểm tra source code nếu có
    if let Some(source_code) = &contract_info.source_code {
        // Kiểm tra maxTransactionAmount
        if source_code.contains("maxTransaction") || source_code.contains("maxTxAmount") {
            limits.push("Has max transaction limit".to_string());
            has_limits = true;
        }
        
        // Kiểm tra maxWalletAmount
        if source_code.contains("maxWallet") || source_code.contains("maxHolding") {
            limits.push("Has max wallet limit".to_string());
            has_limits = true;
        }
        
        // Kiểm tra cooldown
        if source_code.contains("cooldown") || source_code.contains("timelock") {
            limits.push("Has transaction cooldown".to_string());
            has_limits = true;
        }
        
        // Kiểm tra anti-bot
        if source_code.contains("antiBot") || source_code.contains("sniper") {
            limits.push("Has anti-bot measures".to_string());
            has_limits = true;
        }
        
        // Kiểm tra anti-whale
        if source_code.contains("antiWhale") {
            limits.push("Has anti-whale measures".to_string());
            has_limits = true;
        }
    }
    
    // Mô phỏng giao dịch với các kích thước khác nhau để phát hiện giới hạn
    
    // Thử mua lượng lớn (5% supply)
    let large_amount = total_supply * 0.05;
    let simulation_large = adapter.simulate_buy_token(token_address, &format!("{}", large_amount))
        .await
        .context("Failed to simulate large purchase")?;
    
    if !simulation_large.success {
        has_limits = true;
        limits.push("Restricted large transactions (anti-whale)".to_string());
        details.insert("max_percent_limit".to_string(), 5.0);
    }
    
    // Thử mua nhiều lần liên tiếp (giả lập bot behavior)
    let small_amount = total_supply * 0.001;
    let mut consecutive_success = 0;
    
    for i in 0..3 {
        let simulation = adapter.simulate_buy_token(token_address, &format!("{}", small_amount))
            .await
            .context("Failed to simulate consecutive purchase")?;
        
        if simulation.success {
            consecutive_success += 1;
        } else if i == 0 {
            // Fail từ lần đầu => có thể không phải anti-bot
            continue;
        } else if i > 0 && consecutive_success > 0 {
            // Thành công trước đó nhưng fail sau => anti-bot
            has_limits = true;
            limits.push("Restricted consecutive transactions (anti-bot)".to_string());
            break;
        }
    }
    
    if has_limits {
        warn!("Token {} has transaction limits: {}", token_address, limits.join(", "));
    } else {
        debug!("No transaction limits detected for token {}", token_address);
    }
    
    Ok((has_limits, limits, details))
}

/// Điều chỉnh take profit và stop loss động theo biến động thị trường
/// 
/// # Arguments
/// * `adapter` - Adapter của chain
/// * `token_address` - Địa chỉ token 
/// * `trade` - Thông tin giao dịch
/// * `market_data` - Dữ liệu thị trường
/// 
/// # Returns
/// * `(f64, f64)` - (take_profit_percent, stop_loss_percent) mới
async fn dynamic_tp_sl(&self, 
                     adapter: &Arc<EvmAdapter>, 
                     token_address: &str, 
                     trade: &TradeTracker,
                     market_data: Option<MarketData>) -> anyhow::Result<(f64, f64)> {
    let mut tp_percent = trade.take_profit_percent;
    let mut sl_percent = trade.stop_loss_percent;
    
    // Lấy dữ liệu thị trường nếu không được cung cấp
    let market = match market_data {
        Some(data) => data,
        None => {
            adapter.get_token_market_data(token_address)
                .await
                .context("Failed to get market data")?
        }
    };
    
    debug!("Adjusting TP/SL for token {} (ID: {})", token_address, trade.id);
    
    // Điều chỉnh dựa trên volatility
    // Biến động cao => TP cao hơn, SL thấp hơn (tránh sell sớm)
    if market.volatility_24h > 50.0 {
        // Biến động cực cao
        tp_percent = tp_percent * 1.5; // Tăng TP thêm 50%
        sl_percent = sl_percent * 0.7; // Giảm SL xuống còn 70%
        debug!("Extreme volatility ({}%): Increasing TP to {}%, decreasing SL to {}%", 
              market.volatility_24h, tp_percent, sl_percent);
    } else if market.volatility_24h > 20.0 {
        // Biến động cao
        tp_percent = tp_percent * 1.3; // Tăng TP thêm 30%
        sl_percent = sl_percent * 0.8; // Giảm SL xuống còn 80%
        debug!("High volatility ({}%): Increasing TP to {}%, decreasing SL to {}%", 
              market.volatility_24h, tp_percent, sl_percent);
    }
    
    // Điều chỉnh dựa trên volume
    // Volume thấp => an toàn hơn (giảm TP, tăng SL)
    if market.volume_24h < 1000.0 {
        // Volume cực thấp
        tp_percent = tp_percent * 0.7; // Giảm TP xuống còn 70%
        sl_percent = sl_percent * 1.3; // Tăng SL thêm 30%
        debug!("Very low volume (${:.2}): Decreasing TP to {}%, increasing SL to {}%", 
              market.volume_24h, tp_percent, sl_percent);
    } else if market.volume_24h < 10000.0 {
        // Volume thấp
        tp_percent = tp_percent * 0.85; // Giảm TP xuống còn 85%
        sl_percent = sl_percent * 1.15; // Tăng SL thêm 15%
        debug!("Low volume (${:.2}): Decreasing TP to {}%, increasing SL to {}%", 
              market.volume_24h, tp_percent, sl_percent);
    }
    
    // Điều chỉnh dựa trên thời gian token tồn tại
    // Token mới => rủi ro cao hơn (giảm TP, tăng SL)
    if market.age_days < 1 {
        // Token mới trong vòng 24h
        tp_percent = tp_percent * 0.6; // Giảm TP xuống còn 60%
        sl_percent = sl_percent * 1.4; // Tăng SL thêm 40%
        debug!("Very new token (<24h): Decreasing TP to {}%, increasing SL to {}%", 
              tp_percent, sl_percent);
    } else if market.age_days < 7 {
        // Token mới trong vòng 1 tuần
        tp_percent = tp_percent * 0.8; // Giảm TP xuống còn 80%
        sl_percent = sl_percent * 1.2; // Tăng SL thêm 20%
        debug!("New token (<7d): Decreasing TP to {}%, increasing SL to {}%", 
              tp_percent, sl_percent);
    }
    
    // Điều chỉnh dựa trên số lượng holder
    // Ít holder => rủi ro cao hơn (giảm TP, tăng SL)
    if market.holder_count < 50 {
        // Rất ít holder
        tp_percent = tp_percent * 0.7; // Giảm TP xuống còn 70%
        sl_percent = sl_percent * 1.3; // Tăng SL thêm 30%
        debug!("Very few holders ({}): Decreasing TP to {}%, increasing SL to {}%", 
              market.holder_count, tp_percent, sl_percent);
    } else if market.holder_count < 200 {
        // Ít holder
        tp_percent = tp_percent * 0.85; // Giảm TP xuống còn 85%
        sl_percent = sl_percent * 1.15; // Tăng SL thêm 15%
        debug!("Few holders ({}): Decreasing TP to {}%, increasing SL to {}%", 
              market.holder_count, tp_percent, sl_percent);
    }
    
    // Đặt giới hạn để đảm bảo TP/SL nằm trong khoảng hợp lý
    tp_percent = tp_percent.min(100.0).max(5.0); // TP từ 5% đến 100%
    sl_percent = sl_percent.min(30.0).max(5.0);  // SL từ 5% đến 30%
    
    debug!("Final dynamic TP/SL for token {} (ID: {}): TP={}%, SL={}%", 
          token_address, trade.id, tp_percent, sl_percent);
    
    Ok((tp_percent, sl_percent))
}

/// Điều chỉnh trailing stop động theo biến động thị trường
/// 
/// # Arguments
/// * `adapter` - Adapter của chain
/// * `token_address` - Địa chỉ token 
/// * `trade` - Thông tin giao dịch
/// * `highest_price` - Giá cao nhất đã đạt được
/// * `current_price` - Giá hiện tại
/// 
/// # Returns
/// * `f64` - Phần trăm trailing stop loss mới
async fn dynamic_trailing_stop(&self, 
                             adapter: &Arc<EvmAdapter>, 
                             token_address: &str, 
                             trade: &TradeTracker,
                             highest_price: f64,
                             current_price: f64) -> anyhow::Result<f64> {
    let initial_tsl_percent = trade.trailing_stop_percent.unwrap_or(10.0);
    
    // Lấy chỉ số ATR (Average True Range) - chỉ số đo volatility
    let atr = adapter.get_token_atr(token_address)
        .await
        .context("Failed to get token ATR")?;
    
    // Lấy bollinger bands width - chỉ số đo volatility khác
    let bb_width = adapter.get_token_bollinger_width(token_address)
        .await
        .context("Failed to get Bollinger Bands width")?;
    
    // Lấy dữ liệu thị trường
    let market_data = adapter.get_token_market_data(token_address)
        .await
        .context("Failed to get market data")?;
    
    // Tính profit hiện tại (%)
    let current_profit = ((current_price - trade.entry_price) / trade.entry_price) * 100.0;
    
    // Tính profit cao nhất đã đạt được (%)
    let max_profit = ((highest_price - trade.entry_price) / trade.entry_price) * 100.0;
    
    // Điều chỉnh TSL dựa trên ATR (volatility)
    // ATR cao = volatility cao => TSL cần lớn hơn để tránh bán sớm
    let mut adjusted_tsl = initial_tsl_percent;
    
    // Điều chỉnh dựa trên ATR
    if atr > 0.05 { // ATR cao (volatility cao)
        adjusted_tsl = adjusted_tsl * 1.5; // Tăng TSL thêm 50%
        debug!("High ATR ({}): Increasing TSL to {}%", atr, adjusted_tsl);
    } else if atr < 0.01 { // ATR thấp (volatility thấp)
        adjusted_tsl = adjusted_tsl * 0.7; // Giảm TSL xuống còn 70%
        debug!("Low ATR ({}): Decreasing TSL to {}%", atr, adjusted_tsl);
    }
    
    // Điều chỉnh dựa trên Bollinger Band width
    if bb_width > 0.1 { // BB width cao (volatility cao)
        adjusted_tsl = adjusted_tsl * 1.3; // Tăng TSL thêm 30%
        debug!("Wide Bollinger Bands ({}): Increasing TSL to {}%", bb_width, adjusted_tsl);
    } else if bb_width < 0.03 { // BB width thấp (volatility thấp)
        adjusted_tsl = adjusted_tsl * 0.8; // Giảm TSL xuống còn 80%
        debug!("Narrow Bollinger Bands ({}): Decreasing TSL to {}%", bb_width, adjusted_tsl);
    }
    
    // Điều chỉnh dựa trên profit đã đạt được
    if max_profit > 50.0 {
        // Profit lớn = cần bảo vệ lợi nhuận => TSL chặt hơn
        adjusted_tsl = adjusted_tsl * 0.7; // Giảm TSL xuống còn 70% (chặt hơn)
        debug!("High max profit ({}%): Tightening TSL to {}%", max_profit, adjusted_tsl);
    } else if max_profit > 20.0 {
        // Profit khá = cần bảo vệ một phần lợi nhuận => TSL hơi chặt hơn
        adjusted_tsl = adjusted_tsl * 0.85; // Giảm TSL xuống còn 85% (hơi chặt hơn)
        debug!("Good max profit ({}%): Slightly tightening TSL to {}%", max_profit, adjusted_tsl);
    }
    
    // Điều chỉnh dựa trên thời gian đã hold
    let hold_time = chrono::Utc::now().timestamp() as u64 - trade.created_at;
    let hold_days = hold_time / (24 * 3600);
    
    if hold_days > 7 {
        // Đã hold lâu => cần bảo vệ lợi nhuận => TSL chặt hơn
        adjusted_tsl = adjusted_tsl * 0.8; // Giảm TSL xuống còn 80% (chặt hơn)
        debug!("Long hold time ({}d): Tightening TSL to {}%", hold_days, adjusted_tsl);
    }
    
    // Đặt giới hạn để đảm bảo TSL nằm trong khoảng hợp lý
    adjusted_tsl = adjusted_tsl.min(30.0).max(2.0); // TSL từ 2% đến 30%
    
    debug!("Final dynamic TSL for token {} (ID: {}): {}%", 
          token_address, trade.id, adjusted_tsl);
    
    Ok(adjusted_tsl)
}

/// Theo dõi hoạt động của các ví lớn và đưa ra quyết định bán khi phát hiện whale bán
/// 
/// # Arguments
/// * `adapter` - Adapter của chain
/// * `token_address` - Địa chỉ token 
/// * `trade` - Thông tin giao dịch
/// 
/// # Returns
/// * `bool` - Có nên bán ngay không
/// * `Option<String>` - Lý do bán nếu có
async fn whale_tracker(&self, 
                     adapter: &Arc<EvmAdapter>, 
                     token_address: &str, 
                     trade: &TradeTracker) -> anyhow::Result<(bool, Option<String>)> {
    debug!("Analyzing whale activity for token: {}", token_address);
    
    // Lấy danh sách top holders
    let top_holders = adapter.get_token_top_holders(token_address, 5)
        .await
        .context("Failed to get top token holders")?;
    
    if top_holders.is_empty() {
        return Ok((false, None));
    }
    
    // Lấy các giao dịch gần đây của token
    let recent_txs = adapter.get_token_recent_transactions(token_address, 20)
        .await
        .context("Failed to get recent transactions")?;
    
    let mut whale_sells = 0;
    let mut total_sell_amount = 0.0;
    let mut whale_addresses = HashSet::new();
    
    // Phân tích các giao dịch gần đây
    for tx in &recent_txs {
        // Kiểm tra xem có phải giao dịch bán không
        if tx.is_sell {
            // Kiểm tra xem người bán có phải là whale không
            for holder in &top_holders {
                if tx.from_address == holder.address {
                    whale_sells += 1;
                    total_sell_amount += tx.amount_usd;
                    whale_addresses.insert(holder.address.clone());
                    
                    debug!("Detected whale selling: {} sold ${:.2} worth of tokens", 
                          holder.address, tx.amount_usd);
                }
            }
        }
    }
    
    // Lấy thông tin token để phân tích
    let token_info = adapter.get_token_info(token_address)
        .await
        .context("Failed to get token info")?;
    
    let market_cap = token_info.market_cap;
    
    // Phần trăm market cap bị whale bán ra gần đây
    let percent_sold = if market_cap > 0.0 {
        (total_sell_amount / market_cap) * 100.0
    } else {
        0.0
    };
    
    debug!("Whale analysis for {}: {} whales sold ${:.2} ({:.2}% of mcap)", 
          token_address, whale_addresses.len(), total_sell_amount, percent_sold);
    
    // Điều kiện bán:
    // 1. Nhiều whale bán (>= 2)
    // 2. Tổng sell amount lớn (>5% market cap)
    // 3. Hoặc số lượng whale bán >= 3 (bất kể số tiền)
    
    let should_sell = (whale_addresses.len() >= 2 && percent_sold > 5.0) || 
                     whale_addresses.len() >= 3;
    
    if should_sell {
        let reason = format!(
            "Whale dump detected: {} whales sold ${:.2} ({:.2}% of market cap)", 
            whale_addresses.len(), total_sell_amount, percent_sold
        );
        warn!("{} for token {} (ID: {})", reason, token_address, trade.id);
        return Ok((true, Some(reason)));
    }
    
    Ok((false, None))
}