//! Arbitrage and MEV opportunity detection
//!
//! This module provides functions to detect arbitrage opportunities and
//! MEV (Miner Extractable Value) in mempool transactions.

use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH, Duration};

use crate::analys::mempool::types::{
    MempoolTransaction, TransactionType, SuspiciousPattern
};

/// Phát hiện cơ hội arbitrage từ các giao dịch mempool
///
/// # Parameters
/// - `transactions`: Danh sách giao dịch mempool
/// - `min_profit_threshold`: Ngưỡng lợi nhuận tối thiểu (%)
/// - `min_value_eth`: Giá trị giao dịch tối thiểu (ETH)
/// - `limit`: Số lượng cơ hội tối đa trả về
///
/// # Returns
/// - `Vec<(MempoolTransaction, f64)>`: Danh sách (giao dịch, lợi nhuận ước tính %)
pub async fn detect_arbitrage_opportunities(
    transactions: &HashMap<String, MempoolTransaction>,
    min_profit_threshold: f64,
    min_value_eth: f64,
    limit: usize,
) -> Vec<(MempoolTransaction, f64)> {
    let mut opportunities = Vec::new();
    
    for (_, tx) in transactions {
        // Chỉ xem xét các giao dịch swap và có giá trị đủ lớn
        if tx.value < min_value_eth {
            continue;
        }
        
        // Phát hiện swap từ function signature
        let is_swap = match &tx.function_signature {
            Some(sig) => {
                sig.contains("swap") || sig.contains("Swap") || tx.transaction_type == TransactionType::Swap
            },
            None => false,
        };
        
        if !is_swap {
            continue;
        }
        
        // Phân tích arbitrage path từ input data
        if let Some(profit) = estimate_arbitrage_profit(tx) {
            if profit >= min_profit_threshold {
                opportunities.push((tx.clone(), profit));
            }
        }
    }
    
    // Sắp xếp theo lợi nhuận giảm dần
    opportunities.sort_by(|(_, a), (_, b)| {
        // Sử dụng partial_cmp an toàn với default Equal nếu không so sánh được
        b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal)
    });
    
    // Giới hạn số lượng kết quả
    opportunities.truncate(limit);
    
    opportunities
}

/// Ước tính lợi nhuận arbitrage từ giao dịch
///
/// # Parameters
/// - `transaction`: Giao dịch cần phân tích
///
/// # Returns
/// - `Option<f64>`: Lợi nhuận ước tính (%) nếu có
fn estimate_arbitrage_profit(transaction: &MempoolTransaction) -> Option<f64> {
    // Placeholder - Cần phân tích input data thực tế để tính toán chính xác
    // Đây chỉ là một ước tính đơn giản dựa trên gas price cao
    
    let gas_price_ratio = transaction.gas_price / 5.0; // Giả sử gas trung bình là 5 Gwei
    if gas_price_ratio > 2.0 {
        // Gas cao gấp đôi trung bình thường chỉ có ý nghĩa khi có lợi nhuận tốt
        Some(gas_price_ratio * 0.5)
    } else {
        None
    }
}

/// Find MEV bots in the mempool
///
/// # Arguments
/// * `pending_txs` - Map of pending transactions
/// * `high_frequency_threshold` - Number of transactions to consider as high frequency
/// * `time_window_sec` - Time window to analyze
///
/// # Returns
/// * `Vec<(String, usize)>` - List of potential MEV bot addresses and their transaction counts
pub fn find_mev_bots(
    pending_txs: &HashMap<String, MempoolTransaction>,
    high_frequency_threshold: usize,
    time_window_sec: u64
) -> Vec<(String, usize)> {
    let mut address_tx_count: HashMap<String, usize> = HashMap::new();
    let mut results = Vec::new();
    
    // Get current time
    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(e) => {
            tracing::warn!("Failed to get current time in find_mev_bots: {}", e);
            // Fallback to a reasonable default
            let fallback_time = 1609459200; // 2021-01-01 00:00:00 UTC
            tracing::warn!("Using fallback time: {}", fallback_time);
            fallback_time
        }
    };
    
    let window_start = now.saturating_sub(time_window_sec);
    
    // Count transactions per address within time window
    for tx in pending_txs.values() {
        if tx.detected_at >= window_start {
            *address_tx_count.entry(tx.from_address.clone()).or_insert(0) += 1;
        }
    }
    
    // Find addresses with high frequency transactions
    for (address, count) in address_tx_count {
        if count >= high_frequency_threshold {
            results.push((address, count));
        }
    }
    
    // Sort by transaction count (descending)
    results.sort_by(|a, b| b.1.cmp(&a.1));
    
    results
}

/// Detect potential sandwich attacks in the mempool
///
/// # Arguments
/// * `pending_txs` - Map of pending transactions
/// * `min_value_eth` - Minimum transaction value to consider as a victim
/// * `time_window_ms` - Time window for attack detection
///
/// # Returns
/// * `Vec<Vec<MempoolTransaction>>` - List of potential sandwich attack transaction groups
pub fn find_sandwich_attacks(
    pending_txs: &HashMap<String, MempoolTransaction>,
    min_value_eth: f64,
    time_window_ms: u64
) -> Vec<Vec<MempoolTransaction>> {
    let mut results = Vec::new();
    let mut processed_txs = HashSet::new();
    
    // Convert time window to seconds
    let time_window_sec = time_window_ms / 1000;
    
    // Get all swap transactions
    let swap_txs: Vec<&MempoolTransaction> = pending_txs.values()
        .filter(|tx| tx.transaction_type == TransactionType::Swap)
        .collect();
    
    for tx in &swap_txs {
        // Skip already processed transactions
        if processed_txs.contains(&tx.tx_hash) {
            continue;
        }
        
        // Find potential victim transactions (larger value swaps)
        if tx.value >= min_value_eth {
            // Look for front-running transaction
            let front_runners: Vec<&MempoolTransaction> = swap_txs.iter()
                .filter(|&other_tx| {
                    // Different sender but targeting same token/pool
                    other_tx.from_address != tx.from_address &&
                    other_tx.to_address == tx.to_address &&
                    // Higher gas price to get executed first
                    other_tx.gas_price > tx.gas_price &&
                    // Within time window before victim
                    other_tx.detected_at <= tx.detected_at &&
                    other_tx.detected_at >= tx.detected_at.saturating_sub(time_window_sec)
                })
                .copied()
                .collect();
            
            // Look for back-running transaction
            let back_runners: Vec<&MempoolTransaction> = swap_txs.iter()
                .filter(|&other_tx| {
                    // Same sender as front-runner
                    front_runners.iter().any(|fr| fr.from_address == other_tx.from_address) &&
                    // Within time window after victim
                    other_tx.detected_at >= tx.detected_at &&
                    other_tx.detected_at <= tx.detected_at + time_window_sec
                })
                .copied()
                .collect();
            
            // If found both front and back transactions
            if !front_runners.is_empty() && !back_runners.is_empty() {
                let mut attack_group = Vec::new();
                
                // Add front-running transaction
                for fr in front_runners {
                    attack_group.push(fr.clone());
                    processed_txs.insert(fr.tx_hash.clone());
                }
                
                // Add victim transaction
                attack_group.push(tx.clone());
                processed_txs.insert(tx.tx_hash.clone());
                
                // Add back-running transactions
                for br in back_runners {
                    attack_group.push(br.clone());
                    processed_txs.insert(br.tx_hash.clone());
                }
                
                results.push(attack_group);
            }
        }
    }
    
    results
}

/// Detect potential frontrunning transactions in the mempool
///
/// # Arguments
/// * `pending_txs` - Map of pending transactions
/// * `min_value_eth` - Minimum transaction value to consider as a target
/// * `time_window_ms` - Time window for attack detection
///
/// # Returns
/// * `Vec<(MempoolTransaction, MempoolTransaction)>` - List of (frontrunner, victim) transaction pairs
pub fn find_frontrunning_transactions(
    pending_txs: &HashMap<String, MempoolTransaction>,
    min_value_eth: f64,
    time_window_ms: u64
) -> Vec<(MempoolTransaction, MempoolTransaction)> {
    let mut results = Vec::new();
    let mut processed_txs = HashSet::new();
    
    // Convert time window to seconds
    let time_window_sec = time_window_ms / 1000;
    
    // Get potential target transactions (larger value)
    let target_txs: Vec<&MempoolTransaction> = pending_txs.values()
        .filter(|tx| tx.value >= min_value_eth)
        .collect();
    
    for target in &target_txs {
        // Skip already processed transactions
        if processed_txs.contains(&target.tx_hash) {
            continue;
        }
        
        // Look for front-running transactions
        for (_, tx) in pending_txs {
            // Different sender but similar transaction type
            if tx.from_address != target.from_address && 
               tx.transaction_type == target.transaction_type &&
               // Higher gas price to get executed first
               tx.gas_price > target.gas_price * 1.2 &&
               // Within time window before target
               tx.detected_at < target.detected_at &&
               tx.detected_at >= target.detected_at.saturating_sub(time_window_sec) {
                
                // If targeting same contract/token
                if let (Some(target_to), Some(tx_to)) = (&target.to_address, &tx.to_address) {
                    if target_to == tx_to {
                        results.push((tx.clone(), target.clone()));
                        processed_txs.insert(target.tx_hash.clone());
                        processed_txs.insert(tx.tx_hash.clone());
                        break;
                    }
                }
            }
        }
    }
    
    results
} 