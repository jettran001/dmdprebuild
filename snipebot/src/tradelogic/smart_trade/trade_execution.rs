//! Trade execution helper functions
//!
//! This module contains helper functions for trade execution to reduce
//! code duplication in executor.rs.

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Result, Context, anyhow};
use tracing::{debug, error, info, warn};
use chrono::Utc;

use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::{TradeParams, TradeType};
use super::types::{TradeResult, TradeStatus, TradeStrategy, TradeTracker};

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
    
    // Step 1: Get current token price before execution
    let current_price = match adapter.get_token_price(&params.token_address).await {
        Ok(price) => price,
        Err(e) => {
            warn!("Could not get token price: {}, using 0.0", e);
            0.0
        }
    };
    
    // Step 2: Prepare transaction parameters
    let slippage = params.slippage.unwrap_or(0.5); // Default 0.5% slippage
    let deadline = params.deadline.unwrap_or(now + 300); // Default 5 minutes
    
    let gas_price = match adapter.get_gas_price().await {
        Ok(price) => price * 1.05, // Add 5% to current gas price
        Err(_) => 0.0, // Use default gas price
    };
    
    // Step 3: Execute the transaction
    let tx_result = match params.trade_type {
        TradeType::Buy => {
            adapter.execute_buy_token(
                &params.token_address,
                &params.amount.to_string(),
                slippage,
                deadline,
                gas_price,
            ).await
        },
        TradeType::Sell => {
            adapter.execute_sell_token(
                &params.token_address,
                &params.amount.to_string(),
                slippage,
                deadline,
                gas_price,
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
            error!("Trade execution failed: {}", e);
            
            // Update tracker with failed status
            let mut updated_tracker = tracker.clone();
            updated_tracker.status = TradeStatus::Failed;
            updated_tracker.updated_at = now;
            updated_tracker.error_message = Some(format!("Execution failed: {}", e));
            
            // Update active trades
            {
                let mut trades = active_trades.write().await;
                if let Some(pos) = trades.iter().position(|t| t.id == updated_tracker.id) {
                    trades[pos] = updated_tracker;
                } else {
                    trades.push(updated_tracker);
                }
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
                exit_reason: Some("Execution failed".to_string()),
                gas_used: 0.0,
                safety_score: 0,
                risk_factors: Vec::new(),
            });
        }
    };
    
    // Step 5: Wait for transaction to be mined
    info!("Transaction submitted: {}", transaction_hash);
    
    let tx_receipt = match adapter.wait_for_transaction(&transaction_hash, 180).await {
        Ok(receipt) => receipt,
        Err(e) => {
            warn!("Could not get transaction receipt: {}", e);
            
            // Update tracker with pending status
            let mut updated_tracker = tracker.clone();
            updated_tracker.status = TradeStatus::Pending;
            updated_tracker.updated_at = now;
            updated_tracker.transaction_hash = Some(transaction_hash.clone());
            
            // Update active trades
            {
                let mut trades = active_trades.write().await;
                if let Some(pos) = trades.iter().position(|t| t.id == updated_tracker.id) {
                    trades[pos] = updated_tracker;
                } else {
                    trades.push(updated_tracker);
                }
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
    
    // Step 7: Get updated token price after execution
    let updated_price = match adapter.get_token_price(&params.token_address).await {
        Ok(price) => price,
        Err(_) => current_price,
    };
    
    // Step 8: Update trade tracker
    let mut updated_tracker = tracker.clone();
    updated_tracker.status = status.clone();
    updated_tracker.updated_at = now;
    updated_tracker.transaction_hash = Some(transaction_hash);
    updated_tracker.entry_price = current_price;
    updated_tracker.current_price = updated_price;
    updated_tracker.gas_used = tx_receipt.gas_used.unwrap_or(0) as f64;
    
    // Update active trades
    {
        let mut trades = active_trades.write().await;
        if let Some(pos) = trades.iter().position(|t| t.id == updated_tracker.id) {
            trades[pos] = updated_tracker;
        } else {
            trades.push(updated_tracker);
        }
    }
    
    // Step 9: Create and return trade result
    let result = TradeResult {
        id: trade_id.to_string(),
        chain_id: params.chain_id,
        token_address: params.token_address.clone(),
        token_name: "Unknown".to_string(), // Would be populated from token data
        token_symbol: "UNK".to_string(),   // Would be populated from token data
        amount: params.amount,
        entry_price: current_price,
        exit_price: None,
        current_price: updated_price,
        profit_loss: if current_price > 0.0 {
            (updated_price - current_price) / current_price * 100.0
        } else {
            0.0
        },
        status,
        strategy: tracker.strategy,
        created_at: now,
        updated_at: now,
        completed_at: if status == TradeStatus::Failed { Some(now) } else { None },
        exit_reason: if status == TradeStatus::Failed { Some("Transaction failed".to_string()) } else { None },
        gas_used: tx_receipt.gas_used.unwrap_or(0) as f64,
        safety_score: 0,
        risk_factors: Vec::new(),
    };
    
    Ok(result)
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