//! Risk Manager - Quản lý rủi ro
//!
//! Module này chứa logic đánh giá rủi ro, phân tích token an toàn,
//! và cung cấp các API kiểm tra an toàn giao dịch.

use std::sync::Arc;
use std::collections::HashMap;
use anyhow::Result;
use tracing::debug;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::{TokenPair, TradeParams, TradeType};

// Module imports
use crate::tradelogic::smart_trade::executor::core::SmartTradeExecutor;

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
            TokenIssue::ContractNotVerified => {
                if config.block_unverified_contracts {
                    issues.push("Critical: Contract not verified".to_string());
                }
            },
            TokenIssue::HoneypotDetected => {
                issues.push("Critical: Honeypot detected".to_string());
            },
            TokenIssue::HighSellTax => {
                issues.push(format!("Critical: High sell tax detected ({}%)", token_status.tax_sell));
            },
            TokenIssue::BlacklistDetected => {
                issues.push("Critical: Blacklist function detected".to_string());
            },
            TokenIssue::SuspiciousOwner => {
                issues.push("High: Suspicious owner activity detected".to_string());
            },
            TokenIssue::LockSellDetected => {
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
        risk_factors.push(RiskFactor::Critical("Token is a honeypot".to_string()));
    }
    
    if token_status.tax_sell > 10.0 {
        risk_factors.push(RiskFactor::High(format!("High sell tax: {}%", token_status.tax_sell)));
    } else if token_status.tax_sell > 5.0 {
        risk_factors.push(RiskFactor::Medium(format!("Medium sell tax: {}%", token_status.tax_sell)));
    }
    
    if token_status.blacklist {
        risk_factors.push(RiskFactor::High("Blacklist function detected".to_string()));
    }
    
    if token_status.lock_sell {
        risk_factors.push(RiskFactor::Critical("Lock sell function detected".to_string()));
    }
    
    if token_status.low_liquidity {
        risk_factors.push(RiskFactor::Medium("Low liquidity".to_string()));
    }
    
    // Tính điểm rủi ro dựa trên các risk factors
    let mut risk_score = 0;
    for factor in &risk_factors {
        match factor {
            RiskFactor::Critical(_) => risk_score += 30,
            RiskFactor::High(_) => risk_score += 20,
            RiskFactor::Medium(_) => risk_score += 10,
            RiskFactor::Low(_) => risk_score += 5,
        }
    }
    
    // Đề xuất dựa trên điểm rủi ro
    let recommendation = if risk_score >= 30 {
        TradeRecommendation::Avoid
    } else if risk_score >= 15 {
        TradeRecommendation::ProceedWithCaution("Proceed with small position".to_string())
    } else {
        TradeRecommendation::SafeToProceed
    };
    
    Ok(TradeRiskAnalysis {
        risk_score: risk_score.min(100) as u8,
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
    
    // Lấy risk threshold từ cấu hình cho chain này
    let risk_thresholds = match config.market_risk_thresholds.get(&chain_id) {
        Some(thresholds) => thresholds,
        None => &config.default_market_risk_thresholds,
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
    let market_risk_factor = config.market_risk_factor.get(&chain_id).unwrap_or(&1.0);
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
        if tx.sender_new_token_interaction_count > 10 {
            suspicious_wallets.insert(tx.sender.clone());
        }
        
        // Kiểm tra giao dịch từ các ví bot đã biết
        if adapter.is_known_bot_wallet(&tx.sender).await? {
            suspicious_wallets.insert(tx.sender.clone());
            warnings.push(format!("Transaction from known bot wallet: {}", tx.sender));
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
    
    // Kiểm tra hoạt động sandwich gần đây
    let sandwiches = mempool_analyzer.detect_sandwich_attacks(token_address).await?;
    
    if !sandwiches.is_empty() {
        warnings.push(format!(
            "Detected {} sandwich attacks on this token recently",
            sandwiches.len()
        ));
    }
    
    // Kiểm tra hoạt động front-running
    let frontrunning = mempool_analyzer.detect_frontrunning(token_address).await?;
    
    if !frontrunning.is_empty() {
        warnings.push(format!(
            "Detected {} front-running instances on this token recently",
            frontrunning.len()
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
    let token_pair = TokenPair {
        base_token_address: "".to_string(), // Placeholder
        token_address: token_address.to_string(),
    };
    
    let params = TradeParams {
        chain_id,
        token_pair,
        trade_type: TradeType::Buy,
        amount: 0.1, // Placeholder amount
        slippage: None,
        deadline_minutes: None,
        target_profit: None,
        stop_loss: None,
        wallet_address: "".to_string(),
    };
    
    let risk_analysis = analyze_risk(executor, &params).await?;
    details.insert("risk_score".to_string(), format!("{:.1}", risk_analysis.risk_score));
    
    // Map recommendation sang chuỗi
    let recommendation = match risk_analysis.recommendation {
        TradeRecommendation::Safe => "Safe",
        TradeRecommendation::Proceed => "Proceed with caution",
        TradeRecommendation::Risky => "Risky",
        TradeRecommendation::HighRisk => "High risk",
        TradeRecommendation::Avoid => "Avoid",
    };
    details.insert("recommendation".to_string(), recommendation.to_string());
    
    // Thông tin thuế
    details.insert("buy_tax".to_string(), format!("{:.2}%", token_status.buy_tax * 100.0));
    details.insert("sell_tax".to_string(), format!("{:.2}%", token_status.sell_tax * 100.0));
    
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
        match issue.severity {
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
    } else if risk_analysis.risk_score > 70.0 {
        "Token RỦI RO: Điểm rủi ro cao nhưng không phát hiện vấn đề cụ thể".to_string()
    } else {
        "Token an toàn: Không phát hiện vấn đề nghiêm trọng".to_string()
    };
    
    Ok((is_safe, details, summary))
} 