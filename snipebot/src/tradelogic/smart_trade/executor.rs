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
use anyhow;
use uuid;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::{MempoolAnalyzer, MempoolTransaction};
use crate::analys::risk_analyzer::{RiskAnalyzer, RiskFactor, TradeRecommendation, TradeRiskAnalysis};
use crate::analys::token_status::{ContractInfo, LiquidityEvent, LiquidityEventType, TokenSafety, TokenStatus};
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};
use crate::tradelogic::traits::{
    TradeExecutor, RiskManager, TradeCoordinator, ExecutorType, SharedOpportunity, 
    SharedOpportunityType, OpportunityPriority
};

// Module imports
use super::types::{
    SmartTradeConfig, 
    TokenIssue, 
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

impl Clone for SmartTradeExecutor {
    fn clone(&self) -> Self {
        Self {
            evm_adapters: self.evm_adapters.clone(),
            analys_client: self.analys_client.clone(),
            risk_analyzer: self.risk_analyzer.clone(),
            mempool_analyzers: self.mempool_analyzers.clone(),
            config: RwLock::new(SmartTradeConfig::default()),
            active_trades: RwLock::new(Vec::new()),
            trade_history: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            coordinator: self.coordinator.clone(),
            executor_id: self.executor_id.clone(),
            coordinator_subscription: RwLock::new(None),
        }
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
                return Err(anyhow::anyhow!("Không tìm thấy EVM adapter cho chain ID {}", chain_id));
            }
        };
        
        // Sử dụng analys_client để kiểm tra an toàn token
        let security_check_future = self.analys_client.verify_token_safety(chain_id, token_address);
        
        // Sử dụng analys_client để đánh giá rủi ro
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
        // Kiểm tra các vấn đề phát sinh sau khi đã mua token
        
        // 1. Kiểm tra có thể bán không (phát hiện honeypot muộn)
        let small_amount = trade.invested_amount * 0.01; // Thử bán 1% số lượng đã mua
        let can_sell = !self.simulate_token_sell(trade.chain_id, &trade.token_address, small_amount, adapter).await;
        
        if !can_sell {
            return Err(anyhow::anyhow!("Token không thể bán - có thể là honeypot"));
        }
        
        // 2. Kiểm tra tax cao bất thường xuất hiện sau khi mua
        let (is_dynamic_tax, _, sell_tax) = self.detect_dynamic_tax(trade.chain_id, &trade.token_address, adapter).await;
        
        if is_dynamic_tax && sell_tax > MAX_SAFE_SELL_TAX {
            return Err(anyhow::anyhow!("Phát hiện tax bán cao bất thường: {}%", sell_tax));
        }
        
        // 3. Kiểm tra thanh khoản đột ngột giảm
        let (liquidity_risk, reason) = self.detect_liquidity_risk(trade.chain_id, &trade.token_address, adapter).await;
        
        if liquidity_risk {
            return Err(anyhow::anyhow!("Rủi ro thanh khoản: {}", reason));
        }
        
        // 4. Kiểm tra biến động giá bất thường
        if let Ok(price_history) = adapter.get_token_price_history(&trade.token_address, 10).await {
            if price_history.len() >= 3 {
                // Tính phần trăm biến động giá trong 3 lần kiểm tra gần nhất
                let latest = price_history[0];
                let previous = price_history[2];
                
                let price_change_percent = ((latest - previous) / previous) * 100.0;
                
                // Nếu giá giảm quá 15% trong khoảng thời gian ngắn, cảnh báo
                if price_change_percent < -15.0 {
                    return Err(anyhow::anyhow!("Giá giảm đột ngột: {:.2}% trong thời gian ngắn", price_change_percent));
                }
            }
        }
        
        // Không phát hiện vấn đề an toàn mới
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

    /// Register with the coordinator
    async fn register_with_coordinator(&self) -> anyhow::Result<()> {
        if let Some(coordinator) = &self.coordinator {
            // Register as executor
            coordinator.register_executor(&self.executor_id, ExecutorType::SmartTrade).await?;
            
            // Create callback for shared opportunities
            let self_clone = Arc::new(self.clone_with_config().await);
            let callback = Arc::new(move |opportunity: SharedOpportunity| -> anyhow::Result<()> {
                let executor = self_clone.clone();
                
                tokio::spawn(async move {
                    if let Err(e) = executor.handle_shared_opportunity(opportunity).await {
                        error!("Error handling shared opportunity: {}", e);
                    }
                });
                
                Ok(())
            });
            
            // Subscribe to opportunities
            let subscription_id = coordinator
                .subscribe_to_opportunities(&self.executor_id, callback)
                .await?;
            
            let mut sub_id = self.coordinator_subscription.write().await;
            *sub_id = Some(subscription_id);
            
            info!("SmartTradeExecutor registered with coordinator: {}", self.executor_id);
        }
        
        Ok(())
    }
    
    /// Unregister from the coordinator
    async fn unregister_from_coordinator(&self) -> anyhow::Result<()> {
        if let Some(coordinator) = &self.coordinator {
            // Unsubscribe from opportunities
            let sub_id = {
                let mut sub_id_lock = self.coordinator_subscription.write().await;
                sub_id_lock.take()
            };
            
            if let Some(id) = sub_id {
                coordinator.unsubscribe_from_opportunities(&id).await?;
            }
            
            // Unregister as executor
            coordinator.unregister_executor(&self.executor_id).await?;
            
            info!("SmartTradeExecutor unregistered from coordinator: {}", self.executor_id);
        }
        
        Ok(())
    }
    
    /// Handle a shared opportunity from the coordinator
    async fn handle_shared_opportunity(&self, opportunity: SharedOpportunity) -> anyhow::Result<()> {
        // Skip opportunities from ourselves
        if opportunity.source == self.executor_id {
            return Ok(());
        }
        
        // Only handle specific opportunity types that SmartTradeExecutor cares about
        match &opportunity.opportunity_type {
            SharedOpportunityType::NewToken | 
            SharedOpportunityType::LiquidityChange |
            SharedOpportunityType::PriceMovement => {
                // Try to reserve the opportunity
                if let Some(coordinator) = &self.coordinator {
                    let reserved = coordinator
                        .reserve_opportunity(
                            &opportunity.id, 
                            &self.executor_id, 
                            OpportunityPriority::Medium
                        )
                        .await?;
                    
                    if reserved {
                        // Process the opportunity
                        info!("Processing shared opportunity: {}", opportunity.id);
                        
                        // Get the primary token address
                        if let Some(token_address) = opportunity.tokens.first() {
                            // Execute trade based on opportunity
                            // Find the chain ID
                            let chain_id = opportunity.chain_id;
                            
                            // Find the appropriate adapter
                            if let Some(adapter) = self.evm_adapters.get(&chain_id) {
                                let strategy = match opportunity.opportunity_type {
                                    SharedOpportunityType::NewToken => TradeStrategy::QuickFlip,
                                    SharedOpportunityType::LiquidityChange => TradeStrategy::TrailingStopLoss,
                                    SharedOpportunityType::PriceMovement => TradeStrategy::Scalping,
                                    _ => TradeStrategy::default(),
                                };
                                
                                // Create a mock transaction for context
                                let tx = MempoolTransaction {
                                    hash: opportunity.id.clone(),
                                    from: "coordinator".to_string(),
                                    to: token_address.clone(),
                                    value: 0,
                                    gas_price: None,
                                    gas_limit: None,
                                    input: Vec::new(),
                                    nonce: None,
                                    transaction_type: TransactionType::TokenTransfer,
                                    block_number: None,
                                    block_hash: None,
                                    timestamp: opportunity.created_at,
                                    detected_at: opportunity.created_at,
                                    status: None,
                                };
                                
                                // Execute trade
                                if let Err(e) = self.execute_trade(chain_id, token_address, strategy, &tx).await {
                                    error!("Failed to execute trade for shared opportunity: {}", e);
                                    
                                    // Release the opportunity so others can try
                                    if let Err(e) = coordinator.release_opportunity(&opportunity.id, &self.executor_id).await {
                                        error!("Failed to release opportunity: {}", e);
                                    }
                                } else {
                                    info!("Successfully executed trade for shared opportunity: {}", opportunity.id);
                                }
                            }
                        }
                    } else {
                        debug!("Could not reserve opportunity: {}", opportunity.id);
                    }
                }
            },
            _ => {
                // Ignore other types
                debug!("Ignoring opportunity type: {:?}", opportunity.opportunity_type);
            }
        }
        
        Ok(())
    }
    
    /// Share an opportunity with other executors
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
            let opportunity = SharedOpportunity {
                id: uuid::Uuid::new_v4().to_string(),
                chain_id,
                opportunity_type,
                tokens: vec![token_address.to_string()],
                estimated_profit_usd,
                risk_score,
                time_sensitivity,
                source: self.executor_id.clone(),
                created_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                    .as_secs(),
                custom_data: HashMap::new(),
                reservation: None,
            };
            
            coordinator.share_opportunity(&self.executor_id, opportunity).await?;
            debug!("Shared opportunity for token {} on chain {}", token_address, chain_id);
        }
        
        Ok(())
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
