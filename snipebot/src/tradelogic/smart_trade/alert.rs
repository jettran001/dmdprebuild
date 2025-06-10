//! Alert implementation for SmartTradeExecutor
//!
//! This module contains functions for sending alerts, logging trade decisions,
//! and communicating important events to users.

// External imports
use std::sync::Arc;
use std::collections::HashMap;
use serde_json::json;
use tracing::{debug, error, info, warn};
use chrono::Utc;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::notifications::NotificationsManager;

// Module imports
use super::types::{TradeStatus, TradeResult, TokenIssue};
use super::executor::SmartTradeExecutor;

// Thêm definition cho trait DbOperations đơn giản
#[async_trait::async_trait]
pub trait DbOperations: Send + Sync + 'static {
    async fn insert_trade_log(&self, log_entry: &serde_json::Value) -> anyhow::Result<()>;
}

// Cập nhật SmartTradeExecutor để thêm các field cần thiết
impl SmartTradeExecutor {
    // Thêm các phương thức get/set cho notification_manager và trade_log_db
    
    /// Thiết lập notification manager
    pub fn set_notification_manager(&mut self, manager: Arc<NotificationManager>) {
        self.notification_manager = Some(manager);
    }
    
    /// Thiết lập trade log database
    pub fn set_trade_log_db(&mut self, db: Arc<dyn DbOperations>) {
        self.trade_log_db = Some(db);
    }
    
    /// Lấy reference đến notification manager
    pub fn get_notification_manager(&self) -> Option<&Arc<NotificationManager>> {
        self.notification_manager.as_ref()
    }
    
    /// Lấy reference đến trade log database
    pub fn get_trade_log_db(&self) -> Option<&Arc<dyn DbOperations>> {
        self.trade_log_db.as_ref()
    }
    
    /// Lấy thông tin giao dịch theo ID
    pub async fn get_trade_result_by_id(&self, trade_id: &str) -> Option<TradeResult> {
        // Thêm implement để tìm giao dịch theo ID
        self.trade_history.read().await
            .iter()
            .find(|trade| trade.trade_id == trade_id)
            .cloned()
    }

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
                    if let Some(trade) = self.get_trade_result_by_id(trade_id).await {
                        format!(
                            "Kết quả: {} ETH/BNB, {}%", 
                            trade.exit_price.unwrap_or_default(), 
                            trade.profit_percent.unwrap_or_default()
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
    pub async fn fetch_external_token_data(&self, _token_address: &str, _chain_id: &str) -> Option<HashMap<String, String>> {
        // Giả lập việc lấy dữ liệu từ API bên ngoài
        warn!("fetch_external_token_data: Chưa kết nối đến API thực, trả về dữ liệu mẫu");
        
        let mut result = HashMap::new();
        result.insert("name".to_string(), "Sample Token".to_string());
        result.insert("symbol".to_string(), "SMPL".to_string());
        result.insert("price".to_string(), "0.001".to_string());
        result.insert("market_cap".to_string(), "1000000".to_string());
        result.insert("is_scam".to_string(), "false".to_string());
        
        Some(result)
    }
    
    /// Phân tích toàn diện về token
    ///
    /// Thực hiện phân tích sâu về token để phát hiện các vấn đề bảo mật
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - `(bool, Vec<TokenIssue>, String)`: (Có vấn đề không, Danh sách vấn đề, Chi tiết)
    pub async fn comprehensive_analysis(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, Vec<TokenIssue>, String) {
        // Giả lập việc phân tích token
        warn!("comprehensive_analysis: Chưa thực hiện phân tích thực, trả về dữ liệu mẫu");
        
        let mut issues = Vec::new();
        let mut details = String::new();
        
        // Giả lập phát hiện một số vấn đề mẫu
        issues.push(TokenIssue::HighTax);
        details.push_str("- Phát hiện tax bất thường: Buy 10%, Sell 15%\n");
        
        // Kiểm tra thông tin từ API bên ngoài
        if let Some(external_data) = self.fetch_external_token_data(token_address, &chain_id.to_string()).await {
            if external_data.get("is_scam").map_or(false, |v| v == "true") {
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
        // Giả lập việc tạo báo cáo
        warn!("generate_enhanced_report: Chưa thực hiện phân tích thực, trả về báo cáo mẫu");
        
        let mut report = String::new();
        
        // Thông tin cơ bản về token
        report.push_str(&format!("# Báo cáo token: Sample Token (SMPL)\n\n"));
        report.push_str(&format!("- Địa chỉ: {}\n", token_address));
        report.push_str(&format!("- Chain ID: {}\n", chain_id));
        report.push_str(&format!("- Decimals: 18\n"));
        report.push_str(&format!("- Total Supply: 1,000,000,000\n\n"));
        
        // Thông tin giá và thanh khoản
        report.push_str(&format!("## Thông tin thị trường\n\n"));
        report.push_str(&format!("- Giá hiện tại: $0.001\n"));
        report.push_str(&format!("- Thanh khoản: $100,000\n"));
        report.push_str(&format!("- Volume 24h: $50,000\n\n"));
        
        // Kết quả phân tích toàn diện
        let (has_issues, issues, details) = self.comprehensive_analysis(chain_id, token_address, adapter).await;
        
        report.push_str("## Phân tích an toàn\n\n");
        
        if has_issues {
            report.push_str("⚠️ **PHÁT HIỆN VẤN ĐỀ**\n\n");
            report.push_str(details.as_str());
            report.push_str("\n");
        } else {
            report.push_str("✅ Không phát hiện vấn đề\n\n");
        }
        
        // Tính toán score
        let safety_score = if has_issues {
            let num_issues = issues.len() as f64;
            let max_score = 100.0;
            let score = f64::max(max_score - (num_issues * 10.0), 0.0);
            score
        } else {
            95.0 // Không hoàn hảo 100% vì luôn có rủi ro
        };
        
        report.push_str(&format!("\n## Safety Score: {:.1}/100\n", safety_score));
        
        // Khuyến nghị giao dịch
        report.push_str("\n## Khuyến nghị\n\n");
        
        if safety_score < 50.0 {
            report.push_str("❌ Không nên giao dịch token này\n");
        } else if safety_score < 80.0 {
            report.push_str("⚠️ Thận trọng khi giao dịch, có một số rủi ro\n");
        } else {
            report.push_str("✅ Token có vẻ an toàn, nhưng luôn thận trọng\n");
        }
        
        report
    }
}

/// Khởi tạo notification manager dựa trên các cấu hình có sẵn
///
/// # Parameters
/// - `telegram_token`: Token Telegram bot
/// - `telegram_chat_id`: Chat ID Telegram
/// - `discord_webhook`: Webhook URL Discord
///
/// # Returns
/// - `Option<Arc<NotificationManager>>`: Manager đã khởi tạo, hoặc None nếu không có cấu hình
pub async fn init_notification_manager(
    telegram_token: Option<String>,
    telegram_chat_id: Option<String>,
    discord_webhook: Option<String>,
) -> Option<Arc<crate::notifications::NotificationManager>> {
    let mut manager = None;
    
    // Khởi tạo notification manager nếu có cấu hình
    if telegram_token.is_some() || discord_webhook.is_some() {
        debug!("Khởi tạo notification manager");
        
        // Tạo notification manager
        let notification_manager = crate::notifications::NotificationManager::new();
        
        // Thêm Telegram provider nếu có đủ thông tin
        if let (Some(token), Some(chat_id)) = (telegram_token, telegram_chat_id) {
            if !token.is_empty() && !chat_id.is_empty() {
                debug!("Thêm Telegram provider");
                notification_manager.init_telegram(token, chat_id).await;
            }
        }
        
        // Thêm Discord provider nếu có webhook
        if let Some(webhook) = discord_webhook {
            if !webhook.is_empty() {
                debug!("Thêm Discord provider");
                notification_manager.init_discord(webhook).await;
            }
        }
        
        manager = Some(Arc::new(notification_manager));
    }
    
    manager
}
