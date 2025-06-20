//! Common trade execution functionality
//!
//! This module provides shared functionality for executing trades
//! that can be used by both smart_trade and manual_trade modules.
//! These functions help reduce code duplication and ensure consistent trade execution.

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
use crate::tradelogic::common::utils::{validate_trade_params, simulate_trade_execution, perform_token_security_check, execute_blockchain_transaction, wait_for_transaction};
use super::types::{TokenIssueType};
use crate::types::TradeStatus;

// Import TradeResult, TradeStrategy, TradeTracker from smart_trade
use crate::tradelogic::smart_trade::types::{TradeResult, TradeStrategy, TradeTracker};

/// Thời gian chờ mặc định cho các cuộc gọi API (30 giây)
const DEFAULT_API_TIMEOUT_SECS: u64 = 30;

/// Số lần retry mặc định cho các cuộc gọi API
const DEFAULT_RETRY_COUNT: usize = 3;

/// Standard trade execution flow
///
/// Provides a standardized flow for executing trades, including validation,
/// security checks, simulation, execution, and confirmation.
///
/// # Arguments
/// * `params` - Trade parameters
/// * `adapter` - EVM adapter for the chain
/// * `wallet_address` - Address of the wallet executing the trade
/// * `security_threshold` - Risk score threshold (0-100, lower is safer)
/// * `transaction_timeout` - Timeout for transaction confirmation (seconds)
///
/// # Returns
/// * `Result<(String, f64)>` - (tx_hash, amount_received) if successful
pub async fn execute_trade_flow(
    params: &TradeParams,
    adapter: &Arc<EvmAdapter>,
    wallet_address: &str,
    security_threshold: u8,
    transaction_timeout: u64
) -> Result<(String, f64)> {
    // Step 1: Validate trade parameters
    validate_trade_params(params, adapter).await
        .context("Failed to validate trade parameters")?;
    
    // Step 2: Perform security check for token
    if params.trade_type == TradeType::Buy {
        let security_result = perform_token_security_check(&params.token_address, adapter).await
            .context("Failed to perform security check")?;
        
        // Kiểm tra điểm rủi ro
        if security_result.score > security_threshold {
            return Err(anyhow!(
                "Token failed security check, risk score: {}, issues: {:?}",
                security_result.score,
                security_result.issues
            ));
        }
        
        // Special checks for critical security issues
        let has_critical_issue = security_result.issues.iter().any(|issue| {
            matches!(
                issue.issue_type,
                TokenIssueType::Honeypot | TokenIssueType::MaliciousTransfer
            ) && issue.severity > 70 // Chỉ xem xét các vấn đề nghiêm trọng
        });
        
        if has_critical_issue || security_result.is_honeypot {
            return Err(anyhow!("Token has critical security issues: {:?}", security_result.issues));
        }
    }
    
    // Step 3: Simulate trade to get expected output and gas
    let (expected_output, estimated_gas) = simulate_trade_execution(params, adapter).await
        .context("Failed to simulate trade execution")?;
    
    info!(
        "Trade simulation successful. Expected output: {}, Estimated gas: {}",
        expected_output, estimated_gas
    );
    
    // Step 4: Execute the transaction
    let (tx_hash, gas_used) = execute_blockchain_transaction(params, adapter, wallet_address).await
        .context("Failed to execute transaction")?;
    
    info!(
        "Transaction submitted: {}, Gas used: {}",
        tx_hash, gas_used
    );
    
    // Step 5: Wait for transaction confirmation
    let success = wait_for_transaction(&tx_hash, adapter, transaction_timeout).await
        .context("Failed while waiting for transaction confirmation")?;
    
    if !success {
        return Err(anyhow!("Transaction failed or timed out: {}", tx_hash));
    }
    
    info!("Transaction confirmed successfully: {}", tx_hash);
    
    Ok((tx_hash, expected_output))
}

/// Execute a token approval transaction
///
/// Approves the DEX router to spend tokens on behalf of the wallet
///
/// # Arguments
/// * `token_address` - Address of the token to approve
/// * `adapter` - EVM adapter for the chain
/// * `wallet_address` - Address of the wallet executing the approval
/// * `amount` - Amount to approve (or max uint256 if None)
///
/// # Returns
/// * `Result<String>` - Transaction hash if successful
pub async fn execute_token_approval(
    token_address: &str,
    adapter: &Arc<EvmAdapter>,
    wallet_address: &str,
    amount: Option<f64>
) -> Result<String> {
    // Get chain_id from adapter
    let chain_id = adapter.get_chain_id().await?;
    
    // Create approval params
    let params = TradeParams {
        chain_type: crate::types::ChainType::EVM(chain_id),
        trade_type: TradeType::Approve,
        token_address: token_address.to_string(),
        amount: match amount {
            Some(amt) => amt,
            None => f64::MAX
        }, // Use max amount if not specified
        slippage: 0.5, // Default slippage for approvals
        deadline_minutes: 30, // Default 30 minutes
        router_address: "".to_string(), // Default empty router (use default)
        gas_limit: None,
        gas_price: None,
        strategy: None,
        stop_loss: None,
        take_profit: None,
        max_hold_time: None,
        custom_params: None,
    };
    
    // Execute approval transaction
    let (tx_hash, _) = execute_blockchain_transaction(&params, adapter, wallet_address).await
        .context("Failed to execute approval transaction")?;
    
    // Wait for transaction confirmation
    let success = wait_for_transaction(&tx_hash, adapter, 60).await
        .context("Failed while waiting for approval confirmation")?;
    
    if !success {
        return Err(anyhow!("Approval transaction failed: {}", tx_hash));
    }
    
    Ok(tx_hash)
}

/// Check if token approval is needed
///
/// Checks if the wallet has already approved enough tokens for trading
///
/// # Arguments
/// * `token_address` - Address of the token to check
/// * `adapter` - EVM adapter for the chain
/// * `wallet_address` - Address of the wallet
/// * `amount_needed` - Amount needed for the trade
///
/// # Returns
/// * `Result<bool>` - True if approval is needed, False otherwise
pub async fn is_approval_needed(
    token_address: &str,
    adapter: &Arc<EvmAdapter>,
    wallet_address: &str,
    amount_needed: f64
) -> Result<bool> {
    // Sử dụng router address mặc định vì phương thức get_default_router_address không tồn tại
    let router_address = "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D"; // Uniswap V2 Router
    
    let allowance = adapter.get_token_allowance(token_address, wallet_address, &router_address)
        .await
        .context("Failed to get token allowance")?;
    
    Ok(allowance < amount_needed)
}

/// Verify transaction success and calculate profit/loss
///
/// # Arguments
/// * `adapter` - EVM adapter for the chain
/// * `tx_hash` - Transaction hash
/// * `initial_amount` - Initial amount invested
/// * `token_address` - Address of the token
///
/// # Returns
/// * `Result<(f64, f64)>` - (amount_received, profit_percentage)
pub async fn verify_transaction_result(
    adapter: &Arc<EvmAdapter>,
    tx_hash: &str,
    initial_amount: f64,
    token_address: &str
) -> Result<(f64, f64)> {
    // Get transaction details
    let tx_details = adapter.get_transaction_details(tx_hash)
        .await
        .context("Failed to get transaction details")?;
    
    // Calculate amount received based on transaction logs
    let amount_received = tx_details.amount_received.unwrap_or(0.0);
    
    // Calculate profit/loss percentage
    let profit_percentage = if initial_amount > 0.0 {
        ((amount_received - initial_amount) / initial_amount) * 100.0
    } else {
        0.0
    };
    
    Ok((amount_received, profit_percentage))
}

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
        params.chain_id(), params.token_address, params.amount, params.trade_type
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
    let slippage = match params.slippage {
        Some(s) => s,
        None => {
            warn!("No slippage specified, using default 0.5%");
            0.5
        }
    };
    
    let deadline = match params.deadline {
        Some(d) => d,
        None => {
            let default_deadline = now + 300; // Default 5 minutes
            warn!("No deadline specified, using default: {}", default_deadline);
            default_deadline
        }
    };
    
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
            match params.chain_id() {
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
                chain_id: params.chain_id(),
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
                chain_id: params.chain_id(),
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
            warn!("Could not get updated token price: {}, using current price", e);
            current_price
        }
    };
    
    // Step 8: Calculate profit/loss if applicable
    let profit_loss = if params.trade_type == TradeType::Sell && current_price > 0.0 {
        ((updated_price - current_price) / current_price) * 100.0
    } else {
        0.0
    };
    
    // Step 9: Update tracker with success status
    let mut updated_tracker = tracker.clone();
    updated_tracker.status = status;
    updated_tracker.updated_at = now;
    updated_tracker.transaction_hash = Some(transaction_hash.clone());
    
    // Update active trades với timeout để tránh blocking
    match timeout(
        Duration::from_secs(5),
        update_active_trades(active_trades, updated_tracker.clone())
    ).await {
        Ok(_) => debug!("Successfully updated active trades with new status: {:?}", status),
        Err(_) => warn!("Timeout updating active trades with new status"),
    }
    
    // Return result with appropriate status
    Ok(TradeResult {
        id: trade_id.to_string(),
        chain_id: params.chain_id(),
        token_address: params.token_address.clone(),
        token_name: "Unknown".to_string(), // Would be updated later with token info
        token_symbol: "UNK".to_string(),   // Would be updated later with token info
        amount: params.amount,
        entry_price: current_price,
        exit_price: if params.trade_type == TradeType::Sell { Some(updated_price) } else { None },
        current_price: updated_price,
        profit_loss,
        status,
        strategy: tracker.strategy,
        created_at: now,
        updated_at: now,
        completed_at: if status == TradeStatus::Failed { Some(now) } else { None },
        exit_reason: if status == TradeStatus::Failed { 
            Some("Transaction failed on chain".to_string()) 
        } else { 
            None 
        },
        gas_used: tx_receipt.gas_used.unwrap_or(0.0),
        safety_score: 0, // Would be updated later with token safety info
        risk_factors: Vec::new(), // Would be updated later with risk analysis
    })
}

/// Update the active trades collection with a new tracker
///
/// # Arguments
/// * `active_trades` - Reference to the collection of active trades
/// * `tracker` - New tracker to add or update
async fn update_active_trades(active_trades: &RwLock<Vec<TradeTracker>>, tracker: TradeTracker) {
    let mut trades = active_trades.write().await;
    
    // Find if this trade already exists in the collection
    if let Some(index) = trades.iter().position(|t| t.id == tracker.id) {
        // Update existing trade
        trades[index] = tracker;
    } else {
        // Add new trade
        trades.push(tracker);
    }
}

/// Retry an async operation with timeout
///
/// # Arguments
/// * `operation` - Function to retry
/// * `operation_name` - Name of the operation for logging
/// * `retries` - Number of retry attempts
/// * `timeout_secs` - Timeout in seconds
///
/// # Returns
/// * `Result<T>` - Result of the operation
pub async fn retry_with_timeout<T, F, Fut>(
    operation: F,
    operation_name: &str,
    retries: usize,
    timeout_secs: u64,
) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut attempts = 0;
    let max_attempts = retries + 1; // Initial attempt + retries
    
    loop {
        attempts += 1;
        
        debug!("Attempt {}/{} for {}", attempts, max_attempts, operation_name);
        
        // Execute operation with timeout
        let operation_result = match timeout(Duration::from_secs(timeout_secs), operation()).await {
            Ok(result) => result,
            Err(_) => {
                warn!("{} timed out after {} seconds", operation_name, timeout_secs);
                Err(anyhow!("Operation timed out"))
            }
        };
        
        match operation_result {
            Ok(value) => {
                return Ok(value);
            }
            Err(e) => {
                if attempts >= max_attempts {
                    warn!(
                        "Failed to {} after {} attempts: {}",
                        operation_name, max_attempts, e
                    );
                    return Err(e).context(format!("Failed after {} attempts", max_attempts));
                }
                
                // Exponential backoff with jitter
                let base_delay = 500; // 500ms
                let max_delay = 5000; // 5 seconds
                let delay = std::cmp::min(
                    base_delay * 2u64.pow(attempts as u32 - 1),
                    max_delay
                );
                
                // Add jitter (±20%)
                let jitter = (rand::random::<f64>() * 0.4 - 0.2) * delay as f64;
                let delay_with_jitter = (delay as f64 + jitter) as u64;
                
                warn!(
                    "Attempt {}/{} for {} failed: {}. Retrying in {}ms",
                    attempts, max_attempts, operation_name, e, delay_with_jitter
                );
                
                tokio::time::sleep(Duration::from_millis(delay_with_jitter)).await;
            }
        }
    }
}

/// Create a trade tracker from trade parameters
///
/// # Arguments
/// * `params` - Trade parameters
/// * `trade_id` - Unique ID for the trade
/// * `timestamp` - Current timestamp
///
/// # Returns
/// * `TradeTracker` - New trade tracker
pub fn create_trade_tracker(
    params: &TradeParams,
    trade_id: &str,
    timestamp: u64,
) -> TradeTracker {
    TradeTracker {
        id: trade_id.to_string(),
        chain_id: params.chain_id(),
        token_address: params.token_address.clone(),
        amount: params.amount,
        status: TradeStatus::Pending,
        strategy: params.strategy.clone().unwrap_or_else(|| TradeStrategy::Manual),
        created_at: timestamp,
        updated_at: timestamp,
        transaction_hash: None,
        error_message: None,
    }
} 