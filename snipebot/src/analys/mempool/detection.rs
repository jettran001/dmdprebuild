//! Mempool transaction pattern detection
//!
//! This module contains functions for detecting various transaction patterns
//! in the mempool, such as front-running, sandwich attacks, and suspicious activities.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::types::*;

/// Get current timestamp in seconds since UNIX epoch
/// 
/// This helper function safely gets the current time or returns a fallback value
/// 
/// # Arguments
/// * `fallback` - Optional fallback timestamp if current time can't be determined
/// 
/// # Returns
/// * Current timestamp in seconds
fn get_current_timestamp(fallback: Option<u64>) -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(e) => {
            tracing::warn!("Failed to get current time: {}", e);
            // Use fallback value or 0
            fallback.unwrap_or(0)
        }
    }
}

/// Filter transactions within a time window
/// 
/// # Arguments
/// * `transactions` - Map of transactions to filter
/// * `window_start` - Start time of the window (seconds since UNIX epoch)
/// * `additional_filter` - Optional additional filter function
/// 
/// # Returns
/// * Vector of transactions within the time window that match the filter
fn filter_transactions_in_window<F>(
    transactions: &HashMap<String, MempoolTransaction>,
    window_start: u64,
    additional_filter: Option<F>
) -> Vec<&MempoolTransaction>
where
    F: Fn(&&MempoolTransaction) -> bool,
{
    match additional_filter {
        Some(filter) => transactions.values()
            .filter(|tx| tx.detected_at >= window_start && filter(tx))
            .collect(),
        None => transactions.values()
            .filter(|tx| tx.detected_at >= window_start)
            .collect()
    }
}

/// Compare addresses ignoring case
/// 
/// # Arguments
/// * `addr1` - First address to compare
/// * `addr2` - Second address to compare
/// 
/// # Returns
/// * true if addresses match (case insensitive), false otherwise
fn address_matches(addr1: &str, addr2: &str) -> bool {
    addr1.to_lowercase() == addr2.to_lowercase()
}

/// Check if addresses match ignoring case, handling Option values
/// 
/// # Arguments
/// * `addr1` - First optional address
/// * `addr2` - Second optional address
/// 
/// # Returns
/// * true if both are Some and match (case insensitive), false otherwise
fn optional_address_matches(addr1: &Option<String>, addr2: &Option<String>) -> bool {
    match (addr1, addr2) {
        (Some(a1), Some(a2)) => address_matches(a1, a2),
        _ => false
    }
}

/// Check if token addresses match, handling Option values
/// 
/// # Arguments
/// * `token1` - First optional token info
/// * `token2` - Second optional token info
/// 
/// # Returns
/// * true if both are Some and addresses match, false otherwise
fn token_address_matches(token1: &Option<TokenInfo>, token2: &Option<TokenInfo>) -> bool {
    match (token1, token2) {
        (Some(t1), Some(t2)) => address_matches(&t1.address, &t2.address),
        _ => false
    }
}

/// Detect front-running pattern
///
/// Front-running occurs when a transaction is submitted with a higher gas price
/// to be executed before another pending transaction.
///
/// # Arguments
/// * `transaction` - The transaction to check
/// * `pending_txs` - Map of pending transactions
/// * `time_window_ms` - Time window to consider (milliseconds)
///
/// # Returns
/// * `Option<SuspiciousPattern>` - SuspiciousPattern::FrontRunning if detected
pub async fn detect_front_running(
    transaction: &MempoolTransaction,
    pending_txs: &HashMap<String, MempoolTransaction>,
    time_window_ms: u64
) -> Option<SuspiciousPattern> {
    use tracing::{debug, info};
    
    // Get current timestamp
    let now = get_current_timestamp(Some(transaction.detected_at.saturating_add(1)));
    let time_window_sec = time_window_ms / 1000;
    let window_start = now.saturating_sub(time_window_sec);
    
    // Filter for large transactions in the time window
    let large_txs = filter_transactions_in_window(pending_txs, window_start, Some(|tx: &&MempoolTransaction| {
        tx.value >= LARGE_TRANSACTION_ETH && 
        tx.tx_hash != transaction.tx_hash &&
        !address_matches(&tx.from_address, &transaction.from_address)
    }));
    
    if large_txs.is_empty() {
        return None;
    }
    
    // Initialize confidence score and related transaction
    let mut highest_confidence = 0.0;
    let mut front_run_detail = None;
    let mut target_tx = None;
    
    // Check if this transaction has significantly higher gas price
    for large_tx in large_txs {
        let mut confidence = 0.0;
        
        // Basic check - higher gas price
        if transaction.gas_price > large_tx.gas_price * 1.5 {
            confidence = 0.5; // Base confidence
            
            // Check for transaction type match
            if transaction.transaction_type == large_tx.transaction_type {
                confidence += 0.1;
                
                // Target contract match
                if optional_address_matches(&transaction.to_address, &large_tx.to_address) {
                    confidence += 0.2;
                    
                    // Timing is very close
                    let time_difference = transaction.detected_at.saturating_sub(large_tx.detected_at);
                    if time_difference < 3 {
                        confidence += 0.1;
                    }
                    
                    // Token match (stronger evidence)
                    if token_address_matches(&transaction.from_token, &large_tx.from_token) || 
                       token_address_matches(&transaction.to_token, &large_tx.to_token) {
                        confidence += 0.2;
                        debug!("Found token match evidence for front-running");
                    }
                    
                    // Method signature match
                    if let (Some(tx_method), Some(large_tx_method)) = 
                       (&transaction.method_signature, &large_tx.method_signature) {
                        if tx_method == large_tx_method {
                            confidence += 0.1;
                        }
                    }
                    
                    // Check if sender is a known MEV bot
                    if is_known_mev_bot(&transaction.from_address) {
                        confidence += 0.2;
                    }
                    
                    // Store highest confidence match
                    if confidence > highest_confidence {
                        highest_confidence = confidence;
                        
                        // Format detailed message
                        let detail = format!(
                            "Front-running detected with {:.1}% confidence. Front-runner tx: {}, victim tx: {}. Gas price: {:.2} vs {:.2} gwei",
                            confidence * 100.0,
                            transaction.tx_hash,
                            large_tx.tx_hash,
                            transaction.gas_price,
                            large_tx.gas_price
                        );
                        
                        front_run_detail = Some(detail);
                        target_tx = Some(large_tx);
                    }
                }
            }
        }
    }
    
    // Return result if confidence threshold is met
    if highest_confidence >= 0.7 && front_run_detail.is_some() {
        let detail = front_run_detail.unwrap();
        info!("{}", detail);
        
        // Create pattern with metadata
        let mut pattern = SuspiciousPattern::FrontRunning;
        pattern.set_confidence(highest_confidence);
        pattern.set_detail(detail);
        
        // Add additional metadata if target transaction is available
        if let Some(victim_tx) = target_tx {
            pattern.add_metadata("front_runner_tx_hash", &transaction.tx_hash);
            pattern.add_metadata("victim_tx_hash", &victim_tx.tx_hash);
            pattern.add_metadata("front_runner_gas_price", &transaction.gas_price.to_string());
            pattern.add_metadata("victim_gas_price", &victim_tx.gas_price.to_string());
            pattern.add_metadata("front_runner_address", &transaction.from_address);
            pattern.add_metadata("victim_address", &victim_tx.from_address);
            pattern.add_metadata("time_gap", &transaction.detected_at.saturating_sub(victim_tx.detected_at).to_string());
        }
        
        return Some(pattern);
    }
    
    None
}

/// Detect sandwich attack pattern
///
/// Sandwich attack occurs when an attacker places two transactions around a victim's transaction,
/// buying before and selling after to profit from the price impact.
///
/// # Arguments
/// * `transaction` - The transaction to check
/// * `pending_txs` - Map of pending transactions
/// * `time_window_ms` - Time window to consider (milliseconds)
///
/// # Returns
/// * `Option<SuspiciousPattern>` - SuspiciousPattern::SandwichAttack if detected with confidence score
pub async fn detect_sandwich_attack(
    transaction: &MempoolTransaction,
    pending_txs: &HashMap<String, MempoolTransaction>,
    time_window_ms: u64
) -> Option<SuspiciousPattern> {
    use tracing::{debug, info, warn};
    
    // Only check swap transactions with sufficient value
    if transaction.transaction_type != TransactionType::Swap {
        return None;
    }
    
    // Ignore very small transactions - less likely to be sandwich targeted
    if transaction.value < 0.01 {
        return None;
    }
    
    let time_window_sec = time_window_ms / 1000;
    
    // Find potential front-running transaction (buy side of sandwich)
    let front_txs = filter_transactions_in_window(pending_txs, 
        transaction.detected_at.saturating_sub(time_window_sec), 
        Some(|tx: &&MempoolTransaction| {
            // Must be a different sender (attacker)
            !address_matches(&tx.from_address, &transaction.from_address) &&
            // Must be a swap transaction (buying tokens)
            tx.transaction_type == TransactionType::Swap &&
            // Must have a higher gas price to get in before victim
            tx.gas_price > transaction.gas_price &&
            // Must have been detected before our transaction
            tx.detected_at < transaction.detected_at &&
            // Must be within the time window
            tx.detected_at >= transaction.detected_at.saturating_sub(time_window_sec)
        })
    );
    
    if front_txs.is_empty() {
        return None;
    }
    
    debug!("Found {} potential front-running transactions for possible sandwich attack", front_txs.len());
    
    // Find potential back-running transaction (sell side of sandwich)
    let back_txs = filter_transactions_in_window(pending_txs,
        transaction.detected_at,
        Some(|tx: &&MempoolTransaction| {
            // From the same address as any front-runner
            front_txs.iter().any(|ft| address_matches(&ft.from_address, &tx.from_address)) &&
            // Must be a swap transaction (selling tokens)
            tx.transaction_type == TransactionType::Swap &&
            // Must come after our transaction
            tx.detected_at > transaction.detected_at &&
            // Must be within the time window
            tx.detected_at <= transaction.detected_at + time_window_sec
        })
    );
    
    if back_txs.is_empty() {
        return None;
    }
    
    debug!("Found {} potential back-running transactions for possible sandwich attack", back_txs.len());
    
    // Enhanced detection with token, value, and path analysis
    let mut confidence_score = 0.0;
    let mut sandwich_details = None;
    let mut most_likely_front_tx = None;
    let mut most_likely_back_tx = None;
    
    // Check if there's a sandwich pattern with token info
    for front_tx in &front_txs {
        for back_tx in &back_txs {
            // Reset confidence for this pair
            confidence_score = 0.5; // Start with base confidence
            
            // Calculate time proximity score
            let time_gap_front = transaction.detected_at.saturating_sub(front_tx.detected_at);
            let time_gap_back = back_tx.detected_at.saturating_sub(transaction.detected_at);
            
            // Higher confidence for transactions close in time
            if time_gap_front < 3 && time_gap_back < 3 {
                confidence_score += 0.2; // Very close in time
            } else if time_gap_front < 10 && time_gap_back < 10 {
                confidence_score += 0.1; // Reasonably close
            }
            
            // Check for token path signature
            if transaction.from_token.is_some() && 
               front_tx.to_token.is_some() && 
               back_tx.from_token.is_some() {
                
                // Perfect path match (strongest evidence)
                let front_buys_victims_token = token_address_matches(&front_tx.to_token, &transaction.from_token);
                let victim_buys_target = transaction.to_token.is_some();
                let back_sells_same_token = victim_buys_target && 
                                           token_address_matches(&back_tx.from_token, &transaction.to_token);
                
                if front_buys_victims_token && back_sells_same_token {
                    confidence_score += 0.3;
                    debug!("Found strong token path evidence for sandwich attack");
                }
                
                // Check for router signature
                if optional_address_matches(&front_tx.to_address, &transaction.to_address) && 
                   optional_address_matches(&transaction.to_address, &back_tx.to_address) {
                    confidence_score += 0.1;
                }
            }
            // If no token info, rely on router contract interaction
            else if address_matches(&front_tx.from_address, &back_tx.from_address) {
                if optional_address_matches(&front_tx.to_address, &transaction.to_address) && 
                   optional_address_matches(&transaction.to_address, &back_tx.to_address) {
                    confidence_score += 0.2;
                }
            }
            
            // Check gas price patterns
            if front_tx.gas_price > transaction.gas_price + (transaction.gas_price * 0.2) {
                // Significantly higher gas price (typical for front-running)
                confidence_score += 0.1;
            }
            
            // Check value patterns
            if front_tx.value > 0.0 && back_tx.value > front_tx.value {
                // Back transaction value is higher (profit taking)
                confidence_score += 0.1;
            }
            
            // NEW: Check transaction method signature if available
            if let (Some(front_method), Some(tx_method), Some(back_method)) = 
               (&front_tx.method_signature, &transaction.method_signature, &back_tx.method_signature) {
                // Common swap methods in DEXes like Uniswap, Sushiswap, etc.
                let swap_methods = ["swapExactTokensForTokens", "swapTokensForExactTokens", 
                                   "swapExactETHForTokens", "swapTokensForExactETH"];
                                   
                // If all three use swap methods, increase confidence
                if swap_methods.iter().any(|&m| front_method.contains(m)) && 
                   swap_methods.iter().any(|&m| tx_method.contains(m)) && 
                   swap_methods.iter().any(|&m| back_method.contains(m)) {
                    confidence_score += 0.15;
                }
            }
            
            // NEW: Check if the address is known MEV bot
            if is_known_mev_bot(&front_tx.from_address) {
                confidence_score += 0.2;
            }
            
            // NEW: Reduce confidence if there are too many transactions from the same address
            // (Less likely to be targeted sandwich if sender has many transactions)
            let sender_tx_count = pending_txs.values()
                .filter(|tx| address_matches(&tx.from_address, &transaction.from_address))
                .count();
                
            if sender_tx_count > 10 {
                confidence_score -= 0.1; // Likely a bot or high-frequency trader, not a victim
            }
            
            // If we exceed confidence threshold, consider it a sandwich attack
            if confidence_score >= 0.7 {
                // Store the highest confidence match
                if sandwich_details.is_none() || confidence_score > sandwich_details.unwrap().1 {
                    let detail = format!(
                        "Sandwich attack detected with {:.1}% confidence. Front tx: {}, victim tx: {}, back tx: {}",
                        confidence_score * 100.0,
                        front_tx.hash,
                        transaction.hash,
                        back_tx.hash
                    );
                    sandwich_details = Some((detail, confidence_score));
                    most_likely_front_tx = Some(front_tx);
                    most_likely_back_tx = Some(back_tx);
                }
            }
        }
    }
    
    // Return the highest confidence sandwich attack if found
    if let Some((detail, confidence)) = sandwich_details {
        info!("{}", detail);
        
        // Add additional details to pattern
        let mut pattern = SuspiciousPattern::SandwichAttack;
        pattern.set_confidence(confidence);
        pattern.set_detail(detail);
        
        // NEW: Add metadata with front and back tx hashes for further analysis
        if let (Some(front_tx), Some(back_tx)) = (most_likely_front_tx, most_likely_back_tx) {
            pattern.add_metadata("front_tx_hash", &front_tx.hash);
            pattern.add_metadata("back_tx_hash", &back_tx.hash);
            
            // Add gas prices for comparison
            pattern.add_metadata("front_tx_gas_price", &front_tx.gas_price.to_string());
            pattern.add_metadata("victim_tx_gas_price", &transaction.gas_price.to_string());
            pattern.add_metadata("back_tx_gas_price", &back_tx.gas_price.to_string());
            
            // Add timestamps for timing analysis
            pattern.add_metadata("front_tx_time", &front_tx.detected_at.to_string());
            pattern.add_metadata("victim_tx_time", &transaction.detected_at.to_string());
            pattern.add_metadata("back_tx_time", &back_tx.detected_at.to_string());
        }
        
        return Some(pattern);
    }
    
    None
}

/// Check if address is a known MEV bot
/// 
/// This function checks against a list of known MEV bot addresses
/// 
/// # Arguments
/// * `address` - Address to check
/// 
/// # Returns
/// * `bool` - True if address is a known MEV bot, false otherwise
fn is_known_mev_bot(address: &str) -> bool {
    // List of known MEV bot addresses (can be expanded)
    const KNOWN_MEV_BOTS: &[&str] = &[
        "0x00000000003b3cc22af3ae1eac0440bcee416b40", // MEV Bot
        "0x00000000000000adc04c56bf30ac9d3c0aaf14dc", // Flashbots 
        "0x4f3a120e72c76c22ae802d129f599bfdbc31cb81", // MEV Bot
        "0x0000000000a73d4a0b5a593d9ecc13c527fef298", // MEV Bot
        "0x0000000000000000000000000000000000000000", // Placeholder - remove in production
    ];
    
    let address_lower = address.to_lowercase();
    KNOWN_MEV_BOTS.iter().any(|&bot_addr| address_lower == bot_addr)
}

/// Detect high frequency trading from a single address
///
/// High frequency trading is characterized by a large number of transactions
/// from the same address within a short time window.
///
/// # Arguments
/// * `transaction` - The transaction to check
/// * `pending_txs` - Map of pending transactions
///
/// # Returns
/// * `Option<SuspiciousPattern>` - SuspiciousPattern::HighFrequencyTrading if detected
pub async fn detect_high_frequency_trading(
    transaction: &MempoolTransaction,
    pending_txs: &HashMap<String, MempoolTransaction>
) -> Option<SuspiciousPattern> {
    const HIGH_FREQUENCY_THRESHOLD: usize = 5; // 5+ transactions in the window
    const TIME_WINDOW_SEC: u64 = 60; // 1 minute window
    
    // Get current timestamp
    let now = get_current_timestamp(Some(transaction.detected_at));
    let window_start = now.saturating_sub(TIME_WINDOW_SEC);
    
    // Filter transactions from the same sender in the time window
    let sender_txs = filter_transactions_in_window(pending_txs, window_start,
        Some(|tx: &&MempoolTransaction| {
            address_matches(&tx.from_address, &transaction.from_address)
        })
    );
    
    if sender_txs.len() >= HIGH_FREQUENCY_THRESHOLD {
        return Some(SuspiciousPattern::HighFrequencyTrading);
    }
    
    None
}

/// Detect sudden liquidity removal (potential rug pull)
///
/// Sudden liquidity removal is characterized by a large liquidity removal transaction,
/// especially soon after token creation or a large price increase.
///
/// # Arguments
/// * `transaction` - The transaction to check
/// * `token_info` - Additional token information if available
///
/// # Returns
/// * `Option<SuspiciousPattern>` - SuspiciousPattern::SuddenLiquidityRemoval if detected
pub fn detect_sudden_liquidity_removal(
    transaction: &MempoolTransaction,
    token_info: Option<&TokenInfo>
) -> Option<SuspiciousPattern> {
    // Check if this is a remove liquidity transaction
    if transaction.transaction_type != TransactionType::RemoveLiquidity {
        return None;
    }
    
    // If we have token info and it's a recently created token
    if let Some(token) = token_info {
        // Calculate time since token creation
        let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_secs(),
            Err(e) => {
                tracing::warn!("Failed to get current time in detect_sudden_liquidity_removal: {}", e);
                return None;
            }
        };
        
        let token_age_hours = (now - token.created_at) / 3600;
        
        // If token is less than 24 hours old and a large amount of liquidity is being removed
        if token_age_hours < 24 && transaction.value > LARGE_TRANSACTION_ETH {
            return Some(SuspiciousPattern::SuddenLiquidityRemoval);
        }
        
        // Or if there was a large price increase recently
        if token.price_change_24h > 50.0 && transaction.value > LARGE_TRANSACTION_ETH {
            return Some(SuspiciousPattern::SuddenLiquidityRemoval);
        }
    }
    
    None
}

/// Detect whale wallet movements
///
/// Whale movements are large value transactions from addresses that hold a significant
/// amount of tokens.
///
/// # Arguments
/// * `transaction` - The transaction to check
/// * `whale_threshold_eth` - Threshold in ETH to consider a transaction as a whale movement
///
/// # Returns
/// * `Option<SuspiciousPattern>` - SuspiciousPattern::WhaleMovement if detected
pub fn detect_whale_movement(
    transaction: &MempoolTransaction,
    whale_threshold_eth: f64,
) -> Option<SuspiciousPattern> {
    if transaction.value >= whale_threshold_eth {
        return Some(SuspiciousPattern::WhaleMovement);
    }
    
    None
}

/// Detect token creation and liquidity addition pattern
///
/// This pattern is characterized by token creation followed shortly by
/// liquidity addition, which could indicate a new token launch or a rug pull setup.
///
/// # Arguments
/// * `transactions` - Map of pending transactions
/// * `address` - Address to check
/// * `time_window_sec` - Time window to consider
///
/// # Returns
/// * `Option<SuspiciousPattern>` - SuspiciousPattern::TokenLiquidityPattern if detected
pub async fn detect_token_liquidity_pattern(
    transactions: &HashMap<String, MempoolTransaction>,
    address: &str,
    time_window_sec: u64,
) -> Option<SuspiciousPattern> {
    // Find token creation transaction
    let token_creation: Option<&MempoolTransaction> = transactions.values()
        .find(|tx| {
            tx.transaction_type == TransactionType::TokenCreation &&
            tx.from_address == address
        });
    
    if token_creation.is_none() {
        return None;
    }
    
    let token_creation = token_creation.unwrap();
    
    // Find liquidity addition after token creation
    let liquidity_addition: Option<&MempoolTransaction> = transactions.values()
        .find(|tx| {
            tx.transaction_type == TransactionType::AddLiquidity &&
            tx.from_address == address && 
            tx.detected_at > token_creation.detected_at &&
            tx.detected_at <= token_creation.detected_at + time_window_sec
        });
    
    if liquidity_addition.is_some() {
        return Some(SuspiciousPattern::TokenLiquidityPattern);
    }
    
    None
} 