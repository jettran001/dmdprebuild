use std::collections::HashMap;

use crate::analys::mempool::types::{MempoolTransaction, TransactionPriority};
use crate::analys::mempool::utils;

/// Phân loại ưu tiên giao dịch dựa trên gas price
///
/// # Parameters
/// - `gas_price`: Gas price giao dịch (Gwei)
/// - `avg_gas_price`: Gas price trung bình hiện tại (Gwei)
///
/// # Returns
/// - `TransactionPriority`: Mức độ ưu tiên
pub fn classify_priority(gas_price: f64, avg_gas_price: f64) -> TransactionPriority {
    if avg_gas_price <= 0.0 {
        return TransactionPriority::Medium;
    }
    
    let ratio = gas_price / avg_gas_price;
    
    if ratio >= 1.5 {
        TransactionPriority::VeryHigh
    } else if ratio >= 1.0 {
        TransactionPriority::High
    } else if ratio >= 0.7 {
        TransactionPriority::Medium
    } else {
        TransactionPriority::Low
    }
}

/// Ước tính thời gian xác nhận giao dịch
///
/// # Parameters
/// - `gas_price`: Gas price giao dịch (Gwei)
/// - `avg_gas_price`: Gas price trung bình hiện tại (Gwei)
/// - `chain_id`: ID của blockchain
///
/// # Returns
/// - `String`: Ước tính thời gian xác nhận
pub fn estimate_confirmation_time(
    transaction_priority: &TransactionPriority,
    chain_id: u32,
) -> String {
    utils::estimate_confirmation_time(transaction_priority, chain_id)
}

/// Phân tích phân phối gas trong mempool
///
/// # Parameters
/// - `transactions`: Danh sách giao dịch mempool
///
/// # Returns
/// - `(f64, f64, f64, f64)`: (min, max, median, average) gas price
pub fn analyze_gas_distribution(
    transactions: &HashMap<String, MempoolTransaction>
) -> (f64, f64, f64, f64) {
    if transactions.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    
    // Lấy danh sách gas price
    let mut gas_prices: Vec<f64> = transactions.values()
        .map(|tx| tx.gas_price)
        .collect();
    
    // Sắp xếp để tính median
    gas_prices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    
    // Tính các giá trị thống kê
    let min = gas_prices.first().cloned().unwrap_or(0.0);
    let max = gas_prices.last().cloned().unwrap_or(0.0);
    let sum: f64 = gas_prices.iter().sum();
    let avg = sum / gas_prices.len() as f64;
    
    // Tính median
    let median = if gas_prices.len() % 2 == 0 {
        let mid = gas_prices.len() / 2;
        (gas_prices[mid - 1] + gas_prices[mid]) / 2.0
    } else {
        gas_prices[gas_prices.len() / 2]
    };
    
    (min, max, median, avg)
}

/// Ước tính gas cần thiết để xác nhận nhanh
///
/// # Parameters
/// - `transactions`: Danh sách giao dịch mempool
/// - `percentile`: Phần trăm độ nhanh (VD: 90 = nhanh hơn 90% giao dịch)
///
/// # Returns
/// - `f64`: Gas price ước tính (Gwei)
pub fn estimate_fast_gas_price(
    transactions: &HashMap<String, MempoolTransaction>,
    percentile: usize,
) -> f64 {
    if transactions.is_empty() {
        return 0.0;
    }
    
    // Kiểm tra phần trăm hợp lệ
    let percentile = percentile.clamp(1, 100);
    
    // Lấy danh sách gas price
    let mut gas_prices: Vec<f64> = transactions.values()
        .map(|tx| tx.gas_price)
        .collect();
    
    // Sắp xếp tăng dần
    gas_prices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    
    // Tính vị trí phần trăm
    let index = (gas_prices.len() * percentile) / 100;
    
    // Lấy giá trị tại vị trí phần trăm
    if index >= gas_prices.len() {
        *gas_prices.last().unwrap_or(&0.0)
    } else {
        gas_prices[index]
    }
}

/// Tính bội số gas tối ưu với chi phí tối thiểu
///
/// # Parameters
/// - `transactions`: Danh sách giao dịch mempool
/// - `min_priority`: Mức độ ưu tiên tối thiểu
///
/// # Returns
/// - `f64`: Bội số gas tối ưu (so với gas trung bình)
pub fn calculate_optimal_gas_multiplier(
    transactions: &HashMap<String, MempoolTransaction>,
    min_priority: TransactionPriority,
) -> f64 {
    if transactions.is_empty() {
        return 1.0;
    }
    
    // Lấy danh sách gas price
    let mut gas_prices: Vec<f64> = transactions.values()
        .map(|tx| tx.gas_price)
        .collect();
    
    // Sắp xếp tăng dần
    gas_prices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    
    // Tính gas trung bình
    let sum: f64 = gas_prices.iter().sum();
    let avg_gas = sum / gas_prices.len() as f64;
    
    // Xác định bội số theo mức độ ưu tiên
    match min_priority {
        TransactionPriority::VeryHigh => 1.5,
        TransactionPriority::High => 1.1,
        TransactionPriority::Medium => 1.0,
        TransactionPriority::Low => 0.7,
    }
}

/// Phân tích block space đã sử dụng
///
/// # Parameters
/// - `transactions`: Danh sách giao dịch mempool
/// - `max_gas_per_block`: Gas tối đa mỗi block
///
/// # Returns
/// - `(f64, u64)`: (Phần trăm block space đã dùng, ước tính số block cần để xử lý hết)
pub fn analyze_block_utilization(
    transactions: &HashMap<String, MempoolTransaction>,
    max_gas_per_block: u64,
) -> (f64, u64) {
    if transactions.is_empty() || max_gas_per_block == 0 {
        return (0.0, 0);
    }
    
    // Tổng gas trong mempool
    let total_gas: u64 = transactions.values()
        .map(|tx| tx.gas_limit)
        .sum();
    
    // Phần trăm block space đã dùng
    let utilization_percent = (total_gas as f64 / max_gas_per_block as f64) * 100.0;
    
    // Số block cần để xử lý hết
    let blocks_needed = (total_gas + max_gas_per_block - 1) / max_gas_per_block;
    
    (utilization_percent, blocks_needed)
}

/// Tính toán chi phí gas cho giao dịch
///
/// # Parameters
/// - `gas_price`: Gas price (Gwei)
/// - `gas_limit`: Gas limit
/// - `eth_price_usd`: Giá ETH/BNB (USD)
///
/// # Returns
/// - `(f64, f64)`: (Chi phí ETH/BNB, Chi phí USD)
pub fn calculate_gas_cost(
    gas_price: f64,
    gas_limit: u64,
    eth_price_usd: f64,
) -> (f64, f64) {
    // Chuyển đổi gas price từ Gwei sang ETH
    let gas_price_eth = gas_price * 1e-9;
    
    // Tính chi phí ETH
    let eth_cost = gas_price_eth * gas_limit as f64;
    
    // Tính chi phí USD
    let usd_cost = eth_cost * eth_price_usd;
    
    (eth_cost, usd_cost)
} 