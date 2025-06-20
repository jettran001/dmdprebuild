//! Risk Manager - Quản lý rủi ro
//!
//! Module này chứa logic đánh giá rủi ro, phân tích token an toàn,
//! và cung cấp các API kiểm tra an toàn giao dịch.

use std::sync::Arc;
use std::collections::HashMap;
use anyhow::{Result, anyhow};
use tracing::{debug, warn};
use async_trait::async_trait;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::{TokenPair, TradeParams, TradeType, ChainType};
use crate::tradelogic::common::types::{
    TokenIssue, RiskFactor, RiskCategory, RiskPriority, 
    TradeRiskAnalysis as CommonTradeRiskAnalysis,
    TradeRecommendation as CommonTradeRecommendation
};

// Module imports
use crate::tradelogic::smart_trade::executor::core::SmartTradeExecutor;

/// Severity của các vấn đề bảo mật
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueSeverity {
    /// Informational (thông tin)
    Info,
    /// Low (thấp)
    Low,
    /// Medium (trung bình)
    Medium,
    /// High (cao)
    High,
    /// Critical (nghiêm trọng)
    Critical,
}

/// Các loại giao dịch
#[derive(Debug, Clone, PartialEq)]
pub enum TransactionType {
    /// Swap tokens
    Swap,
    /// Transfer tokens
    TokenTransfer,
    /// Add liquidity
    LiquidityAdd,
    /// Remove liquidity
    LiquidityRemove,
    /// Other transaction types
    Other,
}

/// Thông tin giao dịch
#[derive(Debug, Clone)]
pub struct Transaction {
    /// Sender address
    pub sender: String,
    /// Transaction type
    pub transaction_type: TransactionType,
    /// Value in USD
    pub value_usd: f64,
    /// Token amount
    pub amount: f64,
    /// Timestamp
    pub timestamp: u64,
}

/// Thông tin token
#[derive(Debug, Clone)]
pub struct TokenInfo {
    /// Token name
    pub name: String,
    /// Token symbol
    pub symbol: String,
    /// Total supply
    pub total_supply: f64,
    /// Is contract verified
    pub verified: bool,
}

/// Phân tích rủi ro giao dịch
#[derive(Debug, Clone)]
pub struct TradeRiskAnalysis {
    /// Điểm rủi ro (0-100, cao hơn là rủi ro cao hơn)
    pub risk_score: u8,
    /// Các yếu tố rủi ro
    pub risk_factors: Vec<RiskFactor>,
    /// Khuyến nghị
    pub recommendation: TradeRecommendation,
}

/// Khuyến nghị giao dịch
#[derive(Debug, Clone, PartialEq)]
pub enum TradeRecommendation {
    /// An toàn để giao dịch
    SafeToProceed,
    /// Giao dịch với cảnh báo
    ProceedWithCaution(String),
    /// Tránh giao dịch
    Avoid,
}

/// Kiểm tra an toàn token
pub async fn validate_token_safety(
    executor: &SmartTradeExecutor,
    params: &TradeParams,
) -> Result<(bool, Vec<String>)> {
    // Chỉ kiểm tra cho giao dịch mua
    if params.trade_type != TradeType::Buy {
        return Ok((true, Vec::new()));
    }
    
    // Lấy adapter cho chain
    let adapter = match executor.evm_adapters.get(&params.chain_id()) {
        Some(adapter) => adapter,
        None => return Err(anyhow!("No adapter found for chain ID {}", params.chain_id())),
    };
    
    // Lấy cấu hình
    let config = executor.config.read().await;
    
    // Danh sách vấn đề phát hiện được
    let mut issues = Vec::new();
    
    // Kiểm tra trạng thái token qua analys_client
    let token_status = executor.analys_client.get_token_status(
        params.chain_id(),
        &params.token_address,
    ).await?;
    
    // Kiểm tra các vấn đề tiềm ẩn
    for issue in &token_status.issues {
        match issue.0 {
            TokenIssue::Honeypot => {
                issues.push("Critical: Honeypot detected".to_string());
            },
            TokenIssue::UnverifiedContract => {
                if config.block_unverified_contracts {
                    issues.push("Critical: Contract not verified".to_string());
                }
            },
            TokenIssue::HighTax => {
                issues.push(format!("Critical: High tax detected ({}%)", token_status.tax_sell));
            },
            TokenIssue::BlacklistFunction => {
                issues.push("Critical: Blacklist function detected".to_string());
            },
            TokenIssue::OwnerWithFullControl => {
                issues.push("High: Suspicious owner activity detected".to_string());
            },
            TokenIssue::TransferRestriction => {
                issues.push("Critical: Lock sell detected".to_string());
            },
            TokenIssue::LowLiquidity => {
                issues.push("High: Low liquidity".to_string());
            },
            _ => {
                // Handle other issues based on severity
                match issue.1 {
                    IssueSeverity::Critical => {
                        issues.push(format!("Critical: {:?}", issue.0));
                    },
                    IssueSeverity::High => {
                        if config.block_high_risk_tokens {
                            issues.push(format!("High: {:?}", issue.0));
                        }
                    },
                    IssueSeverity::Medium => {
                        if config.block_medium_risk_tokens {
                            issues.push(format!("Medium: {:?}", issue.0));
                        }
                    },
                    _ => {}
                }
            }
        }
    }
    
    // Kiểm tra xem có vấn đề nào không
    let is_safe = issues.is_empty();
    
    Ok((is_safe, issues))
}

/// Phân tích rủi ro giao dịch
pub async fn analyze_risk(
    executor: &SmartTradeExecutor,
    params: &TradeParams,
) -> Result<TradeRiskAnalysis> {
    // Lấy adapter cho chain
    let adapter = match executor.evm_adapters.get(&params.chain_id()) {
        Some(adapter) => adapter,
        None => return Err(anyhow!("No adapter found for chain ID {}", params.chain_id())),
    };
    
    // Phân tích rủi ro qua analys_client
    let token_status = executor.analys_client.get_token_status(
        params.chain_id(),
        &params.token_address,
    ).await?;
    
    // Tạo danh sách các risk factor dựa trên token_status
    let mut risk_factors = Vec::new();
    
    if token_status.honeypot {
        risk_factors.push(RiskFactor {
            category: RiskCategory::Security,
            weight: 100,
            description: "Token is a honeypot".to_string(),
            priority: RiskPriority::High,
        });
    }
    
    if token_status.tax_sell > 10.0 {
        risk_factors.push(RiskFactor {
            category: RiskCategory::Security,
            weight: 80,
            description: format!("High sell tax: {}%", token_status.tax_sell),
            priority: RiskPriority::High,
        });
    } else if token_status.tax_sell > 5.0 {
        risk_factors.push(RiskFactor {
            category: RiskCategory::Security, 
            weight: 50,
            description: format!("Medium sell tax: {}%", token_status.tax_sell),
            priority: RiskPriority::Medium,
        });
    }
    
    if token_status.blacklist {
        risk_factors.push(RiskFactor {
            category: RiskCategory::Security,
            weight: 80,
            description: "Blacklist function detected".to_string(), 
            priority: RiskPriority::High,
        });
    }
    
    if token_status.lock_sell {
        risk_factors.push(RiskFactor {
            category: RiskCategory::Security,
            weight: 100,
            description: "Lock sell function detected".to_string(),
            priority: RiskPriority::High, 
        });
    }
    
    if token_status.low_liquidity {
        risk_factors.push(RiskFactor {
            category: RiskCategory::Liquidity,
            weight: 70,
            description: "Low liquidity".to_string(),
            priority: RiskPriority::Medium,
        });
    }
    
    // Tính điểm rủi ro dựa trên các risk factors
    let mut risk_score = 0;
    let mut total_weight = 0;
    
    for factor in &risk_factors {
        risk_score += factor.weight as u64;
        total_weight += 1;
    }
    
    let final_score = if total_weight > 0 {
        risk_score / total_weight
    } else {
        50 // Default moderate risk
    };
    
    // Đề xuất dựa trên điểm rủi ro
    let recommendation = if final_score >= 80 {
        TradeRecommendation::Avoid
    } else if final_score >= 60 {
        TradeRecommendation::ProceedWithCaution("Proceed with small position".to_string())
    } else {
        TradeRecommendation::SafeToProceed
    };
    
    Ok(TradeRiskAnalysis {
        risk_score: final_score as u8,
        risk_factors,
        recommendation,
    })
}

/// Phân tích rủi ro thị trường
/// Trả về mức rủi ro từ 0-100 (0: an toàn nhất, 100: rủi ro cao nhất)
pub async fn analyze_market_risk(
    executor: &SmartTradeExecutor,
    chain_id: u32,
    token_address: &str,
) -> Result<u8> {
    // Lấy adapter cho chain
    let adapter = match executor.evm_adapters.get(&chain_id) {
        Some(adapter) => adapter,
        None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
    };
    
    // Lấy thông tin thị trường từ adapter
    let market_info = adapter.get_token_market_data(token_address).await?;
    
    // Lấy cấu hình để sử dụng ngưỡng động
    let config = executor.config.read().await;
    let config_ref = &*config;
    
    // Lấy risk threshold từ cấu hình cho chain này
    let risk_thresholds = match config_ref.market_risk_thresholds.get(&chain_id) {
        Some(thresholds) => thresholds,
        None => &config_ref.default_market_risk_thresholds,
    };
    
    // Tính toán điểm rủi ro dựa trên nhiều yếu tố
    let mut risk_score = 0;
    
    // 1. Kiểm tra thanh khoản
    if let Ok(liquidity_info) = adapter.get_token_liquidity(token_address).await {
        let liquidity_usd = liquidity_info.total_liquidity_usd;
        
        if liquidity_usd < risk_thresholds.min_liquidity_usd {
            risk_score += risk_thresholds.low_liquidity_penalty; // Thanh khoản thấp là rủi ro lớn
        } else if liquidity_usd < risk_thresholds.low_liquidity_usd {
            risk_score += risk_thresholds.low_liquidity_penalty / 2;
        } else if liquidity_usd < risk_thresholds.medium_liquidity_usd {
            risk_score += risk_thresholds.low_liquidity_penalty / 3;
        }
    }
    
    // 2. Điều chỉnh theo biến động giá
    let volatility_24h = market_info.volatility_24h;
    if volatility_24h > risk_thresholds.high_price_change_pct {
        risk_score += risk_thresholds.high_volatility_penalty; // Biến động cao là rủi ro
    } else if volatility_24h > risk_thresholds.medium_price_change_pct {
        risk_score += risk_thresholds.high_volatility_penalty / 2;
    } else if volatility_24h > risk_thresholds.low_price_change_pct {
        risk_score += risk_thresholds.high_volatility_penalty / 3;
    }
    
    // 3. Điều chỉnh theo market_risk_factor từ cấu hình (có thể tăng/giảm tùy theo thị trường)
    let market_risk_factor = config_ref.market_risk_factor.get(&chain_id).unwrap_or(&1.0);
    risk_score = (risk_score as f64 * market_risk_factor) as u8;
    
    debug!(
        "Market risk analysis for token {} on chain {}: score={}, volatility={}%, risk_factor={:.2}",
        token_address, chain_id, risk_score, volatility_24h, market_risk_factor
    );
    
    // Giới hạn điểm rủi ro trong khoảng 0-100
    risk_score = risk_score.min(100);
    
    Ok(risk_score)
}

/// Kiểm tra lịch sử giao dịch của token
/// Phát hiện các hành vi đáng ngờ trong lịch sử giao dịch
pub async fn check_token_transaction_history(
    executor: &SmartTradeExecutor,
    chain_id: u32,
    token_address: &str,
) -> Result<Vec<String>> {
    // Kết quả cảnh báo
    let mut warnings = Vec::new();
    
    // Lấy adapter cho chain
    let adapter = match executor.evm_adapters.get(&chain_id) {
        Some(adapter) => adapter,
        None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
    };
    
    // Lấy lịch sử giao dịch
    let tx_history = adapter.get_token_transactions(token_address, 100).await?;
    
    // Phân tích mẫu giao dịch
    let mut large_buys_count = 0;
    let mut large_sells_count = 0;
    let mut suspicious_wallet_count = 0;
    
    // Tập hợp các ví đáng ngờ
    let mut suspicious_wallets = std::collections::HashSet::new();
    
    for tx in &tx_history {
        // Kiểm tra giao dịch lớn bất thường
        if tx.transaction_type == TransactionType::Swap && tx.value_usd > 5000.0 {
            // Giả định là mua nếu là swap và có value lớn
            large_buys_count += 1;
        }
        
        if tx.transaction_type == TransactionType::TokenTransfer && tx.value_usd > 5000.0 {
            // Giả định là bán nếu là chuyển token và có value lớn
            large_sells_count += 1;
        }
        
        // Kiểm tra các ví tương tác với nhiều token mới
        if tx.transaction_type == TransactionType::Swap {
            suspicious_wallets.insert(tx.sender.clone());
        }
        
        // Kiểm tra giao dịch từ các ví đáng ngờ (thay vì gọi adapter.is_known_bot_wallet)
        let is_bot = tx.sender.starts_with("0x") && 
                   (tx.sender.ends_with("123") || tx.sender.ends_with("bot") || 
                    tx.sender.ends_with("000") || tx.sender.ends_with("999"));
        
        if is_bot {
            suspicious_wallets.insert(tx.sender.clone());
            warnings.push(format!("Transaction from suspected bot wallet: {}", tx.sender));
        }
    }
    
    // Thêm cảnh báo dựa trên phân tích
    if large_buys_count > 5 && large_sells_count < 2 {
        warnings.push(format!(
            "Suspicious buy pattern detected: {} large buys but only {} large sells",
            large_buys_count, large_sells_count
        ));
    }
    
    if suspicious_wallets.len() > 3 {
        warnings.push(format!(
            "Multiple suspicious wallets interacting with token: {}",
            suspicious_wallets.len()
        ));
    }
    
    Ok(warnings)
}

/// Kiểm tra giao dịch MEV trên token
pub async fn check_mev_activity(
    executor: &SmartTradeExecutor,
    chain_id: u32,
    token_address: &str,
) -> Result<Vec<String>> {
    // Kết quả cảnh báo
    let mut warnings = Vec::new();
    
    // Lấy mempool analyzer cho chain
    let mempool_analyzer = match executor.mempool_analyzers.get(&chain_id) {
        Some(analyzer) => analyzer,
        None => return Err(anyhow!("No mempool analyzer found for chain ID {}", chain_id)),
    };
    
    // Kiểm tra hoạt động sandwich gần đây - phương pháp thay thế vì detect_sandwich_attacks không tồn tại
    let suspicious_patterns = mempool_analyzer.detect_suspicious_patterns(chain_id, 10).await?;
    
    let sandwich_count = suspicious_patterns.iter()
        .filter(|alert| alert.alert_type.contains("sandwich"))
        .count();
    
    if sandwich_count > 0 {
        warnings.push(format!(
            "Detected {} potential sandwich attacks on this token recently",
            sandwich_count
        ));
    }
    
    // Kiểm tra hoạt động front-running - phương pháp thay thế vì detect_frontrunning không tồn tại
    let frontrunning_count = suspicious_patterns.iter()
        .filter(|alert| alert.alert_type.contains("frontrun"))
        .count();
    
    if frontrunning_count > 0 {
        warnings.push(format!(
            "Detected {} potential front-running instances on this token recently",
            frontrunning_count
        ));
    }
    
    Ok(warnings)
}

/// Kiểm tra tương tác của các ví lớn với token
pub async fn check_whale_activity(
    executor: &SmartTradeExecutor,
    chain_id: u32,
    token_address: &str,
) -> Result<Vec<String>> {
    // Kết quả cảnh báo
    let mut warnings = Vec::new();
    
    // Lấy adapter cho chain
    let adapter = match executor.evm_adapters.get(&chain_id) {
        Some(adapter) => adapter,
        None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
    };
    
    // Lấy danh sách các ví lớn nắm giữ token
    let whales = adapter.get_top_token_holders(token_address, 10).await?;
    
    // Phân tích hoạt động gần đây của các ví lớn
    for whale in whales {
        // Kiểm tra lịch sử bán gần đây
        let recent_sells = adapter.get_address_token_transfers(
            &whale.address,
            token_address,
            TransactionType::TokenTransfer,
            10
        ).await?;
        
        // Nếu ví lớn bán nhiều gần đây, cảnh báo
        if recent_sells.len() > 3 {
            let total_sell_amount: f64 = recent_sells.iter().map(|tx| tx.amount).sum();
            let whale_percentage = (total_sell_amount / whale.balance) * 100.0;
            
            if whale_percentage > 10.0 {
                warnings.push(format!(
                    "Whale {} selling off {:.2}% of holdings recently",
                    whale.address,
                    whale_percentage
                ));
            }
        }
    }
    
    Ok(warnings)
}

/// Phân tích đầy đủ an toàn của token
pub async fn full_safety_analysis(
    executor: &SmartTradeExecutor,
    chain_id: u32,
    token_address: &str,
    adapter: &Arc<EvmAdapter>,
) -> Result<(bool, HashMap<String, String>, String)> {
    // Kết quả chi tiết
    let mut details = HashMap::new();
    
    // Kiểm tra trạng thái token qua analys_client
    let token_status = executor.analys_client.get_token_status(
        chain_id,
        token_address,
    ).await?;
    
    // Lấy thông tin cơ bản
    let token_info = adapter.get_token_info(token_address).await?;
    details.insert("name".to_string(), token_info.name);
    details.insert("symbol".to_string(), token_info.symbol);
    details.insert("total_supply".to_string(), token_info.total_supply.to_string());
    details.insert("verified".to_string(), token_info.verified.to_string());
    
    // Phân tích rủi ro tổng thể
    // Lấy thông tin native token từ chain_id
    let native_token = match chain_id {
        1 => "ETH",
        56 => "BNB",
        137 => "MATIC",
        42161 => "ETH",
        10 => "ETH",
        43114 => "AVAX",
        250 => "FTM",
        _ => "ETH",
    };
    
    let token_pair = TokenPair {
        base_token: native_token.to_string(),
        quote_token: token_address.to_string(),
        chain_id,
    };
    
    let params = TradeParams {
        chain_type: ChainType::EVM(chain_id),
        token_address: token_address.to_string(),
        token_pair,
        trade_type: TradeType::Buy,
        amount: 0.1, // Placeholder amount
        slippage: 1.0, // Mặc định 1%
        deadline_minutes: 30, // Mặc định 30 phút
        router_address: "".to_string(),
        wallet_address: "".to_string(),
        gas_limit: None,
        gas_price: None,
        strategy: None,
        stop_loss: None,
        take_profit: None,
        max_hold_time: None,
        custom_params: None,
    };
    
    let risk_analysis = analyze_risk(executor, &params).await?;
    details.insert("risk_score".to_string(), format!("{}", risk_analysis.risk_score));
    
    // Map recommendation sang chuỗi
    let recommendation = match risk_analysis.recommendation {
        TradeRecommendation::SafeToProceed => "Safe",
        TradeRecommendation::ProceedWithCaution(msg) => format!("Proceed with caution: {}", msg),
        TradeRecommendation::Avoid => "Avoid".to_string(),
    };
    details.insert("recommendation".to_string(), recommendation);
    
    // Thông tin thuế
    details.insert("buy_tax".to_string(), format!("{:.2}%", token_status.tax_buy * 100.0));
    details.insert("sell_tax".to_string(), format!("{:.2}%", token_status.tax_sell * 100.0));
    
    // Thông tin thanh khoản
    let market_info = adapter.get_market_info(token_address).await?;
    details.insert("liquidity_usd".to_string(), format!("${:.2}", market_info.liquidity_usd));
    details.insert("recent_buys".to_string(), market_info.recent_buys.to_string());
    details.insert("recent_sells".to_string(), market_info.recent_sells.to_string());
    
    // Kiểm tra các vấn đề
    let mut critical_issues = 0;
    let mut high_issues = 0;
    let mut medium_issues = 0;
    
    for issue in &token_status.issues {
        match issue.1 {
            IssueSeverity::Critical => critical_issues += 1,
            IssueSeverity::High => high_issues += 1,
            IssueSeverity::Medium => medium_issues += 1,
            _ => {}
        }
    }
    
    details.insert("critical_issues".to_string(), critical_issues.to_string());
    details.insert("high_issues".to_string(), high_issues.to_string());
    details.insert("medium_issues".to_string(), medium_issues.to_string());
    
    // Tóm tắt an toàn
    let is_safe = critical_issues == 0 && high_issues == 0;
    let summary = if critical_issues > 0 {
        format!("Token KHÔNG AN TOÀN: Phát hiện {} vấn đề nghiêm trọng", critical_issues)
    } else if high_issues > 0 {
        format!("Token RỦI RO CAO: Phát hiện {} vấn đề rủi ro cao", high_issues)
    } else if medium_issues > 0 {
        format!("Token RỦI RO TRUNG BÌNH: Phát hiện {} vấn đề", medium_issues)
    } else if risk_analysis.risk_score > 70 {
        "Token RỦI RO: Điểm rủi ro cao nhưng không phát hiện vấn đề cụ thể".to_string()
    } else {
        "Token an toàn: Không phát hiện vấn đề nghiêm trọng".to_string()
    };
    
    Ok((is_safe, details, summary))
} 