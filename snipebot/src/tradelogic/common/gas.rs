//! Gas management utilities for optimizing transactions
//!
//! This module provides common utilities for calculating optimal gas prices
//! across different trade types (smart trades, MEV, etc). It centralizes
//! gas-related logic to avoid code duplication and ensure consistent behavior.

use std::sync::Arc;
use tokio::sync::{RwLock, OnceCell};
use std::collections::HashMap;
use anyhow::{Result, anyhow};
use tracing::{debug, warn};
use tokio::time::Duration;

use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::tradelogic::traits::{MempoolAnalysisProvider, RpcAdapter};
use crate::types::{GasPrice, GasLimit, Transaction};
use crate::tradelogic::common::types::GasEstimate;

/// Network congestion levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkCongestion {
    /// Low congestion (low gas prices)
    Low,
    
    /// Medium congestion (average gas prices)
    Medium,
    
    /// High congestion (high gas prices)
    High,
}

/// Transaction priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionPriority {
    /// Low priority transaction (can wait)
    Low,
    
    /// Medium priority transaction (standard)
    Medium,
    
    /// High priority transaction (needs fast confirmation)
    High,
    
    /// Very high priority (needs immediate confirmation)
    VeryHigh,
}

impl From<TransactionPriority> for u8 {
    fn from(priority: TransactionPriority) -> Self {
        match priority {
            TransactionPriority::Low => 0,
            TransactionPriority::Medium => 1,
            TransactionPriority::High => 2,
            TransactionPriority::VeryHigh => 3,
        }
    }
}

impl From<u8> for TransactionPriority {
    fn from(value: u8) -> Self {
        match value {
            0 => TransactionPriority::Low,
            1 => TransactionPriority::Medium,
            2 => TransactionPriority::High,
            _ => TransactionPriority::VeryHigh,
        }
    }
}

/// Gas history tracker for optimizing gas prices
#[derive(Debug, Clone)]
pub struct GasHistory {
    /// Gas price history by chain ID
    gas_prices: HashMap<u32, Vec<(u64, f64)>>, // (timestamp, gas_price)
    
    /// Success rate statistics by gas tier
    success_rates: HashMap<u32, HashMap<u64, (usize, usize)>>, // (gas_tier, (success, total))
}

impl GasHistory {
    /// Create a new gas history tracker
    pub fn new() -> Self {
        Self {
            gas_prices: HashMap::new(),
            success_rates: HashMap::new(),
        }
    }
    
    /// Add a gas price data point to history
    pub fn add_gas_price(&mut self, chain_id: u32, gas_price: f64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let entry = self.gas_prices.entry(chain_id).or_insert_with(Vec::new);
        entry.push((now, gas_price));
        
        // Keep only the most recent 100 entries
        if entry.len() > 100 {
            entry.sort_by_key(|(timestamp, _)| *timestamp);
            entry.drain(0..(entry.len() - 100));
        }
    }
    
    /// Add execution result to success rate statistics
    pub fn add_execution_result(&mut self, chain_id: u32, gas_tier: u64, success: bool) {
        let chain_stats = self.success_rates.entry(chain_id).or_insert_with(HashMap::new);
        let (successes, total) = chain_stats.entry(gas_tier).or_insert((0, 0));
        *total += 1;
        if success {
            *successes += 1;
        }
    }
    
    /// Get optimal gas price based on history and priority
    pub fn get_optimal_gas_price(&self, chain_id: u32, priority: TransactionPriority) -> f64 {
        // Get gas price history for the chain
        let gas_prices = match self.gas_prices.get(&chain_id) {
            Some(prices) => prices,
            None => return 50.0, // Default if no history available
        };
        
        if gas_prices.is_empty() {
            return 50.0;
        }
        
        // Filter to prices from the last 10 minutes
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let recent_prices: Vec<f64> = gas_prices.iter()
            .filter(|(timestamp, _)| now - timestamp < 600) // 10 minutes
            .map(|(_, price)| *price)
            .collect();
        
        if recent_prices.is_empty() {
            return 50.0;
        }
        
        // Calculate optimal gas price based on priority
        let priority_u8: u8 = priority.into();
        match priority_u8 {
            0 => { // Low
                let min_price = recent_prices.iter().fold(f64::INFINITY, |a, &b| a.min(b));
                min_price * 0.9 // 90% of lowest price
            },
            1 => { // Medium
                let sum: f64 = recent_prices.iter().sum();
                let avg = sum / recent_prices.len() as f64;
                avg * 1.1 // 110% of average price
            },
            _ => { // High/Very High (2+)
                let max_price = recent_prices.iter().fold(0.0, |a, &b| a.max(b));
                max_price * 1.25 // 125% of highest price
            },
        }
    }
}

/// Singleton gas history tracker using tokio::sync::OnceCell
static GAS_HISTORY: OnceCell<RwLock<GasHistory>> = OnceCell::const_new();

/// Get or initialize the gas history singleton
pub async fn get_gas_history() -> &'static RwLock<GasHistory> {
    GAS_HISTORY.get_or_init(|| async {
        RwLock::new(GasHistory::new())
    }).await
}

/// Calculate optimal gas price based on network conditions
///
/// This function combines data from multiple sources:
/// - Mempool analysis
/// - Chain data
/// - Historical gas prices
/// - Network congestion
///
/// # Parameters
/// * `chain_id` - Target blockchain ID
/// * `priority` - Transaction priority level
/// * `evm_adapter` - EVM adapter for the chain
/// * `mempool_provider` - Optional mempool analysis provider
/// * `mev_protection` - Whether to add premium to avoid MEV attacks
///
/// # Returns
/// * `Result<f64>` - Optimal gas price in Gwei
pub async fn calculate_optimal_gas_price(
    chain_id: u32,
    priority: TransactionPriority,
    evm_adapter: &Arc<EvmAdapter>,
    mempool_provider: Option<&Arc<dyn MempoolAnalysisProvider>>,
    mev_protection: bool,
) -> Result<f64> {
    // Fallback values
    let mut base_gas_price = match priority {
        TransactionPriority::Low => 5.0,
        TransactionPriority::Medium => 10.0,
        TransactionPriority::High => 15.0,
        TransactionPriority::VeryHigh => 20.0,
    };
    
    // Try to get more accurate gas price
    let mut gas_price = match get_gas_price_from_chain(evm_adapter).await {
        Ok(price) => {
            debug!("Got gas price from chain: {} Gwei", price);
            price
        }
        Err(e) => {
            debug!("Failed to get gas price from chain: {}, using fallback", e);
            base_gas_price
        }
    };
    
    // Apply priority multiplier
    let priority_multiplier = match priority {
        TransactionPriority::Low => 0.9,
        TransactionPriority::Medium => 1.0,
        TransactionPriority::High => 1.25,
        TransactionPriority::VeryHigh => 1.5,
    };
    
    gas_price *= priority_multiplier;
    
    // Adjust based on mempool congestion if provider available
    if let Some(provider) = mempool_provider {
        let congestion = check_network_congestion(chain_id, Some(provider)).await;
        let congestion_multiplier = match congestion {
            NetworkCongestion::Low => 1.0,
            NetworkCongestion::Medium => 1.1,
            NetworkCongestion::High => 1.2,
        };
        gas_price *= congestion_multiplier;
        debug!("Applied congestion multiplier: {}", congestion_multiplier);
    }
    
    // Add MEV protection premium if requested
    if mev_protection {
        gas_price *= 1.15; // 15% premium
        debug!("Applied MEV protection premium");
    }
    
    // Round to 2 decimal places
    gas_price = (gas_price * 100.0).round() / 100.0;
    
    debug!("Final calculated gas price: {} Gwei", gas_price);
    Ok(gas_price)
}

/// Get gas price directly from the chain
async fn get_gas_price_from_chain(evm_adapter: &Arc<EvmAdapter>) -> Result<f64> {
    // Use the helper function to get gas price
    match evm_adapter.get_gas_price().await {
        Ok(price) => Ok(price as f64), // Convert u64 to f64
        Err(e) => {
            warn!("Failed to get gas price from chain: {}", e);
            Err(anyhow!("Failed to get gas price from chain: {}", e))
        }
    }
}

/// Calculate optimal gas for MEV transactions
///
/// This function is specialized for MEV operations, taking into account
/// opportunity type and estimated profit.
///
/// # Parameters
/// * `chain_id` - Target blockchain ID
/// * `opportunity_type` - Type of MEV opportunity
/// * `estimated_profit` - Estimated profit in USD
/// * `evm_adapter` - EVM adapter for the chain
///
/// # Returns
/// Tuple of (gas_limit, gas_price)
pub async fn calculate_optimal_gas_for_mev(
    chain_id: u32,
    opportunity_type: &crate::tradelogic::mev_logic::types::MevOpportunityType,
    estimated_profit: f64,
    evm_adapter: &Arc<EvmAdapter>,
) -> Result<(u64, f64)> {
    use crate::tradelogic::mev_logic::types::MevOpportunityType;
    
    // Determine priority based on opportunity type and profit
    let priority = match opportunity_type {
        MevOpportunityType::Arbitrage => {
            if estimated_profit > 100.0 {
                TransactionPriority::High
            } else if estimated_profit > 50.0 {
                TransactionPriority::Medium
            } else {
                TransactionPriority::Low
            }
        },
        MevOpportunityType::Sandwich => {
            if estimated_profit > 200.0 {
                TransactionPriority::VeryHigh
            } else if estimated_profit > 100.0 {
                TransactionPriority::High
            } else {
                TransactionPriority::Medium
            }
        },
        MevOpportunityType::FrontRun => TransactionPriority::VeryHigh,
        _ => TransactionPriority::Medium,
    };
    
    // Calculate gas price with MEV protection
    let gas_price = calculate_optimal_gas_price(
        chain_id, 
        priority, 
        evm_adapter, 
        None, 
        true // Always apply MEV protection for MEV operations
    ).await?;
    
    // Determine gas limit based on opportunity type
    let gas_limit = match opportunity_type {
        MevOpportunityType::Arbitrage => 350000,
        MevOpportunityType::Sandwich => 500000,
        MevOpportunityType::FrontRun => 300000,
        MevOpportunityType::NewToken => 400000,
        _ => 250000,
    };
    
    Ok((gas_limit, gas_price))
}

/// Check network congestion based on pending transaction count
///
/// # Parameters
/// * `chain_id` - Target blockchain ID
/// * `mempool_provider` - Optional mempool analysis provider
///
/// # Returns
/// NetworkCongestion level enum
async fn check_network_congestion(
    chain_id: u32,
    mempool_provider: Option<&Arc<dyn MempoolAnalysisProvider>>
) -> NetworkCongestion {
    // Try to get pending transaction count from mempool provider if available
    if let Some(provider) = mempool_provider {
        match provider.get_pending_transactions(100).await.map(|txs| txs.len()) {
            Ok(count) => {
                // Classify congestion based on pending transaction count
                // Thresholds can be adjusted per chain
                match chain_id {
                    1 => { // Ethereum Mainnet
                        if count > 50000 { 
                            return NetworkCongestion::High;
                        } else if count > 15000 {
                            return NetworkCongestion::Medium;
                        }
                    },
                    56 => { // BSC
                        if count > 100000 {
                            return NetworkCongestion::High;
                        } else if count > 30000 {
                            return NetworkCongestion::Medium;
                        }
                    },
                    _ => { // Other chains
                        if count > 10000 {
                            return NetworkCongestion::High;
                        } else if count > 5000 {
                            return NetworkCongestion::Medium;
                        }
                    }
                }
                NetworkCongestion::Low
            },
            Err(e) => {
                warn!("Could not get pending transaction count: {}", e);
                NetworkCongestion::Medium // Default to medium if can't get data
            }
        }
    } else {
        NetworkCongestion::Medium // Default when no mempool provider available
    }
}

/// Determine if a chain needs MEV protection
///
/// # Parameters
/// * `chain_id` - Target blockchain ID
///
/// # Returns
/// Boolean indicating if MEV protection should be applied
fn needs_mev_protection(chain_id: u32) -> bool {
    // Only apply MEV protection on mainnet and some large chains with MEV
    matches!(chain_id, 1 | 56 | 137 | 43114 | 42161) // ETH, BSC, Polygon, Avalanche, Arbitrum
}

/// Get current gas price for a specific chain
///
/// Uses RpcAdapter trait to get the current gas price from the blockchain
///
/// # Parameters
/// * `chain_id` - Target blockchain ID
/// * `evm_adapter` - EVM adapter for the chain
///
/// # Returns
/// * `Result<f64>` - Current gas price in gwei
pub async fn get_gas_price(chain_id: u32, evm_adapter: &Arc<EvmAdapter>) -> Result<f64> {
    // Use RpcAdapter trait method to get gas price
    let gas_price = evm_adapter.get_gas_price().await? as f64;
    
    // Add to gas history for future optimization
    let history = get_gas_history().await;
    let mut history_write = history.write().await;
    history_write.add_gas_price(chain_id, gas_price);
    
    Ok(gas_price)
}

/// Get pending transaction count for a specific chain
///
/// Uses MempoolAnalysisProvider to get the pending transaction count
///
/// # Parameters
/// * `chain_id` - Target blockchain ID
/// * `mempool_provider` - Mempool analysis provider
///
/// # Returns
/// * `Result<usize>` - Number of pending transactions
pub async fn get_pending_transaction_count(
    _chain_id: u32,
    mempool_provider: &Arc<dyn MempoolAnalysisProvider>
) -> Result<usize> {
    // Get pending transactions from mempool provider
    // Phương thức get_pending_transactions chỉ nhận một tham số limit
    let pending_txs = mempool_provider.get_pending_transactions(1000).await?;
    
    // Return the count
    Ok(pending_txs.len())
}

/// Synchronous version of get_gas_price to use in sync contexts
/// 
/// This function returns a default value based on transaction priority
/// and should only be used in contexts where async is not possible.
pub fn get_gas_price_sync(priority: TransactionPriority) -> f64 {
    // Fallback gas prices for different priorities
    match priority {
        TransactionPriority::Low => 5.0,
        TransactionPriority::Medium => 10.0,
        TransactionPriority::High => 15.0,
        TransactionPriority::VeryHigh => 20.0,
    }
}

/// Calculate gas for MEV opportunity
///
/// Calculates optimal gas price and gas limit for a specific MEV opportunity
/// based on its type, estimated profit, and current network conditions.
///
/// # Arguments
/// * `chain_id` - Chain ID
/// * `opportunity_type` - Type of MEV opportunity
/// * `estimated_profit` - Estimated profit in USD
/// * `evm_adapter` - EVM adapter for the chain
///
/// # Returns
/// * `(u64, f64)` - (Gas limit, max fee per gas in gwei)
// Alias cho calculate_optimal_gas_for_mev để tương thích ngược
pub async fn calculate_mev_opportunity_gas(
    chain_id: u32,
    opportunity_type: &crate::tradelogic::mev_logic::MevOpportunityType,
    estimated_profit: f64,
    evm_adapter: &Arc<EvmAdapter>,
) -> Result<(u64, f64)> {
    use crate::tradelogic::mev_logic::MevOpportunityType;
    
    // Determine priority based on opportunity type and profit
    let priority = match opportunity_type {
        MevOpportunityType::Arbitrage => {
            if estimated_profit > 100.0 {
                TransactionPriority::High
            } else if estimated_profit > 50.0 {
                TransactionPriority::Medium
            } else {
                TransactionPriority::Low
            }
        },
        MevOpportunityType::Sandwich => {
            if estimated_profit > 200.0 {
                TransactionPriority::VeryHigh
            } else if estimated_profit > 100.0 {
                TransactionPriority::High
            } else {
                TransactionPriority::Medium
            }
        },
        MevOpportunityType::FrontRun => TransactionPriority::VeryHigh,
        _ => TransactionPriority::Medium,
    };
    
    // Calculate gas price with MEV protection
    let gas_price = calculate_optimal_gas_price(
        chain_id, 
        priority, 
        evm_adapter, 
        None, 
        true // Always apply MEV protection for MEV operations
    ).await?;
    
    // Determine gas limit based on opportunity type
    let gas_limit = match opportunity_type {
        MevOpportunityType::Arbitrage => 350000,
        MevOpportunityType::Sandwich => 500000,
        MevOpportunityType::FrontRun => 300000,
        MevOpportunityType::NewToken => 400000,
        _ => 250000,
    };
    
    Ok((gas_limit, gas_price))
}

/// Tính toán gas price tối ưu dựa trên các yếu tố
/// 
/// # Arguments
/// * `adapter` - EVM adapter
/// * `base_gas_price` - Gas price cơ bản
/// * `priority_fee` - Phí ưu tiên
/// * `max_gas_price` - Gas price tối đa cho phép
/// 
/// # Returns
/// * `Result<GasPrice>` - Gas price tối ưu
pub async fn calculate_eip1559_gas_price(
    adapter: &Arc<EvmAdapter>,
    base_gas_price: f64,
    priority_fee: f64,
    max_gas_price: f64,
) -> Result<GasPrice> {
    // Lấy gas price hiện tại từ mạng
    let current_gas_price = adapter.get_gas_price().await?;
    
    // Lấy chain_id từ adapter
    let chain_id = adapter.get_chain_id().await?;
    
    // Lấy mức độ tắc nghẽn của mempool
    let congestion = get_mempool_congestion(adapter, chain_id).await?;
    
    // Tính toán gas price tối ưu
    let optimal_price = if congestion > 0.8 {
        // Mạng đang tắc nghẽn, tăng gas price
        (base_gas_price * 1.2).min(max_gas_price)
    } else if congestion < 0.3 {
        // Mạng thông thoáng, giảm gas price
        (base_gas_price * 0.9).max(current_gas_price)
    } else {
        // Mạng bình thường, giữ nguyên
        base_gas_price
    };
    
    Ok(GasPrice {
        base_fee: optimal_price,
        priority_fee,
        max_fee: max_gas_price,
    })
}

/// Lấy mức độ tắc nghẽn của mempool
/// 
/// # Arguments
/// * `adapter` - EVM adapter
/// * `chain_id` - Chain ID của blockchain
/// 
/// # Returns
/// * `Result<f64>` - Mức độ tắc nghẽn (0-1)
async fn get_mempool_congestion(adapter: &Arc<EvmAdapter>, chain_id: u32) -> Result<f64> {
    // Lấy số lượng transaction đang pending
    // Thêm phương thức get_pending_transaction_count vào EvmAdapter
    let pending_txs = match adapter.get_pending_transaction_count(chain_id).await {
        Ok(count) => count,
        Err(_) => 0, // Fallback to 0 if the method fails
    };
    
    // Lấy gas price trung bình
    let avg_gas_price = adapter.get_gas_price().await?;
    
    // Tính toán mức độ tắc nghẽn
    let congestion = if pending_txs > 1000 {
        0.9 // Rất tắc nghẽn
    } else if pending_txs > 500 {
        0.7 // Tắc nghẽn vừa phải
    } else if pending_txs > 100 {
        0.5 // Bình thường
    } else {
        0.3 // Thông thoáng
    };
    
    Ok(congestion)
}

/// Ước tính gas limit cho transaction
/// 
/// # Arguments
/// * `adapter` - EVM adapter
/// * `tx` - Transaction cần ước tính
/// * `safety_margin` - Hệ số an toàn (1.0 = 100%)
/// 
/// # Returns
/// * `Result<GasLimit>` - Gas limit ước tính
pub async fn estimate_gas_limit(
    adapter: &Arc<EvmAdapter>,
    tx: &Transaction,
    safety_margin: f64,
) -> Result<GasLimit> {
    // Ước tính gas cơ bản
    let base_estimate = adapter.estimate_gas(tx).await?;
    
    // Áp dụng hệ số an toàn
    let gas_limit = (base_estimate as f64 * safety_margin) as u64;
    
    Ok(GasLimit {
        limit: gas_limit,
        used: 0,
    })
}

/// Tính toán gas cost cho transaction
/// 
/// # Arguments
/// * `gas_price` - Gas price
/// * `gas_limit` - Gas limit
/// * `eth_price_usd` - Giá ETH/USD
/// 
/// # Returns
/// * `Result<f64>` - Gas cost (USD)
pub fn calculate_gas_cost(
    gas_price: &GasPrice,
    gas_limit: &GasLimit,
    eth_price_usd: f64,
) -> Result<f64> {
    let gas_cost_eth = (gas_price.base_fee + gas_price.priority_fee) * gas_limit.limit as f64;
    let gas_cost_usd = gas_cost_eth * eth_price_usd;
    
    Ok(gas_cost_usd)
} 