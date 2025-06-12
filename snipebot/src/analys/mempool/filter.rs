//! Filtering functions for mempool transactions
//!
//! This module provides functions to filter and select transactions from the mempool
//! based on various criteria such as transaction type, value, gas price, etc.

use std::collections::HashMap;
use super::types::*;

/// Get filtered transactions based on provided filter options
///
/// # Arguments
/// * `pending_txs` - Map of pending transactions
/// * `filter_options` - Filter criteria
/// * `limit` - Maximum number of transactions to return
///
/// # Returns
/// * `Vec<MempoolTransaction>` - Filtered transactions
pub fn get_filtered_transactions(
    pending_txs: &HashMap<String, MempoolTransaction>,
    filter_options: TransactionFilterOptions,
    limit: usize
) -> Vec<MempoolTransaction> {
    // Start with all transactions
    let mut filtered: Vec<MempoolTransaction> = pending_txs.values().cloned().collect();
    
    // Apply filter by transaction type
    if let Some(types) = &filter_options.transaction_types {
        if !types.is_empty() {
            filtered.retain(|tx| types.contains(&tx.transaction_type));
        }
    }
    
    // Apply filter by priority
    if let Some(priorities) = &filter_options.priorities {
        if !priorities.is_empty() {
            filtered.retain(|tx| priorities.contains(&tx.priority));
        }
    }
    
    // Apply value filters
    if let Some(min_value) = filter_options.min_value {
        filtered.retain(|tx| tx.value >= min_value);
    }
    
    if let Some(max_value) = filter_options.max_value {
        filtered.retain(|tx| tx.value <= max_value);
    }
    
    // Apply gas price filters
    if let Some(min_gas) = filter_options.min_gas_price {
        filtered.retain(|tx| tx.gas_price >= min_gas);
    }
    
    if let Some(max_gas) = filter_options.max_gas_price {
        filtered.retain(|tx| tx.gas_price <= max_gas);
    }
    
    // Apply address filters
    if let Some(from_addresses) = &filter_options.from_addresses {
        if !from_addresses.is_empty() {
            filtered.retain(|tx| from_addresses.contains(&tx.from_address));
        }
    }
    
    if let Some(to_addresses) = &filter_options.to_addresses {
        if !to_addresses.is_empty() {
            filtered.retain(|tx| {
                if let Some(to) = &tx.to_address {
                    to_addresses.contains(to)
                } else {
                    false
                }
            });
        }
    }
    
    // Apply function signature filter
    if let Some(signatures) = &filter_options.function_signatures {
        if !signatures.is_empty() {
            filtered.retain(|tx| {
                if let Some(sig) = &tx.function_signature {
                    signatures.contains(sig)
                } else {
                    false
                }
            });
        }
    }
    
    // Apply time range filters
    if let Some(start_time) = filter_options.start_time {
        filtered.retain(|tx| tx.detected_at >= start_time);
    }
    
    if let Some(end_time) = filter_options.end_time {
        filtered.retain(|tx| tx.detected_at <= end_time);
    }
    
    // Apply input data filter
    if let Some(has_input) = filter_options.has_input_data {
        filtered.retain(|tx| {
            let has_data = tx.input_data.len() > 2; // More than just "0x"
            has_input == has_data
        });
    }
    
    // Sort results if specified
    if let Some(sort_by) = &filter_options.sort_by {
        let ascending = filter_options.sort_ascending.unwrap_or(true);
        
        match sort_by {
            TransactionSortCriteria::DetectedTime => {
                if ascending {
                    filtered.sort_by_key(|tx| tx.detected_at);
                } else {
                    filtered.sort_by_key(|tx| std::cmp::Reverse(tx.detected_at));
                }
            },
            TransactionSortCriteria::Value => {
                if ascending {
                    filtered.sort_by(|a, b| a.value.partial_cmp(&b.value).unwrap_or(std::cmp::Ordering::Equal));
                } else {
                    filtered.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal));
                }
            },
            TransactionSortCriteria::GasPrice => {
                if ascending {
                    filtered.sort_by(|a, b| a.gas_price.partial_cmp(&b.gas_price).unwrap_or(std::cmp::Ordering::Equal));
                } else {
                    filtered.sort_by(|a, b| b.gas_price.partial_cmp(&a.gas_price).unwrap_or(std::cmp::Ordering::Equal));
                }
            },
            TransactionSortCriteria::Nonce => {
                if ascending {
                    filtered.sort_by_key(|tx| tx.nonce);
                } else {
                    filtered.sort_by_key(|tx| std::cmp::Reverse(tx.nonce));
                }
            },
            TransactionSortCriteria::Priority => {
                if ascending {
                    filtered.sort_by_key(|tx| tx.priority.clone());
                } else {
                    filtered.sort_by_key(|tx| std::cmp::Reverse(tx.priority.clone()));
                }
            },
        }
    }
    
    // Apply limit
    if filtered.len() > limit {
        filtered.truncate(limit);
    }
    
    filtered
}

/// Kiểm tra xem một giao dịch có liên quan đến token cụ thể không
///
/// # Parameters
/// - `tx`: Giao dịch cần kiểm tra
/// - `token_address`: Địa chỉ token cần kiểm tra
///
/// # Returns
/// - `bool`: True nếu giao dịch liên quan đến token
pub fn is_token_transaction(tx: &MempoolTransaction, token_address: &str) -> bool {
    // Kiểm tra từ input data
    if tx.input_data.to_lowercase().contains(&token_address.to_lowercase()) {
        return true;
    }
    
    // Kiểm tra nếu địa chỉ nhận là token
    if let Some(to) = &tx.to_address {
        if to.to_lowercase() == token_address.to_lowercase() {
            return true;
        }
    }
    
    // Kiểm tra function signature là ERC20
    if let Some(sig) = &tx.function_signature {
        // ERC20 functions
        if sig == "0xa9059cbb" || // transfer
           sig == "0x23b872dd" || // transferFrom
           sig == "0x095ea7b3" {  // approve
            return true;
        }
    }
    
    false
}

/// Phân loại giao dịch vào các nhóm
///
/// # Parameters
/// - `transactions`: Danh sách giao dịch cần phân loại
///
/// # Returns
/// - `HashMap<TransactionType, Vec<MempoolTransaction>>`: Giao dịch đã phân loại
pub fn classify_transactions(
    transactions: &HashMap<String, MempoolTransaction>
) -> HashMap<TransactionType, Vec<MempoolTransaction>> {
    let mut classified = HashMap::new();
    
    // Tạo map rỗng cho tất cả các loại giao dịch
    for tx_type in [
        TransactionType::NativeTransfer,
        TransactionType::TokenTransfer,
        TransactionType::TokenCreation,
        TransactionType::AddLiquidity,
        TransactionType::RemoveLiquidity,
        TransactionType::Swap,
        TransactionType::NftMint,
        TransactionType::ContractInteraction,
        TransactionType::ContractDeployment,
        TransactionType::Unknown,
    ].iter() {
        classified.insert(tx_type.clone(), Vec::new());
    }
    
    // Phân loại giao dịch
    for (_, tx) in transactions {
        if let Some(txs) = classified.get_mut(&tx.transaction_type) {
            txs.push(tx.clone());
        }
    }
    
    classified
} 