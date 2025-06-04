/// MEV Bot trait definition and implementations
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use anyhow::{Result, anyhow};
use tracing::info;
use tokio::join;

use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::{MempoolTransaction, SuspiciousPattern};
use crate::analys::token_status::TokenSafety;
use crate::tradelogic::traits::{
    MempoolAnalysisProvider,
    TokenAnalysisProvider,
    RiskAnalysisProvider,
    MevOpportunityProvider
};
use crate::types::TradeParams;

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
        // Example implementation for executing an opportunity
        let chain_id = opportunity.chain_id;
        
        let chain_adapters = self.chain_adapters.read().await;
        let adapter = chain_adapters.get(&chain_id)
            .ok_or_else(|| anyhow!("No chain adapter found for chain ID {}", chain_id))?;
        
        // Create a trade params object from the opportunity
        let params = TradeParams {
            // Fill in parameters based on opportunity details
            chain_id,
            token_address: opportunity.token_pairs[0].token0.address.clone(),
            amount: 0.0, // Calculate based on opportunity
            // Fill in other required parameters
            ..Default::default()
        };
        
        // Execute the trade using the chain adapter
        let result = adapter.execute_transaction(&params).await?;
        
        // Update opportunity status
        let mut opportunity_manager = self.opportunity_manager.write().await;
        opportunity_manager.mark_as_executed(
            &opportunity.id,
            result.clone()
        )?;
        
        Ok(result)
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
                            from: tx.from.clone(),
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
                            from: tx.from.clone(),
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
            if let (Some(gas_price), Ok(current_gas_price)) = (tx.gas_price, adapter.get_gas_price().await) {
                if gas_price < current_gas_price * 0.8 {
                    failing_txs.push(PotentialFailingTx {
                        tx_hash: tx.hash.clone(),
                        from: tx.from.clone(),
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
            if let (Some(value), Ok(balance)) = (tx.value, adapter.get_balance(&tx.from).await) {
                // Tính tổng chi phí = value + (gas_limit * gas_price)
                let gas_cost = tx.gas_limit.unwrap_or(21000) as f64 * tx.gas_price.unwrap_or(1.0);
                let total_cost = value + gas_cost;
                
                if balance < total_cost {
                    failing_txs.push(PotentialFailingTx {
                        tx_hash: tx.hash.clone(),
                        from: tx.from.clone(),
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
    
    /// Ước tính xác suất thành công của một giao dịch
    async fn estimate_transaction_success_probability(
        &self,
        chain_id: u32,
        transaction: &MempoolTransaction,
        include_details: bool,
    ) -> (f64, Option<String>) {
        info!("Estimating success probability for transaction {}", transaction.hash);
        
        // Get adapter for chain
        let adapter = match self.chain_adapters.read().await.get(&chain_id) {
            Some(adapter) => adapter.clone(),
            None => {
                warn!("No adapter found for chain ID {}", chain_id);
                return (0.0, Some("No chain adapter found".to_string()));
            }
        };
        
        // Initialize base probability (85% by default - most transactions succeed)
        let mut success_probability = 85.0;
        let mut details = Vec::new();
        
        // Check gas limit
        if let Ok(estimated_gas) = adapter.estimate_gas_for_transaction(&transaction.hash).await {
            if let Some(gas_limit) = transaction.gas_limit {
                if (gas_limit as f64) < estimated_gas {
                    success_probability -= 50.0;
                    details.push(format!("Gas limit too low: {} < {}", gas_limit, estimated_gas));
                } else if (gas_limit as f64) < estimated_gas * 1.1 {
                    success_probability -= 20.0;
                    details.push(format!("Gas limit risky: {} < {} + 10%", gas_limit, estimated_gas));
                }
            }
        }
        
        // Check gas price
        if let Ok(current_gas_price) = adapter.get_gas_price().await {
            if let Some(tx_gas_price) = transaction.gas_price {
                if tx_gas_price < current_gas_price * 0.8 {
                    let percent_decrease = ((current_gas_price - tx_gas_price) / current_gas_price) * 100.0;
                    success_probability -= 15.0 + percent_decrease.min(35.0);
                    details.push(format!("Gas price low: {} vs network {}", tx_gas_price, current_gas_price));
                }
            }
        }
        
        // Check balance
        if let Ok(balance) = adapter.get_balance(&transaction.from).await {
            let gas_cost = (transaction.gas_limit.unwrap_or(21000) as f64) * (transaction.gas_price.unwrap_or(1.0));
            let tx_value = transaction.value.unwrap_or(0.0);
            let total_cost = tx_value + gas_cost;
            
            if balance < total_cost {
                success_probability -= 80.0;
                details.push(format!("Insufficient balance: {} < {}", balance, total_cost));
            } else if balance < total_cost * 1.05 {
                success_probability -= 10.0;
                details.push(format!("Very low balance margin: {} vs needed {}", balance, total_cost));
            }
        }
        
        // Check nonce validity
        if let (Some(nonce), Ok(expected_nonce)) = (transaction.nonce, adapter.get_expected_nonce(&transaction.from).await) {
            if nonce < expected_nonce {
                success_probability -= 95.0;
                details.push(format!("Nonce too low: {} < expected {}", nonce, expected_nonce));
            } else if nonce > expected_nonce + 2 {
                success_probability -= 40.0;
                details.push(format!("Nonce gap: {} > expected {}", nonce, expected_nonce));
            }
        }
        
        // Check slippage for swaps
        if transaction.is_swap() {
            let slippage = transaction.slippage.unwrap_or(0.5);
            
            if let Some(token_addr) = &transaction.token_address {
                let volatility = self.calculate_token_volatility(token_addr, chain_id).await;
                
                if volatility > slippage * 2.0 {
                    success_probability -= 30.0;
                    details.push(format!("High volatility vs slippage: {}% vs {}%", volatility, slippage));
                }
            }
        }
        
        // Cap probability to 0-100%
        success_probability = success_probability.max(0.0).min(100.0);
        
        // Return with or without details
        if include_details && !details.is_empty() {
            (success_probability, Some(details.join("; ")))
        } else {
            (success_probability, None)
        }
    }
    
    /// Phân tích hành vi của trader để phát hiện mẫu giao dịch
    async fn analyze_trader_behavior(
        &self,
        chain_id: u32,
        trader_address: &str,
        time_window_sec: u64,
    ) -> Option<TraderBehaviorAnalysis> {
        info!("Analyzing trader behavior for {} on chain {}", trader_address, chain_id);
        
        // Get adapter for chain
        let adapter = match self.chain_adapters.read().await.get(&chain_id) {
            Some(adapter) => adapter.clone(),
            None => {
                warn!("No adapter found for chain ID {}", chain_id);
                return None;
            }
        };
        
        // Get transaction history for address within time window
        let now = chrono::Utc::now().timestamp() as u64;
        let from_time = now.saturating_sub(time_window_sec);
        
        let transactions = match adapter.get_address_transactions_in_range(trader_address, from_time, now).await {
            Ok(txs) => txs,
            Err(e) => {
                error!("Failed to get transaction history: {}", e);
                return None;
            }
        };
        
        // Not enough transactions for meaningful analysis
        if transactions.len() < 5 {
            debug!("Not enough transactions for address {} to analyze (only {})", trader_address, transactions.len());
            return None;
        }
        
        // Analyze transactions
        let total_value: f64 = transactions.iter().filter_map(|tx| tx.value).sum();
        let avg_value = total_value / transactions.len() as f64;
        
        // Count successes and failures
        let successful_txs = transactions.iter().filter(|tx| tx.status.is_success()).count();
        let failed_txs = transactions.len() - successful_txs;
        
        // Calculate transaction frequency (per hour)
        let tx_frequency = if time_window_sec > 0 {
            (transactions.len() as f64 / time_window_sec as f64) * 3600.0
        } else {
            0.0
        };
        
        // Analyze gas behavior
        let gas_prices: Vec<f64> = transactions.iter().filter_map(|tx| tx.gas_price).collect();
        let avg_gas_price = if !gas_prices.is_empty() {
            gas_prices.iter().sum::<f64>() / gas_prices.len() as f64
        } else {
            0.0
        };
        
        let max_gas_price = gas_prices.iter().fold(0.0, |max, &price| max.max(price));
        let min_gas_price = gas_prices.iter().fold(f64::MAX, |min, &price| min.min(price));
        
        // Determine if trader has gas strategy
        let gas_strategy = max_gas_price > avg_gas_price * 1.5;
        
        // Success rate
        let success_rate = if !transactions.is_empty() {
            successful_txs as f64 / transactions.len() as f64
        } else {
            0.0
        };
        
        // Find common tokens and DEXes
        let mut token_counts: HashMap<String, usize> = HashMap::new();
        let mut dex_counts: HashMap<String, usize> = HashMap::new();
        
        for tx in &transactions {
            if let Some(token) = &tx.token_address {
                *token_counts.entry(token.clone()).or_insert(0) += 1;
            }
            
            if let Some(dex) = &tx.dex {
                *dex_counts.entry(dex.clone()).or_insert(0) += 1;
            }
        }
        
        // Sort tokens and dexes by frequency
        let mut token_counts: Vec<(String, usize)> = token_counts.into_iter().collect();
        token_counts.sort_by(|a, b| b.1.cmp(&a.1));
        
        let mut dex_counts: Vec<(String, usize)> = dex_counts.into_iter().collect();
        dex_counts.sort_by(|a, b| b.1.cmp(&a.1));
        
        // Find active hours
        let mut hours: HashSet<u8> = HashSet::new();
        for tx in &transactions {
            if let Some(timestamp) = tx.timestamp {
                // Convert timestamp to UTC hour (0-23)
                let dt = DateTime::<Utc>::from_timestamp(timestamp as i64, 0).expect("Valid timestamp");
                hours.insert(dt.hour() as u8);
            }
        }
        
        // Determine trader type and expertise level
        let (behavior_type, expertise_level) = if transactions.len() > 50 && gas_strategy && successful_txs > failed_txs * 10 {
            // Likely a bot or professional trader
            if tx_frequency > 10.0 {
                (TraderBehaviorType::MevBot, TraderExpertiseLevel::Automated)
            } else {
                (TraderBehaviorType::HighFrequencyTrader, TraderExpertiseLevel::Professional)
            }
        } else if avg_value > 10.0 { // Value in ETH
            (TraderBehaviorType::Whale, TraderExpertiseLevel::Professional)
        } else if tx_frequency > 5.0 {
            (TraderBehaviorType::Retail, TraderExpertiseLevel::Intermediate)
        } else {
            (TraderBehaviorType::Retail, TraderExpertiseLevel::Beginner)
        };
        
        // Return the analysis
        Some(TraderBehaviorAnalysis {
            address: trader_address.to_string(),
            behavior_type,
            expertise_level,
            transaction_frequency: tx_frequency,
            average_transaction_value: avg_value,
            common_transaction_types: Vec::new(), // Would need to be extracted from transactions
            gas_behavior: GasBehavior {
                average_gas_price: avg_gas_price,
                highest_gas_price: max_gas_price,
                lowest_gas_price: min_gas_price,
                has_gas_strategy: gas_strategy,
                success_rate,
            },
            frequently_traded_tokens: token_counts.into_iter()
                .take(5)
                .map(|(token, _)| token)
                .collect(),
            preferred_dexes: dex_counts.into_iter()
                .take(3)
                .map(|(dex, _)| dex)
                .collect(),
            active_hours: hours.into_iter().collect(),
            prediction_score: 70.0, // Moderate confidence
            additional_notes: None,
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

    async fn analyze_trading_opportunity(&self, chain_id: u32, token_address: &str, token_safety: TokenSafety) -> Option<MevOpportunity> {
        use tracing::info;
        use tokio::join;
        
        // Kiểm tra an toàn của token trước khi thực hiện các truy vấn khác
        if !token_safety.is_safe || token_safety.risk_score > 50 {
            info!("Token {} không đủ an toàn để giao dịch: risk_score={}", token_address, token_safety.risk_score);
            return None; // Too risky
        }
        
        // Thực hiện nhiều truy vấn song song để cải thiện hiệu suất
        let adapter = match self.chain_adapters.read().await.get(&chain_id).cloned() {
            Some(adapter) => adapter,
            None => {
                info!("Không tìm thấy adapter cho chain ID {}", chain_id);
                return None;
            }
        };
        
        // Chuẩn bị các Future để thực hiện song song
        let liquidity_future = adapter.get_token_liquidity(token_address);
        let price_future = adapter.get_token_price(token_address);
        let price_history_future = adapter.get_token_price_history(token_address, 24);
        let market_data_future = self.token_provider.get_token_market_data(chain_id, token_address);
        
        // Thực hiện các truy vấn song song
        let (liquidity_result, price_result, price_history_result, market_data_result) = 
            join!(liquidity_future, price_future, price_history_future, market_data_future);
        
        // Xử lý kết quả truy vấn liquidity
        let liquidity = match liquidity_result {
            Ok(liquidity) => liquidity,
            Err(e) => {
                info!("Không thể lấy thông tin thanh khoản cho token {}: {}", token_address, e);
                return None; // Không đủ thông tin để phân tích
            }
        };
        
        // Kiểm tra thanh khoản tối thiểu
        if liquidity < 10000.0 { // $10,000 thanh khoản tối thiểu
            info!("Token {} có thanh khoản thấp: ${}", token_address, liquidity);
            return None; // Thanh khoản quá thấp
        }
        
        // Xử lý kết quả truy vấn giá
        let price = price_result.unwrap_or_default();
        
        // Xử lý kết quả truy vấn lịch sử giá
        let price_history = price_history_result.unwrap_or_default();
        
        // Tính toán biến động giá
        let volatility = if price_history.len() >= 2 {
            let max_price = price_history.iter().fold(0.0f64, |a, b| a.max(*b));
            let min_price = price_history.iter().fold(f64::INFINITY, |a, b| a.min(*b));
            if min_price > 0.0 {
                (max_price - min_price) / min_price
            } else {
                0.0
            }
        } else {
            0.0
        };
        
        // Xử lý kết quả truy vấn dữ liệu thị trường
        let market_data = market_data_result.ok();
        
        // Đánh giá cơ hội dựa trên các yếu tố thu thập được
        // Tính toán lợi nhuận ước tính (ví dụ đơn giản)
        let estimated_profit = if volatility > 0.05 && liquidity > 50000.0 {
            // Tạo cơ hội giao dịch nếu biến động giá > 5% và thanh khoản > $50,000
            info!("Đã phát hiện cơ hội giao dịch cho token {}: biến động={}%, thanh khoản=${}", 
                  token_address, volatility * 100.0, liquidity);
            
            // Tính toán lợi nhuận dựa trên biến động giá và thanh khoản
            let estimated_profit_usd = volatility * 100.0; // Đơn giản hóa tính toán lợi nhuận
            let estimated_gas_cost_usd = 50.0; // Ước tính chi phí gas
            
            // Tạo cặp token
            let token_pairs = vec![
                TokenPair {
                    token0: TokenInfo {
                        address: token_address.clone(),
                        symbol: "UNKNOWN".to_string(),
                        decimals: 18,
                    },
                    token1: TokenInfo {
                        address: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string(), // WETH
                        symbol: "WETH".to_string(),
                        decimals: 18,
                    },
                }
            ];
            
            // Tạo cơ hội MEV
            Some(MevOpportunity::new(
                MevOpportunityType::Arbitrage,
                chain_id,
                estimated_profit_usd,
                estimated_gas_cost_usd,
                token_pairs,
                30, // Risk score
                MevExecutionMethod::Standard,
            ))
        } else {
            None
        };
        
        estimated_profit
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
        
        // If active, register callback for this chain
        let is_active = *self.is_active.read().await;
        if is_active {
            drop(chain_adapters); // Release lock before async call
            if let Err(e) = self.register_chain_callback(chain_id).await {
                eprintln!("Error registering callback for chain {}: {}", chain_id, e);
            }
        }
    }
    
    async fn evaluate_new_token(&self, chain_id: u32, token_address: &str) -> Option<TokenSafety> {
        match self.token_provider.analyze_token_safety(chain_id, token_address).await {
            Ok(safety) => Some(safety),
            Err(e) => {
                eprintln!("Error evaluating token {}: {}", token_address, e);
                None
            }
        }
    }
    
    async fn detect_suspicious_transaction_patterns(&self, chain_id: u32, transactions: &[MempoolTransaction]) -> Vec<SuspiciousPattern> {
        // This is a placeholder implementation
        Vec::new()
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
        info!("Scanning for potential failing transactions on chain {}", chain_id);
        
        // Lấy adapter cho chain
        let adapter_opt = self.chain_adapters.read().await.get(&chain_id).cloned();
        let adapter = match adapter_opt {
            Some(adapter) => adapter,
            None => {
                // Log chi tiết hơn khi không tìm thấy adapter
                let error_msg = format!("Không tìm thấy adapter cho chain ID {}. Cần đăng ký adapter trước khi sử dụng.", chain_id);
                error!(
                    "{}. Không thể tiếp tục quét các giao dịch có khả năng thất bại. \
                     Available chain IDs: {:?}",
                    error_msg,
                    self.chain_adapters.read().await.keys().collect::<Vec<_>>()
                );
                return vec![(
                    MempoolTransaction::default_with_metadata(
                        "error".to_string(),
                        "system".to_string(),
                        "system".to_string(),
                        Some(chrono::Utc::now().timestamp() as u64),
                    ),
                    error_msg
                )];
            }
        };
        
        // Lấy mempool analyzer cho chain
        let mempool_analyzer_opt = self.mempool_provider.get_mempool_analyzer(chain_id);
        let mempool_analyzer = match mempool_analyzer_opt {
            Some(analyzer) => analyzer.clone(),
            None => {
                // Log chi tiết hơn khi không tìm thấy mempool analyzer
                let supported_chains = self.mempool_provider.get_supported_chains();
                let error_msg = format!(
                    "Không tìm thấy mempool analyzer cho chain ID {}. Chain này có thể không được mempool provider hỗ trợ.",
                    chain_id
                );
                error!(
                    "{}. Không thể quét các giao dịch mempool. \
                     Các chain được hỗ trợ: {:?}. \
                     Vui lòng kiểm tra cấu hình mempool provider.",
                    error_msg, supported_chains
                );
                return vec![(
                    MempoolTransaction::default_with_metadata(
                        "error".to_string(), 
                        "system".to_string(), 
                        "system".to_string(),
                        Some(chrono::Utc::now().timestamp() as u64),
                    ),
                    error_msg
                )];
            }
        };
        
        // Lấy giao dịch từ mempool
        let mempool_txs = match mempool_analyzer.get_all_pending_transactions().await {
            Ok(txs) => txs,
            Err(e) => {
                // Log chi tiết hơn khi không lấy được giao dịch mempool
                let error_msg = format!("Lỗi khi lấy giao dịch từ mempool: {}", e);
                error!(
                    "{}. Chi tiết lỗi: {}. \
                     Vui lòng kiểm tra kết nối RPC, cấu hình mempool, và trạng thái node.",
                    error_msg, e
                );
                return vec![(
                    MempoolTransaction::default_with_metadata(
                        "error".to_string(), 
                        "system".to_string(), 
                        "system".to_string(),
                        Some(chrono::Utc::now().timestamp() as u64),
                    ),
                    error_msg
                )];
            }
        };
        
        // Log số lượng giao dịch tìm thấy để dễ theo dõi
        info!("Tìm thấy {} giao dịch đang chờ trên chain {}", mempool_txs.len(), chain_id);
        
        // Phân tích các giao dịch có khả năng thất bại
        let mut failing_txs = Vec::new();
        let now = chrono::Utc::now().timestamp() as u64;
        
        for tx in mempool_txs {
            let mut fail_reasons = Vec::new();
            
            // Check for nonce gaps if requested
            if include_nonce_gaps {
                if let (Some(nonce), Ok(expected_nonce)) = (tx.nonce, adapter.get_expected_nonce(&tx.from).await) {
                    if nonce > expected_nonce + 1 {
                        fail_reasons.push(format!("Nonce gap: tx has {} but expected {}", nonce, expected_nonce));
                    }
                }
            }
            
            // Check for low gas price if requested
            if include_low_gas {
                if let (Some(gas_price), Ok(current_gas_price)) = (tx.gas_price, adapter.get_gas_price().await) {
                    if gas_price < current_gas_price * 0.8 {
                        fail_reasons.push(format!("Low gas price: {} vs network {}", gas_price, current_gas_price));
                    }
                }
            }
            
            // Check for long pending transactions if requested
            if include_long_pending && tx.timestamp.is_some() {
                let tx_time = tx.timestamp.unwrap();
                if now - tx_time > min_wait_time_sec {
                    fail_reasons.push(format!("Pending for {}s (limit: {}s)", now - tx_time, min_wait_time_sec));
                }
            }
            
            // Add to result if any reasons found
            if !fail_reasons.is_empty() {
                // Combine all reasons
                let reason = fail_reasons.join("; ");
                failing_txs.push((tx, reason));
                
                // Limit results if needed
                if failing_txs.len() >= limit {
                    break;
                }
            }
        }
        
        // Log kết quả để dễ theo dõi
        if failing_txs.is_empty() {
            info!("Không tìm thấy giao dịch có khả năng thất bại trên chain {}", chain_id);
        } else {
            info!("Tìm thấy {} giao dịch có khả năng thất bại trên chain {}", failing_txs.len(), chain_id);
        }
        
        failing_txs
    }
    
    async fn analyze_trader_behavior(
        &self,
        chain_id: u32,
        trader_address: &str,
        time_window_sec: u64,
    ) -> Option<TraderBehaviorAnalysis> {
        info!("Analyzing trader behavior for {} on chain {}", trader_address, chain_id);
        
        // Get adapter for chain
        let adapter = match self.chain_adapters.read().await.get(&chain_id) {
            Some(adapter) => adapter.clone(),
            None => {
                warn!("No adapter found for chain ID {}", chain_id);
                return None;
            }
        };
        
        // Get transaction history for address within time window
        let now = chrono::Utc::now().timestamp() as u64;
        let from_time = now.saturating_sub(time_window_sec);
        
        let transactions = match adapter.get_address_transactions_in_range(trader_address, from_time, now).await {
            Ok(txs) => txs,
            Err(e) => {
                error!("Failed to get transaction history: {}", e);
                return None;
            }
        };
        
        // Not enough transactions for meaningful analysis
        if transactions.len() < 5 {
            debug!("Not enough transactions for address {} to analyze (only {})", trader_address, transactions.len());
            return None;
        }
        
        // Analyze transactions
        let total_value: f64 = transactions.iter().filter_map(|tx| tx.value).sum();
        let avg_value = total_value / transactions.len() as f64;
        
        // Count successes and failures
        let successful_txs = transactions.iter().filter(|tx| tx.status.is_success()).count();
        let failed_txs = transactions.len() - successful_txs;
        
        // Calculate transaction frequency (per hour)
        let tx_frequency = if time_window_sec > 0 {
            (transactions.len() as f64 / time_window_sec as f64) * 3600.0
        } else {
            0.0
        };
        
        // Analyze gas behavior
        let gas_prices: Vec<f64> = transactions.iter().filter_map(|tx| tx.gas_price).collect();
        let avg_gas_price = if !gas_prices.is_empty() {
            gas_prices.iter().sum::<f64>() / gas_prices.len() as f64
        } else {
            0.0
        };
        
        let max_gas_price = gas_prices.iter().fold(0.0, |max, &price| max.max(price));
        let min_gas_price = gas_prices.iter().fold(f64::MAX, |min, &price| min.min(price));
        
        // Determine if trader has gas strategy
        let gas_strategy = max_gas_price > avg_gas_price * 1.5;
        
        // Success rate
        let success_rate = if !transactions.is_empty() {
            successful_txs as f64 / transactions.len() as f64
        } else {
            0.0
        };
        
        // Find common tokens and DEXes
        let mut token_counts: HashMap<String, usize> = HashMap::new();
        let mut dex_counts: HashMap<String, usize> = HashMap::new();
        
        for tx in &transactions {
            if let Some(token) = &tx.token_address {
                *token_counts.entry(token.clone()).or_insert(0) += 1;
            }
            
            if let Some(dex) = &tx.dex {
                *dex_counts.entry(dex.clone()).or_insert(0) += 1;
            }
        }
        
        // Sort tokens and dexes by frequency
        let mut token_counts: Vec<(String, usize)> = token_counts.into_iter().collect();
        token_counts.sort_by(|a, b| b.1.cmp(&a.1));
        
        let mut dex_counts: Vec<(String, usize)> = dex_counts.into_iter().collect();
        dex_counts.sort_by(|a, b| b.1.cmp(&a.1));
        
        // Find active hours
        let mut hours: HashSet<u8> = HashSet::new();
        for tx in &transactions {
            if let Some(timestamp) = tx.timestamp {
                // Convert timestamp to UTC hour (0-23)
                let dt = DateTime::<Utc>::from_timestamp(timestamp as i64, 0).expect("Valid timestamp");
                hours.insert(dt.hour() as u8);
            }
        }
        
        // Determine trader type and expertise level
        let (behavior_type, expertise_level) = if transactions.len() > 50 && gas_strategy && successful_txs > failed_txs * 10 {
            // Likely a bot or professional trader
            if tx_frequency > 10.0 {
                (TraderBehaviorType::MevBot, TraderExpertiseLevel::Automated)
            } else {
                (TraderBehaviorType::HighFrequencyTrader, TraderExpertiseLevel::Professional)
            }
        } else if avg_value > 10.0 { // Value in ETH
            (TraderBehaviorType::Whale, TraderExpertiseLevel::Professional)
        } else if tx_frequency > 5.0 {
            (TraderBehaviorType::Retail, TraderExpertiseLevel::Intermediate)
        } else {
            (TraderBehaviorType::Retail, TraderExpertiseLevel::Beginner)
        };
        
        // Return the analysis
        Some(TraderBehaviorAnalysis {
            address: trader_address.to_string(),
            behavior_type,
            expertise_level,
            transaction_frequency: tx_frequency,
            average_transaction_value: avg_value,
            common_transaction_types: Vec::new(), // Would need to be extracted from transactions
            gas_behavior: GasBehavior {
                average_gas_price: avg_gas_price,
                highest_gas_price: max_gas_price,
                lowest_gas_price: min_gas_price,
                has_gas_strategy: gas_strategy,
                success_rate,
            },
            frequently_traded_tokens: token_counts.into_iter()
                .take(5)
                .map(|(token, _)| token)
                .collect(),
            preferred_dexes: dex_counts.into_iter()
                .take(3)
                .map(|(dex, _)| dex)
                .collect(),
            active_hours: hours.into_iter().collect(),
            prediction_score: 70.0, // Moderate confidence
            additional_notes: None,
        })
    }
    
    // Helper function to determine trader type and expertise
    fn determine_trader_type(
        &self,
        transactions: &[MempoolTransaction],
        tx_frequency: f64,
        avg_value: f64,
        has_gas_strategy: bool
    ) -> (TraderBehaviorType, TraderExpertiseLevel) {
        // Check for arbitrage patterns
        let arbitrage_count = transactions.iter()
            .filter(|tx| tx.is_swap())
            .filter(|tx| {
                // Look for multiple swaps in the same block
                transactions.iter().any(|other_tx| {
                    other_tx.hash != tx.hash &&
                    other_tx.block_number == tx.block_number &&
                    other_tx.is_swap()
                })
            })
            .count();
        
        let arbitrage_ratio = if !transactions.is_empty() {
            arbitrage_count as f64 / transactions.len() as f64
        } else {
            0.0
        };
        
        // Determine trader type
        let behavior_type = if arbitrage_ratio > 0.3 {
            TraderBehaviorType::Arbitrageur
        } else if tx_frequency > 20.0 && has_gas_strategy {
            TraderBehaviorType::MevBot
        } else if tx_frequency > 10.0 {
            TraderBehaviorType::HighFrequencyTrader
        } else if avg_value > 10.0 { // Assuming value in ETH
            TraderBehaviorType::Whale
        } else if transactions.iter().any(|tx| tx.is_liquidity_provision()) {
            TraderBehaviorType::MarketMaker
        } else {
            // Default to retail
            TraderBehaviorType::Retail
        };
        
        // Determine expertise level
        let expertise = if matches!(behavior_type, TraderBehaviorType::MevBot | TraderBehaviorType::Arbitrageur) {
            TraderExpertiseLevel::Automated
        } else if tx_frequency > 5.0 && has_gas_strategy {
            TraderExpertiseLevel::Professional
        } else if transactions.len() > 20 || has_gas_strategy {
            TraderExpertiseLevel::Intermediate
        } else {
            TraderExpertiseLevel::Beginner
        };
        
        (behavior_type, expertise)
    }
    
    // Helper function to extract transaction types
    fn extract_transaction_types(&self, transactions: &[MempoolTransaction]) -> Vec<crate::analys::mempool::TransactionType> {
        use std::collections::HashSet;
        let mut types = HashSet::new();
        
        for tx in transactions {
            if let Some(tx_type) = tx.transaction_type {
                types.insert(tx_type);
            }
        }
        
        types.into_iter().collect()
    }
    
    // Helper function to calculate prediction reliability score
    fn calculate_prediction_score(&self, tx_count: usize, time_window_sec: u64) -> f64 {
        // More transactions and shorter time window = higher score
        let base_score = 50.0;
        
        // Adjust for transaction count (more txs = better prediction)
        let tx_factor = match tx_count {
            0..=4 => 0.5,
            5..=19 => 0.7,
            20..=49 => 0.9,
            50..=99 => 1.1,
            _ => 1.3
        };
        
        // Adjust for time window (shorter window = more recent data = better prediction)
        let time_factor = match time_window_sec {
            0..=3600 => 1.3,             // 1 hour
            3601..=86400 => 1.1,         // 1 day
            86401..=604800 => 0.9,       // 1 week
            604801..=2592000 => 0.7,     // 1 month
            _ => 0.5                     // over 1 month
        };
        
        let score = base_score * tx_factor * time_factor;
        score.min(100.0).max(0.0)        // Cap between 0 and 100
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