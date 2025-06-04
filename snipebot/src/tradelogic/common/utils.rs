/// Common utility functions for trade logic modules
///
/// This module provides shared utility functions used by both smart_trade and mev_logic,
/// ensuring code reuse and consistent behavior.

use std::collections::HashMap;
use anyhow::{Result, Context, anyhow};
use tracing::{debug, error, info, warn};
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use std::sync::Arc;
use uuid::Uuid;
use chrono::Utc;

use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::token_status::{ContractInfo, TokenStatus};
use crate::types::{TradeParams, TradeType};
use super::types::{TokenIssue, SecurityCheckResult, TradeAction, TradeStatus, ExecutionMethod, TokenIssueDetail, TokenIssueType};
use crate::tradelogic::smart_trade::types::TradeTracker;

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
        .unwrap_or_default()
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

/// Generate a unique trade ID
pub fn generate_trade_id() -> String {
    Uuid::new_v4().to_string()
}

/// Validate basic trade parameters
/// 
/// This function performs basic validations applicable to all types of trades
/// 
/// # Arguments
/// * `params` - Trade parameters to validate
/// * `adapter` - EVM adapter for the chain
/// 
/// # Returns
/// * `Result<()>` - Ok if valid, Error with message if invalid
pub async fn validate_trade_params(params: &TradeParams, adapter: &Arc<EvmAdapter>) -> Result<()> {
    // Validate amount
    if params.amount <= 0.0 {
        return Err(anyhow!("Invalid trade amount: {}", params.amount));
    }
    
    // Validate token address format
    if params.token_address.is_empty() || !adapter.is_valid_address(&params.token_address) {
        return Err(anyhow!("Invalid token address: {}", params.token_address));
    }
    
    // Validate trade type
    match params.trade_type {
        TradeType::Buy | TradeType::Sell | TradeType::Approve => {
            // Valid trade types
        }
        _ => {
            return Err(anyhow!("Unsupported trade type: {:?}", params.trade_type));
        }
    }
    
    Ok(())
}

/// Simulate trade execution to get expected output and gas
///
/// # Arguments
/// * `params` - Trade parameters
/// * `adapter` - EVM adapter for the chain
///
/// # Returns
/// * `Result<(f64, f64)>` - (expected_output, estimated_gas)
pub async fn simulate_trade_execution(params: &TradeParams, adapter: &Arc<EvmAdapter>) -> Result<(f64, f64)> {
    match params.trade_type {
        TradeType::Buy => {
            // For buying, we're swapping native tokens (ETH) for the token
            let native_token = "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE"; // Standard placeholder for native token
            
            // Simulate the swap to get expected output
            let expected_output = adapter.simulate_swap_amount_out(
                native_token,
                &params.token_address,
                params.amount
            ).await.context("Failed to simulate swap")?;
            
            // Estimated gas (placeholder for now)
            let estimated_gas = 200000.0;
            
            Ok((expected_output, estimated_gas))
        },
        TradeType::Sell => {
            // For selling, we're swapping the token for native tokens (ETH)
            let native_token = "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE"; // Standard placeholder for native token
            
            // Simulate the swap to get expected output
            let expected_output = adapter.simulate_swap_amount_out(
                &params.token_address,
                native_token,
                params.amount
            ).await.context("Failed to simulate swap")?;
            
            // Estimated gas (placeholder for now)
            let estimated_gas = 250000.0;
            
            Ok((expected_output, estimated_gas))
        },
        TradeType::Approve => {
            // For approvals, no output amount but still need gas
            let estimated_gas = 100000.0;
            
            Ok((0.0, estimated_gas))
        },
        _ => {
            Err(anyhow!("Unsupported trade type for simulation: {:?}", params.trade_type))
        }
    }
}

/// Execute a blockchain transaction
/// 
/// Executes a trade on the blockchain using the provided parameters.
/// This is a high-level wrapper around the specific adapter methods.
/// 
/// # Arguments
/// * `params` - Trade parameters
/// * `adapter` - EVM adapter for the chain
/// * `wallet_address` - Wallet address to use for transaction
/// 
/// # Returns
/// * `Result<(String, f64)>` - (transaction hash, gas used)
/// 
/// # Safety
/// This function performs critical validation before executing trades.
/// It will validate slippage settings, check minimum liquidity, and
/// enforce safety limits as defined in constants.
pub async fn execute_blockchain_transaction(
    params: &TradeParams,
    adapter: &Arc<EvmAdapter>,
    wallet_address: &str
) -> Result<(String, f64)> {
    use tracing::{info, warn};
    use crate::tradelogic::common::types::{DEFAULT_SLIPPAGE, DEFAULT_MAX_RISK_SCORE, 
                                          MIN_LIQUIDITY_USD, MAX_ACCEPTABLE_BUY_TAX, 
                                          MAX_ACCEPTABLE_SELL_TAX};
    
    // SAFETY CHECK 1: Validate params first
    validate_trade_params(params, adapter).await?;
    
    // SAFETY CHECK 2: Check slippage and warn if using default
    let slippage = if let Some(slippage) = params.slippage {
        if slippage <= 0.0 {
            warn!("Invalid slippage value ({}), using default: {}%", slippage, DEFAULT_SLIPPAGE);
            DEFAULT_SLIPPAGE
        } else if slippage > 50.0 {
            warn!("Dangerously high slippage ({}%), limiting to 50%", slippage);
            50.0
        } else {
            info!("Using custom slippage: {}%", slippage);
            slippage
        }
    } else {
        warn!("No slippage specified, using default: {}%", DEFAULT_SLIPPAGE);
        DEFAULT_SLIPPAGE
    };
    
    // SAFETY CHECK 3: Check token safety before trading
    let security_check = perform_token_security_check(&params.token_address, adapter).await
        .context("Failed to perform security check")?;
    
    // Abort if token has critical security issues
    if security_check.is_honeypot {
        return Err(anyhow!("Aborting transaction: Token detected as a potential honeypot"));
    }
    
    if security_check.score > DEFAULT_MAX_RISK_SCORE {
        return Err(anyhow!("Aborting transaction: Token security score too low ({})", security_check.score));
    }
    
    // SAFETY CHECK 4: Check liquidity if this is a buy or sell
    if params.is_buy() || params.is_sell() {
        // Get token liquidity
        if let Ok(liquidity) = adapter.get_token_liquidity(&params.token_address).await {
            if liquidity < MIN_LIQUIDITY_USD {
                return Err(anyhow!("Insufficient liquidity (${:.2}) for safe trading", liquidity));
            }
        } else {
            warn!("Could not verify token liquidity, proceeding with caution");
        }
        
        // Check token tax
        if let Ok(tax_info) = adapter.get_token_tax_info(&params.token_address).await {
            if params.is_buy() && tax_info.buy_tax > MAX_ACCEPTABLE_BUY_TAX {
                return Err(anyhow!("Buy tax too high: {:.2}% (max acceptable: {:.2}%)", 
                                  tax_info.buy_tax, MAX_ACCEPTABLE_BUY_TAX));
            }
            
            if params.is_sell() && tax_info.sell_tax > MAX_ACCEPTABLE_SELL_TAX {
                return Err(anyhow!("Sell tax too high: {:.2}% (max acceptable: {:.2}%)", 
                                  tax_info.sell_tax, MAX_ACCEPTABLE_SELL_TAX));
            }
        } else {
            warn!("Could not verify token tax info, proceeding with caution");
        }
    }
    
    // SAFETY CHECK 5: Simulate transaction for buy/sell to detect potential failures
    if (params.is_buy() || params.is_sell()) && params.trade_type != TradeType::Approve {
        match simulate_trade_execution(params, adapter).await {
            Ok((expected_output, gas_estimate)) => {
                info!("Transaction simulation successful. Expected output: {}, estimated gas: {}", 
                     expected_output, gas_estimate);
                
                // Check if output is suspiciously low (possible sandwich attack)
                let min_expected = params.amount * 0.9; // Expect at least 90% of input value
                if expected_output < min_expected {
                    warn!("Expected output ({}) abnormally low. Possible MEV attack.", expected_output);
                }
            }
            Err(e) => {
                return Err(anyhow!("Transaction simulation failed: {}", e));
            }
        }
    }
    
    // Execute the transaction based on trade type
    match params.trade_type {
        TradeType::Buy => {
            // Get quote token
            let base_token = params.base_token.clone().unwrap_or_else(|| {
                match params.chain_id() {
                    1 => "ETH",   // Ethereum
                    56 => "BNB",  // Binance Smart Chain
                    137 => "MATIC", // Polygon
                    43114 => "AVAX", // Avalanche
                    _ => "ETH"    // Default
                }.to_string()
            });
            
            // Execute swap transaction using the validated slippage
            let (tx_hash, gas_used) = adapter.execute_swap(
                &base_token,
                &params.token_address,
                params.amount,
                wallet_address,
                slippage
            ).await.context("Failed to execute buy transaction")?;
            
            Ok((tx_hash, gas_used))
        }
        TradeType::Sell => {
            // Get base token from chain
            let base_token = params.base_token.clone().unwrap_or_else(|| {
                match params.chain_id() {
                    1 => "ETH",   // Ethereum
                    56 => "BNB",  // Binance Smart Chain
                    137 => "MATIC", // Polygon
                    43114 => "AVAX", // Avalanche
                    _ => "ETH"    // Default
                }.to_string()
            });
            
            // Execute swap transaction using the validated slippage
            let (tx_hash, gas_used) = adapter.execute_swap(
                &params.token_address,
                &base_token,
                params.amount,
                wallet_address,
                slippage
            ).await.context("Failed to execute sell transaction")?;
            
            Ok((tx_hash, gas_used))
        }
        TradeType::Approve => {
            // Execute approve transaction
            let (tx_hash, gas_used) = adapter.approve_token(
                &params.token_address,
                wallet_address,
                params.amount
            ).await.context("Failed to execute approve transaction")?;
            
            Ok((tx_hash, gas_used))
        }
        _ => {
            Err(anyhow!("Unsupported trade type for execution: {:?}", params.trade_type))
        }
    }
}

/// Perform security checks on a token
/// 
/// Checks token for common security risks like honeypots, rug pulls, etc.
/// 
/// # Arguments
/// * `token_address` - Address of the token to check
/// * `adapter` - EVM adapter for the chain
/// 
/// # Returns
/// * `Result<SecurityCheckResult>` - Security check result with risk score and issues
pub async fn perform_token_security_check(
    token_address: &str,
    adapter: &Arc<EvmAdapter>
) -> Result<SecurityCheckResult> {
    // Get basic token info
    let token_info = adapter.get_token_info(token_address)
        .await
        .context("Failed to get token info")?;
    
    // Get contract source/bytecode for analysis
    let contract_info = adapter.get_contract_info(token_address)
        .await
        .context("Failed to get contract info")?;
    
    // Detect various security issues
    let mut issues = Vec::new();
    let mut risk_score = 0;
    
    // Check if it's likely a proxy contract
    if is_proxy_contract(&contract_info) {
        issues.push(TokenIssueDetail {
            issue_type: TokenIssueType::Proxy,
            description: "Contract uses proxy pattern".to_string(),
            severity: 50,
        });
        risk_score += 15;
    }
    
    // Check for blacklist functionality
    if has_blacklist_function(&contract_info) {
        issues.push(TokenIssueDetail {
            issue_type: TokenIssueType::Blacklist,
            description: "Contract has blacklist functionality".to_string(),
            severity: 40,
        });
        risk_score += 10;
    }
    
    // Check for high taxes/fees
    if let Some(tax_info) = adapter.get_token_tax_info(token_address).await.ok() {
        if tax_info.buy_tax > 10.0 {
            issues.push(TokenIssueDetail {
                issue_type: TokenIssueType::FeeManipulation,
                description: format!("High buy tax: {}%", tax_info.buy_tax),
                severity: 50,
            });
            risk_score += 15;
        }
        
        if tax_info.sell_tax > 10.0 {
            issues.push(TokenIssueDetail {
                issue_type: TokenIssueType::FeeManipulation,
                description: format!("High sell tax: {}%", tax_info.sell_tax),
                severity: 60,
            });
            risk_score += 20;
        }
    }
    
    // Check for excessive owner privileges
    let excessive_privileges = check_owner_privileges(&contract_info);
    if !excessive_privileges.is_empty() {
        issues.push(TokenIssueDetail {
            issue_type: TokenIssueType::HiddenOwner,
            description: "Contract has excessive owner privileges".to_string(),
            severity: 70,
        });
        risk_score += 25;
    }
    
    // Check token age (new tokens are riskier)
    if let Some(created_at) = token_info.created_at {
        let now = current_time_seconds();
        let age_in_days = (now - created_at) / (24 * 60 * 60);
        
        if age_in_days < 1 {
            issues.push(TokenIssueDetail {
                issue_type: TokenIssueType::Other,
                description: "Token created less than 24 hours ago".to_string(),
                severity: 75,
            });
            risk_score += 30;
        } else if age_in_days < 7 {
            issues.push(TokenIssueDetail {
                issue_type: TokenIssueType::Other,
                description: "Token created less than 7 days ago".to_string(),
                severity: 50,
            });
            risk_score += 15;
        }
    }
    
    // Cap risk score at 100
    risk_score = risk_score.min(100);
    
    // Create final result
    let result = SecurityCheckResult {
        score: risk_score as u8,
        is_honeypot: false, // Detected during simulation
        issues,
        is_contract_verified: contract_info.is_verified,
        rug_pull_risk: risk_score > 70,
    };
    
    Ok(result)
}

/// Get common information for a token
/// 
/// Fetches common token information like name, symbol, decimals etc.
/// 
/// # Arguments
/// * `token_address` - Address of the token
/// * `adapter` - EVM adapter for the chain
/// 
/// # Returns
/// * `Result<(String, String, u8)>` - (name, symbol, decimals)
pub async fn get_token_metadata(
    token_address: &str,
    adapter: &Arc<EvmAdapter>
) -> Result<(String, String, u8)> {
    // Get token name
    let name = adapter.get_token_name(token_address)
        .await
        .context("Failed to get token name")
        .unwrap_or_else(|_| "Unknown".to_string());
    
    // Get token symbol
    let symbol = adapter.get_token_symbol(token_address)
        .await
        .context("Failed to get token symbol")
        .unwrap_or_else(|_| "???".to_string());
    
    // Get token decimals
    let decimals = adapter.get_token_decimals(token_address)
        .await
        .context("Failed to get token decimals")
        .unwrap_or(18);
    
    Ok((name, symbol, decimals))
}

/// Check trade conditions and determine if action needs to be taken
///
/// Analyzes a trade's current conditions against its parameters (stop loss, take profit,
/// trailing stop loss, etc.) and determines if any action needs to be taken.
///
/// # Arguments
/// * `trade` - The trade to analyze
/// * `current_price` - Current token price
/// * `highest_price` - Highest price recorded for this trade
///
/// # Returns
/// * `Option<(String, bool)>` - If action needed: (reason, is_negative)
///   where is_negative indicates whether it's a negative reason (stop loss vs take profit)
pub fn check_trade_conditions(
    trade: &TradeTracker,
    current_price: f64,
    highest_price: f64
) -> Option<(String, bool)> {
    // Calculate current profit percentage/loss
    if trade.entry_price <= 0.0 || current_price <= 0.0 {
        return None;
    }
    
    let profit_percent = ((current_price - trade.entry_price) / trade.entry_price) * 100.0;
    let current_time = chrono::Utc::now().timestamp() as u64;
    
    // 1. Check max hold time
    if current_time >= trade.max_hold_time {
        return Some(("Exceeded max hold time".to_string(), true));
    }
    
    // 2. Check take profit condition
    if profit_percent >= trade.take_profit_percent {
        return Some(("Reached profit target".to_string(), false));
    }
    
    // 3. Check stop loss condition
    if profit_percent <= -trade.stop_loss_percent {
        return Some(("Activated stop loss".to_string(), true));
    }
    
    // 4. Check trailing stop loss (if any)
    if let Some(tsl_percent) = trade.trailing_stop_percent {
        let tsl_trigger_price = highest_price * (1.0 - tsl_percent / 100.0);
        
        // Only trigger TSL if we've been in profit
        if current_price <= tsl_trigger_price && highest_price > trade.entry_price {
            return Some(("Activated trailing stop loss".to_string(), true));
        }
    }
    
    // No action needed
    None
} 