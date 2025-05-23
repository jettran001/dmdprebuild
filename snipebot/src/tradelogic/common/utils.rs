/// Common utility functions for trade logic modules
///
/// This module provides shared utility functions used by both smart_trade and mev_logic,
/// ensuring code reuse and consistent behavior.

use std::collections::HashMap;
use anyhow::{Result, Context};
use tracing::{debug, error, info, warn};
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use std::sync::Arc;

use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::token_status::{ContractInfo, TokenStatus};
use super::types::TokenIssue;

/// Analyzes transactions from mempool to identify patterns
///
/// Takes a collection of mempool transactions and analyzes them to identify
/// patterns such as large buys, sells, or other significant transaction types.
///
/// # Arguments
/// * `transactions` - List of mempool transactions to analyze
///
/// # Returns
/// * `HashMap<String, f64>` - Map of pattern types to their significance (0-1)
pub fn analyze_transaction_pattern(
    transactions: &[crate::analys::mempool::MempoolTransaction]
) -> (usize, usize) {
    // Count buy and sell transactions
    let mut buy_count = 0;
    let mut sell_count = 0;
    
    for tx in transactions {
        match tx.tx_type {
            crate::analys::mempool::TransactionType::Swap => {
                // Determine if it's a buy or sell based on token direction
                if let (Some(from_token), Some(_)) = (&tx.from_token, &tx.to_token) {
                    if from_token.is_base_token {
                        buy_count += 1;
                    } else {
                        sell_count += 1;
                    }
                }
            },
            _ => {},
        }
    }
    
    (buy_count, sell_count)
}

/// Calculates liquidity price impact for a given transaction size
///
/// # Arguments
/// * `token_address` - Token address to check
/// * `amount_in_eth` - Transaction size in ETH/BNB
/// * `adapter` - EVM adapter for blockchain interaction
///
/// # Returns
/// * `Result<f64>` - Estimated price impact as percentage
pub async fn calculate_price_impact(
    token_address: &str,
    amount_in_eth: f64,
    adapter: &std::sync::Arc<EvmAdapter>
) -> Result<f64> {
    // Get current token price
    let current_price = adapter.get_token_price(token_address).await
        .context("Failed to get token price")?;
    
    // Simulate buy at current amount
    let (expected_tokens, _) = adapter.simulate_swap_amount_out(
        "ETH", 
        token_address, 
        amount_in_eth
    ).await.context("Failed to simulate swap")?;
    
    // Calculate expected price
    let expected_price = amount_in_eth / expected_tokens;
    
    // Calculate price impact
    let price_impact = ((expected_price - current_price) / current_price) * 100.0;
    
    Ok(price_impact.abs())
}

/// Checks if a token contract is likely a proxy
///
/// # Arguments
/// * `contract_info` - Contract information
///
/// # Returns
/// * `bool` - True if the contract is likely a proxy
pub fn is_proxy_contract(contract_info: &ContractInfo) -> bool {
    // Check for common proxy patterns in bytecode or source code
    let is_proxy_bytecode = if let Some(bytecode) = &contract_info.bytecode {
        // Look for delegatecall signatures (0xF4, 0x45, 0x14, 0xFC)
        bytecode.contains("f4") && 
        bytecode.contains("delegatecall") &&
        bytecode.len() < 5000 // Proxies typically have small bytecode
    } else {
        false
    };
    
    let is_proxy_source = if let Some(source) = &contract_info.source_code {
        // Look for proxy patterns in source code
        source.contains("delegatecall") &&
        (source.contains("implementation") || 
         source.contains("upgradeablility") ||
         source.contains("proxy") ||
         source.contains("delegate"))
    } else {
        false
    };
    
    is_proxy_bytecode || is_proxy_source
}

/// Checks if a token has blacklist or whitelist functionality
///
/// # Arguments
/// * `contract_info` - Contract information
///
/// # Returns
/// * `(bool, bool)` - (has_blacklist, has_whitelist)
pub fn check_token_lists(contract_info: &ContractInfo) -> (bool, bool) {
    let mut has_blacklist = false;
    let mut has_whitelist = false;
    
    if let Some(source) = &contract_info.source_code {
        // Check for blacklist patterns
        has_blacklist = source.contains("blacklist") || 
                        source.contains("blocklist") ||
                        source.contains("banned") ||
                        source.contains("denylist");
        
        // Check for whitelist patterns
        has_whitelist = source.contains("whitelist") ||
                        source.contains("allowlist");
    }
    
    // Check for blacklist in function signatures
    if let Some(abi) = &contract_info.abi {
        has_blacklist = has_blacklist || 
                        abi.contains("blacklist") ||
                        abi.contains("blocklist") ||
                        abi.contains("ban");
        
        has_whitelist = has_whitelist ||
                        abi.contains("whitelist") ||
                        abi.contains("allowlist");
    }
    
    (has_blacklist, has_whitelist)
}

/// Formats token address with checksum and shortcode
///
/// # Arguments
/// * `address` - Token address to format
///
/// # Returns
/// * `String` - Formatted address (e.g. "0xABCD...1234")
pub fn format_token_address(address: &str) -> String {
    if address.len() < 10 {
        return address.to_string();
    }
    
    let start = &address[0..6];
    let end = &address[address.len() - 4..];
    format!("{}...{}", start, end)
}

/// Checks if a token is likely to be a pump and dump scheme
///
/// Analyzes token metadata, transaction patterns, and other factors
/// to determine if a token is likely a short-term pump and dump.
///
/// # Arguments
/// * `token_address` - Address of the token to check
/// * `adapter` - EVM adapter for blockchain interaction
///
/// # Returns
/// * `Result<bool>` - True if token shows pump and dump indicators
pub async fn is_pump_and_dump(
    token_address: &str,
    adapter: &std::sync::Arc<EvmAdapter>
) -> Result<bool> {
    // Check token age
    let contract_info = adapter.get_contract_info(token_address).await
        .context("Failed to get contract info")?;
    
    // Get price history
    let price_history = adapter.get_token_price_history(token_address, 24).await
        .context("Failed to get price history")?;
    
    // Get transaction history
    let tx_history = adapter.get_token_transaction_history(token_address, 50).await
        .context("Failed to get transaction history")?;
    
    // Calculate buy/sell ratio
    let (buy_count, sell_count) = analyze_transaction_pattern(&tx_history);
    
    // Calculate price volatility
    let has_high_volatility = if price_history.len() >= 2 {
        let max_price = price_history.iter().fold(0.0f64, |a, b| a.max(*b));
        let min_price = price_history.iter().fold(f64::INFINITY, |a, b| a.min(*b));
        
        if min_price > 0.0 {
            let volatility = (max_price - min_price) / min_price;
            volatility > 0.3 // 30% volatility threshold
        } else {
            false
        }
    } else {
        false
    };
    
    // Calculate suspicion factors
    let factors = [
        // Unverified contract is suspicious
        !contract_info.is_verified,
        
        // High buy to sell ratio indicates artificial pump
        buy_count > sell_count * 3,
        
        // High volatility indicates manipulation
        has_high_volatility,
        
        // Check if owner has significant token percentage
        if let Some(owner) = &contract_info.owner_address {
            let owner_balance = adapter.get_token_balance(token_address, owner).await.unwrap_or(0.0);
            let total_supply = adapter.get_token_total_supply(token_address).await.unwrap_or(0.0);
            if total_supply > 0.0 {
                (owner_balance / total_supply) > 0.3 // Owner holds >30%
            } else {
                false
            }
        } else {
            false
        },
    ];
    
    // If at least 2 factors are true, consider it suspicious
    let factor_count = factors.iter().filter(|&&factor| factor).count();
    
    Ok(factor_count >= 2)
}

/// Calculates risk score for a token based on identified issues
///
/// # Arguments
/// * `issues` - List of identified token issues
///
/// # Returns
/// * `f64` - Risk score (0-100, higher is riskier)
pub fn calculate_risk_score(issues: &[TokenIssue]) -> f64 {
    if issues.is_empty() {
        return 0.0;
    }
    
    // Define risk weights for each issue type
    let risk_weights: HashMap<TokenIssue, f64> = [
        // High risk issues
        (TokenIssue::Honeypot, 100.0),
        (TokenIssue::HighTax, 70.0),
        (TokenIssue::DynamicTax, 80.0),
        (TokenIssue::OwnerWithFullControl, 75.0),
        (TokenIssue::ArbitraryCodeExecution, 95.0),
        (TokenIssue::ContractSelfDestruct, 90.0),
        (TokenIssue::DelegateCall, 85.0),
        
        // Medium risk issues
        (TokenIssue::UnverifiedContract, 50.0),
        (TokenIssue::OwnershipNotRenounced, 60.0),
        (TokenIssue::ProxyContract, 55.0),
        (TokenIssue::LowLiquidity, 45.0),
        (TokenIssue::BlacklistFunction, 65.0),
        (TokenIssue::UpgradeableLogic, 60.0),
        (TokenIssue::UnlimitedMintAuthority, 70.0),
        
        // Lower risk issues
        (TokenIssue::WhitelistFunction, 35.0),
        (TokenIssue::TradingCooldown, 30.0),
        (TokenIssue::MaxTransactionLimit, 25.0),
        (TokenIssue::MaxWalletLimit, 20.0),
    ].iter().cloned().collect();
    
    // Calculate average risk score based on identified issues
    let total_risk: f64 = issues.iter()
        .map(|issue| risk_weights.get(issue).unwrap_or(&20.0))
        .sum();
    
    // Apply diminishing returns formula for multiple issues
    // More issues are riskier, but with diminishing impact
    let risk_score = total_risk / (1.0 + 0.1 * (issues.len() as f64 - 1.0));
    
    // Cap at 100
    risk_score.min(100.0)
}

/// Get current timestamp in seconds
pub fn current_time_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs()
}

/// Calculate percentage change between two values
pub fn calculate_percentage_change(old_value: f64, new_value: f64) -> f64 {
    if old_value == 0.0 {
        return 0.0;
    }
    ((new_value - old_value) / old_value) * 100.0
}

/// Convert ETH amount to USD using given ETH price
pub fn eth_to_usd(eth_amount: f64, eth_price_usd: f64) -> f64 {
    eth_amount * eth_price_usd
}

/// Convert USD amount to ETH using given ETH price
pub fn usd_to_eth(usd_amount: f64, eth_price_usd: f64) -> f64 {
    if eth_price_usd == 0.0 {
        return 0.0;
    }
    usd_amount / eth_price_usd
}

/// Calculate profit from trade
pub fn calculate_profit(buy_price: f64, sell_price: f64, amount: f64, buy_tax: f64, sell_tax: f64) -> (f64, f64) {
    // Calculate effective amounts after taxes
    let effective_buy_amount = amount * (1.0 - buy_tax / 100.0);
    let buy_value = effective_buy_amount * buy_price;
    
    let effective_sell_amount = amount * (1.0 - sell_tax / 100.0);
    let sell_value = effective_sell_amount * sell_price;
    
    // Calculate profit amount and percentage
    let profit_amount = sell_value - buy_value;
    let profit_percent = if buy_value > 0.0 {
        (profit_amount / buy_value) * 100.0
    } else {
        0.0
    };
    
    (profit_amount, profit_percent)
}

/// Wait for transaction confirmation
pub async fn wait_for_transaction(tx_hash: &str, adapter: &Arc<EvmAdapter>, timeout_seconds: u64) -> Result<bool> {
    let start_time = current_time_seconds();
    let timeout = Duration::from_secs(timeout_seconds);
    
    while current_time_seconds() - start_time < timeout_seconds {
        match adapter.get_transaction_receipt(tx_hash).await {
            Ok(Some(receipt)) => {
                if receipt.status == Some(1.into()) {
                    info!("Transaction {} confirmed successfully", tx_hash);
                    return Ok(true);
                } else {
                    error!("Transaction {} failed", tx_hash);
                    return Ok(false);
                }
            }
            Ok(None) => {
                debug!("Transaction {} still pending", tx_hash);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(e) => {
                error!("Error checking transaction {}: {}", tx_hash, e);
                return Err(e.into());
            }
        }
    }
    
    warn!("Transaction {} timed out after {} seconds", tx_hash, timeout_seconds);
    Ok(false)
}

/// Convert raw balance to token units
pub fn convert_to_token_units(balance: &str, decimals: u8) -> f64 {
    if let Ok(raw_balance) = balance.parse::<f64>() {
        raw_balance / 10_f64.powi(decimals as i32)
    } else {
        0.0
    }
}

/// Calculate moving average from price history
pub fn calculate_moving_average(price_history: &[f64], period: usize) -> f64 {
    if price_history.is_empty() || period == 0 {
        return 0.0;
    }
    
    let start_idx = if price_history.len() > period {
        price_history.len() - period
    } else {
        0
    };
    
    let sum: f64 = price_history[start_idx..].iter().sum();
    sum / (price_history.len() - start_idx) as f64
}

/// Find price extremes in history
pub fn find_price_extremes(price_history: &[f64], period: usize) -> (f64, f64) {
    if price_history.is_empty() {
        return (0.0, 0.0);
    }
    
    let start_idx = if price_history.len() > period {
        price_history.len() - period
    } else {
        0
    };
    
    let slice = &price_history[start_idx..];
    let min = slice.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let max = slice.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    
    (min, max)
}

/// Calculate RSI from price history
pub fn calculate_rsi(price_history: &[f64], period: usize) -> f64 {
    if price_history.len() < 2 || period == 0 {
        return 50.0; // Default neutral RSI
    }
    
    let mut gains = 0.0;
    let mut losses = 0.0;
    
    for i in 1..price_history.len() {
        let change = price_history[i] - price_history[i-1];
        if change >= 0.0 {
            gains += change;
        } else {
            losses -= change;
        }
    }
    
    if losses == 0.0 {
        return 100.0;
    }
    
    let rs = gains / losses;
    100.0 - (100.0 / (1.0 + rs))
}

/// Detect price trend from price history
pub fn detect_price_trend(price_history: &[f64], period: usize) -> (bool, bool, f64) {
    if price_history.len() < period {
        return (false, false, 0.0);
    }
    
    // Get most recent data
    let recent_prices = &price_history[price_history.len() - period..];
    
    // Count price increases and decreases
    let mut increases = 0;
    let mut decreases = 0;
    
    for i in 1..recent_prices.len() {
        if recent_prices[i] > recent_prices[i-1] {
            increases += 1;
        } else if recent_prices[i] < recent_prices[i-1] {
            decreases += 1;
        }
    }
    
    // Calculate trend strength
    let total_changes = increases + decreases;
    let uptrend_strength = if total_changes > 0 {
        increases as f64 / total_changes as f64
    } else {
        0.5 // No changes, consider neutral
    };
    
    let downtrend_strength = 1.0 - uptrend_strength;
    
    // Threshold for trend determination (70% changes in same direction)
    let is_uptrend = uptrend_strength >= 0.7;
    let is_downtrend = downtrend_strength >= 0.7;
    
    let trend_strength = (uptrend_strength - 0.5).abs() * 2.0; // Convert to 0-1 scale
    
    (is_uptrend, is_downtrend, trend_strength)
} 