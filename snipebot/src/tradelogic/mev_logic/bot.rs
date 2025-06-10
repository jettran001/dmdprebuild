/// MEV Bot trait definition and implementations
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use anyhow::{Result, anyhow};
use tracing::info;

use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::{MempoolTransaction, SuspiciousPattern};
use crate::analys::token_status::TokenSafety;
use crate::tradelogic::traits::{
    MempoolAnalysisProvider,
    TokenAnalysisProvider,
    RiskAnalysisProvider,
    MevOpportunityProvider
};

use super::opportunity::{MevOpportunity, OpportunityManager};
use super::trader_behavior::TraderBehaviorAnalysis;
use super::types::MevConfig;

use common::trading_types::{
    PotentialFailingTx, FailureReason,
    TraderProfile, RiskAppetite, TradingPattern
};

/// Trait for MEV bot implementations
#[async_trait::async_trait]
pub trait MevBot: Send + Sync + 'static {
    /// Start monitoring for MEV
    async fn start(&self);
    
    /// Stop monitoring for MEV
    async fn stop(&self);
    
    /// Update configuration
    async fn update_config(&self, config: MevConfig);
    
    /// Get list of opportunities
    async fn get_opportunities(&self) -> Vec<MevOpportunity>;
    
    /// Add chain to monitor
    async fn add_chain(&mut self, chain_id: u32, adapter: Arc<EvmAdapter>);
    
    /// Evaluate a new token
    async fn evaluate_new_token(&self, chain_id: u32, token_address: &str) -> Option<TokenSafety>;
    
    /// Analyze trading opportunity from evaluated token
    async fn analyze_trading_opportunity(&self, chain_id: u32, token_address: &str, token_safety: TokenSafety) -> Option<MevOpportunity>;
    
    /// Monitor token liquidity
    async fn monitor_token_liquidity(&self, chain_id: u32, token_address: &str);
    
    /// Detect suspicious transaction patterns
    async fn detect_suspicious_transaction_patterns(&self, chain_id: u32, transactions: &[MempoolTransaction]) -> Vec<SuspiciousPattern>;
    
    /// Find potentially failing transactions in mempool
    /// 
    /// # Parameters
    /// - chain_id: Blockchain ID
    /// - include_nonce_gaps: Include transactions with nonce gaps
    /// - include_low_gas: Include transactions with low gas price
    /// - include_long_pending: Include transactions pending for too long
    /// - min_wait_time_sec: Minimum wait time to consider as long (seconds)
    /// - limit: Maximum number of transactions to return
    async fn find_potential_failing_transactions(
        &self,
        chain_id: u32,
        include_nonce_gaps: bool,
        include_low_gas: bool,
        include_long_pending: bool,
        min_wait_time_sec: u64,
        limit: usize,
    ) -> Vec<(MempoolTransaction, String)>;
    
    /// Estimate success probability of a specific transaction
    /// 
    /// # Parameters
    /// - chain_id: Blockchain ID
    /// - transaction: Transaction to evaluate
    /// - include_details: Include details of reasons
    async fn estimate_transaction_success_probability(
        &self,
        chain_id: u32,
        transaction: &MempoolTransaction,
        include_details: bool,
    ) -> (f64, Option<String>);
    
    /// Analyze specific trader behavior to predict behavior
    /// 
    /// # Parameters
    /// - chain_id: Blockchain ID
    /// - trader_address: Trader address to analyze
    /// - time_window_sec: Time window for analysis (seconds)
    async fn analyze_trader_behavior(
        &self,
        chain_id: u32,
        trader_address: &str,
        time_window_sec: u64,
    ) -> Option<TraderBehaviorAnalysis>;
}

/// Implementation of MevBot using the API providers from analys
pub struct MevBotImpl {
    /// Config for the MEV bot
    config: RwLock<MevConfig>,
    /// Chain adapters for blockchain access
    chain_adapters: RwLock<HashMap<u32, Arc<EvmAdapter>>>,
    /// Whether the bot is active
    is_active: RwLock<bool>,
    /// Mempool analysis provider for detecting opportunities
    mempool_provider: Arc<dyn MempoolAnalysisProvider>,
    /// Token analysis provider for token safety checks
    token_provider: Arc<dyn TokenAnalysisProvider>,
    /// Risk analysis provider for risk assessment
    risk_provider: Arc<dyn RiskAnalysisProvider>,
    /// MEV opportunity provider
    opportunity_provider: Arc<dyn MevOpportunityProvider>,
    /// Opportunity manager
    opportunity_manager: RwLock<OpportunityManager>,
    /// Subscription IDs for tracking callbacks
    subscription_ids: RwLock<HashMap<u32, String>>,
}

impl MevBotImpl {
    /// Create a new MEV bot
    pub fn new(
        mempool_provider: Arc<dyn MempoolAnalysisProvider>,
        token_provider: Arc<dyn TokenAnalysisProvider>,
        risk_provider: Arc<dyn RiskAnalysisProvider>,
        opportunity_provider: Arc<dyn MevOpportunityProvider>,
        config: MevConfig,
    ) -> Self {
        let opportunity_manager = OpportunityManager::new(
            mempool_provider.clone(),
            token_provider.clone(),
            risk_provider.clone(),
        );
        
        Self {
            config: RwLock::new(config),
            chain_adapters: RwLock::new(HashMap::new()),
            is_active: RwLock::new(false),
            mempool_provider,
            token_provider,
            risk_provider,
            opportunity_provider,
            opportunity_manager: RwLock::new(opportunity_manager),
            subscription_ids: RwLock::new(HashMap::new()),
        }
    }
    
    /// Register a callback for new opportunities in a specific chain
    ///
    /// This method creates a callback function and subscribes it to the mempool provider
    /// to receive new MEV opportunities. The callback processes opportunities and adds
    /// them to the opportunity manager if they pass security and risk checks.
    ///
    /// # Thread safety
    /// This implementation avoids cloning Arc unnecessarily and properly handles
    /// shared state access to prevent race conditions and deadlocks.
    ///
    /// # Arguments
    /// * `chain_id` - Chain ID to register for
    ///
    /// # Returns
    /// * `Result<()>` - Success or error
    async fn register_chain_callback(&self, chain_id: u32) -> Result<()> {
        // Create a single callback for new opportunities, using weak references 
        // to avoid circular references and potential memory leaks
        let opportunity_manager = Arc::downgrade(&self.opportunity_manager);
        let mempool_provider = Arc::downgrade(&self.mempool_provider);
        let token_provider = Arc::downgrade(&self.token_provider);
        let risk_provider = Arc::downgrade(&self.risk_provider);
        
        let callback = move |opportunity: MevOpportunity| -> Result<()> {
            // Create a task to process the opportunity asynchronously
            let weak_opportunity_manager = opportunity_manager.clone();
            let weak_token_provider = token_provider.clone();
            let weak_risk_provider = risk_provider.clone();
            
            tokio::spawn(async move {
                // Try to upgrade weak references to strong references
                let opportunity_manager = match weak_opportunity_manager.upgrade() {
                    Some(manager) => manager,
                    None => {
                        error!("Opportunity manager has been dropped");
                        return;
                    }
                };
                
                let token_provider = match weak_token_provider.upgrade() {
                    Some(provider) => provider,
                    None => {
                        error!("Token provider has been dropped");
                        return;
                    }
                };
                
                let risk_provider = match weak_risk_provider.upgrade() {
                    Some(provider) => provider,
                    None => {
                        error!("Risk provider has been dropped");
                        return;
                    }
                };
                
                // Clone opportunity before async processing to avoid ownership issues
                let opportunity = opportunity.clone();

                // Use sequential processing with proper error handling
                // First analyze risk
                let risk_result = opportunity.analyze_risk(&*risk_provider).await;
                if let Err(e) = risk_result {
                    error!("Failed to analyze risk for opportunity {}: {}", opportunity.id, e);
                    return;
                }
                
                // Then verify token safety
                match opportunity.verify_token_safety(&*token_provider).await {
                    Ok(true) => {
                        // Safe opportunity, add it to manager
                        let mut manager = opportunity_manager.write().await;
                        manager.add_opportunity(opportunity);
                    },
                    Ok(false) => {
                        info!("Skipping unsafe opportunity: {}", opportunity.id);
                    },
                    Err(e) => {
                        error!("Error checking token safety: {}", e);
                    }
                }
            });
            
            Ok(())
        };
        
        // Get strong reference to mempool provider for registration
        let mempool_provider = match mempool_provider.upgrade() {
            Some(provider) => provider,
            None => return Err(anyhow::anyhow!("Mempool provider has been dropped")),
        };
        
        // Register the callback with the mempool provider
        mempool_provider.register_opportunity_callback(chain_id, Box::new(callback)).await?;
        
        Ok(())
    }
    
    /// Unregister opportunity callback for a specific chain
    async fn unregister_chain_callback(&self, chain_id: u32) -> Result<()> {
        let mut subscription_ids = self.subscription_ids.write().await;
        
        if let Some(subscription_id) = subscription_ids.remove(&chain_id) {
            self.mempool_provider.unsubscribe(&subscription_id).await?;
        }
        
        Ok(())
    }
    
    /// Execute an MEV opportunity
    pub async fn execute_opportunity(&self, opportunity: &MevOpportunity) -> Result<String> {
        // Lấy chain adapter tương ứng
        let chain_adapters = self.chain_adapters.read().await;
        let chain_id = opportunity.chain_id;
        
        let adapter = chain_adapters.get(&chain_id).ok_or_else(|| {
            anyhow!("No adapter found for chain ID {}", chain_id)
        })?;
        
        // Xác định execution method
        let execution_method = match opportunity.method {
            MevExecutionMethod::Flash => {
                // Flash transaction (sử dụng flash loan)
                ExecutionMethod::Flash
            },
            MevExecutionMethod::Bundle => {
                // Bundle transactions
                ExecutionMethod::Bundle
            },
            MevExecutionMethod::StandardTransaction => {
                // Standard transaction
                ExecutionMethod::Standard
            },
            _ => {
                // Default to standard
                ExecutionMethod::Standard
            }
        };
        
        // Chuẩn bị transaction params
        let gas_price = crate::chain_adapters::evm_adapter::get_gas_price_from_ref(adapter).await?;
        let nonce = adapter.get_expected_nonce(self.get_wallet_address(chain_id).await?).await?;
        
        // Tạo và gửi transaction
        let tx_hash = adapter.send_transaction(
            &opportunity.transactions[0].data,
            gas_price,
            Some(nonce),
        ).await?;
        
        // Cập nhật trạng thái opportunity
        self.opportunity_provider.update_opportunity_status(
            opportunity.id.clone(),
            "executed",
            Some(tx_hash.clone()),
        ).await?;
        
        Ok(tx_hash)
    }

    /// Tìm các giao dịch tiềm năng sẽ thất bại trên chuỗi
    async fn find_potential_failing_transactions(&self, chain_id: u32) -> Result<Vec<PotentialFailingTx>> {
        info!("Đang quét các giao dịch có khả năng thất bại trên chain {}", chain_id);
        
        // Lấy adapter cho chain
        let chain_adapters = self.chain_adapters.read().await;
        let adapter = match chain_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                error!("Không có adapter cho chain ID {}. Các chain được hỗ trợ: {:?}", 
                       chain_id, 
                       chain_adapters.keys().collect::<Vec<_>>());
                return Ok(vec![
                    PotentialFailingTx {
                        tx_hash: "none".to_string(),
                        from: "none".to_string(),
                        to: "none".to_string(),
                        gas_limit: 0,
                        estimated_gas: 0,
                        reason: FailureReason::Other("Adapter không được cấu hình cho chain này".to_string()),
                        confidence: 100,
                    }
                ]);
            }
        };
        
        // Lấy mempool analyzer cho chain - sử dụng reference thay vì clone
        let mempool_analyzer = match self.mempool_provider.get_mempool_analyzer(chain_id) {
            Some(analyzer) => analyzer,
            None => {
                let supported_chains = self.mempool_provider.get_supported_chains().await;
                error!("Không có mempool analyzer cho chain ID {}. Các chain được hỗ trợ bởi mempool: {:?}", 
                       chain_id, supported_chains);
                return Ok(vec![
                    PotentialFailingTx {
                        tx_hash: "none".to_string(),
                        from: "none".to_string(),
                        to: "none".to_string(),
                        gas_limit: 0,
                        estimated_gas: 0,
                        reason: FailureReason::Other("Mempool analyzer không được cấu hình cho chain này".to_string()),
                        confidence: 100,
                    }
                ]);
            }
        };
        
        // Lấy giao dịch từ mempool
        let mempool_txs = mempool_analyzer.get_all_pending_transactions().await?;
        
        // Phân tích các giao dịch có khả năng thất bại
        let mut failing_txs = Vec::new();
        
        for tx in mempool_txs {
            // Kiểm tra nếu gas limit quá thấp so với loại giao dịch
            let estimated_gas = adapter.estimate_gas_for_transaction(&tx.hash).await;
            
            if let Ok(estimated) = estimated_gas {
                if let Some(gas_limit) = tx.gas_limit {
                    let buffer_ratio = 1.1; // Yêu cầu buffer 10%
                    if (gas_limit as f64) < estimated * buffer_ratio {
                        failing_txs.push(PotentialFailingTx {
                            tx_hash: tx.hash.clone(),
                            from: tx.from_address.clone(),
                            to: tx.to.clone(),
                            gas_limit,
                            estimated_gas: estimated as u64,
                            reason: FailureReason::InsufficientGas,
                            confidence: 85, // Khá chắc chắn
                        });
                        continue;
                    }
                }
            }
            
            // Kiểm tra nếu token swap với tỷ lệ trượt giá quá thấp
            if tx.is_swap() && tx.slippage.unwrap_or(0.5) < 0.5 {
                if let Some(token_address) = &tx.token_address {
                    let current_price = adapter.get_token_price(token_address).await.unwrap_or(0.0);
                    let price_volatility = self.calculate_token_volatility(token_address, chain_id).await;
                    
                    if price_volatility > 5.0 && tx.slippage.unwrap_or(0.5) < price_volatility / 2.0 {
                        failing_txs.push(PotentialFailingTx {
                            tx_hash: tx.hash.clone(),
                            from: tx.from_address.clone(),
                            to: tx.to.clone(),
                            gas_limit: tx.gas_limit.unwrap_or(0),
                            estimated_gas: 0,
                            reason: FailureReason::SlippageToLow,
                            confidence: 70, // Không chắc chắn bằng gas
                        });
                        continue;
                    }
                }
            }
            
            // Kiểm tra giao dịch với giá gas quá thấp
            if let (Some(gas_price), Ok(current_gas_price)) = (tx.gas_price, crate::chain_adapters::evm_adapter::get_gas_price_from_ref(adapter).await) {
                if gas_price < current_gas_price * 0.8 {
                    failing_txs.push(PotentialFailingTx {
                        tx_hash: tx.hash.clone(),
                        from_address: tx.from_address.clone(),
                        to: tx.to.clone(),
                        gas_limit: tx.gas_limit.unwrap_or(0),
                        estimated_gas: 0,
                        reason: FailureReason::GasPriceTooLow,
                        confidence: 60,
                    });
                    continue;
                }
            }
            
            // Kiểm tra số dư không đủ
            if let (Some(value), Ok(balance)) = (tx.value, adapter.get_balance(&tx.from_address).await) {
                // Tính tổng chi phí = value + (gas_limit * gas_price)
                let gas_limit = tx.gas_limit.unwrap_or(21000) as f64;
                let gas_price = match tx.gas_price {
                    Some(price) => price,
                    None => 1.0,
                };
                let gas_cost = gas_limit * gas_price;
                let total_cost = value + gas_cost;
                
                if balance < total_cost {
                    failing_txs.push(PotentialFailingTx {
                        tx_hash: tx.hash.clone(),
                        from: tx.from_address.clone(),
                        to: tx.to.clone(),
                        gas_limit: tx.gas_limit.unwrap_or(0),
                        estimated_gas: 0,
                        reason: FailureReason::InsufficientBalance,
                        confidence: 95, // Rất chắc chắn
                    });
                }
            }
        }
        
        Ok(failing_txs)
    }
    
    /// Estimate transaction success probability
    async fn estimate_transaction_success_probability(
        &self,
        chain_id: u32,
        transaction: &MempoolTransaction,
        include_details: bool,
    ) -> (f64, Option<String>) {
        // Setup default values
        let mut probability: f64 = 0.5; // 50% default
        let mut details = if include_details { Some(String::new()) } else { None };
        
        // Try to access chain adapter
        let chain_adapters = match self.chain_adapters.read().await {
            Ok(adapters) => adapters,
            Err(_) => return (0.0, Some("Failed to access chain adapters".to_string())),
        };
        
        let adapter = match chain_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return (0.0, Some(format!("No adapter for chain ID {}", chain_id))),
        };
        
        // Calculate factors that affect probability
        
        // 1. Gas price compared to network average
        let network_gas_price = match crate::chain_adapters::evm_adapter::get_gas_price_from_ref(adapter).await {
            Ok(price) => price,
            Err(_) => return (0.0, Some("Failed to get network gas price".to_string())),
        };
        
        let tx_gas_price = transaction.gas_price.unwrap_or(0.0);
        let gas_factor = if tx_gas_price > 0.0 && network_gas_price > 0.0 {
            tx_gas_price / network_gas_price
        } else {
            1.0
        };
        
        // 2. Nonce gap
        let wallet_address = transaction.from_address.clone();
        
        let expected_nonce = match adapter.get_expected_nonce(&wallet_address).await {
            Ok(nonce) => nonce,
            Err(_) => 0, // Use 0 if we can't get the nonce
        };
        
        let tx_nonce = transaction.nonce.unwrap_or(0);
        
        // Nonce too low = failed transaction
        if tx_nonce < expected_nonce {
            return (0.0, Some("Transaction nonce is lower than current nonce".to_string()));
        }
        
        // Nonce gap affects probability
        let nonce_gap = tx_nonce - expected_nonce;
        let nonce_factor = if nonce_gap == 0 {
            1.0  // Ideal
        } else if nonce_gap <= 3 {
            0.9 - (0.1 * nonce_gap as f64) // Small gap
        } else {
            0.5 // Large gap
        };
        
        // 3. Transaction pending time
        let pending_seconds = match &transaction.timestamp {
            Some(ts) => {
                let now = chrono::Utc::now().timestamp() as u64;
                if *ts < now {
                    now - *ts
                } else {
                    0
                }
            },
            None => 0,
        };
        
        let pending_factor = if pending_seconds < 60 {
            1.0 // Fresh transaction
        } else if pending_seconds < 300 {
            0.9 - (pending_seconds as f64 - 60.0) / (300.0 - 60.0) * 0.4 // 1-5 minutes
        } else {
            0.5 // More than 5 minutes
        };
        
        // Calculate final probability based on various factors
        probability = gas_factor * 0.5 + nonce_factor * 0.3 + pending_factor * 0.2;
        
        // Clamp between 0 and 1
        probability = probability.max(0.0).min(1.0);
        
        // Build detailed explanation if requested
        if include_details {
            let mut explanation = Vec::new();
            explanation.push(format!("Gas price factor: {:.2} (tx: {} gwei, network: {} gwei)", 
                gas_factor, tx_gas_price, network_gas_price));
            
            explanation.push(format!("Nonce factor: {:.2} (tx nonce: {}, expected: {}, gap: {})",
                nonce_factor, tx_nonce, expected_nonce, nonce_gap));
                
            explanation.push(format!("Pending time factor: {:.2} (pending for {} seconds)",
                pending_factor, pending_seconds));
                
            details = Some(explanation.join("\n"));
        }
        
        (probability, details)
    }
    
    /// Analyze trader behavior to predict future actions
    async fn analyze_trader_behavior(
        &self,
        chain_id: u32,
        trader_address: &str,
        time_window_sec: u64,
    ) -> Option<TraderBehaviorAnalysis> {
        // Default return value
        let default_behavior = TraderBehaviorAnalysis {
            address: trader_address.to_string(),
            transaction_count: 0,
            unique_tokens_traded: 0,
            risk_appetite: RiskAppetite::Moderate,
            trading_pattern: TradingPattern::Normal,
            average_hold_time_seconds: 0,
            average_position_size_usd: 0.0,
            profit_loss_ratio: 0.0,
            is_bot: false,
            confidence: 0.0,
            last_updated: chrono::Utc::now().timestamp() as u64,
        };
        
        // Access chain adapter
        let chain_adapters = match self.chain_adapters.read().await {
            Ok(adapters) => adapters,
            Err(_) => return Some(default_behavior),
        };
        
        let adapter = match chain_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Some(default_behavior),
        };
        
        // Get token transactions for the trader
        let from_time = chrono::Utc::now().timestamp() as u64 - time_window_sec;
        let to_time = chrono::Utc::now().timestamp() as u64;
        
        let transactions = match adapter.get_address_transactions(trader_address, from_time, to_time, 100).await {
            Ok(txs) => txs,
            Err(_) => return Some(default_behavior),
        };
        
        if transactions.is_empty() {
            return Some(default_behavior);
        }
        
        // Calculate metrics from transactions
        let transaction_count = transactions.len();
        
        // Track unique tokens
        let mut unique_tokens = std::collections::HashSet::new();
        
        // Track trades for hold time analysis
        let mut buys: HashMap<String, (u64, f64)> = HashMap::new(); // token -> (timestamp, amount)
        let mut sells: HashMap<String, Vec<(u64, f64)>> = HashMap::new(); // token -> [(timestamp, amount)]
        
        let mut total_position_size_usd = 0.0;
        let mut total_positions = 0;
        
        // Process transactions
        for tx in &transactions {
            // For swap transactions
            if tx.is_swap() {
                // Identify the token being traded
                if let (Some(from_token), Some(to_token)) = (&tx.from_token, &tx.to_token) {
                    unique_tokens.insert(from_token.address.clone());
                    unique_tokens.insert(to_token.address.clone());
                    
                    // Track buys and sells
                    if from_token.is_base_token {
                        // Buying token
                        buys.insert(to_token.address.clone(), (tx.timestamp.unwrap_or(0), tx.value_usd.unwrap_or(0.0)));
                        
                        total_position_size_usd += tx.value_usd.unwrap_or(0.0);
                        total_positions += 1;
                    } else if to_token.is_base_token {
                        // Selling token
                        let entry = sells.entry(from_token.address.clone()).or_insert_with(Vec::new);
                        entry.push((tx.timestamp.unwrap_or(0), tx.value_usd.unwrap_or(0.0)));
                    }
                }
            }
        }
        
        // Calculate hold times
        let mut total_hold_time = 0;
        let mut hold_count = 0;
        
        for (token, (buy_time, _)) in &buys {
            if let Some(sell_entries) = sells.get(token) {
                for (sell_time, _) in sell_entries {
                    if *sell_time > *buy_time {
                        total_hold_time += sell_time - buy_time;
                        hold_count += 1;
                    }
                }
            }
        }
        
        let average_hold_time = if hold_count > 0 {
            total_hold_time / hold_count
        } else {
            0
        };
        
        let average_position_size = if total_positions > 0 {
            total_position_size_usd / total_positions as f64
        } else {
            0.0
        };
        
        // Determine if the address is a bot
        let potential_bot_indicators = [
            transaction_count > 20, // High tx count
            average_hold_time < 300, // Very short hold times (<5min)
            transactions.iter().any(|tx| tx.gas_price.unwrap_or(0.0) > 1.5 * tx.max_fee_per_gas.unwrap_or(0.0)), // High gas prices
        ];
        
        let bot_score = potential_bot_indicators.iter().filter(|&&x| x).count() as f64 / potential_bot_indicators.len() as f64;
        let is_bot = bot_score > 0.7;
        
        // Determine risk appetite
        let risk_appetite = if average_position_size > 10000.0 || unique_tokens.len() > 30 {
            RiskAppetite::High
        } else if average_position_size > 1000.0 || unique_tokens.len() > 10 {
            RiskAppetite::Moderate
        } else {
            RiskAppetite::Low
        };
        
        // Determine trading pattern
        let unique_tokens_count = unique_tokens.len();
        let trading_pattern = if unique_tokens_count <= 3 {
            TradingPattern::Focused
        } else if unique_tokens_count >= 20 {
            TradingPattern::Diversified
        } else {
            TradingPattern::Normal
        };

        // Create the behavior analysis
        Some(TraderBehaviorAnalysis {
            address: trader_address.to_string(),
            transaction_count,
            unique_tokens_traded: unique_tokens.len(),
            risk_appetite,
            trading_pattern,
            average_hold_time_seconds: average_hold_time,
            average_position_size_usd: average_position_size,
            profit_loss_ratio: 0.0, // Would need more data to calculate accurately
            is_bot,
            confidence: 0.8, // Confidence in our analysis
            last_updated: chrono::Utc::now().timestamp() as u64,
        })
    }
    
    /// Tính toán độ biến động của token trong thời gian gần đây
    async fn calculate_token_volatility(&self, token_address: &str, chain_id: u32) -> f64 {
        // Lấy adapter cho chain
        let adapter = match self.chain_adapters.read().await.get(&chain_id) {
            None => return 0.0,
            Some(adapter) => adapter,
        };
        
        // Lấy lịch sử giá gần đây
        let price_history = match adapter.get_token_price_history(token_address, 24).await {
            Ok(history) => history,
            Err(_) => return 0.0,
        };
        
        // Nếu không có đủ dữ liệu, trả về giá trị mặc định
        if price_history.len() < 2 {
            return 0.0;
        }
        
        // Tính volatility dựa trên độ lệch chuẩn của % thay đổi giá
        let mut percent_changes = Vec::new();
        for i in 1..price_history.len() {
            let prev_price = price_history[i-1];
            let curr_price = price_history[i];
            
            if prev_price <= 0.0 {
                continue;
            }
            
            let percent_change = (curr_price - prev_price) / prev_price * 100.0;
            percent_changes.push(percent_change);
        }
        
        // Tính trung bình
        let mean = percent_changes.iter().sum::<f64>() / percent_changes.len() as f64;
        
        // Tính độ lệch chuẩn
        let variance = percent_changes.iter()
            .map(|x| (*x - mean).powi(2))
            .sum::<f64>() / percent_changes.len() as f64;
        
        let volatility = variance.sqrt();
        
        volatility
    }
    
    /// Theo dõi thanh khoản của token để phát hiện cơ hội
    async fn monitor_token_liquidity(&self, chain_id: u32, token_address: &str) {
        info!("Monitoring liquidity for token {} on chain {}", token_address, chain_id);
        
        // Get adapter for chain
        let adapter_opt = self.chain_adapters.read().await.get(&chain_id).cloned();
        let adapter = match adapter_opt {
            Some(adapter) => adapter,
            None => {
                warn!("No adapter found for chain ID {}", chain_id);
                return;
            }
        };
        
        // Get token metadata
        let (name, symbol) = match adapter.get_token_metadata(token_address).await {
            Ok((name, symbol)) => (name, symbol),
            Err(e) => {
                error!("Failed to get token metadata: {}", e);
                return;
            }
        };
        
        info!("Starting liquidity monitoring for {}/{} ({})", name, symbol, token_address);
        
        // Get initial liquidity
        let initial_liquidity = match adapter.get_token_liquidity(token_address).await {
            Ok(liquidity) => liquidity,
            Err(e) => {
                error!("Failed to get initial liquidity: {}", e);
                return;
            }
        };
        
        info!("Initial liquidity for {}: ${:.2}", symbol, initial_liquidity);
        
        // Start a background task to monitor liquidity
        let token_address_clone = token_address.to_string();
        let chain_id_clone = chain_id;
        
        tokio::spawn(async move {
            let check_interval = 60; // 1 minute
            let mut previous_liquidity = initial_liquidity;
            let mut last_check = chrono::Utc::now().timestamp() as u64;
            
            // Monitor for 4 hours max
            let end_time = last_check + 4 * 60 * 60;
            
            while chrono::Utc::now().timestamp() as u64 < end_time {
                // Wait for next check
                tokio::time::sleep(std::time::Duration::from_secs(check_interval)).await;
                
                // Get current liquidity
                let current_liquidity = match adapter.get_token_liquidity(token_address_clone.as_str()).await {
                    Ok(liquidity) => liquidity,
                    Err(e) => {
                        error!("Failed to get liquidity for {}: {}", token_address_clone, e);
                        continue;
                    }
                };
                
                // Calculate percent change
                let percent_change = if previous_liquidity > 0.0 {
                    ((current_liquidity - previous_liquidity) / previous_liquidity) * 100.0
                } else {
                    0.0
                };
                
                // Log significant changes
                if percent_change.abs() >= 5.0 {
                    if percent_change > 0.0 {
                        info!("Liquidity increased by {:.2}% for {} (${:.2} -> ${:.2})", 
                              percent_change, symbol, previous_liquidity, current_liquidity);
                    } else {
                        warn!("Liquidity decreased by {:.2}% for {} (${:.2} -> ${:.2})", 
                             percent_change.abs(), symbol, previous_liquidity, current_liquidity);
                        
                        // If liquidity drops by more than 40%, emit warning
                        if percent_change <= -40.0 {
                            error!("CRITICAL: Large liquidity reduction (-{:.2}%) for {}. Possible rug pull!", 
                                  percent_change.abs(), symbol);
                        }
                    }
                }
                
                previous_liquidity = current_liquidity;
                last_check = chrono::Utc::now().timestamp() as u64;
            }
            
            info!("Completed liquidity monitoring for {}/{}", name, symbol);
        });
    }
    
    /// Tạo cơ hội từ sự thay đổi thanh khoản
    async fn create_liquidity_opportunity(&self, token_address: &str, chain_id: u32, change_percent: f64) -> Result<()> {
        let adapter = match self.chain_adapters.read().await.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                return Err(anyhow::anyhow!("Không có adapter cho chain ID {}", chain_id));
            }
        };
        
        // Lấy thông tin token
        let (name, symbol) = adapter.get_token_metadata(token_address).await?;
        
        // Kiểm tra các yếu tố an toàn
        let risk_score = adapter.get_token_risk_score(token_address).await?;
        
        if risk_score.risk_score > 70 {
            info!("Không tạo cơ hội cho token {} do rủi ro cao: {}", symbol, risk_score.risk_score);
            return Ok(());
        }
        
        // Tính ước lượng lợi nhuận tiềm năng
        let potential_profit = change_percent * 0.2; // Giả định có thể lợi dụng được 20% của sự thay đổi
        
        // Tạo cơ hội nếu lợi nhuận đủ cao
        if potential_profit > 5.0 {
            let opportunity = MevOpportunity {
                id: Uuid::new_v4().to_string(),
                chain_id,
                token_address: token_address.to_string(),
                token_name: name,
                token_symbol: symbol,
                opportunity_type: MevOpportunityType::LiquidityChange,
                estimated_profit_usd: potential_profit,
                estimated_net_profit_usd: potential_profit * 0.8, // Trừ chi phí
                risk_score: risk_score.risk_score as u8,
                risk_factors: risk_score.factors.iter().map(|f| f.description.clone()).collect(),
                created_at: Utc::now().timestamp() as u64,
                expires_at: Utc::now().timestamp() as u64 + 30 * 60, // 30 phút
                executed: false,
                execution_status: TradeStatus::Pending,
                specific_params: HashMap::new(),
            };
            
            info!("Đã tạo cơ hội thanh khoản cho token {}: ID {}, lợi nhuận ước tính ${:.2}", 
                 symbol, opportunity.id, opportunity.estimated_profit_usd);
            
            // Thêm vào danh sách cơ hội
            let mut opportunities = self.opportunity_manager.write().await.opportunities.write().await;
            opportunities.push(opportunity);
            
            // Giới hạn số lượng cơ hội để quản lý bộ nhớ
            if opportunities.len() > 1000 {
                opportunities.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                opportunities.truncate(1000);
            }
        }
        
        Ok(())
    }

    /// Analyze trading opportunity from evaluated token
    async fn analyze_trading_opportunity(&self, chain_id: u32, token_address: &str, token_safety: TokenSafety) -> Option<MevOpportunity> {
        // Delegate to the implementation function
        self.analyze_trading_opportunity(chain_id, token_address, token_safety).await
    }

    /// Monitor token liquidity
    async fn monitor_token_liquidity(&self, chain_id: u32, token_address: &str) {
        // Delegate to the implementation function
        self.monitor_token_liquidity(chain_id, token_address).await;
    }

    /// Estimate success probability of a specific transaction
    async fn estimate_transaction_success_probability(
        &self,
        chain_id: u32,
        transaction: &MempoolTransaction,
        include_details: bool,
    ) -> (f64, Option<String>) {
        // Delegate to the implementation function
        self.estimate_transaction_success_probability(chain_id, transaction, include_details).await
    }

    /// Analyze specific trader behavior to predict behavior
    async fn analyze_trader_behavior(
        &self,
        chain_id: u32,
        trader_address: &str,
        time_window_sec: u64,
    ) -> Option<TraderBehaviorAnalysis> {
        // Delegate to the implementation function
        self.analyze_trader_behavior(chain_id, trader_address, time_window_sec).await
    }

    /// Get wallet address for a specific chain
    pub async fn get_wallet_address(&self, chain_id: u32) -> Result<String> {
        let config = self.config.read().await;
        let default_wallet = "0x0000000000000000000000000000000000000000".to_string();
        
        // Get from config
        let wallet = config.chain_configs
            .get(&chain_id)
            .and_then(|c| c.wallet_address.clone())
            .unwrap_or(default_wallet);
            
        Ok(wallet)
    }
}

#[async_trait::async_trait]
impl MevBot for MevBotImpl {
    async fn start(&self) {
        let mut is_active = self.is_active.write().await;
        if *is_active {
            return; // Already running
        }
        
        *is_active = true;
        
        // Register callbacks for all chains
        let chain_adapters = self.chain_adapters.read().await;
        for chain_id in chain_adapters.keys() {
            if let Err(e) = self.register_chain_callback(*chain_id).await {
                eprintln!("Error registering callback for chain {}: {}", chain_id, e);
            }
        }
    }
    
    async fn stop(&self) {
        let mut is_active = self.is_active.write().await;
        if !*is_active {
            return; // Already stopped
        }
        
        *is_active = false;
        
        // Unregister callbacks for all chains
        let subscription_ids = self.subscription_ids.read().await;
        for (chain_id, _) in subscription_ids.iter() {
            if let Err(e) = self.unregister_chain_callback(*chain_id).await {
                eprintln!("Error unregistering callback for chain {}: {}", chain_id, e);
            }
        }
    }
    
    async fn update_config(&self, config: MevConfig) {
        let mut current_config = self.config.write().await;
        *current_config = config;
    }
    
    async fn get_opportunities(&self) -> Vec<MevOpportunity> {
        let mut opportunities = Vec::new();
        
        // Get opportunities from all configured chains
        let chain_adapters = self.chain_adapters.read().await;
        for chain_id in chain_adapters.keys() {
            if let Ok(chain_opportunities) = self.opportunity_provider.get_all_opportunities(*chain_id).await {
                opportunities.extend(chain_opportunities);
            }
        }
        
        opportunities
    }
    
    async fn add_chain(&mut self, chain_id: u32, adapter: Arc<EvmAdapter>) {
        let mut chain_adapters = self.chain_adapters.write().await;
        chain_adapters.insert(chain_id, adapter);
    }
    
    async fn evaluate_new_token(&self, chain_id: u32, token_address: &str) -> Option<TokenSafety> {
        match self.token_provider.get_token_safety(chain_id, token_address).await {
            Ok(safety) => safety,
            Err(e) => {
                eprintln!("Error evaluating token {}: {}", token_address, e);
                None
            }
        }
    }
    
    async fn detect_suspicious_transaction_patterns(&self, chain_id: u32, transactions: &[MempoolTransaction]) -> Vec<SuspiciousPattern> {
        match self.mempool_provider.detect_suspicious_patterns(chain_id, transactions).await {
            Ok(patterns) => patterns,
            Err(_) => Vec::new(),
        }
    }
    
    async fn find_potential_failing_transactions(
        &self,
        chain_id: u32,
        include_nonce_gaps: bool,
        include_low_gas: bool,
        include_long_pending: bool,
        min_wait_time_sec: u64,
        limit: usize,
    ) -> Vec<(MempoolTransaction, String)> {
        // Implementation here...
        Vec::new()
    }
    
    /// Analyze trading opportunity from evaluated token
    async fn analyze_trading_opportunity(&self, chain_id: u32, token_address: &str, token_safety: TokenSafety) -> Option<MevOpportunity> {
        // Delegate to the implementation function
        self.analyze_trading_opportunity(chain_id, token_address, token_safety).await
    }
    
    /// Monitor token liquidity
    async fn monitor_token_liquidity(&self, chain_id: u32, token_address: &str) {
        // Delegate to the implementation function
        self.monitor_token_liquidity(chain_id, token_address).await;
    }
    
    /// Estimate success probability of a specific transaction
    async fn estimate_transaction_success_probability(
        &self,
        chain_id: u32,
        transaction: &MempoolTransaction,
        include_details: bool,
    ) -> (f64, Option<String>) {
        // Delegate to the implementation function
        self.estimate_transaction_success_probability(chain_id, transaction, include_details).await
    }
    
    async fn analyze_trader_behavior(
        &self,
        chain_id: u32,
        trader_address: &str,
        time_window_sec: u64,
    ) -> Option<TraderBehaviorAnalysis> {
        // Delegate to the implementation function
        self.analyze_trader_behavior(chain_id, trader_address, time_window_sec).await
    }
}

/// Factory function to create a new MevBot
pub fn create_mev_bot(
    mempool_provider: Arc<dyn MempoolAnalysisProvider>,
    token_provider: Arc<dyn TokenAnalysisProvider>,
    risk_provider: Arc<dyn RiskAnalysisProvider>,
    opportunity_provider: Arc<dyn MevOpportunityProvider>,
    config: MevConfig,
) -> Arc<dyn MevBot> {
    Arc::new(MevBotImpl::new(
        mempool_provider,
        token_provider,
        risk_provider,
        opportunity_provider,
        config,
    ))
} 