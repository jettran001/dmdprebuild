//! Trade execution helper functions
//!
//! This module contains helper functions for trade execution to reduce
//! code duplication in executor.rs.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::timeout;
use anyhow::{Result, Context, anyhow};
use tracing::{debug, error, info, warn};
use chrono::Utc;
use futures::future::Future;

use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::{TradeParams, TradeType};
use super::types::{TradeResult, TradeStatus, TradeStrategy, TradeTracker};
use super::utils::retry_async;

/// Thời gian chờ mặc định cho các cuộc gọi API (30 giây)
const DEFAULT_API_TIMEOUT_SECS: u64 = 30;

/// Số lần retry mặc định cho các cuộc gọi API
const DEFAULT_RETRY_COUNT: usize = 3;

/// Execute a trade with the given parameters and update the trade tracker
///
/// This function centralizes the actual trade execution logic that was previously
/// duplicated across multiple locations in the executor.rs file.
///
/// # Arguments
/// * `adapter` - Reference to the blockchain adapter for the target chain
/// * `params` - Trade parameters
/// * `trade_id` - Unique ID for the trade
/// * `tracker` - Trade tracker object to update with execution results
/// * `active_trades` - Reference to the collection of active trades
///
/// # Returns
/// * `Result<TradeResult>` - Result of the trade execution
pub async fn execute_trade_helper(
    adapter: &Arc<EvmAdapter>,
    params: &TradeParams,
    trade_id: &str,
    tracker: TradeTracker,
    active_trades: &RwLock<Vec<TradeTracker>>,
) -> Result<TradeResult> {
    let now = Utc::now().timestamp() as u64;
    
    // Log the trade execution attempt
    info!(
        "Executing trade on chain {} for token {}, amount: {}, type: {:?}",
        params.chain_id, params.token_address, params.amount, params.trade_type
    );
    
    // Step 1: Get current token price before execution with timeout và retry
    let current_price = match retry_with_timeout(
        || adapter.get_token_price(&params.token_address),
        "get token price",
        DEFAULT_RETRY_COUNT,
        DEFAULT_API_TIMEOUT_SECS,
    ).await {
        Ok(price) => price,
        Err(e) => {
            warn!("Could not get token price after multiple attempts: {}, using 0.0", e);
            0.0
        }
    };
    
    // Step 2: Prepare transaction parameters
    let slippage = params.slippage.unwrap_or(0.5); // Default 0.5% slippage
    let deadline = params.deadline.unwrap_or(now + 300); // Default 5 minutes
    
    // Lấy gas price với retry và timeout
    let gas_price = match retry_with_timeout(
        || adapter.get_gas_price(),
        "get gas price",
        DEFAULT_RETRY_COUNT,
        DEFAULT_API_TIMEOUT_SECS,
    ).await {
        Ok(price) => price * 1.05, // Add 5% to current gas price
        Err(e) => {
            warn!("Could not get gas price after multiple attempts: {}, using default", e);
            // Sử dụng gas price mặc định theo chain
            match params.chain_id {
                1 => 50.0, // Ethereum mainnet
                56 => 5.0, // BSC
                137 => 100.0, // Polygon
                _ => 50.0, // Default for other chains
            }
        }
    };
    
    // Step 3: Execute the transaction with retry logic
    let tx_result = match params.trade_type {
        TradeType::Buy => {
            retry_with_timeout(
                || adapter.execute_buy_token(
                    &params.token_address,
                    &params.amount.to_string(),
                    slippage,
                    deadline,
                    gas_price,
                ),
                "execute buy token",
                DEFAULT_RETRY_COUNT,
                DEFAULT_API_TIMEOUT_SECS * 2, // Double timeout for transactions
            ).await
        },
        TradeType::Sell => {
            retry_with_timeout(
                || adapter.execute_sell_token(
                    &params.token_address,
                    &params.amount.to_string(),
                    slippage,
                    deadline,
                    gas_price,
                ),
                "execute sell token",
                DEFAULT_RETRY_COUNT,
                DEFAULT_API_TIMEOUT_SECS * 2, // Double timeout for transactions
            ).await
        },
        _ => {
            return Err(anyhow!("Unsupported trade type: {:?}", params.trade_type));
        }
    };
    
    // Step 4: Process the result
    let transaction_hash = match tx_result {
        Ok(hash) => hash,
        Err(e) => {
            error!("Trade execution failed after multiple attempts: {}", e);
            
            // Update tracker with failed status
            let mut updated_tracker = tracker.clone();
            updated_tracker.status = TradeStatus::Failed;
            updated_tracker.updated_at = now;
            updated_tracker.error_message = Some(format!("Execution failed: {}", e));
            
            // Update active trades với timeout để tránh blocking
            match timeout(
                Duration::from_secs(5),
                update_active_trades(active_trades, updated_tracker.clone())
            ).await {
                Ok(_) => debug!("Successfully updated active trades with failed status"),
                Err(_) => warn!("Timeout updating active trades with failed status"),
            }
            
            // Return result with failed status
            return Ok(TradeResult {
                id: trade_id.to_string(),
                chain_id: params.chain_id,
                token_address: params.token_address.clone(),
                token_name: "Unknown".to_string(),
                token_symbol: "UNK".to_string(),
                amount: params.amount,
                entry_price: 0.0,
                exit_price: None,
                current_price,
                profit_loss: 0.0,
                status: TradeStatus::Failed,
                strategy: tracker.strategy,
                created_at: now,
                updated_at: now,
                completed_at: Some(now),
                exit_reason: Some(format!("Execution failed: {}", e)),
                gas_used: 0.0,
                safety_score: 0,
                risk_factors: Vec::new(),
            });
        }
    };
    
    // Step 5: Wait for transaction to be mined với timeout
    info!("Transaction submitted: {}, waiting for confirmation...", transaction_hash);
    
    let tx_receipt = match retry_with_timeout(
        || adapter.wait_for_transaction(&transaction_hash, 180),
        "wait for transaction",
        2, // Fewer retries for waiting
        180, // 3 minutes timeout
    ).await {
        Ok(receipt) => receipt,
        Err(e) => {
            warn!("Could not get transaction receipt after waiting: {}", e);
            
            // Update tracker with pending status
            let mut updated_tracker = tracker.clone();
            updated_tracker.status = TradeStatus::Pending;
            updated_tracker.updated_at = now;
            updated_tracker.transaction_hash = Some(transaction_hash.clone());
            
            // Update active trades với timeout để tránh blocking
            match timeout(
                Duration::from_secs(5),
                update_active_trades(active_trades, updated_tracker.clone())
            ).await {
                Ok(_) => debug!("Successfully updated active trades with pending status"),
                Err(_) => warn!("Timeout updating active trades with pending status"),
            }
            
            // Return result with pending status
            return Ok(TradeResult {
                id: trade_id.to_string(),
                chain_id: params.chain_id,
                token_address: params.token_address.clone(),
                token_name: "Unknown".to_string(),
                token_symbol: "UNK".to_string(),
                amount: params.amount,
                entry_price: current_price,
                exit_price: None,
                current_price,
                profit_loss: 0.0,
                status: TradeStatus::Pending,
                strategy: tracker.strategy,
                created_at: now,
                updated_at: now,
                completed_at: None,
                exit_reason: None,
                gas_used: 0.0,
                safety_score: 0,
                risk_factors: Vec::new(),
            });
        }
    };
    
    // Step 6: Determine if transaction was successful
    let status = if tx_receipt.status {
        TradeStatus::Active
    } else {
        TradeStatus::Failed
    };
    
    // Step 7: Get updated token price after execution với retry
    let updated_price = match retry_with_timeout(
        || adapter.get_token_price(&params.token_address),
        "get updated token price",
        DEFAULT_RETRY_COUNT,
        DEFAULT_API_TIMEOUT_SECS,
    ).await {
        Ok(price) => price,
        Err(e) => {
            warn!("Could not get updated token price: {}, using previous price", e);
            current_price
        }
    };
    
    // Step 8: Update trade tracker
    let mut updated_tracker = tracker.clone();
    updated_tracker.status = status.clone();
    updated_tracker.updated_at = now;
    updated_tracker.transaction_hash = Some(transaction_hash);
    updated_tracker.gas_used = tx_receipt.gas_used.unwrap_or(0);
    updated_tracker.gas_price = Some(gas_price);
    updated_tracker.trade_price = Some(if params.trade_type == TradeType::Buy { updated_price } else { current_price });
    
    // Update active trades với timeout
    match timeout(
        Duration::from_secs(5),
        update_active_trades(active_trades, updated_tracker.clone())
    ).await {
        Ok(_) => debug!("Successfully updated active trades after execution"),
        Err(_) => warn!("Timeout updating active trades after execution"),
    }
    
    // Step 9: Calculate profit/loss
    let profit_loss = match params.trade_type {
        TradeType::Buy => 0.0, // Không có profit/loss ngay khi mua
        TradeType::Sell => {
            let buy_price = tracker.trade_price.unwrap_or(0.0);
            if buy_price > 0.0 {
                ((updated_price / buy_price) - 1.0) * 100.0
            } else {
                0.0
            }
        },
        _ => 0.0,
    };
    
    // Step 10: Build and return result
    let result = TradeResult {
        id: trade_id.to_string(),
        chain_id: params.chain_id,
        token_address: params.token_address.clone(),
        token_name: tracker.token_name.clone(),
        token_symbol: tracker.token_symbol.clone(),
        amount: params.amount,
        entry_price: match params.trade_type {
            TradeType::Buy => updated_price,
            _ => tracker.trade_price.unwrap_or(0.0),
        },
        exit_price: match params.trade_type {
            TradeType::Sell => Some(updated_price),
            _ => None,
        },
        current_price: updated_price,
        profit_loss,
        status: status.clone(),
        strategy: tracker.strategy,
        created_at: now,
        updated_at: now,
        completed_at: if status == TradeStatus::Failed || params.trade_type == TradeType::Sell {
            Some(now)
        } else {
            None
        },
        exit_reason: if status == TradeStatus::Failed {
            Some("Transaction failed on-chain".to_string())
        } else if params.trade_type == TradeType::Sell {
            Some("Sell executed".to_string())
        } else {
            None
        },
        gas_used: tx_receipt.gas_used.unwrap_or(0) as f64,
        safety_score: 0,
        risk_factors: Vec::new(),
    };
    
    Ok(result)
}

/// Helper function to update active trades
async fn update_active_trades(active_trades: &RwLock<Vec<TradeTracker>>, tracker: TradeTracker) {
    let mut trades = active_trades.write().await;
    if let Some(pos) = trades.iter().position(|t| t.id == tracker.id) {
        trades[pos] = tracker;
    } else {
        trades.push(tracker);
    }
}

/// Utility function to retry an async operation with timeout
///
/// # Arguments
/// * `operation` - Closure that returns a Future to retry
/// * `operation_name` - Name of the operation for logging
/// * `retries` - Number of retry attempts
/// * `timeout_secs` - Timeout in seconds
///
/// # Returns
/// * `Result<T>` - Result of the operation or error
async fn retry_with_timeout<T, F, Fut>(
    operation: F,
    operation_name: &str,
    retries: usize,
    timeout_secs: u64,
) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    retry_async(
        operation,
        retries,
        |attempt, err| {
            warn!(
                "Attempt {} for {} failed: {}. Retrying...",
                attempt, operation_name, err
            );
            // Exponential backoff with jitter
            let base_delay = 500; // 500ms
            let max_delay = 5000; // 5s
            let delay = std::cmp::min(
                max_delay,
                base_delay * (2_u64.pow(attempt as u32))
            );
            // Add jitter (±20%)
            let jitter = (rand::random::<f64>() * 0.4 - 0.2) * delay as f64;
            Duration::from_millis((delay as f64 + jitter) as u64)
        },
        || {
            // Wrap with timeout
            async move {
                match timeout(Duration::from_secs(timeout_secs), operation()).await {
                    Ok(result) => result,
                    Err(_) => Err(anyhow!("Operation {} timed out after {} seconds", operation_name, timeout_secs)),
                }
            }
        },
    ).await
}

/// Create a trade tracker object from the given parameters
///
/// This function centralizes the creation of TradeTracker objects to avoid
/// duplicated code across different executor methods.
///
/// # Arguments
/// * `params` - Trade parameters
/// * `trade_id` - Unique ID for the trade
/// * `timestamp` - Creation timestamp
///
/// # Returns
/// * `TradeTracker` - A new trade tracker instance
pub fn create_trade_tracker(
    params: &TradeParams,
    trade_id: &str,
    timestamp: u64,
) -> TradeTracker {
    let strategy = if let Some(strategy_type) = &params.strategy {
        match strategy_type.as_str() {
            "dca" => TradeStrategy::DollarCostAverage {
                interval: params.custom_params.get("interval").map(|v| v.parse::<u64>().unwrap_or(86400)).unwrap_or(86400),
                total_periods: params.custom_params.get("total_periods").map(|v| v.parse::<u32>().unwrap_or(10)).unwrap_or(10),
                completed_periods: 0,
            },
            "grid" => TradeStrategy::Grid {
                upper_bound: params.custom_params.get("upper_bound").map(|v| v.parse::<f64>().unwrap_or(0.0)).unwrap_or(0.0),
                lower_bound: params.custom_params.get("lower_bound").map(|v| v.parse::<f64>().unwrap_or(0.0)).unwrap_or(0.0),
                grid_levels: params.custom_params.get("grid_levels").map(|v| v.parse::<u32>().unwrap_or(5)).unwrap_or(5),
            },
            "trailing_stop" => TradeStrategy::TrailingStop {
                activation_percent: params.custom_params.get("activation_percent").map(|v| v.parse::<f64>().unwrap_or(10.0)).unwrap_or(10.0),
                trail_percent: params.custom_params.get("trail_percent").map(|v| v.parse::<f64>().unwrap_or(2.0)).unwrap_or(2.0),
                highest_price: 0.0,
            },
            _ => TradeStrategy::Standard,
        }
    } else {
        TradeStrategy::Standard
    };
    
    TradeTracker {
        id: trade_id.to_string(),
        chain_id: params.chain_id,
        token_address: params.token_address.clone(),
        amount: params.amount,
        trade_type: params.trade_type.clone(),
        entry_price: 0.0,
        current_price: 0.0,
        status: TradeStatus::Pending,
        transaction_hash: None,
        error_message: None,
        gas_used: 0.0,
        strategy,
        created_at: timestamp,
        updated_at: timestamp,
        stop_loss: params.stop_loss,
        take_profit: params.take_profit,
        max_hold_time: params.max_hold_time.unwrap_or(86400 * 7), // Default 7 days
        custom_params: params.custom_params.clone().unwrap_or_default(),
    }
} 