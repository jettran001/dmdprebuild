use std::collections::HashMap;

use crate::analys::mempool::types::{
    MempoolTransaction, TransactionType, TransactionPriority,
    TransactionFilterOptions, TransactionSortCriteria
};

/// Lọc giao dịch mempool theo các tiêu chí
///
/// # Parameters
/// - `transactions`: Danh sách giao dịch cần lọc
/// - `filter_options`: Các tùy chọn lọc
/// - `limit`: Số lượng tối đa kết quả trả về
///
/// # Returns
/// - `Vec<MempoolTransaction>`: Danh sách giao dịch sau khi lọc
pub fn get_filtered_transactions(
    transactions: &HashMap<String, MempoolTransaction>,
    filter_options: &TransactionFilterOptions,
    limit: usize,
) -> Vec<MempoolTransaction> {
    let mut result: Vec<MempoolTransaction> = Vec::new();
    
    // Áp dụng các bộ lọc
    for (_, tx) in transactions {
        // Kiểm tra điều kiện lọc
        if !passes_filter(tx, filter_options) {
            continue;
        }
        
        // Thêm vào kết quả
        result.push(tx.clone());
    }
    
    // Sắp xếp kết quả theo tiêu chí
    if let Some(sort_by) = &filter_options.sort_by {
        let ascending = filter_options.sort_ascending.unwrap_or(false);
        sort_transactions(&mut result, sort_by, ascending);
    }
    
    // Giới hạn số lượng kết quả
    if result.len() > limit {
        result.truncate(limit);
    }
    
    result
}

/// Kiểm tra xem giao dịch có thỏa mãn bộ lọc không
///
/// # Parameters
/// - `tx`: Giao dịch cần kiểm tra
/// - `filter`: Bộ lọc cần áp dụng
///
/// # Returns
/// - `bool`: True nếu giao dịch thỏa mãn bộ lọc
fn passes_filter(tx: &MempoolTransaction, filter: &TransactionFilterOptions) -> bool {
    // Lọc theo loại giao dịch
    if let Some(types) = &filter.transaction_types {
        if !types.contains(&tx.transaction_type) {
            return false;
        }
    }
    
    // Lọc theo mức độ ưu tiên
    if let Some(priorities) = &filter.priorities {
        if !priorities.contains(&tx.priority) {
            return false;
        }
    }
    
    // Lọc theo giá trị giao dịch
    if let Some(min_value) = filter.min_value {
        if tx.value < min_value {
            return false;
        }
    }
    
    if let Some(max_value) = filter.max_value {
        if tx.value > max_value {
            return false;
        }
    }
    
    // Lọc theo gas price
    if let Some(min_gas) = filter.min_gas_price {
        if tx.gas_price < min_gas {
            return false;
        }
    }
    
    if let Some(max_gas) = filter.max_gas_price {
        if tx.gas_price > max_gas {
            return false;
        }
    }
    
    // Lọc theo địa chỉ gửi
    if let Some(from_addresses) = &filter.from_addresses {
        if !from_addresses.iter().any(|addr| addr.to_lowercase() == tx.from_address.to_lowercase()) {
            return false;
        }
    }
    
    // Lọc theo địa chỉ nhận
    if let Some(to_addresses) = &filter.to_addresses {
        if let Some(to) = &tx.to_address {
            if !to_addresses.iter().any(|addr| addr.to_lowercase() == to.to_lowercase()) {
                return false;
            }
        } else {
            return false; // Không có địa chỉ nhận
        }
    }
    
    // Lọc theo function signature
    if let Some(signatures) = &filter.function_signatures {
        if let Some(sig) = &tx.function_signature {
            if !signatures.iter().any(|s| s.to_lowercase() == sig.to_lowercase()) {
                return false;
            }
        } else {
            return false; // Không có function signature
        }
    }
    
    // Lọc theo thời gian
    if let Some(start_time) = filter.start_time {
        if tx.detected_at < start_time {
            return false;
        }
    }
    
    if let Some(end_time) = filter.end_time {
        if tx.detected_at > end_time {
            return false;
        }
    }
    
    // Lọc theo input data
    if let Some(has_input) = filter.has_input_data {
        let has_actual_input = tx.input_data.len() > 2; // Dài hơn "0x"
        if has_input != has_actual_input {
            return false;
        }
    }
    
    // Thỏa mãn tất cả điều kiện
    true
}

/// Sắp xếp danh sách giao dịch theo tiêu chí
///
/// # Parameters
/// - `transactions`: Danh sách giao dịch cần sắp xếp
/// - `criteria`: Tiêu chí sắp xếp
/// - `ascending`: Sắp xếp tăng dần hay giảm dần
fn sort_transactions(
    transactions: &mut Vec<MempoolTransaction>,
    criteria: &TransactionSortCriteria,
    ascending: bool
) {
    match criteria {
        TransactionSortCriteria::DetectedTime => {
            if ascending {
                transactions.sort_by(|a, b| a.detected_at.cmp(&b.detected_at));
            } else {
                transactions.sort_by(|a, b| b.detected_at.cmp(&a.detected_at));
            }
        },
        TransactionSortCriteria::Value => {
            if ascending {
                transactions.sort_by(|a, b| a.value.partial_cmp(&b.value).unwrap_or(std::cmp::Ordering::Equal));
            } else {
                transactions.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal));
            }
        },
        TransactionSortCriteria::GasPrice => {
            if ascending {
                transactions.sort_by(|a, b| a.gas_price.partial_cmp(&b.gas_price).unwrap_or(std::cmp::Ordering::Equal));
            } else {
                transactions.sort_by(|a, b| b.gas_price.partial_cmp(&a.gas_price).unwrap_or(std::cmp::Ordering::Equal));
            }
        },
        TransactionSortCriteria::Nonce => {
            if ascending {
                transactions.sort_by(|a, b| a.nonce.cmp(&b.nonce));
            } else {
                transactions.sort_by(|a, b| b.nonce.cmp(&a.nonce));
            }
        },
        TransactionSortCriteria::Priority => {
            if ascending {
                transactions.sort_by(|a, b| a.priority.cmp(&b.priority));
            } else {
                transactions.sort_by(|a, b| b.priority.cmp(&a.priority));
            }
        },
    }
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