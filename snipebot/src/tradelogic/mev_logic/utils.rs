//! Utilities cho MEV Logic
//!
//! Module này cung cấp các hàm tiện ích hỗ trợ cho MEV logic.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use anyhow::Result;
use ethers::types::{U256, H256, Address};

use crate::analys::mempool::TokenInfo;
use crate::types::TokenPair;
use super::types::MarketData;

/// Tính toán thời gian hiện tại tính từ UNIX epoch (giây)
pub fn current_time_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs()
}

/// Tính toán điểm rủi ro từ nhiều yếu tố
pub fn calculate_risk_score(factors: &HashMap<String, f64>, weights: &HashMap<String, f64>) -> f64 {
    let mut total_score = 0.0;
    let mut total_weight = 0.0;
    
    for (factor, value) in factors {
        if let Some(weight) = weights.get(factor) {
            total_score += value * weight;
            total_weight += weight;
        }
    }
    
    if total_weight > 0.0 {
        total_score / total_weight
    } else {
        50.0 // Default risk score nếu không có trọng số
    }
}

/// Đánh giá token từ thông tin cơ bản
pub fn evaluate_token_from_info(token: &TokenInfo) -> (f64, Vec<String>) {
    let mut risk_score = 50.0;
    let mut warning_messages = Vec::new();
    
    // Kiểm tra thanh khoản
    if token.liquidity_usd < 10000.0 {
        risk_score += 20.0;
        warning_messages.push(format!("Thanh khoản thấp: ${:.2}", token.liquidity_usd));
    } else if token.liquidity_usd > 1000000.0 {
        risk_score -= 10.0;
        // Không cần warning cho điều tốt
    }
    
    // Kiểm tra token mới
    if token.created_at > 0 {
        let now = current_time_seconds();
        let age_days = (now - token.created_at) as f64 / 86400.0;
        
        if age_days < 1.0 {
            risk_score += 15.0;
            warning_messages.push("Token mới (<24h)".to_string());
        } else if age_days < 7.0 {
            risk_score += 5.0;
            warning_messages.push("Token tương đối mới (<7 ngày)".to_string());
        }
    }
    
    // Chuẩn hóa risk score
    risk_score = risk_score.max(0.0).min(100.0);
    
    (risk_score, warning_messages)
}

/// Tạo market data mẫu cho testing
#[cfg(test)]
pub fn create_sample_market_data() -> MarketData {
    MarketData {
        volatility_24h: 5.2,
        volume_24h: 1_500_000.0,
        best_liquidity_dex: Some("uniswap".to_string()),
        price_trend: Some("bullish".to_string()),
        first_seen: Some(current_time_seconds() - 86400 * 30), // 30 ngày trước
    }
}

/// Tính toán tỷ lệ phần trăm thay đổi giữa hai giá trị
pub fn calculate_percentage_change(old_value: f64, new_value: f64) -> f64 {
    if old_value == 0.0 {
        return 0.0;
    }
    
    ((new_value - old_value) / old_value) * 100.0
}

/// Chuyển đổi giá trị ETH sang USD sử dụng giá ETH cho trước
pub fn eth_to_usd(eth_amount: f64, eth_price_usd: f64) -> f64 {
    eth_amount * eth_price_usd
}

/// Chuyển đổi giá trị USD sang ETH sử dụng giá ETH cho trước
pub fn usd_to_eth(usd_amount: f64, eth_price_usd: f64) -> f64 {
    if eth_price_usd == 0.0 {
        return 0.0;
    }
    
    usd_amount / eth_price_usd
} 