//! Mempool transaction pattern detection
//!
//! This module contains functions for detecting various transaction patterns
//! in the mempool, such as front-running, sandwich attacks, and suspicious activities.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::Result;
use once_cell::sync::Lazy;
use std::cell::RefCell;
use std::ops::DerefMut;
use tracing::warn;

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
            warn!("Failed to get current time: {}", e);
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
///
/// # Returns
/// * `Result<Option<SuspiciousPattern>, anyhow::Error>` - SuspiciousPattern::FrontRunning if detected
pub async fn detect_front_running(
    transaction: &MempoolTransaction,
    pending_txs: &HashMap<String, MempoolTransaction>
) -> Result<Option<SuspiciousPattern>> {
    // Only analyze certain transaction types that might be front-running
    if !matches!(transaction.transaction_type, 
        TransactionType::Swap | 
        TransactionType::AddLiquidity | 
        TransactionType::TokenTransfer) {
        return Ok(None);
    }
    
    // Look for potential target transactions with similar patterns but lower gas price
    let now = transaction.detected_at;
    let window_start = now.saturating_sub(FRONT_RUNNING_TIME_WINDOW_MS);
    
    // Find potential target transactions in the time window
    let potential_targets: Vec<&MempoolTransaction> = pending_txs.values()
        .filter(|tx| {
            // Must be in the time window
            tx.detected_at >= window_start && 
            tx.detected_at <= now &&
            // Different from our transaction
            tx.tx_hash != transaction.tx_hash &&
            // Lower gas price (potential victim)
            tx.gas_price < transaction.gas_price &&
            // Similar transaction type
            matches!(tx.transaction_type, 
                TransactionType::Swap | 
                TransactionType::TokenTransfer |
                TransactionType::AddLiquidity)
        })
        .collect();
    
    if potential_targets.is_empty() {
        return Ok(None);
    }
    
    // Score potential targets by similarity and likelihood of being front-run
    let mut target_scores: Vec<(&MempoolTransaction, f64)> = Vec::new();
    let mut highest_confidence = 0.5; // Start with base confidence
    let mut target_tx = None;
    
    for target in potential_targets {
        // Reset for this target
        let mut target_confidence = 0.5;
        
        // Higher confidence if gas price is much higher than target
        if transaction.gas_price > target.gas_price * 1.5 {
            target_confidence += 0.15;
        }
        
        // Higher confidence if targeting same contract
        if transaction.to_address == target.to_address {
            target_confidence += 0.15;
        }
        
        // Higher confidence if using same/similar token
        if let (Some(t1), Some(t2)) = (&transaction.to_token, &target.to_token) {
            if t1.address == t2.address {
                target_confidence += 0.2;
            }
        }
        
        // Higher confidence if similar function signature
        if let (Some(f1), Some(f2)) = (&transaction.function_signature, &target.function_signature) {
            if f1 == f2 {
                target_confidence += 0.2;
            } else if f1.starts_with(&f2[..4]) || f2.starts_with(&f1[..4]) {
                target_confidence += 0.1;
            }
        }
        
        // Store the score
        target_scores.push((target, target_confidence));
        
        // Update highest confidence
        if target_confidence > highest_confidence {
            highest_confidence = target_confidence;
            target_tx = Some(target);
        }
    }
    
    // If we have a high confidence target, consider it front-running
    if highest_confidence >= 0.7 {
        // Return front-running pattern with details
        let detail = format!("Front-running detected with {:.1}% confidence", highest_confidence * 100.0);
        warn!("{}", detail);
        
        // Tạo một basic pattern vì struct SuspiciousPattern::FrontRunning không có các fields 
        let mut pattern = SuspiciousPattern::FrontRunning;
        
        // Các metadata có thể thêm vào để client có thể sử dụng
        pattern.add_metadata("confidence", &format!("{:.2}", highest_confidence));
        pattern.add_metadata("detail", &detail);
        pattern.add_metadata("front_runner_tx", &transaction.tx_hash);
        
        if let Some(tx) = target_tx {
            pattern.add_metadata("victim_tx", &tx.tx_hash);
        }
        
        return Ok(Some(pattern));
    }
    
    Ok(None)
}

/// Detect sandwich attack pattern
///
/// Sandwich attack occurs when an attacker places two transactions around a victim's transaction,
/// buying before and selling after to profit from the price impact.
///
/// # Arguments
/// * `transaction` - The transaction to check
/// * `pending_txs` - Map of pending transactions
///
/// # Returns
/// * `Result<Option<SuspiciousPattern>, anyhow::Error>` - SuspiciousPattern::SandwichAttack if detected with confidence score
pub async fn detect_sandwich_attack(
    transaction: &MempoolTransaction,
    pending_txs: &HashMap<String, MempoolTransaction>
) -> Result<Option<SuspiciousPattern>> {
    // Only check swap transactions with sufficient value
    if transaction.transaction_type != TransactionType::Swap {
        return Ok(None);
    }
    
    // Ignore very small transactions - less likely to be sandwich targeted
    if transaction.value < 0.01 {
        return Ok(None);
    }
    
    let time_window_ms = MAX_SANDWICH_TIME_WINDOW_MS;
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
        return Ok(None);
    }
    
    warn!("Found {} potential front-running transactions for possible sandwich attack", front_txs.len());
    
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
        return Ok(None);
    }
    
    warn!("Found {} potential back-running transactions for possible sandwich attack", back_txs.len());
    
    // Enhanced detection with token, value, and path analysis
    let mut confidence_score = 0.0;
    let mut sandwich_details: Option<(String, f64)> = None;
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
                    sandwich_details = Some((format!("Token matching: front buys victim's token, back sells it"), confidence_score));
                }
                
                // Check for router signature
                if optional_address_matches(&front_tx.to_address, &transaction.to_address) && 
                   optional_address_matches(&transaction.to_address, &back_tx.to_address) {
                    confidence_score += 0.2;
                }
            }
            // If no token info, rely on router contract interaction
            else if address_matches(&front_tx.from_address, &back_tx.from_address) &&
                   optional_address_matches(&front_tx.to_address, &back_tx.to_address) {
                confidence_score += 0.15;
                should_update = true;
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
            if let (Some(front_sig), Some(tx_sig), Some(back_sig)) = 
               (&front_tx.function_signature, &transaction.function_signature, &back_tx.function_signature) {
                // Common swap methods in DEXes like Uniswap, Sushiswap, etc.
                let swap_methods = ["swapExactTokensForTokens", "swapTokensForExactTokens", 
                                   "swapExactETHForTokens", "swapTokensForExactETH"];
                                   
                // If all three use swap methods, increase confidence
                if swap_methods.iter().any(|&m| front_sig.contains(m)) && 
                   swap_methods.iter().any(|&m| tx_sig.contains(m)) && 
                   swap_methods.iter().any(|&m| back_sig.contains(m)) {
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
                let should_update = match sandwich_details {
                    None => true,
                    Some((_, existing_confidence)) => confidence_score > existing_confidence
                };
                
                if should_update {
                    let detail = format!(
                        "Sandwich attack detected with {:.1}% confidence. Front tx: {}, victim tx: {}, back tx: {}",
                        confidence_score * 100.0,
                        front_tx.tx_hash,
                        transaction.tx_hash,
                        back_tx.tx_hash
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
        warn!("{}", detail);
        
        // Add additional details to pattern
        let mut pattern = SuspiciousPattern::SandwichAttack;
        pattern.set_confidence(confidence);
        pattern.set_detail(detail);
        
        // NEW: Add metadata with front and back tx hashes for further analysis
        if let (Some(front_tx), Some(back_tx)) = (most_likely_front_tx, most_likely_back_tx) {
            pattern.add_metadata("front_tx_hash", &front_tx.tx_hash);
            pattern.add_metadata("back_tx_hash", &back_tx.tx_hash);
            
            // Add gas prices for comparison
            pattern.add_metadata("front_tx_gas_price", &front_tx.gas_price.to_string());
            pattern.add_metadata("victim_tx_gas_price", &transaction.gas_price.to_string());
            pattern.add_metadata("back_tx_gas_price", &back_tx.gas_price.to_string());
            
            // Add timestamps for timing analysis
            pattern.add_metadata("front_tx_time", &front_tx.detected_at.to_string());
            pattern.add_metadata("victim_tx_time", &transaction.detected_at.to_string());
            pattern.add_metadata("back_tx_time", &back_tx.detected_at.to_string());
        }
        
        return Ok(Some(pattern));
    }
    
    Ok(None)
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
    
    // Return HighFrequencyTrading if threshold is met
    if sender_txs.len() >= HIGH_FREQUENCY_THRESHOLD {
        let mut pattern = SuspiciousPattern::HighFrequencyTrading;
        
        // Calculate time-based metrics
        let first_tx_time = sender_txs.iter()
            .map(|tx| tx.detected_at)
            .min()
            .unwrap_or(now);
            
        let last_tx_time = sender_txs.iter()
            .map(|tx| tx.detected_at)
            .max()
            .unwrap_or(now);
            
        let time_span = last_tx_time.saturating_sub(first_tx_time);
        let avg_time_between_txs = if sender_txs.len() > 1 {
            time_span as f64 / (sender_txs.len() - 1) as f64
        } else {
            0.0
        };
        
        // Add metadata for further analysis
        pattern.add_metadata("from_address", &transaction.from_address);
        pattern.add_metadata("tx_count", &sender_txs.len().to_string());
        pattern.add_metadata("time_window_sec", &TIME_WINDOW_SEC.to_string());
        pattern.add_metadata("avg_time_between_txs", &format!("{:.2}", avg_time_between_txs));
        pattern.add_metadata("first_tx_time", &first_tx_time.to_string());
        pattern.add_metadata("last_tx_time", &last_tx_time.to_string());
        pattern.add_metadata("total_tx_count", &sender_txs.len().to_string());
        
        Some(pattern)
    } else {
        None
    }
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
                warn!("Failed to get current time in detect_sudden_liquidity_removal: {}", e);
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
/// * `