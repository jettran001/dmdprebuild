/// Common trade execution utilities
/// 
/// This module provides functions that are used by both SmartTradeExecutor and ManualTradeExecutor
/// to reduce code duplication and maintain consistent behavior.

use std::collections::HashMap;
use std::sync::Arc;
use anyhow::{Result, Context, anyhow};
use tracing::{debug, error, info, warn};
use chrono::Utc;

use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::{TradeParams, TradeType, ChainType, TokenPair};
use crate::analys::token_status::TokenSafety;

use super::types::{TokenIssue, SecurityCheckResult, TradeStatus};
use super::utils::{validate_trade_params, generate_trade_id, simulate_trade_execution, wait_for_transaction};

/// Basic trade execution flow
/// 
/// Handles the common steps for executing a trade:
/// 1. Validate parameters
/// 2. Create trade ID
/// 3. Perform security checks
/// 4. Simulate transaction
/// 5. Execute transaction
/// 6. Monitor transaction
/// 
/// # Arguments
/// * `params` - Trade parameters
/// * `adapter` - EVM adapter for chain
/// * `wallet_address` - Wallet address to execute from
/// 
/// # Returns
/// * `Result<(String, String, f64, f64, Option<SecurityCheckResult>)>` - 
///   (trade_id, tx_hash, gas_used, price, security_check)
pub async fn execute_trade_flow(
    params: &TradeParams,
    adapter: &Arc<EvmAdapter>,
    wallet_address: &str
) -> Result<(String, String, f64, f64, Option<SecurityCheckResult>)> {
    // 1. Validate parameters
    validate_trade_params(params, adapter).await?;
    
    // 2. Create trade ID and timestamp
    let trade_id = generate_trade_id();
    let now = Utc::now().timestamp() as u64;
    
    // 3. Perform security checks on token
    let security_check = match adapter.analyze_token_safety(&params.token_address).await {
        Ok(safety) => {
            // Convert to SecurityCheckResult
            let issues = safety.rug_indicators.iter()
                .map(|issue| match issue.as_str() {
                    "honeypot" => TokenIssue::Honeypot,
                    "high_buy_tax" => TokenIssue::HighBuyTax,
                    "high_sell_tax" => TokenIssue::HighSellTax,
                    "blacklist" => TokenIssue::HasBlacklist,
                    "owner_privileges" => TokenIssue::ExcessiveOwnerPrivileges,
                    "proxy" => TokenIssue::ProxyImplementation,
                    _ => TokenIssue::Other(issue.clone())
                })
                .collect();
            
            Some(SecurityCheckResult {
                token_address: params.token_address.clone(),
                issues,
                risk_score: safety.risk_score as u8,
                safe_to_trade: safety.risk_score < 70,
                extra_info: HashMap::new(),
            })
        }
        Err(e) => {
            warn!("Failed to analyze token safety: {}", e);
            None
        }
    };
    
    // 4. Simulate transaction
    let (expected_output, estimated_gas) = simulate_trade_execution(params, adapter).await?;
    
    // 5. Execute transaction
    let (tx_hash, gas_used) = match params.trade_type {
        TradeType::Buy => {
            // Get base token from chain
            let base_token = params.base_token.clone().unwrap_or_else(|| {
                match params.chain_id {
                    1 => "ETH",   // Ethereum
                    56 => "BNB",  // Binance Smart Chain
                    137 => "MATIC", // Polygon
                    43114 => "AVAX", // Avalanche
                    _ => "ETH"    // Default
                }.to_string()
            });
            
            // Execute swap
            adapter.execute_swap(
                &base_token,
                &params.token_address,
                params.amount,
                wallet_address,
                params.slippage.unwrap_or(1.0)
            ).await.context("Failed to execute buy transaction")?
        }
        TradeType::Sell => {
            // Get base token from chain
            let base_token = params.base_token.clone().unwrap_or_else(|| {
                match params.chain_id {
                    1 => "ETH",   // Ethereum
                    56 => "BNB",  // Binance Smart Chain
                    137 => "MATIC", // Polygon
                    43114 => "AVAX", // Avalanche
                    _ => "ETH"    // Default
                }.to_string()
            });
            
            // Execute swap
            adapter.execute_swap(
                &params.token_address,
                &base_token,
                params.amount,
                wallet_address,
                params.slippage.unwrap_or(1.0)
            ).await.context("Failed to execute sell transaction")?
        }
        _ => {
            return Err(anyhow!("Unsupported trade type for execution: {:?}", params.trade_type));
        }
    };
    
    // Calculate price
    let price = if expected_output > 0.0 { params.amount / expected_output } else { 0.0 };
    
    Ok((trade_id, tx_hash, gas_used, price, security_check))
} 