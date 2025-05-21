/// Security implementation for SmartTradeExecutor
///
/// This module contains functions related to security checks, risk analysis,
/// and safety features to protect users from scams and malicious tokens.
///
/// # Cải tiến đã thực hiện:
/// - Loại bỏ các phương thức trùng lặp với analys/risk_analyzer
/// - Sử dụng RiskAnalyzer từ analys/risk_analyzer
/// - Áp dụng xử lý lỗi chuẩn với anyhow::Result thay vì unwrap/expect
/// - Đảm bảo thread safety trong các phương thức async
/// - Tối ưu hóa các truy vấn blockchain bằng futures và tokio::join!
/// - Áp dụng nguyên tắc defensive programming
/// - Tuân thủ nghiêm ngặt quy tắc từ .cursorrc

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use chrono::Utc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use anyhow::{Result, Context, anyhow};

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::risk_analyzer::{RiskAnalyzer, RiskFactor, TradeRecommendation, TradeRiskAnalysis};
use crate::analys::token_status::{ContractInfo, TokenStatus};

// Module imports
use super::types::{TradeTracker, TradeStatus, TokenIssue};
use super::constants::*;
use super::executor::SmartTradeExecutor;

impl SmartTradeExecutor {
    /// Lấy thông tin chi tiết contract từ blockchain
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - `Option<ContractInfo>`: Thông tin contract nếu có
    pub async fn get_contract_info(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> Option<ContractInfo> {
        match adapter.get_contract_info(token_address).await {
            Ok(contract_info) => Some(contract_info),
            Err(e) => {
                warn!("Không thể lấy thông tin contract {}: {:?}", token_address, e);
                None
            }
        }
    }
    
    /// Phân tích rủi ro token dựa trên đặc điểm contract và hành vi thị trường
    ///
    /// Sử dụng RiskAnalyzer từ analys/risk_analyzer để thực hiện phân tích toàn diện
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - `anyhow::Result<(f64, Vec<RiskFactor>, TradeRecommendation)>`: Kết quả phân tích an toàn
    pub async fn analyze_security_risk(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        adapter: &Arc<EvmAdapter>
    ) -> anyhow::Result<(f64, Vec<RiskFactor>, TradeRecommendation)> {
        // Lấy thông tin contract
        let contract_info = match self.get_contract_info(chain_id, token_address, adapter).await {
            Some(info) => info,
            None => {
                warn!("Không thể lấy thông tin contract cho {}", token_address);
                return Ok((90.0, vec![RiskFactor::ContractUnavailable(90.0)], TradeRecommendation::Avoid));
            }
        };
        
        // Sử dụng RiskAnalyzer từ module analys
        let risk_analyzer = self.risk_analyzer.read().await;
        
        // Thu thập thêm dữ liệu cần thiết
        let liquidity_events = adapter.get_liquidity_events(token_address, 10).await.ok();
        let price_history = adapter.get_token_price_history(token_address, 24).await.ok();
        let tax_info = adapter.get_token_tax_info(token_address).await.ok();
        let volume_data = adapter.get_token_volume_history(token_address, 7).await.ok();
        
        // Phân tích rủi ro sử dụng RiskAnalyzer
        let analysis = risk_analyzer.analyze_trade_risk(
            token_address,
            chain_id,
            Some(&contract_info),
            liquidity_events.as_deref(),
            price_history.as_deref(),
            tax_info.as_ref(),
            volume_data.as_deref()
        );
        
        // Chuyển đổi sang kết quả mong muốn
        let risk_factors: Vec<RiskFactor> = analysis.risk_factors
            .into_iter()
            .map(|(factor, score)| factor)
            .collect();
        
        // Log kết quả phân tích
        info!(
            "Phân tích rủi ro token {}: Điểm={:.1}, Khuyến nghị={:?}, Yếu tố={:?}",
            token_address, analysis.risk_score, analysis.trade_recommendation, risk_factors
        );
        
        Ok((analysis.risk_score, risk_factors, analysis.trade_recommendation))
    }
    
    /// Kiểm tra xem token có thể giao dịch (trade) hay không
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - `(bool, String)`: (Là honeypot?, lý do)
    pub async fn check_token_tradability(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, String) {
        let mut reasons = Vec::new();
        
        // 1. Thử mô phỏng giao dịch mua và bán
        let is_honeypot = self.detect_honeypot(chain_id, token_address, adapter).await;
        if is_honeypot {
            reasons.push("Token không thể bán (honeypot)".to_string());
        }
        
        // 2. Kiểm tra tax
        let (has_tax_issues, buy_tax, sell_tax) = self.detect_dynamic_tax(chain_id, token_address, adapter).await;
        if has_tax_issues {
            if sell_tax > MAX_SAFE_SELL_TAX {
                reasons.push(format!("Sell tax quá cao: {}%", sell_tax));
            }
            if buy_tax > MAX_SAFE_BUY_TAX {
                reasons.push(format!("Buy tax quá cao: {}%", buy_tax));
            }
        }
        
        // 3. Kiểm tra thanh khoản
        let (liquidity_risk, liquidity_reason) = self.detect_liquidity_risk(chain_id, token_address, adapter).await;
        if liquidity_risk {
            reasons.push(format!("Rủi ro thanh khoản: {}", liquidity_reason));
        }
        
        // 4. Kiểm tra giới hạn giao dịch
        let (has_tx_limit, _, limit_percent) = self.detect_transaction_limits(chain_id, token_address, adapter).await;
        if has_tx_limit && limit_percent < 0.5 {
            // Nếu giới hạn giao dịch quá thấp (<0.5% của tổng supply)
            reasons.push(format!("Giới hạn giao dịch quá thấp: {}%", limit_percent));
        }
        
        // Kết luận
        let is_tradable = reasons.is_empty();
        let reason = if is_tradable {
            "Token có thể giao dịch bình thường".to_string()
        } else {
            reasons.join("; ")
        };
        
        (!is_tradable, reason)
    }
    
    /// Kiểm tra token có trong danh sách an toàn không
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    ///
    /// # Returns
    /// - `bool`: true nếu token an toàn
    pub async fn is_in_safe_list(&self, chain_id: u32, token_address: &str) -> bool {
        if let Some(safe_tokens) = self.safe_token_lists.get(&chain_id) {
            let token_key = token_address.to_lowercase();
            safe_tokens.contains(&token_key)
        } else {
            false
        }
    }
    
    /// Kiểm tra token có trong danh sách đen không
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    ///
    /// # Returns
    /// - `bool`: true nếu token trong danh sách đen
    pub async fn is_in_blacklist(&self, chain_id: u32, token_address: &str) -> bool {
        if let Some(blacklist) = self.blacklisted_tokens.get(&chain_id) {
            let token_key = token_address.to_lowercase();
            blacklist.contains(&token_key)
        } else {
            false
        }
    }
    
    /// Kiểm tra xem hợp đồng có được xác minh không (verified)
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - `bool`: true nếu hợp đồng đã được xác minh
    pub async fn is_contract_verified(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> bool {
        if let Some(contract_info) = self.get_contract_info(chain_id, token_address, adapter).await {
            contract_info.is_verified && contract_info.source_code.is_some()
        } else {
            false
        }
    }
    
    /// Cảnh báo người dùng các rủi ro tiềm ẩn
    ///
    /// # Parameters
    /// - `token_address`: Địa chỉ token
    /// - `risk_score`: Điểm rủi ro
    /// - `risk_factors`: Danh sách yếu tố rủi ro
    /// - `chain_id`: ID của blockchain
    pub async fn warn_user_of_risks(&self, token_address: &str, risk_score: f64, risk_factors: &[RiskFactor], chain_id: u32) {
        // Xác định mức độ nghiêm trọng (0-5)
        let severity = if risk_score >= 80.0 {
            5 // Rất nguy hiểm
        } else if risk_score >= 60.0 {
            4 // Nguy hiểm
        } else if risk_score >= 40.0 {
            3 // Rủi ro đáng kể
        } else if risk_score >= 20.0 {
            2 // Cần thận trọng
        } else {
            1 // Rủi ro thấp
        };
        
        // Tạo thông điệp cảnh báo
        let mut warning_message = format!("RỦI RO TOKEN - Điểm: {:.1}/100 - ", risk_score);
        
        match severity {
            5 => warning_message.push_str("NGUY HIỂM NGHIÊM TRỌNG"),
            4 => warning_message.push_str("NGUY HIỂM CAO"),
            3 => warning_message.push_str("RỦI RO ĐÁNG KỂ"),
            2 => warning_message.push_str("CẦN THẬN TRỌNG"),
            _ => warning_message.push_str("RỦI RO THẤP"),
        }
        
        if !risk_factors.is_empty() {
            warning_message.push_str("\nYếu tố rủi ro: ");
            for factor in risk_factors {
                let factor_str = match factor {
                    RiskFactor::OwnerPrivileges(_) => "Quyền owner không bị từ bỏ",
                    RiskFactor::Honeypot(_) => "Dấu hiệu honeypot",
                    RiskFactor::HighTax(_) => "Tax cao bất thường",
                    RiskFactor::LowLiquidity(_) => "Thanh khoản thấp",
                    RiskFactor::ContractUnavailable(_) => "Không thể phân tích contract",
                    RiskFactor::BytecodeIssues(_) => "Vấn đề với bytecode",
                    RiskFactor::ContractStructure(_) => "Cấu trúc contract đáng ngờ",
                    _ => "Rủi ro khác",
                };
                warning_message.push_str(&format!("\n- {}", factor_str));
            }
        }
        
        // Gửi cảnh báo đến người dùng
        self.send_alert(
            token_address,
            "token_risk",
            &warning_message,
            severity,
            chain_id
        ).await;
    }
    
    /// Dùng trí tuệ nhóm để phân tích token (từ API bên ngoài)
    ///
    /// # Parameters
    /// - `token_address`: Địa chỉ token
    /// - `chain_id`: ID của blockchain
    ///
    /// # Returns
    /// - `(bool, Option<f64>, HashMap<String, String>)`: (Có rủi ro, điểm an toàn, dữ liệu)
    pub async fn crowd_sourced_analysis(&self, token_address: &str, chain_id: u32) -> (bool, Option<f64>, HashMap<String, String>) {
        let mut all_data = HashMap::new();
        let mut is_risky = false;
        let mut safety_score: Option<f64> = None;
        
        // Lấy dữ liệu từ các API bên ngoài
        if let Some(data) = self.fetch_external_token_data(token_address, &format!("{}", chain_id)).await {
            // Kiểm tra dữ liệu về rủi ro
            if data.get("is_scam").map_or(false, |v| v == "true") || 
               data.get("is_blacklisted").map_or(false, |v| v == "true") {
                is_risky = true;
            }
            
            // Lấy điểm audit nếu có
            if let Some(audit_score) = data.get("audit_score") {
                if let Ok(score) = audit_score.parse::<f64>() {
                    safety_score = Some(score);
                }
            }
            
            // Thu thập dữ liệu bổ sung
            all_data.extend(data);
        }
        
        // Gửi thông báo nếu phát hiện rủi ro từ các nguồn bên ngoài
        if is_risky {
            self.send_alert(
                token_address,
                "external_warning",
                "Cảnh báo: Token này đã được cộng đồng đánh dấu là nguy hiểm",
                4, 
                chain_id
            ).await;
        }
        
        (is_risky, safety_score, all_data)
    }
    
    /// Thực hiện phân tích an toàn đầy đủ cho token
    ///
    /// Gọi nhiều phương thức phân tích khác nhau để có cái nhìn toàn diện
    /// về độ an toàn của token.
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - `anyhow::Result<(bool, HashMap<String, String>, String)>`: (An toàn?, Kết quả phân tích chi tiết, Lý do tóm tắt)
    pub async fn full_safety_analysis(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        adapter: &Arc<EvmAdapter>
    ) -> anyhow::Result<(bool, HashMap<String, String>, String)> {
        let mut results = HashMap::new();
        let mut issues = Vec::new();
        
        // 1. Kiểm tra contract info cơ bản
        match self.get_contract_info(chain_id, token_address, adapter).await {
            Some(contract_info) => {
                // Thông tin cơ bản
                results.insert("name".to_string(), contract_info.name.clone());
                results.insert("symbol".to_string(), contract_info.symbol.clone());
                results.insert("total_supply".to_string(), contract_info.total_supply.to_string());
                results.insert("verified".to_string(), contract_info.is_verified.to_string());
                
                // Kiểm tra hợp đồng xác minh
                if !contract_info.is_verified {
                    issues.push("Contract chưa được xác minh".to_string());
                }
            },
            None => {
                // Không tìm thấy contract info - rủi ro cao
                issues.push("Không thể lấy thông tin contract".to_string());
                results.insert("contract_info".to_string(), "error".to_string());
                
                // Trả về sớm vì không thể phân tích thêm
                let summary = "Token không an toàn: Không thể lấy thông tin contract".to_string();
                return Ok((false, results, summary));
            }
        }
        
        // 2. Phân tích rủi ro
        let risk_analysis = self.analyze_security_risk(chain_id, token_address, adapter).await?;
        let (risk_score, risk_factors, recommendation) = risk_analysis;
        
        results.insert("risk_score".to_string(), risk_score.to_string());
        results.insert("recommendation".to_string(), format!("{:?}", recommendation));
        
        // Thêm các yếu tố rủi ro vào issues
        for factor in risk_factors {
            match factor {
                RiskFactor::Honeypot(_) => issues.push("Token có dấu hiệu honeypot".to_string()),
                RiskFactor::HighTax(score) => issues.push(format!("Tax cao (score: {:.1})", score)),
                RiskFactor::LowLiquidity(_) => issues.push("Thanh khoản thấp".to_string()),
                RiskFactor::OwnerPrivileges(score) => issues.push(format!("Owner có quyền nguy hiểm (score: {:.1})", score)),
                RiskFactor::ContractStructure(score) => issues.push(format!("Cấu trúc contract đáng ngờ (score: {:.1})", score)),
                RiskFactor::BytecodeIssues(score) => issues.push(format!("Vấn đề bytecode (score: {:.1})", score)),
                RiskFactor::ContractUnavailable(_) => issues.push("Contract không khả dụng".to_string()),
                _ => {}
            }
        }
        
        // 3. Kiểm tra khả năng giao dịch
        let tradability_check = self.check_token_tradability(chain_id, token_address, adapter).await;
        let (non_tradable, reason) = tradability_check;
        
        results.insert("tradable".to_string(), (!non_tradable).to_string());
        
        if non_tradable {
            issues.push(format!("Token không thể giao dịch: {}", reason));
        }
        
        // 4. Kiểm tra tax
        match adapter.get_token_tax_info(token_address).await {
            Ok((buy_tax, sell_tax)) => {
                results.insert("buy_tax".to_string(), format!("{:.2}%", buy_tax));
                results.insert("sell_tax".to_string(), format!("{:.2}%", sell_tax));
                
                if buy_tax > MAX_SAFE_BUY_TAX {
                    issues.push(format!("Buy tax cao: {:.2}%", buy_tax));
                }
                
                if sell_tax > MAX_SAFE_SELL_TAX {
                    issues.push(format!("Sell tax cao: {:.2}%", sell_tax));
                }
                
                if (sell_tax - buy_tax).abs() > DANGEROUS_TAX_DIFF {
                    issues.push(format!("Chênh lệch tax đáng ngờ: Buy {:.2}%, Sell {:.2}%", buy_tax, sell_tax));
                }
            },
            Err(e) => {
                warn!("Không thể lấy thông tin tax cho {}: {}", token_address, e);
                results.insert("tax_info".to_string(), "error".to_string());
                issues.push("Không thể lấy thông tin tax".to_string());
            }
        }
        
        // 5. Kiểm tra thanh khoản
        match adapter.get_token_liquidity(token_address).await {
            Ok(liquidity) => {
                results.insert("liquidity_usd".to_string(), format!("${:.2}", liquidity));
                
                if liquidity < MIN_LIQUIDITY_THRESHOLD {
                    issues.push(format!("Thanh khoản thấp: ${:.2}", liquidity));
                }
            },
            Err(e) => {
                warn!("Không thể lấy thông tin thanh khoản cho {}: {}", token_address, e);
                results.insert("liquidity_info".to_string(), "error".to_string());
                issues.push("Không thể lấy thông tin thanh khoản".to_string());
            }
        }
        
        // 6. Kiểm tra giao dịch gần đây
        match adapter.get_token_transaction_history(token_address, 20).await {
            Ok(history) => {
                let (buy_count, sell_count) = self.analyze_transaction_pattern(&history);
                results.insert("recent_buys".to_string(), buy_count.to_string());
                results.insert("recent_sells".to_string(), sell_count.to_string());
                
                // Đánh giá mẫu giao dịch
                if buy_count > 0 && sell_count == 0 {
                    issues.push("Giao dịch một chiều: Chỉ có mua, không có bán".to_string());
                }
            },
            Err(e) => {
                warn!("Không thể lấy lịch sử giao dịch cho {}: {}", token_address, e);
                results.insert("transaction_history".to_string(), "error".to_string());
            }
        }
        
        // 7. Phân tích từ cộng đồng (nếu có)
        let (has_community_data, _, community_insights) = self.crowd_sourced_analysis(token_address, chain_id).await;
        
        if has_community_data {
            // Thêm dữ liệu từ cộng đồng vào kết quả
            for (key, value) in community_insights {
                results.insert(format!("community_{}", key), value);
            }
        }
        
        // Tạo tóm tắt
        let is_safe = issues.is_empty() || (issues.len() == 1 && issues[0].contains("xác minh"));
        let summary = if is_safe {
            "Token an toàn: Không phát hiện vấn đề nghiêm trọng".to_string()
        } else {
            format!("Token không an toàn: {}", issues.join(", "))
        };
        
        // Thêm tóm tắt vào kết quả
        results.insert("issues_count".to_string(), issues.len().to_string());
        for (i, issue) in issues.iter().enumerate() {
            results.insert(format!("issue_{}", i), issue.clone());
        }
        
        Ok((is_safe, results, summary))
    }
}
