//! Manual Trading Module
//!
//! This module handles user-initiated trades through API requests. It provides
//! a standardized interface for executing, tracking, and managing manual trades.
//!
//! # Key Features
//! - User trade execution across multiple chains
//! - Trade status tracking
//! - Risk assessment before execution
//! - Comprehensive trade history
//! - Support for various trading parameters (slippage, gas price, etc.)

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::Utc;
use uuid::Uuid;
use async_trait::async_trait;
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};
use hex;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::token_status::{TokenSafety, TokenStatusAnalyzer};
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};
use crate::tradelogic::traits::{TradeExecutor, RiskManager};

/// Configuration for manual trading executor
#[derive(Debug, Clone, Default)]
pub struct ManualTradeConfig {
    /// Maximum concurrent trades per user
    pub max_concurrent_trades_per_user: u32,
    
    /// Default slippage percentage
    pub default_slippage: f64,
    
    /// Default gas price multiplier (1.0 = normal)
    pub default_gas_price_multiplier: f64,
    
    /// Default gas limit multiplier (1.0 = normal)
    pub default_gas_limit_multiplier: f64,
    
    /// Require token safety check before trade
    pub require_safety_check: bool,
    
    /// Maximum risk score allowed (0-100, higher is riskier)
    pub max_risk_score: u8,
    
    /// Blacklisted tokens (addresses)
    pub token_blacklist: Vec<String>,
    
    /// Approved tokens (addresses) - empty means all non-blacklisted are allowed
    pub token_whitelist: Vec<String>,
    
    /// DEX router preferences by chain ID
    pub preferred_dex_routers: HashMap<u32, String>,
}

/// Result of a manual trade
#[derive(Debug, Clone)]
pub struct ManualTradeResult {
    /// Unique trade ID
    pub id: String,
    
    /// User ID or address
    pub user_id: String,
    
    /// Chain ID
    pub chain_id: u32,
    
    /// Token address
    pub token_address: String,
    
    /// Token name (if available)
    pub token_name: Option<String>,
    
    /// Token symbol (if available)
    pub token_symbol: Option<String>,
    
    /// Trade type (Buy/Sell)
    pub trade_type: TradeType,
    
    /// Amount of tokens or base currency
    pub amount: f64,
    
    /// Token pair (e.g., BNB/CAKE)
    pub token_pair: TokenPair,
    
    /// Entry price (in base currency)
    pub price: f64,
    
    /// Gas used for transaction
    pub gas_used: f64,
    
    /// Transaction hash
    pub tx_hash: Option<String>,
    
    /// Transaction status
    pub status: ManualTradeStatus,
    
    /// Transaction timestamp
    pub timestamp: u64,
    
    /// Error message (if failed)
    pub error: Option<String>,
    
    /// Safety score (0-100, higher is safer)
    pub safety_score: Option<u8>,
    
    /// Risk factors identified (if any)
    pub risk_factors: Vec<String>,
}

/// Status of a manual trade
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualTradeStatus {
    /// Trade is queued for execution
    Pending,
    
    /// Trade is being submitted to blockchain
    Submitting,
    
    /// Trade has been submitted and awaiting confirmation
    Submitted,
    
    /// Trade has been confirmed and successful
    Confirmed,
    
    /// Trade failed during execution
    Failed,
    
    /// Trade was rejected before execution (e.g., safety check failed)
    Rejected,
}

/// Manual Trade Executor
///
/// Handles user-initiated trades through API calls.
pub struct ManualTradeExecutor {
    /// EVM adapter for each chain
    evm_adapters: HashMap<u32, Arc<EvmAdapter>>,
    
    /// Token analyzer for safety checks
    token_analyzers: HashMap<u32, Arc<TokenStatusAnalyzer>>,
    
    /// Configuration
    config: RwLock<ManualTradeConfig>,
    
    /// Trade history
    trade_history: RwLock<Vec<ManualTradeResult>>,
    
    /// Active trades by user
    active_trades: RwLock<HashMap<String, Vec<String>>>,
}

impl ManualTradeExecutor {
    /// Create a new manual trade executor
    pub fn new() -> Self {
        Self {
            evm_adapters: HashMap::new(),
            token_analyzers: HashMap::new(),
            config: RwLock::new(ManualTradeConfig::default()),
            trade_history: RwLock::new(Vec::new()),
            active_trades: RwLock::new(HashMap::new()),
        }
    }
    
    /// Add a token analyzer for a specific chain
    pub fn add_token_analyzer(&mut self, chain_id: u32, analyzer: Arc<TokenStatusAnalyzer>) {
        self.token_analyzers.insert(chain_id, analyzer);
    }
}

#[async_trait]
impl TradeExecutor for ManualTradeExecutor {
    /// Configuration type
    type Config = ManualTradeConfig;
    
    /// Result type
    type TradeResult = ManualTradeResult;
    
    /// Start the executor
    async fn start(&self) -> Result<()> {
        debug!("Manual trade executor started");
        Ok(())
    }
    
    /// Stop the executor
    async fn stop(&self) -> Result<()> {
        debug!("Manual trade executor stopped");
        Ok(())
    }
    
    /// Update configuration
    async fn update_config(&self, config: Self::Config) -> Result<()> {
        let mut current_config = self.config.write().await;
        *current_config = config;
        debug!("Manual trade executor configuration updated");
        Ok(())
    }
    
    /// Add a new chain
    async fn add_chain(&mut self, chain_id: u32, adapter: Arc<EvmAdapter>) -> Result<()> {
        self.evm_adapters.insert(chain_id, adapter);
        debug!("Added chain {} to manual trade executor", chain_id);
        Ok(())
    }
    
    /// Execute a trade
    async fn execute_trade(&self, params: TradeParams) -> Result<Self::TradeResult> {
        // 1. Validate all required parameters
        let user_id = match &params.user_id {
            Some(id) if !id.trim().is_empty() => {
                // Validate user has permission
                self.check_user_trade_limits(id).await?;
                id
            },
            _ => return Err(anyhow::anyhow!("Valid user ID is required for manual trade"))
        };
        
        // Validate amount is positive
        if params.amount <= 0.0 {
            return Err(anyhow::anyhow!("Trade amount must be greater than zero"));
        }
        
        // Validate slippage is in valid range (0.1-100%)
        if params.slippage <= 0.0 || params.slippage > 100.0 {
            return Err(anyhow::anyhow!("Slippage must be between 0.1 and 100 percent"));
        }
        
        // Validate token address format
        if !params.token_address.starts_with("0x") || params.token_address.len() != 42 {
            return Err(anyhow::anyhow!("Invalid token address format"));
        }
        
        // Validate the requested chain is supported
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("Chain {} not supported", params.chain_id)),
        };
        
        // 2. Check blacklist before safety check (cheaper operation first)
        let config = self.config.read().await;
        if config.token_blacklist.contains(&params.token_address) {
            return Err(anyhow::anyhow!("Token is blacklisted"));
        }
        
        // If whitelist is not empty, check if token is in whitelist
        if !config.token_whitelist.is_empty() && !config.token_whitelist.contains(&params.token_address) {
            return Err(anyhow::anyhow!("Token is not in whitelist"));
        }
        
        // 3. Check token safety if required
        let safety_result = if config.require_safety_check {
            match self.check_token_safety(params.chain_id, &params.token_address).await {
                Ok(safety) => safety,
                Err(e) => {
                    warn!("Failed to check token safety for {}: {}", params.token_address, e);
                    // Continue but log warning, don't fail the trade
                    None
                }
            }
        } else {
            None
        };
        
        // Reject if token is too risky
        if let Some(safety) = &safety_result {
            if safety.risk_score > config.max_risk_score as u64 {
                return Err(anyhow::anyhow!(
                    "Token rejected due to high risk score: {} (max allowed: {})",
                    safety.risk_score,
                    config.max_risk_score
                ));
            }
            
            // Check for honeypot even if risk score is within limits
            if safety.is_honeypot {
                return Err(anyhow::anyhow!("Token detected as potential honeypot"));
            }
        }
        
        // 4. Create trade result with pending status
        let trade_id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp() as u64;
        
        // Fetch token metadata if possible
        let (token_name, token_symbol) = match adapter.get_token_metadata(&params.token_address).await {
            Ok((name, symbol)) => (Some(name), Some(symbol)),
            Err(e) => {
                warn!("Failed to fetch token metadata: {}", e);
                (None, None)
            }
        };
        
        // Determine base token based on chain
        let base_token = params.base_token.clone().unwrap_or_else(|| {
            match params.chain_id {
                1 => "ETH",   // Ethereum
                56 => "BNB",  // Binance Smart Chain
                137 => "MATIC", // Polygon
                43114 => "AVAX", // Avalanche
                _ => "ETH"    // Default
            }.to_string()
        });
        
        let mut trade_result = ManualTradeResult {
            id: trade_id.clone(),
            user_id: user_id.clone(),
            chain_id: params.chain_id,
            token_address: params.token_address.clone(),
            token_name,
            token_symbol,
            trade_type: params.trade_type.clone(),
            amount: params.amount,
            token_pair: TokenPair {
                base: base_token,
                quote: params.token_address.clone(),
            },
            price: 0.0, // Will be updated after execution
            gas_used: 0.0,
            tx_hash: None,
            status: ManualTradeStatus::Pending,
            timestamp: now,
            error: None,
            safety_score: safety_result.as_ref().map(|s| s.risk_score as u8),
            risk_factors: safety_result
                .as_ref()
                .map(|s| s.rug_indicators.clone())
                .unwrap_or_default(),
        };
        
        // 5. Register the trade as active
        {
            let mut active_trades = self.active_trades.write().await;
            let user_trades = active_trades
                .entry(user_id.clone())
                .or_insert_with(Vec::new);
            user_trades.push(trade_id.clone());
        }
        
        // 6. Try to simulate the trade before executing
        debug!("Simulating trade execution for token {} on chain {}", params.token_address, params.chain_id);
        let simulation_result = self.simulate_trade_execution(&params).await;
        
        // Check simulation result
        if let Err(e) = &simulation_result {
            warn!("Trade simulation failed: {}", e);
            // If this is a buy, we should block with high probability of failure
            if params.trade_type == TradeType::Buy {
                trade_result.status = ManualTradeStatus::Rejected;
                trade_result.error = Some(format!("Simulation failed: {}", e));
                
                // Add to trade history
                let mut history = self.trade_history.write().await;
                history.push(trade_result.clone());
                
                return Err(anyhow::anyhow!("Trade simulation failed: {}", e));
            }
            // For sell, log warning but continue (user might want to exit a position at any cost)
            warn!("Proceeding with sell despite simulation failure");
        }
        
        // 7. Execute the actual trade
        let execution_result = match simulation_result {
            // If simulation is successful, we have price impact data
            Ok((tx_hash, price)) => {
                trade_result.tx_hash = Some(tx_hash);
                trade_result.price = price;
                trade_result.status = ManualTradeStatus::Submitting;
                
                // Set a background task to check for confirmation
                let trade_id_clone = trade_id.clone();
                let adapter_clone = adapter.clone();
                let this = self.clone();
                tokio::spawn(async move {
                    // Wait for transaction to be mined
                    match adapter_clone.wait_for_transaction(&tx_hash, 180).await {
                        Ok(receipt) => {
                            // Update trade status to confirmed
                            if let Some(mut history) = this.trade_history.try_write() {
                                if let Some(trade) = history.iter_mut().find(|t| t.id == trade_id_clone) {
                                    trade.status = ManualTradeStatus::Confirmed;
                                    trade.gas_used = receipt.gas_used as f64;
                                    info!("Trade {} confirmed on-chain", trade_id_clone);
                                }
                            }
                        },
                        Err(e) => {
                            // Update trade status to failed
                            if let Some(mut history) = this.trade_history.try_write() {
                                if let Some(trade) = history.iter_mut().find(|t| t.id == trade_id_clone) {
                                    trade.status = ManualTradeStatus::Failed;
                                    trade.error = Some(format!("Transaction failed: {}", e));
                                    error!("Trade {} failed: {}", trade_id_clone, e);
                                }
                            }
                        }
                    }
                });
                
                info!("Trade {} submitted for user {} on chain {}", trade_id, trade_result.user_id, params.chain_id);
            },
            // If simulation failed but we're continuing (sell case usually)
            Err(_) => {
                trade_result.status = ManualTradeStatus::Submitting;
                self.simulate_trade_execution(&params).await
            }
        };
        
        // 8. Update trade result based on execution
        if let Ok((tx_hash, price)) = execution_result {
            trade_result.tx_hash = Some(tx_hash);
            trade_result.price = price;
            trade_result.status = ManualTradeStatus::Submitted;
            
            // Set a background task to check for confirmation
            let trade_id_clone = trade_id.clone();
            let adapter_clone = adapter.clone();
            let this = self.clone();
            tokio::spawn(async move {
                // Wait for transaction to be mined
                match adapter_clone.wait_for_transaction(&tx_hash, 180).await {
                    Ok(receipt) => {
                        // Update trade status to confirmed
                        if let Some(mut history) = this.trade_history.try_write() {
                            if let Some(trade) = history.iter_mut().find(|t| t.id == trade_id_clone) {
                                trade.status = ManualTradeStatus::Confirmed;
                                trade.gas_used = receipt.gas_used as f64;
                                info!("Trade {} confirmed on-chain", trade_id_clone);
                            }
                        }
                    },
                    Err(e) => {
                        // Update trade status to failed
                        if let Some(mut history) = this.trade_history.try_write() {
                            if let Some(trade) = history.iter_mut().find(|t| t.id == trade_id_clone) {
                                trade.status = ManualTradeStatus::Failed;
                                trade.error = Some(format!("Transaction failed: {}", e));
                                error!("Trade {} failed: {}", trade_id_clone, e);
                            }
                        }
                    }
                }
            });
            
            info!("Trade {} submitted for user {} on chain {}", trade_id, trade_result.user_id, params.chain_id);
        } else if let Err(e) = execution_result {
            trade_result.status = ManualTradeStatus::Failed;
            trade_result.error = Some(e.to_string());
            error!("Failed to execute trade {} for user {}: {}", trade_id, trade_result.user_id, e);
        }
        
        // 9. Add to trade history
        {
            let mut history = self.trade_history.write().await;
            history.push(trade_result.clone());
            
            // Maintain history size to prevent memory leaks
            if history.len() > 1000 {
                // Sort by timestamp (newest first) and keep latest 1000
                history.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
                history.truncate(1000);
                debug!("Truncated trade history to 1000 entries");
            }
        }
        
        // 10. Remove from active trades if completed or failed
        if trade_result.status == ManualTradeStatus::Confirmed 
            || trade_result.status == ManualTradeStatus::Failed
            || trade_result.status == ManualTradeStatus::Rejected 
        {
            let mut active_trades = self.active_trades.write().await;
            if let Some(user_trades) = active_trades.get_mut(&trade_result.user_id) {
                user_trades.retain(|id| id != &trade_id);
            }
        }
        
        // 11. Return the trade result
        Ok(trade_result)
    }
    
    /// Get trade status
    async fn get_trade_status(&self, trade_id: &str) -> Result<Option<Self::TradeResult>> {
        let history = self.trade_history.read().await;
        Ok(history.iter().find(|t| t.id == trade_id).cloned())
    }
    
    /// Get active trades
    async fn get_active_trades(&self) -> Result<Vec<Self::TradeResult>> {
        let active_trades = self.active_trades.read().await;
        let history = self.trade_history.read().await;
        
        let mut result = Vec::new();
        
        for user_trades in active_trades.values() {
            for trade_id in user_trades {
                if let Some(trade) = history.iter().find(|t| &t.id == trade_id) {
                    result.push(trade.clone());
                }
            }
        }
        
        Ok(result)
    }
    
    /// Get trade history
    async fn get_trade_history(&self, from_timestamp: u64, to_timestamp: u64) -> Result<Vec<Self::TradeResult>> {
        let history = self.trade_history.read().await;
        
        let filtered_history = history
            .iter()
            .filter(|trade| trade.timestamp >= from_timestamp && trade.timestamp <= to_timestamp)
            .cloned()
            .collect();
            
        Ok(filtered_history)
    }
    
    /// Evaluate token
    async fn evaluate_token(&self, chain_id: u32, token_address: &str) -> Result<Option<TokenSafety>> {
        let analyzer = match self.token_analyzers.get(&chain_id) {
            Some(analyzer) => analyzer,
            None => return Err(anyhow::anyhow!("No token analyzer found for chain {}", chain_id)),
        };
        
        analyzer.get_token_safety(token_address).await.context("Failed to evaluate token safety")
    }
}

impl ManualTradeExecutor {
    /// Mô phỏng một giao dịch để kiểm tra tính khả thi
    async fn simulate_trade_execution(&self, params: &TradeParams) -> Result<(String, f64)> {
        debug!("Simulating trade execution for token {} on chain {}", 
              params.token_address, params.chain_id);
        
        // 1. Lấy adapter cho chuỗi
        let adapter = match self.evm_adapters.get(&params.chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow::anyhow!("Chain {} not supported", params.chain_id)),
        };
        
        // 2. Xác định router phù hợp
        let router_address = {
            let config = self.config.read().await;
            config.preferred_dex_routers
                .get(&params.chain_id)
                .cloned()
                .unwrap_or_else(|| {
                    // Nếu không có router được cấu hình, dùng mặc định cho mỗi chain
                    match params.chain_id {
                        1 => "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D".to_string(), // Uniswap V2
                        56 => "0x10ED43C718714eb63d5aA57B78B54704E256024E".to_string(), // PancakeSwap
                        137 => "0xa5E0829CaCEd8fFDD4De3c43696c57F7D7A678ff".to_string(), // QuickSwap
                        _ => return Err(anyhow::anyhow!("No default router for chain {}", params.chain_id)),
                    }
                })
        };
        
        // 3. Xác định gas price phù hợp
        let gas_price_multiplier = {
            let config = self.config.read().await;
            config.default_gas_price_multiplier
        };
        
        let gas_price = match adapter.get_gas_price().await {
            Ok(price) => price * gas_price_multiplier,
            Err(e) => {
                warn!("Could not get gas price: {}. Using safe default", e);
                // Sử dụng giá trị an toàn cho mỗi chuỗi
                match params.chain_id {
                    1 => 50_000_000_000, // 50 gwei for Ethereum
                    56 => 5_000_000_000, // 5 gwei for BSC
                    137 => 100_000_000_000, // 100 gwei for Polygon
                    _ => 10_000_000_000, // 10 gwei default
                }
            }
        };
        
        // 4. Xác định gas limit phù hợp
        let gas_limit_multiplier = {
            let config = self.config.read().await;
            config.default_gas_limit_multiplier
        };
        
        let gas_limit = match params.trade_type {
            TradeType::Buy => 300_000, // Base gas limit for buys
            TradeType::Sell => 250_000, // Base gas limit for sells
        } as f64 * gas_limit_multiplier;
        
        // 5. Xác định deadline (thời hạn giao dịch)
        let deadline_minutes = params.deadline_minutes.unwrap_or(20); // Mặc định 20 phút
        let deadline = chrono::Utc::now().timestamp() as u64 + deadline_minutes * 60;
        
        // 6. Chuẩn bị tham số giao dịch
        let trade_params = adapter.prepare_trade_params(
            &params.token_address,
            &params.trade_type,
            params.amount,
            params.slippage,
            Some(router_address),
            Some(gas_price as u64),
            Some(gas_limit as u64),
            Some(deadline),
        ).await.context("Failed to prepare trade parameters")?;
        
        // 7. Thực hiện mô phỏng giao dịch
        let (estimated_price, estimated_output) = adapter.estimate_trade_output(
            &trade_params
        ).await.context("Failed to estimate trade output")?;
        
        // 8. Kiểm tra tính hợp lệ của kết quả
        if estimated_output <= 0.0 {
            return Err(anyhow::anyhow!(
                "Estimated output is zero or negative: {} - trade would likely fail", 
                estimated_output
            ));
        }
        
        // 9. Kiểm tra price impact
        let price_impact = adapter.calculate_price_impact(&trade_params).await
            .unwrap_or(0.0); // Nếu không tính được, giả định là 0
        
        if price_impact > params.slippage * 2.0 {
            warn!("Price impact {:.2}% is significantly higher than slippage {:.2}%", 
                 price_impact, params.slippage);
        }
        
        // 10. Trong thực tế, ở đây sẽ thực hiện giao dịch thực tế
        // Đối với mục đích demo, chúng ta giả định giao dịch thành công và tạo một tx hash giả
        let tx_hash = format!("0x{}", hex::encode(uuid::Uuid::new_v4().as_bytes()));
        
        debug!("Simulated transaction {} for token {} with estimated price {}", 
               tx_hash, params.token_address, estimated_price);
        
        // Trả về tx hash và giá ước tính
        Ok((tx_hash, estimated_price))
    }
    
    /// Kiểm tra giới hạn giao dịch của người dùng
    async fn check_user_trade_limits(&self, user_id: &str) -> Result<()> {
        let config = self.config.read().await;
        let max_concurrent = config.max_concurrent_trades_per_user;
        
        if max_concurrent == 0 {
            // Không giới hạn
            return Ok(());
        }
        
        let active_trades = self.active_trades.read().await;
        let user_active_count = active_trades.get(user_id)
            .map(|trades| trades.len())
            .unwrap_or(0);
        
        if user_active_count >= max_concurrent as usize {
            return Err(anyhow::anyhow!(
                "User has reached maximum concurrent trades limit: {}", 
                max_concurrent
            ));
        }
        
        Ok(())
    }
    
    /// Kiểm tra an toàn của token
    async fn check_token_safety(&self, chain_id: u32, token_address: &str) -> Result<Option<TokenSafety>> {
        // Kiểm tra xem có token analyzer cho chain này không
        let analyzer = match self.token_analyzers.get(&chain_id) {
            Some(analyzer) => analyzer,
            None => {
                warn!("No token analyzer available for chain {}", chain_id);
                return Ok(None);
            }
        };
        
        // Thực hiện kiểm tra an toàn
        match analyzer.analyze_token(token_address).await {
            Ok(safety) => Ok(Some(safety)),
            Err(e) => {
                warn!("Failed to analyze token {}: {}", token_address, e);
                // Không thất bại, chỉ trả về None
                Ok(None)
            }
        }
    }
}

impl Clone for ManualTradeExecutor {
    fn clone(&self) -> Self {
        Self {
            evm_adapters: self.evm_adapters.clone(),
            token_analyzers: self.token_analyzers.clone(),
            config: RwLock::new(ManualTradeConfig::default()),
            trade_history: RwLock::new(Vec::new()),
            active_trades: RwLock::new(HashMap::new()),
        }
    }
}

/// Create a new manual trade executor with default configuration
pub fn create_manual_trade_executor() -> ManualTradeExecutor {
    ManualTradeExecutor::new()
}
