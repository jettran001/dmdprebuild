//! Mempool transaction pattern detection
//!
//! This module contains functions for detecting various transaction patterns
//! in the mempool, such as front-running, sandwich attacks, and suspicious activities.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::types::*;

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
    // Check if gas price is significantly higher than average
    // This requires access to the average gas price, which we don't have here
    // So we'll compare against other pending transactions
    
    // Get all transactions in the last time_window_ms
    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(e) => {
            tracing::warn!("Failed to get current time in detect_front_running: {}", e);
            // Fallback to transaction detected_at as current time if available
            // or use a reasonable default
            transaction.detected_at.saturating_add(1)
        }
    };
    
    let time_window_sec = time_window_ms / 1000;
    let window_start = now.saturating_sub(time_window_sec);
    
    // Check if there are any large value transactions in the time window
    let large_txs: Vec<&MempoolTransaction> = pending_txs.values()
        .filter(|tx| {
            tx.detected_at >= window_start && 
            tx.value >= LARGE_TRANSACTION_ETH &&
            tx.tx_hash != transaction.tx_hash &&
            tx.from_address != transaction.from_address
        })
        .collect();
    
    if large_txs.is_empty() {
        return None;
    }
    
    // Check if this transaction has significantly higher gas price
    for large_tx in large_txs {
        if transaction.gas_price > large_tx.gas_price * 1.5 {
            // This transaction has much higher gas price
            if transaction.transaction_type == large_tx.transaction_type {
                // And targets the same token or contract
                if let Some(to_addr) = &transaction.to_address {
                    if let Some(large_to) = &large_tx.to_address {
                        if to_addr == large_to {
                            return Some(SuspiciousPattern::FrontRunning);
                        }
                    }
                }
            }
        }
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
/// * `Option<SuspiciousPattern>` - SuspiciousPattern::SandwichAttack if detected
pub async fn detect_sandwich_attack(
    transaction: &MempoolTransaction,
    pending_txs: &HashMap<String, MempoolTransaction>,
    time_window_ms: u64
) -> Option<SuspiciousPattern> {
    // Only check swap transactions
    if transaction.transaction_type != TransactionType::Swap {
        return None;
    }
    
    let time_window_sec = time_window_ms / 1000;
    
    // Find potential front-running transaction
    let front_txs: Vec<&MempoolTransaction> = pending_txs.values()
        .filter(|tx| {
            // Different sender
            tx.from_address != transaction.from_address &&
            // Swap transaction
            tx.transaction_type == TransactionType::Swap &&
            // Higher gas price
            tx.gas_price > transaction.gas_price &&
            // Came right before target transaction
            tx.detected_at < transaction.detected_at &&
            tx.detected_at >= transaction.detected_at.saturating_sub(time_window_sec)
        })
        .collect();
    
    if front_txs.is_empty() {
        return None;
    }
    
    // Find potential back-running transaction
    let back_txs: Vec<&MempoolTransaction> = pending_txs.values()
        .filter(|tx| {
            // Check if it's from the same address as any front-runner
            front_txs.iter().any(|ft| ft.from_address == tx.from_address) &&
            // Swap transaction
            tx.transaction_type == TransactionType::Swap &&
            // Came right after target transaction
            tx.detected_at > transaction.detected_at &&
            tx.detected_at <= transaction.detected_at + time_window_sec
        })
        .collect();
    
    if back_txs.is_empty() {
        return None;
    }
    
    // Check if there's a sandwich pattern with token info
    for front_tx in &front_txs {
        for back_tx in &back_txs {
            // If we have token info, check that it's the same token being targeted
            if let (Some(victim_from), Some(front_to)) = (
                &transaction.from_token,
                &front_tx.to_token
            ) {
                // Safe access with match instead of unwrap
                let front_target_matches = match (&front_tx.to_token, &transaction.from_token) {
                    (Some(front_token), Some(victim_token)) => {
                        front_token.address == victim_token.address
                    },
                    _ => false
                };
                
                let back_source_matches = match (&back_tx.from_token, &transaction.to_token) {
                    (Some(back_token), Some(victim_token)) => {
                        back_token.address == victim_token.address
                    },
                    _ => false
                };
                
                if front_target_matches && back_source_matches {
                    return Some(SuspiciousPattern::SandwichAttack);
                }
            }
            // If no token info, check for contract interaction on the same address
            else if front_tx.from_address == back_tx.from_address {
                if let (Some(front_to), Some(victim_to), Some(back_to)) =
                    (&front_tx.to_address, &transaction.to_address, &back_tx.to_address) {
                    if front_to == victim_to && victim_to == back_to {
                        return Some(SuspiciousPattern::SandwichAttack);
                    }
                }
            }
        }
    }
    
    None
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
    
    // Get all transactions from the same sender in the last TIME_WINDOW_SEC
    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(e) => {
            tracing::warn!("Failed to get current time in detect_high_frequency_trading: {}", e);
            // Fallback to transaction detected_at as current time if available
            transaction.detected_at
        }
    };
    
    let window_start = now.saturating_sub(TIME_WINDOW_SEC);
    
    let sender_txs: Vec<&MempoolTransaction> = pending_txs.values()
        .filter(|tx| {
            tx.detected_at >= window_start && 
            tx.from_address == transaction.from_address
        })
        .collect();
    
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