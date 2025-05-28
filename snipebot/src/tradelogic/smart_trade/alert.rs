/// Alert implementation for SmartTradeExecutor
///
/// This module contains functions for sending alerts, logging trade decisions,
/// and communicating important events to users.

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use chrono::Utc;
use serde_json::json;
use tracing::{debug, error, info, warn};

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::notifications::{NotificationManager, NotificationType};

// Module imports
use super::types::{TradeTracker, TradeStatus, TradeResult, TokenIssue};
use super::constants::*;
use super::executor::SmartTradeExecutor;

impl SmartTradeExecutor {
    /// Gửi cảnh báo đến người dùng
    ///
    /// Gửi thông báo về các sự kiện quan trọng như phát hiện token nguy hiểm,
    /// cơ hội giao dịch tốt, hoặc thông tin thị trường quan trọng.
    ///
    /// # Parameters
    /// - `token_address`: Địa chỉ token
    /// - `alert_type`: Loại cảnh báo
    /// - `message`: Nội dung cảnh báo
    /// - `severity`: Mức độ nghiêm trọng (0-5, 5 là cao nhất)
    /// - `chain_id`: ID của blockchain
    pub async fn send_alert(&self, token_address: &str, alert_type: &str, message: &str, severity: u8, chain_id: u32) {
        // Cố gắng sử dụng notification_manager nếu có
        let mut used_notification_system = false;
        
        // Kiểm tra xem có notification_manager trong SmartTradeExecutor không
        if let Some(notification_manager) = &self.notification_manager {
            // Tạo dữ liệu thông báo
            let alert_data = json!({
                "token_address": token_address,
                "alert_type": alert_type,
                "message": message,
                "severity": severity,
                "chain_id": chain_id,
                "timestamp": Utc::now().timestamp(),
            });
            
            // Xác định loại thông báo dựa vào mức độ nghiêm trọng
            let notification_type = if severity >= 4 {
                NotificationType::CriticalAlert
            } else if severity >= 2 {
                NotificationType::ImportantAlert
            } else {
                NotificationType::InfoAlert
            };
            
            // Gửi thông báo
            if let Err(e) = notification_manager.send_notification(notification_type, &alert_data).await {
                error!("Không thể gửi thông báo: {:?}", e);
            } else {
                debug!("Đã gửi thông báo: {}", message);
                used_notification_system = true;
            }
        }
        
        // Fallback: Nếu không có notification_manager hoặc có lỗi, log ra console
        if !used_notification_system {
            info!("ALERT [{}] {}: {}", severity, alert_type, message);
        }
    }
    
    /// Ghi lại quyết định giao dịch
    ///
    /// Lưu lại lịch sử quyết định giao dịch để phân tích và cải thiện sau này
    ///
    /// # Parameters
    /// - `trade_id`: ID của giao dịch
    /// - `token_address`: Địa chỉ token
    /// - `status`: Trạng thái giao dịch
    /// - `reason`: Lý do quyết định
    /// - `chain_id`: ID của blockchain
    pub async fn log_trade_decision(&self, trade_id: &str, token_address: &str, status: TradeStatus, reason: &str, chain_id: u32) {
        // Log ra console
        info!(
            "Trade Decision [{}]: Token {} - Status: {:?} - Reason: {}",
            trade_id, token_address, status, reason
        );
        
        // Lưu vào database nếu có
        if let Some(db) = &self.trade_log_db {
            let log_entry = json!({
                "trade_id": trade_id,
                "token_address": token_address,
                "status": format!("{:?}", status),
                "reason": reason,
                "chain_id": chain_id,
                "timestamp": Utc::now().timestamp(),
            });
            
            if let Err(e) = db.insert_trade_log(&log_entry).await {
                error!("Không thể lưu quyết định giao dịch: {:?}", e);
            }
        }
        
        // Nếu là quyết định quan trọng, gửi thông báo
        match status {
            TradeStatus::Failed | TradeStatus::Rejected => {
                self.send_alert(
                    token_address, 
                    "trade_failed", 
                    &format!("Giao dịch thất bại: {}", reason), 
                    3, 
                    chain_id
                ).await;
            },
            TradeStatus::Completed => {
                let trade_details = if trade_id != "N/A" {
                    if let Some(trade) = self.get_trade_by_id(trade_id).await {
                        format!(
                            "Kết quả: {} ETH/BNB, {}%", 
                            trade.profit_amount, 
                            trade.profit_percent
                        )
                    } else {
                        "".to_string()
                    }
                } else {
                    "".to_string()
                };
                
                self.send_alert(
                    token_address, 
                    "trade_completed", 
                    &format!("Giao dịch hoàn thành: {} {}", reason, trade_details), 
                    2, 
                    chain_id
                ).await;
            },
            _ => {}
        }
    }
    
    /// Lấy dữ liệu token từ API bên ngoài
    ///
    /// Truy vấn dữ liệu từ các API bên ngoài như CoinGecko, Etherscan, Dextools, v.v.
    ///
    /// # Parameters
    /// - `token_address`: Địa chỉ token
    /// - `chain_id`: ID của blockchain (dạng string)
    ///
    /// # Returns
    /// - `Option<HashMap<String, String>>`: Dữ liệu token từ API bên ngoài
    pub async fn fetch_external_token_data(&self, token_address: &str, chain_id: &str) -> Option<HashMap<String, String>> {
        let mut result = HashMap::new();
        
        // Thử lấy dữ liệu từ nhiều nguồn
        let sources = vec![
            ("coingecko", format!("https://api.coingecko.com/api/v3/coins/{}?contract_address={}", chain_id, token_address)),
            ("dextools", format!("https://www.dextools.io/api/token/{}?chainId={}", token_address, chain_id)),
            ("dexscreener", format!("https://api.dexscreener.com/latest/dex/tokens/{}", token_address)),
        ];
        
        for (source, url) in sources {
            match reqwest::get(&url).await {
                Ok(response) => {
                    if let Ok(json) = response.json::<serde_json::Value>().await {
                        // Xử lý dữ liệu từ mỗi nguồn
                        match source {
                            "coingecko" => {
                                if let Some(market_data) = json.get("market_data") {
                                    // Lấy thông tin market cap
                                    if let Some(market_cap) = market_data.get("market_cap").and_then(|m| m.get("usd")) {
                                        result.insert("market_cap".to_string(), market_cap.to_string());
                                    }
                                    
                                    // Lấy thông tin volume
                                    if let Some(volume) = market_data.get("total_volume").and_then(|v| v.get("usd")) {
                                        result.insert("volume_24h".to_string(), volume.to_string());
                                    }
                                }
                                
                                // Kiểm tra xem có trong danh sách đen không
                                if let Some(tickers) = json.get("tickers") {
                                    if tickers.as_array().map_or(0, |t| t.len()) == 0 {
                                        result.insert("is_blacklisted".to_string(), "true".to_string());
                                    }
                                }
                            },
                            "dextools" => {
                                // Kiểm tra xem có bị đánh dấu scam không
                                if let Some(is_scam) = json.get("isScam") {
                                    if is_scam.as_bool().unwrap_or(false) {
                                        result.insert("is_scam".to_string(), "true".to_string());
                                    }
                                }
                                
                                // Kiểm tra điểm audit
                                if let Some(audit_score) = json.get("auditScore") {
                                    result.insert("audit_score".to_string(), audit_score.to_string());
                                }
                            },
                            "dexscreener" => {
                                if let Some(pairs) = json.get("pairs").and_then(|p| p.as_array()) {
                                    if let Some(first_pair) = pairs.first() {
                                        // Lấy thanh khoản
                                        if let Some(liquidity) = first_pair.get("liquidity").and_then(|l| l.get("usd")) {
                                            result.insert("liquidity".to_string(), liquidity.to_string());
                                        }
                                        
                                        // Lấy số lượng holders
                                        if let Some(holders) = first_pair.get("holders") {
                                            result.insert("holders".to_string(), holders.to_string());
                                        }
                                    }
                                }
                            },
                            _ => {}
                        }
                    }
                },
                Err(e) => {
                    debug!("Không thể lấy dữ liệu từ {}: {:?}", source, e);
                }
            }
        }
        
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }
    
    /// Phân tích tổng hợp token
    ///
    /// Phân tích token toàn diện, kết hợp các phân tích riêng lẻ
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - `(bool, Vec<TokenIssue>, String)`: (có vấn đề, danh sách vấn đề, chi tiết)
    pub async fn comprehensive_analysis(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, Vec<TokenIssue>, String) {
        let mut issues = Vec::new();
        let mut details = String::new();
        
        // 1. Phát hiện honeypot
        let is_honeypot = self.detect_honeypot(chain_id, token_address, adapter).await;
        if is_honeypot {
            issues.push(TokenIssue::Honeypot);
            details.push_str("- Phát hiện honeypot: Token không thể bán\n");
        }
        
        // 2. Phát hiện tax động
        let (is_dynamic_tax, buy_tax, sell_tax) = self.detect_dynamic_tax(chain_id, token_address, adapter).await;
        if is_dynamic_tax {
            issues.push(TokenIssue::DynamicTax);
            details.push_str(&format!("- Phát hiện tax bất thường: Buy {}%, Sell {}%\n", buy_tax, sell_tax));
        }
        
        // 3. Phát hiện blacklist
        let has_blacklist = self.detect_blacklist(chain_id, token_address, adapter).await;
        if has_blacklist {
            issues.push(TokenIssue::Blacklist);
            details.push_str("- Contract có cơ chế blacklist\n");
        }
        
        // 4. Phát hiện proxy contract
        let (is_proxy, owner_can_upgrade) = self.detect_proxy(chain_id, token_address, adapter).await;
        if is_proxy && owner_can_upgrade {
            issues.push(TokenIssue::UpgradeableProxy);
            details.push_str("- Contract có thể nâng cấp logic (proxy)\n");
        }
        
        // 5. Phát hiện external call
        let (has_external_call, dangerous_funcs) = self.detect_external_call(chain_id, token_address, adapter).await;
        if has_external_call {
            issues.push(TokenIssue::ExternalCall);
            details.push_str(&format!("- Contract có external call: {:?}\n", dangerous_funcs));
        }
        
        // 6. Phát hiện owner vẫn có quyền
        let (is_renounced, has_backdoor) = self.is_ownership_renounced(chain_id, token_address, adapter).await;
        if !is_renounced || has_backdoor {
            issues.push(TokenIssue::OwnerPrivilege);
            if has_backdoor {
                details.push_str("- Phát hiện backdoor để lấy lại quyền sau khi renounce\n");
            } else if !is_renounced {
                details.push_str("- Owner chưa từ bỏ quyền\n");
            }
        }
        
        // 7. Phát hiện rủi ro thanh khoản
        let (liquidity_risk, liq_details) = self.detect_liquidity_risk(chain_id, token_address, adapter).await;
        if liquidity_risk {
            issues.push(TokenIssue::LiquidityRisk);
            details.push_str(&format!("- Rủi ro thanh khoản: {}\n", liq_details));
        }
        
        // 8. Phát hiện tax ẩn
        let (has_hidden_fees, actual_tax, declared_tax) = self.detect_hidden_fees(chain_id, token_address, adapter).await;
        if has_hidden_fees {
            issues.push(TokenIssue::HiddenFees);
            details.push_str(&format!(
                "- Phí ẩn: Công bố {}%, thực tế {}%\n", 
                declared_tax, actual_tax
            ));
        }
        
        // 9. Phân tích lịch sử liquidity
        let (is_rugpull_pattern, rugpull_details) = self.analyze_liquidity_events(chain_id, token_address, adapter).await;
        if is_rugpull_pattern {
            issues.push(TokenIssue::RugPullPattern);
            details.push_str(&format!("- Mẫu rugpull: {}\n", rugpull_details));
        }
        
        // Kiểm tra thông tin từ API bên ngoài
        if let Some(external_data) = self.fetch_external_token_data(token_address, &format!("{}", chain_id)).await {
            if external_data.get("is_scam").map_or(false, |v| v == "true") || 
               external_data.get("is_blacklisted").map_or(false, |v| v == "true") {
                issues.push(TokenIssue::ExternalBlacklist);
                details.push_str("- Token bị blacklist bởi API bên ngoài\n");
            }
        }
        
        // Kết luận
        let has_issues = !issues.is_empty();
        
        if has_issues {
            self.send_alert(
                token_address,
                "token_analysis",
                &format!("Phát hiện vấn đề với token: {}", details),
                3,
                chain_id
            ).await;
        }
        
        (has_issues, issues, details)
    }
    
    /// Tạo báo cáo nâng cao về token
    ///
    /// Tạo báo cáo chi tiết về token, bao gồm các phân tích và đánh giá
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - `String`: Báo cáo chi tiết về token
    pub async fn generate_enhanced_report(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> String {
        let mut report = String::new();
        
        // Thông tin cơ bản về token
        if let Ok(token_info) = adapter.get_token_info(token_address).await {
            report.push_str(&format!("# Báo cáo token: {} ({})\n\n", token_info.name, token_info.symbol));
            report.push_str(&format!("- Địa chỉ: {}\n", token_address));
            report.push_str(&format!("- Chain ID: {}\n", chain_id));
            report.push_str(&format!("- Decimals: {}\n", token_info.decimals));
            report.push_str(&format!("- Total Supply: {}\n\n", token_info.total_supply));
        } else {
            report.push_str(&format!("# Báo cáo token: {}\n\n", token_address));
            report.push_str(&format!("- Địa chỉ: {}\n", token_address));
            report.push_str(&format!("- Chain ID: {}\n\n", chain_id));
        }
        
        // Thông tin giá và thanh khoản
        if let Ok(price) = adapter.get_token_price(token_address).await {
            report.push_str(&format!("## Thông tin thị trường\n\n"));
            report.push_str(&format!("- Giá hiện tại: ${:.10}\n", price));
            
            if let Ok(liquidity) = adapter.get_token_liquidity(token_address).await {
                report.push_str(&format!("- Thanh khoản: ${:.2}\n", liquidity));
            }
            
            if let Ok(volume) = adapter.get_token_volume(token_address, 24).await {
                report.push_str(&format!("- Volume 24h: ${:.2}\n\n", volume));
            }
        } else {
            report.push_str("## Thông tin thị trường\n\n");
            report.push_str("- Không lấy được thông tin giá\n\n");
        }
        
        // Kết quả phân tích toàn diện
        let (has_issues, issues, details) = self.comprehensive_analysis(chain_id, token_address, adapter).await;
        
        report.push_str("## Phân tích an toàn\n\n");
        
        if has_issues {
            report.push_str("⚠️ **PHÁT HIỆN VẤN ĐỀ**\n\n");
            report.push_str(&details);
            report.push_str("\n");
        } else {
            report.push_str("✅ Không phát hiện vấn đề\n\n");
        }
        
        // Thông tin chi tiết về contract
        report.push_str("## Chi tiết contract\n\n");
        
        // Kiểm tra owner và quyền hạn
        if let Some(contract_info) = self.get_contract_info(chain_id, token_address, adapter).await {
            if let Some(owner) = &contract_info.owner {
                report.push_str(&format!("- Owner: {}\n", owner));
                
                let (is_renounced, has_backdoor) = self.is_ownership_renounced(chain_id, token_address, adapter).await;
                if is_renounced {
                    report.push_str("- Ownership: Đã từ bỏ ✅\n");
                    
                    if has_backdoor {
                        report.push_str("  ⚠️ Phát hiện backdoor có thể lấy lại quyền\n");
                    }
                } else {
                    report.push_str("- Ownership: Chưa từ bỏ ⚠️\n");
                }
            }
            
            // Thông tin về loại contract
            if let Some(contract_type) = &contract_info.contract_type {
                report.push_str(&format!("- Loại contract: {}\n", contract_type));
            }
            
            // Tax và phí
            let (_, buy_tax, sell_tax) = self.detect_dynamic_tax(chain_id, token_address, adapter).await;
            report.push_str(&format!("- Buy Tax: {:.2}%\n", buy_tax));
            report.push_str(&format!("- Sell Tax: {:.2}%\n", sell_tax));
            
            // Phát hiện anti-bot
            let (has_tx_limit, has_anti_whale, has_cooldown) = self.detect_anti_bot(chain_id, token_address, adapter).await;
            if has_tx_limit || has_anti_whale || has_cooldown {
                report.push_str("- Anti-bot measures:\n");
                if has_tx_limit {
                    report.push_str("  * Giới hạn số lượng token trong mỗi giao dịch\n");
                }
                if has_anti_whale {
                    report.push_str("  * Giới hạn số lượng token trong ví (anti-whale)\n");
                }
                if has_cooldown {
                    report.push_str("  * Giới hạn thời gian giữa các giao dịch\n");
                }
            }
            
            // Tính toán score
            let safety_score = if has_issues {
                let num_issues = issues.len() as f64;
                let max_score = 100.0;
                let score = (max_score - (num_issues * 10.0)).max(0.0);
                score
            } else {
                95.0 // Không hoàn hảo 100% vì luôn có rủi ro
            };
            
            report.push_str(&format!("\n## Safety Score: {:.1}/100\n", safety_score));
            
            // Khuyến nghị giao dịch
            report.push_str("\n## Khuyến nghị\n\n");
            
            if safety_score >= 75.0 {
                report.push_str("✅ AN TOÀN để giao dịch với chiến lược thông minh\n");
            } else if safety_score >= 50.0 {
                report.push_str("⚠️ THẬN TRỌNG - Giao dịch nhanh với slippage cao\n");
            } else {
                report.push_str("❌ NGUY HIỂM - Không nên giao dịch\n");
            }
        } else {
            report.push_str("Không lấy được thông tin contract\n");
        }
        
        report
    }
    
    /// Khởi tạo notification manager
    ///
    /// Tạo và cấu hình notification manager để gửi cảnh báo qua các kênh khác nhau
    ///
    /// # Parameters
    /// - `telegram_token`: Token của bot Telegram
    /// - `telegram_chat_id`: ID của chat Telegram
    /// - `discord_webhook`: Webhook URL của Discord
    ///
    /// # Returns
    /// - `Option<Arc<crate::notifications::NotificationManager>>`: Notification manager đã được cấu hình
    pub async fn init_notification_manager(
        telegram_token: Option<String>,
        telegram_chat_id: Option<String>,
        discord_webhook: Option<String>,
    ) -> Option<Arc<crate::notifications::NotificationManager>> {
        // Tạo notification manager mặc định
        match crate::notifications::NotificationManager::create_default().await {
            Ok(manager) => {
                // Cấu hình Telegram nếu có
                if let (Some(token), Some(chat_id)) = (telegram_token, telegram_chat_id) {
                    if let Err(e) = manager.init_telegram(token, chat_id).await {
                        error!("Không thể khởi tạo kênh Telegram: {:?}", e);
                    } else {
                        info!("Đã khởi tạo kênh thông báo Telegram");
                    }
                }
                
                // Cấu hình Discord nếu có
                if let Some(webhook) = discord_webhook {
                    if let Err(e) = manager.init_discord(webhook).await {
                        error!("Không thể khởi tạo kênh Discord: {:?}", e);
                    } else {
                        info!("Đã khởi tạo kênh thông báo Discord");
                    }
                }
                
                Some(manager)
            },
            Err(e) => {
                error!("Không thể tạo notification manager: {:?}", e);
                None
            }
        }
    }
}
