/// Common trade execution functionality
///
/// This module provides shared functionality for executing trades
/// that can be used by both smart_trade and manual_trade modules.
/// These functions help reduce code duplication and ensure consistent trade execution.

use anyhow::{Result, Context, anyhow};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::{TradeParams, TradeType, TokenPair};
use super::types::{TradeStatus, ExecutionMethod, SecurityCheckResult, TokenIssue, TokenIssueType};
use super::utils::{
    wait_for_transaction, 
    validate_trade_params,
    execute_blockchain_transaction,
    simulate_trade_execution,
    perform_token_security_check
};

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
        
        if has_critical_issue {
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
        trade_type: TradeType::Approve,
        token_address: token_address.to_string(),
        amount: amount.unwrap_or(f64::MAX), // Use max amount if not specified
        ..Default::default()
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
    let allowance = adapter.get_token_allowance(token_address, wallet_address)
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