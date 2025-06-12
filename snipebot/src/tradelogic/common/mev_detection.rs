//! Phát hiện và phòng chống MEV - Module dùng chung
//!
//! Module này cung cấp các hàm dùng chung để phát hiện và bảo vệ chống lại
//! các cuộc tấn công MEV như front-running, sandwich attacks, và arbitrage.
//! Được chia sẻ giữa module tradelogic/smart_trade/anti_mev.rs và mev_logic.

use std::collections::{HashMap, HashSet};
use anyhow::Result;
use chrono::{DateTime, Utc};
use tracing::{debug, warn};

use crate::analys::mempool::{
    MempoolTransaction, SuspiciousPattern,
    detect_front_running, detect_sandwich_attack, detect_high_frequency_trading
};

/// Loại MEV attack
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MevAttackType {
    /// Front-running: Bot thấy giao dịch có lợi nhuận, chèn giao dịch của nó lên trước
    FrontRunning,
    
    /// Back-running: Bot đợi giao dịch lớn, sau đó chèn giao dịch của nó sau
    BackRunning,
    
    /// Sandwich attack: Bot chèn giao dịch trước và sau một giao dịch target
    SandwichAttack,
    
    /// Arbitrage thông thường
    Arbitrage,
    
    /// Liquidation thông thường
    Liquidation,
    
    /// JIT (just-in-time) Liquidity
    JitLiquidity,
    
    /// Loại MEV khác
    Other(String),
}

/// Kết quả phân tích MEV
#[derive(Debug, Clone)]
pub struct MevAnalysisResult {
    /// Token address being analyzed
    pub token_address: String,
    
    /// Chain ID
    pub chain_id: u32,
    
    /// Liệu có phát hiện MEV activity không
    pub mev_activity_detected: bool,
    
    /// Loại MEV attack đã phát hiện
    pub attack_types: Vec<MevAttackType>,
    
    /// Địa chỉ của các bot đã phát hiện
    pub mev_bot_addresses: Vec<String>,
    
    /// Mempool transactions liên quan
    pub related_transactions: Vec<MempoolTransaction>,
    
    /// Ước tính profit của MEV bot
    pub estimated_profit: Option<f64>,
    
    /// Risk score (0-100)
    pub risk_score: u8,
    
    /// Đề xuất giao dịch
    pub recommendation: String,
    
    /// Thời gian phân tích
    pub timestamp: DateTime<Utc>,
}

/// Phân tích mempool để phát hiện MEV attacks
///
/// Hàm này phân tích các giao dịch trong mempool để phát hiện các dấu hiệu
/// của hoạt động MEV nhắm vào một token cụ thể.
///
/// # Arguments
/// * `mempool_txs` - Các giao dịch trong mempool
/// * `token_address` - Địa chỉ token cần phân tích
/// * `chain_id` - Chain ID
/// * `known_mev_bots` - Danh sách địa chỉ MEV bot đã biết
///
/// # Returns
/// * `MevAnalysisResult` - Kết quả phân tích
pub async fn analyze_mempool_for_mev(
    mempool_txs: &HashMap<String, MempoolTransaction>,
    token_address: &str,
    chain_id: u32,
    known_mev_bots: &HashSet<String>,
) -> Result<MevAnalysisResult> {
    let mut result = MevAnalysisResult {
        token_address: token_address.to_string(),
        chain_id,
        mev_activity_detected: false,
        attack_types: Vec::new(),
        mev_bot_addresses: Vec::new(),
        related_transactions: Vec::new(),
        estimated_profit: None,
        risk_score: 0,
        recommendation: "No MEV activity detected.".to_string(),
        timestamp: Utc::now(),
    };
    
    // Filter transactions related to the token
    let related_txs: Vec<&MempoolTransaction> = mempool_txs.values()
        .filter(|tx| {
            // Check if transaction is related to the token
            tx.to_token.as_ref().map_or(false, |t| t.address == token_address) ||
            tx.from_token.as_ref().map_or(false, |t| t.address == token_address)
        })
        .collect();
    
    if related_txs.is_empty() {
        return Ok(result);
    }
    
    // Add related transactions to result
    result.related_transactions = related_txs.iter().map(|&tx| tx.clone()).collect();
    
    // Check for front-running
    let mut front_running_detected = false;
    let mut sandwich_attack_detected = false;
    let mut high_freq_detected = false;
    
    for tx in &related_txs {
        // Check if transaction is from a known MEV bot
        if known_mev_bots.contains(&tx.from_address.to_lowercase()) {
            result.mev_activity_detected = true;
            if !result.mev_bot_addresses.contains(&tx.from_address) {
                result.mev_bot_addresses.push(tx.from_address.clone());
            }
        }
        
        // Detect front-running
        if let Ok(pattern) = detect_front_running(tx, mempool_txs).await {
            if let Some(SuspiciousPattern::FrontRunning) = pattern {
                front_running_detected = true;
                result.mev_activity_detected = true;
            }
        }
        
        // Detect sandwich attack
        if let Ok(pattern) = detect_sandwich_attack(tx, mempool_txs).await {
            if let Some(SuspiciousPattern::SandwichAttack) = pattern {
                sandwich_attack_detected = true;
                result.mev_activity_detected = true;
            }
        }
        
        // Detect high frequency trading
        if let Some(pattern) = detect_high_frequency_trading(tx, mempool_txs).await {
            if let SuspiciousPattern::HighFrequencyTrading = pattern {
                high_freq_detected = true;
                result.mev_activity_detected = true;
            }
        }
    }
    
    // Add detected attack types
    if front_running_detected {
        result.attack_types.push(MevAttackType::FrontRunning);
    }
    
    if sandwich_attack_detected {
        result.attack_types.push(MevAttackType::SandwichAttack);
    }
    
    if high_freq_detected {
        // This might be arbitrage or liquidation
        result.attack_types.push(MevAttackType::Arbitrage);
    }
    
    // Calculate risk score based on detected activities
    result.risk_score = calculate_mev_risk_score(&result);
    
    // Generate recommendation based on risk score
    result.recommendation = generate_mev_recommendation(&result);
    
    Ok(result)
}

/// Tính toán MEV risk score dựa trên kết quả phân tích
///
/// # Arguments
/// * `analysis` - Kết quả phân tích MEV
///
/// # Returns
/// * `u8` - Risk score (0-100)
fn calculate_mev_risk_score(analysis: &MevAnalysisResult) -> u8 {
    let mut score = 0;
    
    // Base score for detected MEV activity
    if analysis.mev_activity_detected {
        score += 20;
    }
    
    // Additional score based on attack types
    for attack_type in &analysis.attack_types {
        match attack_type {
            MevAttackType::SandwichAttack => score += 30,
            MevAttackType::FrontRunning => score += 25,
            MevAttackType::BackRunning => score += 15,
            MevAttackType::Arbitrage => score += 10,
            MevAttackType::Liquidation => score += 5,
            MevAttackType::JitLiquidity => score += 15,
            MevAttackType::Other(_) => score += 10,
        }
    }
    
    // Additional score based on number of MEV bots
    score += (analysis.mev_bot_addresses.len() as u8) * 5;
    
    // Cap the score at 100
    score.min(100)
}

/// Generate recommendation based on MEV analysis
///
/// # Arguments
/// * `analysis` - MEV analysis result
///
/// # Returns
/// * `String` - Recommendation
fn generate_mev_recommendation(analysis: &MevAnalysisResult) -> String {
    if !analysis.mev_activity_detected {
        return "No MEV activity detected. Standard trading parameters recommended.".to_string();
    }
    
    let mut recommendation = String::new();
    
    // Generate recommendation based on risk score
    match analysis.risk_score {
        0..=20 => {
            recommendation.push_str("Low MEV risk detected. Consider slightly increasing gas price (1.1x-1.2x) for safer execution.");
        }
        21..=50 => {
            recommendation.push_str("Moderate MEV risk detected. Recommend using higher gas price (1.3x-1.5x) and reduced slippage. Consider smaller trade sizes.");
        }
        51..=75 => {
            recommendation.push_str("High MEV risk detected! Use private transactions if available or significantly higher gas price (1.5x-2.0x). Limit trade size and use lower slippage.");
        }
        _ => {
            recommendation.push_str("CRITICAL MEV risk! Recommend delaying transaction or using private transaction only. High chance of front-running or sandwich attacks.");
        }
    }
    
    // Add specific recommendations based on attack types
    if analysis.attack_types.contains(&MevAttackType::SandwichAttack) {
        recommendation.push_str("\nSandwich attack detected: Use lower slippage and consider splitting into multiple smaller transactions.");
    }
    
    if analysis.attack_types.contains(&MevAttackType::FrontRunning) {
        recommendation.push_str("\nFront-running detected: Use higher gas price or private transaction.");
    }
    
    recommendation
}

/// Tính toán gas price tối ưu dựa trên phân tích MEV
///
/// # Arguments
/// * `analysis` - Kết quả phân tích MEV
/// * `base_gas_price` - Gas price cơ sở (gwei)
/// * `max_multiplier` - Hệ số nhân tối đa cho gas price
///
/// # Returns
/// * `f64` - Gas price được đề xuất (gwei)
pub fn calculate_optimal_gas_price(
    analysis: &MevAnalysisResult,
    base_gas_price: f64,
    max_multiplier: f64,
) -> f64 {
    if !analysis.mev_activity_detected {
        return base_gas_price * 1.05; // Slight increase for safety
    }
    
    // Calculate multiplier based on risk score
    let multiplier = match analysis.risk_score {
        0..=20 => 1.1,
        21..=50 => 1.3,
        51..=75 => 1.6,
        _ => max_multiplier,
    };
    
    // Apply multiplier but cap at max_multiplier
    (base_gas_price * multiplier).min(base_gas_price * max_multiplier)
}

/// Tính toán slippage tối ưu dựa trên phân tích MEV
///
/// # Arguments
/// * `analysis` - Kết quả phân tích MEV
/// * `base_slippage` - Slippage cơ sở (%)
/// * `is_buy` - Có phải là giao dịch mua không
///
/// # Returns
/// * `f64` - Slippage được đề xuất (%)
pub fn calculate_optimal_slippage(
    analysis: &MevAnalysisResult,
    base_slippage: f64,
    is_buy: bool,
) -> f64 {
    if !analysis.mev_activity_detected {
        return base_slippage;
    }
    
    // For buy transactions, we may want higher slippage to ensure execution
    // For sell transactions, we want lower slippage to prevent sandwiching
    let max_slippage = if is_buy { 5.0 } else { 2.0 };
    
    let slippage = match analysis.risk_score {
        0..=20 => base_slippage,
        21..=50 => if is_buy { base_slippage * 1.2 } else { base_slippage * 0.8 },
        51..=75 => if is_buy { base_slippage * 1.5 } else { base_slippage * 0.6 },
        _ => if is_buy { max_slippage } else { base_slippage * 0.5 },
    };
    
    // Ensure slippage is within reasonable bounds
    slippage.max(0.1).min(max_slippage)
}

/// Generates anti-MEV transaction parameters based on risk analysis
///
/// This function creates optimal transaction parameters to protect against
/// detected MEV attacks.
///
/// # Arguments
/// * `analysis` - MEV analysis result
/// * `base_gas_price` - Base gas price (gwei)
/// * `is_buy` - Whether this is a buy transaction
/// * `use_private_tx` - Whether to use private transactions
/// * `chain_id` - Chain ID
///
/// # Returns
/// * `Result<AntiMevTxParams>` - Optimal transaction parameters
pub fn generate_anti_mev_tx_params(
    analysis: &MevAnalysisResult,
    base_gas_price: f64,
    base_slippage: f64,
    is_buy: bool,
    use_private_tx: bool,
    private_tx_supported_chains: &[u32],
    chain_id: u32,
) -> Result<AntiMevTxParams> {
    let gas_price = calculate_optimal_gas_price(analysis, base_gas_price, 2.0);
    let slippage = calculate_optimal_slippage(analysis, base_slippage, is_buy);
    
    // Determine if private tx should be used
    let should_use_private_tx = use_private_tx && 
                                private_tx_supported_chains.contains(&chain_id) && 
                                analysis.risk_score > 50;
    
    // Calculate delay based on risk
    let delay_ms = match analysis.risk_score {
        0..=20 => 0,
        21..=50 => 500,
        51..=75 => 1000,
        _ => 2000,
    };
    
    // Calculate deadline based on risk
    let deadline_seconds = match analysis.risk_score {
        0..=20 => 300,   // 5 minutes
        21..=50 => 180,  // 3 minutes
        51..=75 => 120,  // 2 minutes
        _ => 60,         // 1 minute
    };
    
    Ok(AntiMevTxParams {
        gas_price,
        slippage_tolerance: slippage,
        deadline_seconds,
        use_private_tx: should_use_private_tx,
        delay_ms,
        max_nonce_reuse_attempt: 3,
        mev_risk_score: analysis.risk_score,
    })
}

/// Anti-MEV transaction parameters
#[derive(Debug, Clone)]
pub struct AntiMevTxParams {
    /// Gas price tối ưu (gwei)
    pub gas_price: f64,
    
    /// Slippage tolerance (%)
    pub slippage_tolerance: f64,
    
    /// Deadline (giây)
    pub deadline_seconds: u64,
    
    /// Có sử dụng private tx không
    pub use_private_tx: bool,
    
    /// Delay trước khi gửi giao dịch (ms)
    pub delay_ms: u64,
    
    /// Số lần thử lại nếu giao dịch bị thay thế
    pub max_nonce_reuse_attempt: u32,
    
    /// MEV risk score (0-100)
    pub mev_risk_score: u8,
} 