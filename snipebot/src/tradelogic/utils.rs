//! Unified Utilities for Trade Logic Module
//!
//! This module contains shared utility functions used across all trade logic modules.
//! Functions are organized into categories:
//! - Time and ID generation
//! - Price and value calculations
//! - Market data analysis
//! - Transaction and blockchain interaction
//! - Rate limiting and API retry logic

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::str::FromStr;
use chrono::{DateTime, Utc};
use anyhow::{Result, Context, anyhow};
use serde::{Serialize, Deserialize};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use rand::{thread_rng, Rng};
use once_cell::sync::Lazy;
use uuid::Uuid;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::{TradeParams, TradeType, Transaction, TokenPair};
use crate::analys::token_status::{ContractInfo, TokenInfo};
use crate::tradelogic::common::types::{TokenIssue, SecurityCheckResult, TradeAction, TradeStatus, TokenIssueDetail, TokenIssueType};
use crate::tradelogic::smart_trade::types::{TradeTracker, TradeResult, TradeStrategy, MarketData, SwapData, SwapMethod, TokenAmount};

//=============================================================================
// TIME AND ID GENERATION UTILITIES
//=============================================================================

/// Get current timestamp in seconds (Unix epoch)
pub fn current_time_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}

/// Generate random UUID for trades
pub fn generate_trade_id() -> String {
    Uuid::new_v4().to_string()
}

/// Generate general purpose UUID
pub fn generate_id() -> String {
    Uuid::new_v4().to_string()
}

//=============================================================================
// PRICE AND CURRENCY CONVERSION UTILITIES
//=============================================================================

/// Calculate percentage change between two values
pub fn calculate_percentage_change(old_value: f64, new_value: f64) -> f64 {
    if old_value == 0.0 {
        return 0.0;
    }
    ((new_value - old_value) / old_value) * 100.0
}

/// Convert ETH to USD
pub fn eth_to_usd(eth_amount: f64, eth_price_usd: f64) -> f64 {
    eth_amount * eth_price_usd
}

/// Convert USD to ETH
pub fn usd_to_eth(usd_amount: f64, eth_price_usd: f64) -> f64 {
    if eth_price_usd <= 0.0 {
        return 0.0;
    }
    usd_amount / eth_price_usd
}

/// Convert Wei to Gwei
pub fn wei_to_gwei(wei: u64) -> f64 {
    wei as f64 / 1_000_000_000.0
}

/// Convert Gwei to Wei
pub fn gwei_to_wei(gwei: f64) -> u64 {
    (gwei * 1_000_000_000.0) as u64
}

/// Convert Wei to ETH
pub fn wei_to_eth(wei: u64) -> f64 {
    wei as f64 / 1_000_000_000_000_000_000.0
}

/// Convert ETH to Wei
pub fn eth_to_wei(eth: f64) -> u64 {
    (eth * 1_000_000_000_000_000_000.0) as u64
}

/// Calculate gas cost in ETH
pub fn calculate_gas_cost_eth(gas_used: u64, gas_price: u64) -> f64 {
    wei_to_eth(gas_used * gas_price)
}

/// Calculate gas cost in USD
pub fn calculate_gas_cost_usd(gas_used: u64, gas_price: u64, eth_price_usd: f64) -> f64 {
    let cost_eth = calculate_gas_cost_eth(gas_used, gas_price);
    eth_to_usd(cost_eth, eth_price_usd)
}

/// Calculate profit from trade
pub fn calculate_profit(buy_price: f64, sell_price: f64, amount: f64, buy_tax: f64, sell_tax: f64) -> (f64, f64) {
    // Calculate net amount after buy tax
    let net_amount = amount * (1.0 - buy_tax / 100.0);
    
    // Calculate sell value and apply sell tax
    let sell_value = net_amount * sell_price * (1.0 - sell_tax / 100.0);
    let buy_value = amount * buy_price;
    
    // Calculate profit
    let profit_usd = sell_value - buy_value;
    let profit_percent = if buy_value > 0.0 {
        (profit_usd / buy_value) * 100.0
    } else {
        0.0
    };
    
    (profit_usd, profit_percent)
}

/// Convert token balance to human-readable units
pub fn convert_to_token_units(balance: &str, decimals: u8) -> f64 {
    let balance_value = match balance.parse::<f64>() {
        Ok(val) => val,
        Err(_) => return 0.0,
    };
    
    balance_value / 10f64.powi(decimals as i32)
}

/// Format large numbers with appropriate decimals
pub fn format_number(value: f64, decimals: usize) -> String {
    format!("{:.*}", decimals, value)
}

/// Format address to short form (e.g. 0xABCD...1234)
pub fn format_address(address: &str) -> String {
    if address.len() < 10 {
        return address.to_string();
    }
    
    let start = &address[0..6];
    let end = &address[address.len() - 4..];
    format!("{}...{}", start, end)
}

/// Calculate token amount from USD value
pub async fn calculate_token_amount_from_usd(
    adapter: &Arc<EvmAdapter>,
    token_address: &str,
    usd_amount: f64,
) -> Result<f64> {
    let token_price = adapter.get_token_price(token_address).await?;
    
    if token_price <= 0.0 {
        return Err(anyhow!("Invalid token price: {}", token_price));
    }
    
    let token_amount = usd_amount / token_price;
    Ok(token_amount)
}

/// Calculate USD value from token amount
pub async fn calculate_usd_value_from_token(
    adapter: &Arc<EvmAdapter>,
    token_address: &str,
    token_amount: f64,
) -> Result<f64> {
    let token_price = adapter.get_token_price(token_address).await?;
    let usd_value = token_amount * token_price;
    Ok(usd_value)
}

/// Safely convert Option<u64> to f64
pub fn option_u64_to_f64(value: Option<u64>, default: u64) -> f64 {
    value.unwrap_or(default) as f64
}

//=============================================================================
// MARKET AND PRICE ANALYSIS UTILITIES
//=============================================================================

/// Calculate moving average on price history
pub fn calculate_moving_average(price_history: &[f64], period: usize) -> f64 {
    if price_history.is_empty() || period == 0 {
        return 0.0;
    }
    
    let period = period.min(price_history.len());
    let recent_prices = &price_history[price_history.len() - period..];
    
    let sum: f64 = recent_prices.iter().sum();
    sum / period as f64
}

/// Find minimum and maximum prices in a period
pub fn find_price_extremes(price_history: &[f64], period: usize) -> (f64, f64) {
    if price_history.is_empty() {
        return (0.0, 0.0);
    }
    
    let period = period.min(price_history.len());
    let recent_prices = &price_history[price_history.len() - period..];
    
    let min_price = recent_prices.iter()
        .min_by(|a, b| match a.partial_cmp(b) {
            Some(ordering) => ordering,
            None => std::cmp::Ordering::Equal
        })
        .copied()
        .unwrap_or(0.0);
    
    let max_price = recent_prices.iter()
        .max_by(|a, b| match a.partial_cmp(b) {
            Some(ordering) => ordering,
            None => std::cmp::Ordering::Equal
        })
        .copied()
        .unwrap_or(0.0);
    
    (min_price, max_price)
}

/// Calculate Relative Strength Index (RSI)
pub fn calculate_rsi(price_history: &[f64], period: usize) -> f64 {
    if price_history.len() < period + 1 {
        return 50.0; // Default to neutral if not enough data
    }
    
    let mut gains = 0.0;
    let mut losses = 0.0;
    
    // Calculate price changes and separate into gains and losses
    for i in 1..=period {
        let index = price_history.len() - i;
        let prev_index = price_history.len() - i - 1;
        
        let change = price_history[index] - price_history[prev_index];
        
        if change > 0.0 {
            gains += change;
        } else {
            losses -= change; // Make losses positive
        }
    }
    
    // Calculate average gains and losses
    let avg_gain = gains / period as f64;
    let avg_loss = losses / period as f64;
    
    // Calculate RSI
    if avg_loss < 0.000001 { // Avoid division by zero
        return 100.0;
    }
    
    let rs = avg_gain / avg_loss;
    let rsi = 100.0 - (100.0 / (1.0 + rs));
    
    rsi
}

/// Detect price trend (bullish, bearish, strength)
pub fn detect_price_trend(price_history: &[f64], period: usize) -> (bool, bool, f64) {
    if price_history.len() < period {
        return (false, false, 0.0); // Not enough data
    }
    
    let recent_prices = &price_history[price_history.len() - period..];
    
    // Simple trend detection: compare first and last price
    let first_price = recent_prices[0];
    let last_price = recent_prices[recent_prices.len() - 1];
    
    let trend_change = (last_price - first_price) / first_price;
    let is_bullish = trend_change > 0.0;
    let is_bearish = trend_change < 0.0;
    let strength = trend_change.abs() * 100.0; // Percentage change as strength
    
    (is_bullish, is_bearish, strength)
}

/// Check if price deviation exceeds threshold
pub fn is_price_deviation_too_large(
    expected_price: f64,
    actual_price: f64,
    max_deviation_percent: f64,
) -> bool {
    if expected_price <= 0.0 {
        return false;
    }
    
    let deviation = ((actual_price - expected_price) / expected_price).abs() * 100.0;
    deviation > max_deviation_percent
}

/// Calculate slippage between expected and actual prices
pub fn calculate_slippage(
    expected_price: f64,
    actual_price: f64,
) -> f64 {
    if expected_price <= 0.0 {
        return 0.0;
    }
    
    ((actual_price - expected_price) / expected_price).abs() * 100.0
}

/// Calculate average of a list of prices
pub fn calculate_average_price(prices: &[f64]) -> f64 {
    if prices.is_empty() {
        return 0.0;
    }
    
    let sum: f64 = prices.iter().sum();
    sum / prices.len() as f64
}

/// Calculate standard deviation of a set of values
pub fn calculate_standard_deviation(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    
    let mean = calculate_average_price(values);
    let variance: f64 = values.iter()
        .map(|x| {
            let diff = mean - *x;
            diff * diff
        })
        .sum::<f64>() / values.len() as f64;
    
    variance.sqrt()
}

/// Calculate risk score based on token issues
pub fn calculate_risk_score(issues: &[TokenIssue]) -> f64 {
    if issues.is_empty() {
        return 0.0;
    }
    
    // Define weights for different issue types
    let weights = HashMap::from([
        (TokenIssueType::Critical, 1.0),
        (TokenIssueType::High, 0.7),
        (TokenIssueType::Medium, 0.4),
        (TokenIssueType::Low, 0.2),
        (TokenIssueType::Info, 0.1),
    ]);
    
    let mut total_weight = 0.0;
    let mut weighted_sum = 0.0;
    
    for issue in issues {
        if let Some(weight) = weights.get(&issue.severity) {
            weighted_sum += *weight;
            total_weight += 1.0;
        }
    }
    
    if total_weight == 0.0 {
        return 0.0;
    }
    
    // Scale to 0-100
    (weighted_sum / total_weight) * 100.0
}

//=============================================================================
// TOKEN AND CONTRACT ANALYSIS UTILITIES 
//=============================================================================

/// Check if a contract is likely a proxy
pub fn is_proxy_contract(contract_info: &ContractInfo) -> bool {
    let is_proxy_bytecode = if let Some(bytecode) = &contract_info.bytecode {
        bytecode.contains("f4") && 
        bytecode.contains("delegatecall") &&
        bytecode.len() < 5000
    } else {
        false
    };
    
    let is_proxy_source = if let Some(source) = &contract_info.source_code {
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

/// Check if token contract has blacklist or whitelist functionality
pub fn check_token_lists(contract_info: &ContractInfo) -> (bool, bool) {
    let mut has_blacklist = false;
    let mut has_whitelist = false;
    
    if let Some(source) = &contract_info.source_code {
        has_blacklist = source.contains("blacklist") || 
                        source.contains("blocklist") ||
                        source.contains("banned") ||
                        source.contains("denylist");
        
        has_whitelist = source.contains("whitelist") ||
                        source.contains("allowlist");
    }
    
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

/// Check if token contract has blacklist function
pub fn has_blacklist_function(contract_info: &ContractInfo) -> bool {
    let (has_blacklist, _) = check_token_lists(contract_info);
    has_blacklist
}

//=============================================================================
// BLOCKCHAIN INTERACTION UTILITIES
//=============================================================================

/// Wait for transaction confirmation
pub async fn wait_for_transaction(tx_hash: &str, adapter: &Arc<EvmAdapter>, timeout_seconds: u64) -> Result<bool> {
    let start_time = Instant::now();
    let timeout = Duration::from_secs(timeout_seconds);
    
    loop {
        if start_time.elapsed() > timeout {
            return Err(anyhow!("Transaction confirmation timeout after {} seconds", timeout_seconds));
        }
        
        match is_transaction_confirmed(tx_hash, adapter).await {
            Ok(true) => return Ok(true),
            Ok(false) => {
                // Wait before checking again
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            },
            Err(e) => {
                // If error is temporary, continue waiting
                if e.to_string().contains("not found") {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
                return Err(e);
            }
        }
    }
}

/// Check if transaction is confirmed
pub async fn is_transaction_confirmed(tx_hash: &str, adapter: &Arc<EvmAdapter>) -> Result<bool> {
    let tx_status = adapter.get_transaction_status(tx_hash).await?;
    Ok(tx_status.is_confirmed)
}

/// Get gas price from adapter
pub async fn get_gas_price_from_ref(adapter: &Arc<EvmAdapter>) -> Result<u64> {
    adapter.get_gas_price().await
}

/// Calculate optimal gas price based on priority
pub async fn calculate_gas_price(
    adapter: &Arc<EvmAdapter>,
    priority: &str,
) -> Result<u64> {
    let base_gas_price = adapter.get_gas_price().await?;
    
    let gas_price = match priority {
        "high" => base_gas_price * 15 / 10,  // 1.5x
        "medium" => base_gas_price,          // 1.0x
        "low" => base_gas_price * 8 / 10,    // 0.8x
        _ => base_gas_price,
    };
    
    Ok(gas_price)
}

/// Check if address is valid
pub fn is_valid_address(address: &str) -> bool {
    if !address.starts_with("0x") {
        return false;
    }
    
    let address = address.strip_prefix("0x").unwrap_or(address);
    
    if address.len() != 40 {
        return false;
    }
    
    address.chars().all(|c| c.is_ascii_hexdigit())
}

//=============================================================================
// RATE LIMITING AND API UTILITIES
//=============================================================================

/// Rate limiter for API calls
pub struct RateLimiter {
    /// Time between API calls (ms)
    interval_ms: u64,
    
    /// Last call time for each endpoint
    last_call_time: HashMap<String, Instant>,
    
    /// Rate limit (max calls in time window)
    rate_limit: Option<(u32, Duration)>,
    
    /// Call count in current time window
    call_count: HashMap<String, Vec<Instant>>,
}

impl RateLimiter {
    /// Create new rate limiter with basic interval
    pub fn new(interval_ms: u64) -> Self {
        Self {
            interval_ms,
            last_call_time: HashMap::new(),
            rate_limit: None,
            call_count: HashMap::new(),
        }
    }
    
    /// Create rate limiter with call limit in time window
    pub fn with_rate_limit(interval_ms: u64, max_calls: u32, window: Duration) -> Self {
        Self {
            interval_ms,
            last_call_time: HashMap::new(),
            rate_limit: Some((max_calls, window)),
            call_count: HashMap::new(),
        }
    }
    
    /// Wait if needed to comply with rate limiting
    pub async fn wait_if_needed(&mut self, endpoint: &str) {
        // Basic interval limiting
        if let Some(last_call) = self.last_call_time.get(endpoint) {
            let elapsed = last_call.elapsed();
            let interval = Duration::from_millis(self.interval_ms);
            
            if elapsed < interval {
                let wait_time = interval - elapsed;
                tokio::time::sleep(wait_time).await;
            }
        }
        
        // Advanced rate limiting (e.g., X calls per Y seconds)
        if let Some((max_calls, window)) = self.rate_limit {
            let now = Instant::now();
            
            // Get or create call history for this endpoint
            let calls = self.call_count.entry(endpoint.to_string()).or_insert_with(Vec::new);
            
            // Remove calls outside the window
            calls.retain(|time| time.elapsed() < window);
            
            // If at limit, wait until oldest call is outside window
            if calls.len() >= max_calls as usize && !calls.is_empty() {
                let oldest_call = calls[0];
                let time_to_wait = window - oldest_call.elapsed();
                
                if time_to_wait > Duration::from_millis(0) {
                    tokio::time::sleep(time_to_wait).await;
                }
                
                // Re-clean the array after waiting
                calls.retain(|time| time.elapsed() < window);
            }
            
            // Add this call to history
            calls.push(now);
        }
        
        // Update last call time
        self.last_call_time.insert(endpoint.to_string(), Instant::now());
    }
}

/// Call API with rate limiting
pub async fn call_api_with_rate_limit<F, Fut, T, E>(
    api_type: &str,
    endpoint: &str,
    operation: F,
) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, E>>,
    E: std::fmt::Display,
    anyhow::Error: From<E>,
{
    // Get global rate limiter (should be implemented elsewhere)
    // For now we create a new one each time
    let mut limiter = RateLimiter::new(500); // 500ms default interval
    
    // Wait for rate limiting
    limiter.wait_if_needed(endpoint).await;
    
    // Make the API call
    operation().await.map_err(|e| anyhow!("API error: {}", e))
}

/// Retry API call with exponential backoff
pub async fn retry_api_call<F, Fut, T, E>(
    operation: F,
    max_retries: usize,
    initial_delay_ms: u64,
    max_delay_ms: u64,
    api_type: &str,
    endpoint: &str,
) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, E>>,
    E: std::fmt::Display,
    anyhow::Error: From<E>,
{
    let mut current_retry = 0;
    let mut delay = initial_delay_ms;
    
    loop {
        // Call API with rate limiting
        match call_api_with_rate_limit(api_type, endpoint, &operation).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if current_retry >= max_retries {
                    return Err(anyhow!("Max retries ({}) reached: {}", max_retries, e));
                }
                
                // Some errors should not be retried
                if e.to_string().contains("not found") || 
                   e.to_string().contains("unauthorized") {
                    return Err(e);
                }
                
                warn!("API call failed (retry {}/{}): {}", current_retry + 1, max_retries, e);
                
                // Exponential backoff with jitter
                let jitter = thread_rng().gen_range(0..=delay / 4);
                let sleep_time = delay + jitter;
                
                tokio::time::sleep(Duration::from_millis(sleep_time)).await;
                
                // Update for next iteration
                current_retry += 1;
                delay = (delay * 2).min(max_delay_ms);
            }
        }
    }
}

/// Encode parameters for API calls
pub fn encode_api_parameters(params: &HashMap<String, String>) -> String {
    let mut parts = Vec::with_capacity(params.len());
    
    for (key, value) in params {
        parts.push(format!(
            "{}={}",
            urlencoding::encode(key),
            urlencoding::encode(value)
        ));
    }
    
    parts.join("&")
} 