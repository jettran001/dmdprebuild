//! Just-In-Time Liquidity MEV strategy
//!
//! Module này cung cấp các cơ chế để phát hiện và tận dụng cơ hội
//! cung cấp thanh khoản Just-In-Time (JIT), một dạng MEV phổ biến
//! đặc biệt trong các DEX V3 như Uniswap V3.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};
use anyhow::{Result, anyhow};
use uuid::Uuid;

use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::{MempoolAnalyzer, MempoolTransaction, TransactionType};
use super::types::{MevOpportunity, MevOpportunityType, MevExecutionMethod, JITLiquidityConfig, MevOpportunityStatus, MonitorConfig};
use crate::types::TokenPair;

/// Pool information for JIT liquidity
#[derive(Debug, Clone)]
pub struct PoolInfo {
    /// Pool address
    pub address: String,
    /// DEX name (Uniswap, Balancer, etc)
    pub dex_name: String,
    /// Token0 address
    pub token0: String,
    /// Token1 address
    pub token1: String,
    /// Fee tier (if applicable)
    pub fee_tier: Option<u64>,
    /// Current price 
    pub current_price: f64,
    /// Current liquidity
    pub liquidity: f64,
    /// Price range boundaries for concentrated liquidity
    pub price_boundaries: Option<(f64, f64)>,
    /// Pool creation timestamp
    pub created_at: u64,
    /// Last updated timestamp
    pub updated_at: u64,
}

/// Just-In-Time liquidity opportunity
#[derive(Debug, Clone)]
pub struct JITLiquidityOpportunity {
    /// Pool information
    pub pool: PoolInfo,
    /// Chain ID
    pub chain_id: u64,
    /// Detected swap transaction
    pub swap_transaction: String,
    /// Timestamp detected
    pub detected_at: u64,
    /// Expected execution timestamp
    pub execution_timestamp: u64,
    /// Estimated time to next block
    pub estimated_time_to_block_ms: u64,
    /// Optimal token0 amount
    pub optimal_token0_amount: f64,
    /// Optimal token1 amount
    pub optimal_token1_amount: f64,
    /// Estimated profit (USD)
    pub estimated_profit_usd: f64,
    /// Risk score (0-100)
    pub risk_score: f64,
    /// Additional parameters
    pub parameters: HashMap<String, String>,
    /// Execution status
    pub status: MevOpportunityStatus,
    /// Execution method
    pub execution_method: MevExecutionMethod,
    /// Token pair
    pub token_pair: TokenPair,
}

/// JIT Liquidity provider service
#[derive(Clone)]
pub struct JITLiquidityProvider {
    /// Configuration
    config: RwLock<JITLiquidityConfig>,
    /// MEV adapter
    evm_adapters: HashMap<u64, Arc<EvmAdapter>>,
    /// Mempool analyzers
    mempool_analyzers: HashMap<u64, Arc<MempoolAnalyzer>>,
    /// Running state
    running: RwLock<bool>,
    /// Known pools
    pools: RwLock<HashMap<String, PoolInfo>>,
    /// Recent opportunities
    opportunities: RwLock<Vec<JITLiquidityOpportunity>>,
    /// Executed opportunities
    executed_opportunities: RwLock<Vec<(JITLiquidityOpportunity, String)>>, // (opportunity, tx_hash)
    /// Monitor configuration
    monitor_config: RwLock<MonitorConfig>,
}

impl JITLiquidityProvider {
    /// Create a new JIT liquidity provider
    pub fn new() -> Self {
        Self {
            config: RwLock::new(JITLiquidityConfig::default()),
            evm_adapters: HashMap::new(),
            mempool_analyzers: HashMap::new(),
            running: RwLock::new(false),
            pools: RwLock::new(HashMap::new()),
            opportunities: RwLock::new(Vec::new()),
            executed_opportunities: RwLock::new(Vec::new()),
            monitor_config: RwLock::new(MonitorConfig::default()),
        }
    }
    
    /// Add a new chain to monitor
    pub fn add_chain(&mut self, chain_id: u64, adapter: Arc<EvmAdapter>, analyzer: Arc<MempoolAnalyzer>) {
        self.evm_adapters.insert(chain_id, adapter);
        self.mempool_analyzers.insert(chain_id, analyzer);
    }
    
    /// Update configuration
    pub async fn update_config(&self, config: JITLiquidityConfig) {
        let mut current_config = self.config.write().await;
        *current_config = config;
    }
    
    /// Start the JIT liquidity provider
    pub async fn start(&self) {
        let mut running = self.running.write().await;
        if *running {
            return;
        }
        
        *running = true;
        
        // Clone for the async task
        let self_clone = Arc::new(self.clone());
        
        // Start monitoring task
        tokio::spawn(async move {
            self_clone.monitor_loop().await;
        });
    }
    
    /// Stop the JIT liquidity provider
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }
    
    /// Main monitoring loop
    async fn monitor_loop(&self) {
        while *self.running.read().await {
            // Kiểm tra cấu hình JIT một cách chi tiết
            let config = self.config.read().await;
            
            // Kiểm tra nếu tính năng JIT liquidity bị tắt hoàn toàn
            if !config.enabled {
                debug!("JIT liquidity monitoring is disabled. Sleeping for 30 seconds.");
                sleep(Duration::from_secs(30)).await;
                continue;
            }
            
            // Kiểm tra nếu không có mục tiêu cụ thể nào được kích hoạt
            if config.target_pools.is_empty() {
                warn!("JIT liquidity is enabled but no target pools are configured. Sleeping for 30 seconds.");
                sleep(Duration::from_secs(30)).await;
                continue;
            }
            
            // Kiểm tra nếu ngưỡng lợi nhuận quá cao
            if config.min_profit_threshold_usd > 1000.0 {
                warn!("JIT liquidity min profit threshold is very high (${:.2}). This may prevent opportunities.", 
                      config.min_profit_threshold_usd);
            }
            
            // Kiểm tra nếu kiểm soát vốn quá thấp
            if config.max_capital_allocation_usd < 100.0 {
                warn!("JIT liquidity max capital allocation is very low (${:.2}). This may limit profits.",
                      config.max_capital_allocation_usd);
            }
            
                            // Kiểm tra tình trạng mạng trước khi phân tích
            let mut network_issues = false;
            for (&chain_id, adapter) in &self.evm_adapters {
                match adapter.check_connection().await {
                    Ok(connected) => {
                        if !connected {
                            error!("Network connection for chain {} is not available. Skipping analysis.",
                                   chain_id);
                            network_issues = true;
                        } else {
                            debug!("Network connection for chain {} is available", chain_id);
                        }
                    },
                    Err(e) => {
                        error!("Failed to check network connection for chain {}: {}. Skipping analysis.", chain_id, e);
                        network_issues = true;
                    }
                }
            }
            
            if network_issues {
                // Đợi ngắn hơn khi có vấn đề mạng để kiểm tra lại sớm hơn
                warn!("Network issues detected. Waiting for 5 seconds before retry.");
                sleep(Duration::from_secs(5)).await;
                continue;
            }
            
            // Phân tích cơ hội trên mỗi chain được kích hoạt
            for (&chain_id, analyzer) in &self.mempool_analyzers {
                // Kiểm tra xem chain này có được kích hoạt trong cấu hình không
                // Tạm thời coi như tất cả các chain đều được kích hoạt vì trường enabled_chains không tồn tại
                // TODO: Cần cập nhật khi cấu trúc JITLiquidityConfig được cập nhật
                debug!("Assuming chain {} is enabled for JIT liquidity.", chain_id);
                
                // Kiểm tra thời gian block trung bình trước khi phân tích
                if let Some(adapter) = self.evm_adapters.get(&chain_id) {
                    match adapter.get_average_block_time_ms().await {
                        Ok(block_time) => {
                            if block_time < config.min_block_time_ms {
                                warn!(
                                    "Average block time for chain {} is too short: {}ms < {}ms (min acceptable). JIT opportunity may be missed.",
                                    chain_id, block_time, config.min_block_time_ms
                                );
                            }
                        },
                        Err(e) => {
                            error!("Failed to get average block time for chain {}: {}", chain_id, e);
                            continue;
                        }
                    }
                }
                
                // Phân tích cơ hội
                if let Err(e) = self.analyze_chain_opportunities(chain_id, analyzer).await {
                    error!("Error analyzing chain {}: {}. Will retry next cycle.", chain_id, e);
                } else {
                    debug!("Successfully analyzed JIT opportunities for chain {}", chain_id);
                }
            }
            
            // Xóa các cơ hội cũ
            self.cleanup_old_opportunities().await;
            
            // Đợi trước lần quét tiếp theo, điều chỉnh theo cấu hình
            let scan_interval = config.scan_interval_ms.max(100_u64);
            sleep(Duration::from_millis(scan_interval)).await;
        }
    }
    
    /// Analyze opportunities for a specific chain
    async fn analyze_chain_opportunities(&self, chain_id: u64, analyzer: &Arc<MempoolAnalyzer>) -> anyhow::Result<()> {
        info!("Analyzing JIT liquidity opportunities for chain {}", chain_id);
        
        // Get pending transactions from mempool (limit 100)
        let pending_txs = match analyzer.get_pending_transactions(100).await {
            Ok(txs) => txs,
            Err(e) => {
                warn!("Failed to get pending transactions for chain {}: {}", chain_id, e);
                return Err(anyhow::anyhow!("Failed to get pending transactions: {}", e));
            }
        };
        
        info!("Found {} pending transactions for chain {}", pending_txs.len(), chain_id);
        
        // Filter for DEX interactions
        let dex_txs = self.filter_dex_transactions(&pending_txs);
        info!("Found {} DEX transactions for analysis", dex_txs.len());
        
        // Analyze each transaction for JIT opportunity
        for tx in dex_txs {
            if let Some(opportunity) = self.analyze_transaction(&tx, chain_id).await {
                // Add to opportunities list
                let mut opportunities = self.opportunities.write().await;
                opportunities.push(opportunity);
            }
        }
        
        Ok(())
    }
    
    /// Get or fetch pool information
    async fn get_or_fetch_pool_info(
        &self,
        chain_id: u64,
        pool_address: &str,
        adapter: &Arc<EvmAdapter>,
    ) -> anyhow::Result<Option<PoolInfo>> {
        // Check cache first
        {
            let pools = self.pools.read().await;
            if let Some(pool) = pools.get(&format!("{}:{}", chain_id, pool_address)) {
                return Ok(Some(pool.clone()));
            }
        }
        
        // Fetch pool information
        info!("Fetching pool info for {} on chain {}", pool_address, chain_id);
        
        // Check if this is a known DEX pool
        let is_known_dex = self.is_known_dex_pool(chain_id, pool_address).await;
        if !is_known_dex {
            debug!("Address {} is not a known DEX pool", pool_address);
            return Ok(None);
        }
        
        // Get token information
        let (token0, token1) = match self.get_pool_tokens(chain_id, pool_address, adapter).await {
            Ok((t0, t1)) => (t0, t1),
            Err(e) => {
                warn!("Failed to get pool tokens for {}: {}", pool_address, e);
                return Err(anyhow::anyhow!("Failed to get pool tokens: {}", e));
            }
        };
        
        // Get pool reserves
        let (reserve0, reserve1) = match self.get_pool_reserves(chain_id, pool_address, adapter).await {
            Ok((r0, r1)) => (r0, r1),
            Err(e) => {
                warn!("Failed to get pool reserves for {}: {}", pool_address, e);
                return Err(anyhow::anyhow!("Failed to get pool reserves: {}", e));
            }
        };
        
        // Create and cache pool info
        let pool_info = PoolInfo {
            address: pool_address.to_string(),
            chain_id,
            token0,
            token1,
            reserve0,
            reserve1,
            fee: 0.003, // Default fee, should be fetched from contract
            last_updated: chrono::Utc::now().timestamp(),
        };
        
        // Cache the pool info
        {
            let mut pools = self.pools.write().await;
            pools.insert(format!("{}:{}", chain_id, pool_address), pool_info.clone());
        }
        
        Ok(Some(pool_info))
    }
    
    /// Analyze a transaction for JIT opportunity
    async fn analyze_jit_opportunity(
        &self,
        chain_id: u64,
        tx: &MempoolTransaction,
        pool: &PoolInfo,
        current_block: u64,
        avg_block_time_ms: u64
    ) -> Option<JITLiquidityOpportunity> {
        // Get timestamps
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        
        // Calculate time to next block
        let config = self.config.read().await;
        let blocks_ahead = config.monitor_blocks_ahead;
        let block_target = current_block + blocks_ahead;
        let estimated_time_ms = blocks_ahead as u64 * avg_block_time_ms;
        
        // Calculate execution time
        let execution_time = now + (estimated_time_ms / 1000);
        
        // For concentrated liquidity pools like Uniswap V3,
        // calculate optimal position based on the swap details
        
        // In a real implementation, you would:
        // 1. Analyze the swap path and amount
        // 2. Determine the price impact and optimal position
        // 3. Calculate expected fees vs. impermanent loss
        
        // For this example, we'll create a simulated opportunity
        
        // Determine token amounts based on swap size
        let swap_value_usd = tx.value_usd;
        let total_capital = swap_value_usd.min(config.max_capital_allocation_usd);
        
        // For a ETH/USDC pool with current price of 3000
        let (token0_amount, token1_amount) = if pool.token0.ends_with("c4f27ead9083c756cc2") {
            // If token0 is ETH
            let eth_amount = total_capital / 2.0 / pool.current_price;
            let usdc_amount = total_capital / 2.0;
            (eth_amount, usdc_amount)
        } else {
            // If token0 is USDC
            let usdc_amount = total_capital / 2.0;
            let eth_amount = total_capital / 2.0 / pool.current_price;
            (usdc_amount, eth_amount)
        };
        
        // Calculate expected fee earnings
        // This is highly simplified - real calculation would need:
        // - Precise swap amount
        // - Position concentration
        // - Fee tier
        // - Expected price movement
        
        // Assume 0.3% fee tier and 20% of swap going through our position
        let fee_percent = 0.003; // 0.3%
        let position_utilization = 0.2; // 20%
        
        let expected_fee = swap_value_usd * fee_percent * position_utilization;
        
        // Calculate impermanent loss (simplified)
        // Assume a conservative 10% of potential earnings is lost to IL
        let impermanent_loss = expected_fee * 0.1;
        
        // Calculate gas cost
        // Two transactions: add liquidity + remove liquidity
        let gas_cost = 30.0; // Estimated $30 in gas
        
        // Calculate net profit
        let estimated_profit = expected_fee - impermanent_loss - gas_cost;
        
        // Calculate risk score
        let risk_score = calculate_jit_risk_score(
            tx.value_usd,
            pool.liquidity,
            estimated_time_ms,
        );
        
        // Create opportunity
        let opportunity = JITLiquidityOpportunity {
            pool: pool.clone(),
            chain_id,
            // Sửa lỗi: tx.hash không tồn tại, sử dụng tx.tx_hash thay thế
            swap_transaction: tx.tx_hash.clone().unwrap_or_default(),
            detected_at: now,
            execution_timestamp: execution_time,
            estimated_time_to_block_ms: estimated_time_ms,
            optimal_token0_amount: token0_amount,
            optimal_token1_amount: token1_amount,
            estimated_profit_usd: estimated_profit,
            risk_score,
            parameters: HashMap::new(),
            status: MevOpportunityStatus::Detected,
            execution_method: MevExecutionMethod::CustomContract,
            token_pair: TokenPair {
                base_token: pool.token0.clone(),
                quote_token: pool.token1.clone(),
                chain_id: chain_id as u32,
            },
        };
        
        Some(opportunity)
    }
    
    /// Create standard MEV opportunity from JIT
    async fn create_mev_opportunity(&self, jit: &JITLiquidityOpportunity) -> Option<MevOpportunity> {
        let token_pair = TokenPair {
            base_token: jit.pool.token0.clone(),
            quote_token: jit.pool.token1.clone(),
            chain_id: jit.chain_id as u32,
        };
        
        // Create specific parameters
        let mut specific_params = HashMap::new();
        specific_params.insert("pool_address".to_string(), jit.pool.address.clone());
        specific_params.insert("dex_name".to_string(), jit.pool.dex_name.clone());
        specific_params.insert("block_target".to_string(), 
                              (jit.execution_timestamp / 12).to_string()); // Simplified
        specific_params.insert("token0_amount".to_string(), jit.optimal_token0_amount.to_string());
        specific_params.insert("token1_amount".to_string(), jit.optimal_token1_amount.to_string());
        
        // Create MEV opportunity
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        
        let opportunity = MevOpportunity {
            id: Uuid::new_v4().to_string(),
            opportunity_type: MevOpportunityType::Arbitrage, // Use existing type for compatibility
            chain_id: jit.chain_id as u32,
            detected_at: now,
            expires_at: jit.execution_timestamp + 60, // 60 second window
            executed: false,
            estimated_profit_usd: jit.estimated_profit_usd,
            estimated_gas_cost_usd: 30.0, // Estimated
            estimated_net_profit_usd: jit.estimated_profit_usd - 30.0,
            token_pairs: vec![token_pair.clone()],
            token_pair: Some(token_pair),
            parameters: specific_params.clone(),
            risk_score: jit.risk_score,
            related_transactions: vec![jit.swap_transaction.clone()],
            execution_method: jit.execution_method,
            specific_params,
            execution_tx_hash: None,
            status: jit.status.clone(),
            mev_status: MevOpportunityStatus::Detected,
            target_transaction: None,
            bundle_data: None,
            first_transaction: None,
            second_transaction: None,
            expected_profit: jit.estimated_profit_usd, // Assuming expected_profit in ETH is approximately equal to USD value
            actual_profit: None,
        };
        
        // In a real implementation, you would add this to the main opportunity list
        
        Some(opportunity)
    }
    
    /// Execute a JIT liquidity opportunity
    async fn execute_opportunity(
        &self,
        opportunity: &JITLiquidityOpportunity
    ) -> anyhow::Result<String> {
        info!("Executing JIT opportunity: {:?}", opportunity);
        
        // Validate opportunity is still valid
        if !self.validate_opportunity(opportunity, &self.evm_adapters[&opportunity.chain_id]).await {
            return Err(anyhow::anyhow!("Opportunity is no longer valid"));
        }
        
        // Calculate optimal amount to provide
        let amount = self.calculate_optimal_amount(opportunity).await;
        
        // Prepare JIT transaction
        let tx_data = self.prepare_jit_transaction(opportunity, amount).await?;
        
        // Execute transaction
        let adapter = &self.evm_adapters[&opportunity.chain_id];
        let tx_hash = match adapter.send_raw_transaction(&tx_data).await {
            Ok(hash) => hash,
            Err(e) => {
                error!("Failed to execute JIT transaction: {}", e);
                return Err(anyhow::anyhow!("Transaction execution failed: {}", e));
            }
        };
        
        info!("JIT transaction executed with hash: {}", tx_hash);
        
        // Update opportunity status
        self.update_opportunity_status(opportunity, MevOpportunityStatus::Executed, Some(tx_hash.clone())).await;
        
        Ok(tx_hash)
    }
    
    /// Clean up old opportunities
    async fn cleanup_old_opportunities(&self) {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        
        // Clean up main opportunities
        {
            let mut opportunities = self.opportunities.write().await;
            opportunities.retain(|opp| now - opp.detected_at < 600); // Keep last 10 minutes
        }
        
        // Clean up executed opportunities (keep last 24 hours)
        {
            let mut executed = self.executed_opportunities.write().await;
            executed.retain(|(opp, _)| now - opp.detected_at < 86400);
        }
    }
    
    /// Get all current opportunities
    pub async fn get_opportunities(&self) -> Vec<JITLiquidityOpportunity> {
        let opportunities = self.opportunities.read().await;
        opportunities.clone()
    }
    
    /// Get executed opportunities
    pub async fn get_executed_opportunities(&self) -> Vec<(JITLiquidityOpportunity, String)> {
        let executed = self.executed_opportunities.read().await;
        executed.clone()
    }
    
    /// Clone for async contexts
    fn clone(&self) -> Self {
        // Tạo cấu hình mặc định mà không phụ thuộc vào RwLock hiện tại
        // để tránh block_in_place có thể gây deadlock
        let config = JITLiquidityConfig::default();
        let running = false;

        // Clone các trường không phải RwLock
        let evm_adapters = self.evm_adapters.clone();
        let mempool_analyzers = self.mempool_analyzers.clone();

        // Trả về instance mới với RwLock mới
        Self {
            config: RwLock::new(config),
            evm_adapters,
            mempool_analyzers,
            running: RwLock::new(running),
            pools: RwLock::new(HashMap::new()),
            opportunities: RwLock::new(Vec::new()),
            executed_opportunities: RwLock::new(Vec::new()),
            monitor_config: RwLock::new(MonitorConfig::default()),
        }
    }

    /// Clone với trạng thái đầy đủ - phương thức bất đồng bộ an toàn hơn
    /// để sao chép toàn bộ trạng thái hiện tại của đối tượng.
    ///
    /// # Ví dụ
    /// ```
    /// // Sử dụng không an toàn (có thể mất state):
    /// let clone = provider.clone(); 
    ///
    /// // Sử dụng an toàn (giữ nguyên state):
    /// let clone = provider.clone_with_state().await;
    /// ```
    pub async fn clone_with_state(&self) -> Self {
        // Đọc các RwLock một cách an toàn với timeout
        let config = match tokio::time::timeout(
            std::time::Duration::from_millis(500), 
            self.config.read()
        ).await {
            Ok(guard) => guard.clone(),
            Err(_) => {
                warn!("Timeout waiting for config lock in clone_with_state, using default");
                JITLiquidityConfig::default()
            }
        };

        let running = match tokio::time::timeout(
            std::time::Duration::from_millis(500), 
            self.running.read()
        ).await {
            Ok(guard) => *guard,
            Err(_) => {
                warn!("Timeout waiting for running lock in clone_with_state, using default (false)");
                false
            }
        };

        let pools = match tokio::time::timeout(
            std::time::Duration::from_millis(500), 
            self.pools.read()
        ).await {
            Ok(guard) => guard.clone(),
            Err(_) => {
                warn!("Timeout waiting for pools lock in clone_with_state, using empty map");
                HashMap::new()
            }
        };

        let opportunities = match tokio::time::timeout(
            std::time::Duration::from_millis(500), 
            self.opportunities.read()
        ).await {
            Ok(guard) => guard.clone(),
            Err(_) => {
                warn!("Timeout waiting for opportunities lock in clone_with_state, using empty vector");
                Vec::new()
            }
        };

        let executed_opportunities = match tokio::time::timeout(
            std::time::Duration::from_millis(500), 
            self.executed_opportunities.read()
        ).await {
            Ok(guard) => guard.clone(),
            Err(_) => {
                warn!("Timeout waiting for executed_opportunities lock in clone_with_state, using empty vector");
                Vec::new()
            }
        };

        Self {
            config: RwLock::new(config),
            evm_adapters: self.evm_adapters.clone(),
            mempool_analyzers: self.mempool_analyzers.clone(),
            running: RwLock::new(running),
            pools: RwLock::new(pools),
            opportunities: RwLock::new(opportunities),
            executed_opportunities: RwLock::new(executed_opportunities),
            monitor_config: RwLock::new(MonitorConfig::default()),
        }
    }
}

/// Calculate risk score for JIT liquidity
fn calculate_jit_risk_score(swap_value_usd: f64, pool_liquidity: f64, time_to_block_ms: u64) -> f64 {
    // Liquidity risk - smaller swaps relative to pool liquidity are safer
    let liquidity_ratio = swap_value_usd / pool_liquidity;
    let liquidity_risk = (liquidity_ratio * 100.0).min(80.0);
    
    // Time risk - longer time to block means more risk of other transactions
    let time_risk = (time_to_block_ms as f64 / 20000.0 * 100.0).min(70.0);
    
    // Combined risk score
    let risk = (liquidity_risk * 0.4 + time_risk * 0.6).min(100.0);
    
    risk
} 