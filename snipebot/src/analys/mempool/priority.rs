//! Priority analysis and estimation for mempool transactions
//!
//! This module provides functions to analyze transaction priorities and
//! estimate confirmation times based on gas prices and network conditions.

use std::collections::HashMap;
use tracing;

use crate::analys::mempool::types::{MempoolTransaction, TransactionPriority};
use crate::analys::mempool::utils;

/// Calculate transaction priority based on gas price and current network conditions
///
/// # Arguments
/// * `gas_price`: Transaction gas price in gwei
/// * `avg_gas_price`: Current average gas price in gwei
///
/// # Returns
/// * `TransactionPriority`: Calculated priority level
pub fn calculate_transaction_priority(gas_price: f64, avg_gas_price: f64) -> TransactionPriority {
    if avg_gas_price <= 0.0 {
        return TransactionPriority::Medium;
    }
    
    let ratio = gas_price / avg_gas_price;
    
    if ratio >= 1.5 {
        TransactionPriority::VeryHigh
    } else if ratio >= 1.0 {
        TransactionPriority::High
    } else if ratio >= 0.75 {
        TransactionPriority::Medium
    } else {
        TransactionPriority::Low
    }
}

/// Ước tính thời gian xác nhận giao dịch dựa trên priority
///
/// # Arguments
/// * `tx_priority`: Mức độ ưu tiên của giao dịch
/// * `chain_id`: ID của blockchain
///
/// # Returns
/// * `String`: Ước tính thời gian xác nhận
pub fn estimate_confirmation_time(
    tx_priority: &TransactionPriority, 
    chain_id: u32
) -> String {
    // Thời gian cơ bản theo chain
    let base_time = match chain_id {
        1 => 15.0,  // Ethereum ~15 giây/block
        56 => 3.0,  // BSC ~3 giây/block
        137 => 2.0, // Polygon ~2 giây/block
        _ => 10.0,  // Mặc định 10 giây/block
    };
    
    // Ước tính theo priority
    let (blocks, description) = match tx_priority {
        TransactionPriority::VeryHigh => (1, "very quickly"),
        TransactionPriority::High => (2, "quickly"),
        TransactionPriority::Medium => (5, "in a few minutes"),
        TransactionPriority::Low => (10, "may take some time"),
    };
    
    // Tính toán thời gian dự kiến (giây)
    let time_seconds = base_time * blocks as f64;
    
    if time_seconds < 60.0 {
        format!("~{:.0} seconds ({})", time_seconds, description)
    } else {
        format!("~{:.1} minutes ({})", time_seconds / 60.0, description)
    }
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
        tracing::warn!("estimate_fast_gas_price: Empty transaction list, returning default 0.0");
        return 0.0;
    }
    
    // Kiểm tra phần trăm hợp lệ
    let percentile = percentile.clamp(1, 100);
    
    // Lấy danh sách gas price
    let mut gas_prices: Vec<f64> = transactions.values()
        .map(|tx| tx.gas_price)
        .collect();
    
    // Sắp xếp tăng dần
    gas_prices.sort_by(|a, b| {
        a.partial_cmp(b).unwrap_or_else(|| {
            tracing::warn!("Gas price comparison failed, found NaN or invalid value: a={}, b={}", a, b);
            std::cmp::Ordering::Equal
        })
    });
    
    // Tính vị trí phần trăm
    let index = (gas_prices.len() * percentile) / 100;
    
    // Lấy giá trị tại vị trí phần trăm
    if index >= gas_prices.len() {
        gas_prices.last().copied().unwrap_or_else(|| {
            tracing::warn!("estimate_fast_gas_price: Failed to get last gas price, returning default 0.0");
            0.0
        })
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
        tracing::warn!("calculate_optimal_gas_multiplier: Empty transaction list, returning default multiplier 1.0");
        return 1.0;
    }
    
    // Lấy danh sách gas price
    let mut gas_prices: Vec<f64> = transactions.values()
        .map(|tx| tx.gas_price)
        .collect();
    
    // Sắp xếp tăng dần
    gas_prices.sort_by(|a, b| {
        a.partial_cmp(b).unwrap_or_else(|| {
            tracing::warn!("Gas price comparison failed, found NaN or invalid value: a={}, b={}", a, b);
            std::cmp::Ordering::Equal
        })
    });
    
    // Tính gas trung bình
    let sum: f64 = gas_prices.iter().sum();
    let avg_gas = if gas_prices.is_empty() {
        tracing::warn!("calculate_optimal_gas_multiplier: Empty gas prices after filtering, using default");
        0.0
    } else {
        sum / gas_prices.len() as f64
    };
    
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

/// Chuyển đổi bội số gas thành chuỗi mô tả
///
/// # Arguments
/// * `multiplier`: Bội số gas so với giá trung bình
///
/// # Returns
/// * `String`: Mô tả bội số gas
pub fn gas_multiplier_to_string(multiplier: f64) -> String {
    if multiplier <= 0.7 {
        "slow".to_string()
    } else if multiplier <= 0.9 {
        "standard".to_string()
    } else if multiplier <= 1.1 {
        "fast".to_string()
    } else if multiplier <= 1.5 {
        "rapid".to_string()
    } else {
        "urgent".to_string()
    }
}

/// Format gas price to human-readable string
///
/// # Arguments
/// * `gas_price`: Gas price in gwei
///
/// # Returns
/// * `String`: Formatted gas price string
pub fn format_gas_price(gas_price: f64) -> String {
    if gas_price >= 1000.0 {
        format!("{:.2} Gwei", gas_price / 1000.0)
    } else {
        format!("{:.2} wei", gas_price)
    }
}

/// Analyze current gas price trend in the mempool
///
/// # Arguments
/// * `recent_gas_prices`: Vector of recent gas prices with timestamps (timestamp, gas_price)
///
/// # Returns
/// * `(f64, String)`: Percent change and trend description
pub fn analyze_gas_price_trend(recent_gas_prices: &[(u64, f64)]) -> (f64, String) {
    if recent_gas_prices.len() < 2 {
        tracing::warn!("analyze_gas_price_trend: not enough data");
        return (0.0, "Stable".to_string());
    }
    
    // Get oldest and newest prices
    let oldest = recent_gas_prices.first().map(|x| x.1).unwrap_or(0.0);
    let newest = recent_gas_prices.last().map(|x| x.1).unwrap_or(0.0);
    
    // Calculate percent change
    let percent_change = if oldest > 0.0 {
        ((newest - oldest) / oldest) * 100.0
    } else {
        0.0
    };
    
    // Determine trend
    let trend = if percent_change > 10.0 {
        "Rapidly increasing"
    } else if percent_change > 5.0 {
        "Increasing"
    } else if percent_change < -10.0 {
        "Rapidly decreasing"
    } else if percent_change < -5.0 {
        "Decreasing"
    } else {
        "Stable"
    };
    
    (percent_change, trend.to_string())
} 