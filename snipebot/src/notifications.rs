/// Module quản lý và gửi thông báo cho snipebot
///
/// Module này cung cấp các công cụ để gửi thông báo qua nhiều kênh khác nhau
/// như Telegram, Discord, Email, v.v. Các notification được cấu hình và quản lý
/// thông qua một NotificationManager tập trung.

use std::collections::HashMap;
use std::sync::Arc;
use anyhow::{Result, Context, anyhow};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value as JsonValue;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use reqwest::Client;
use std::time::Duration;

/// Các loại thông báo hỗ trợ
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NotificationType {
    /// Thông báo chung
    Info,
    /// Thông báo khẩn cấp
    CriticalAlert,
    /// Thông báo quan trọng
    ImportantAlert,
    /// Thông báo thông tin
    InfoAlert,
    /// Thông báo giao dịch
    TradeNotification,
    /// Thông báo lỗi
    ErrorNotification,
}

/// Kết quả gửi thông báo
#[derive(Debug, Clone)]
pub struct NotificationResult {
    /// Thành công hay không
    pub success: bool,
    /// Thông báo lỗi (nếu có)
    pub error_message: Option<String>,
    /// Thời gian gửi
    pub timestamp: i64,
    /// Loại thông báo
    pub notification_type: NotificationType,
    /// Kênh gửi
    pub channel: String,
}

/// Interface cho các kênh thông báo
#[async_trait]
pub trait NotificationChannel: Send + Sync + 'static {
    /// Tên kênh thông báo
    fn name(&self) -> &str;
    
    /// Kiểm tra kênh thông báo có sẵn sàng không
    async fn is_available(&self) -> bool;
    
    /// Gửi thông báo
    async fn send(&self, notification_type: NotificationType, data: &JsonValue) -> Result<NotificationResult>;
    
    /// Cập nhật cấu hình kênh thông báo
    async fn update_config(&mut self, config: JsonValue) -> Result<()>;
}

/// Kênh thông báo Telegram
pub struct TelegramChannel {
    /// Bot token
    token: String,
    /// Chat ID
    chat_id: String,
    /// HTTP client
    client: Client,
    /// Có sẵn sàng không
    is_ready: bool,
}

impl TelegramChannel {
    /// Tạo mới kênh thông báo Telegram
    pub fn new(token: String, chat_id: String) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| {
                warn!("Không thể tạo HTTP client cho Telegram với cấu hình tùy chỉnh: {}. Sử dụng client mặc định.", e);
                e
            })
            .unwrap_or_else(|_| {
                warn!("Đang sử dụng HTTP client mặc định cho Telegram, có thể bị hạn chế tính năng.");
                Client::new()
            });
        
        Ok(Self {
            token,
            chat_id,
            client,
            is_ready: true,
        })
    }
    
    /// Định dạng tin nhắn theo loại thông báo
    fn format_message(&self, notification_type: &NotificationType, data: &JsonValue) -> String {
        let icon = match notification_type {
            NotificationType::CriticalAlert => "🚨",
            NotificationType::ImportantAlert => "⚠️",
            NotificationType::InfoAlert => "ℹ️",
            NotificationType::TradeNotification => "💰",
            NotificationType::ErrorNotification => "❌",
            NotificationType::Info => "📌",
        };
        
        let mut formatted = String::new();
        
        // Thêm tiêu đề
        if let Some(alert_type) = data.get("alert_type").and_then(|v| v.as_str()) {
            formatted.push_str(&format!("{} *{}*\n\n", icon, alert_type.to_uppercase()));
        } else {
            formatted.push_str(&format!("{} *{}*\n\n", icon, format!("{:?}", notification_type)));
        }
        
        // Thêm tin nhắn chính
        if let Some(message) = data.get("message").and_then(|v| v.as_str()) {
            formatted.push_str(message);
            formatted.push_str("\n\n");
        }
        
        // Thêm thông tin token nếu có
        if let Some(token_address) = data.get("token_address").and_then(|v| v.as_str()) {
            formatted.push_str(&format!("Token: `{}`\n", token_address));
        }
        
        // Thêm thông tin chain nếu có
        if let Some(chain_id) = data.get("chain_id") {
            formatted.push_str(&format!("Chain ID: {}\n", chain_id));
        }
        
        // Thêm timestamp
        if let Some(timestamp) = data.get("timestamp").and_then(|v| v.as_i64()) {
            let datetime = chrono::DateTime::from_timestamp(timestamp, 0)
                .unwrap_or_else(|| {
                    chrono::DateTime::from_timestamp(0, 0)
                        .expect("Fallback timestamp (0, 0) should always be valid")
                });
            formatted.push_str(&format!("Time: {}\n", datetime.format("%Y-%m-%d %H:%M:%S UTC")));
        } else {
            let now = Utc::now();
            formatted.push_str(&format!("Time: {}\n", now.format("%Y-%m-%d %H:%M:%S UTC")));
        }
        
        formatted
    }
}

#[async_trait]
impl NotificationChannel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }
    
    async fn is_available(&self) -> bool {
        self.is_ready
    }
    
    async fn send(&self, notification_type: NotificationType, data: &JsonValue) -> Result<NotificationResult> {
        if !self.is_available().await {
            return Err(anyhow!("Telegram channel not available"));
        }
        
        let message = self.format_message(&notification_type, data);
        let api_url = format!("https://api.telegram.org/bot{}/sendMessage", self.token);
        
        let params = serde_json::json!({
            "chat_id": self.chat_id,
            "text": message,
            "parse_mode": "Markdown"
        });
        
        match self.client.post(&api_url).json(&params).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    debug!("Telegram notification sent successfully");
                    Ok(NotificationResult {
                        success: true,
                        error_message: None,
                        timestamp: Utc::now().timestamp(),
                        notification_type: notification_type.clone(),
                        channel: self.name().to_string(),
                    })
                } else {
                    let error = format!("Failed to send Telegram notification: HTTP {}", response.status());
                    error!("{}", error);
                    Ok(NotificationResult {
                        success: false,
                        error_message: Some(error),
                        timestamp: Utc::now().timestamp(),
                        notification_type: notification_type.clone(),
                        channel: self.name().to_string(),
                    })
                }
            },
            Err(e) => {
                let error = format!("Failed to send Telegram notification: {}", e);
                error!("{}", error);
                Ok(NotificationResult {
                    success: false,
                    error_message: Some(error),
                    timestamp: Utc::now().timestamp(),
                    notification_type: notification_type.clone(),
                    channel: self.name().to_string(),
                })
            }
        }
    }
    
    async fn update_config(&mut self, config: JsonValue) -> Result<()> {
        if let Some(token) = config.get("token").and_then(|v| v.as_str()) {
            self.token = token.to_string();
        }
        
        if let Some(chat_id) = config.get("chat_id").and_then(|v| v.as_str()) {
            self.chat_id = chat_id.to_string();
        }
        
        Ok(())
    }
}

/// Kênh thông báo Discord
pub struct DiscordChannel {
    /// Webhook URL
    webhook_url: String,
    /// HTTP client
    client: Client,
    /// Có sẵn sàng không
    is_ready: bool,
}

impl DiscordChannel {
    /// Tạo mới kênh thông báo Discord
    pub fn new(webhook_url: String) -> Self {
        let client = match Client::builder().timeout(Duration::from_secs(10)).build() {
            Ok(client) => client,
            Err(e) => {
                error!("Failed to build HTTP client for Discord notifications: {}", e);
                Client::new()
            }
        };
        
        Self {
            webhook_url,
            client,
            is_ready: true,
        }
    }
    
    /// Định dạng tin nhắn theo loại thông báo
    fn create_discord_embed(&self, notification_type: &NotificationType, data: &JsonValue) -> JsonValue {
        let color = match notification_type {
            NotificationType::CriticalAlert => 16711680, // Red
            NotificationType::ImportantAlert => 16761600, // Orange
            NotificationType::InfoAlert => 65280,     // Green
            NotificationType::TradeNotification => 5793266, // Blue
            NotificationType::ErrorNotification => 10038562, // Purple
            NotificationType::Info => 7506394,    // Light Blue
        };
        
        let mut title = match notification_type {
            NotificationType::CriticalAlert => "CRITICAL ALERT",
            NotificationType::ImportantAlert => "IMPORTANT ALERT",
            NotificationType::InfoAlert => "INFO ALERT",
            NotificationType::TradeNotification => "TRADE NOTIFICATION",
            NotificationType::ErrorNotification => "ERROR NOTIFICATION",
            NotificationType::Info => "INFORMATION",
        }.to_string();
        
        // Use alert_type if available
        if let Some(alert_type) = data.get("alert_type").and_then(|v| v.as_str()) {
            title = alert_type.to_uppercase();
        }
        
        let mut fields = Vec::new();
        
        // Add token field if available
        if let Some(token_address) = data.get("token_address").and_then(|v| v.as_str()) {
            fields.push(serde_json::json!({
                "name": "Token",
                "value": format!("`{}`", token_address),
                "inline": true
            }));
        }
        
        // Add chain field if available
        if let Some(chain_id) = data.get("chain_id") {
            fields.push(serde_json::json!({
                "name": "Chain ID",
                "value": chain_id.to_string(),
                "inline": true
            }));
        }
        
        // Add severity field if available
        if let Some(severity) = data.get("severity") {
            fields.push(serde_json::json!({
                "name": "Severity",
                "value": severity.to_string(),
                "inline": true
            }));
        }
        
        // Create timestamp
        let timestamp = if let Some(ts) = data.get("timestamp").and_then(|v| v.as_i64()) {
            chrono::DateTime::from_timestamp(ts, 0)
                .unwrap_or_else(|| Utc::now())
                .to_rfc3339()
        } else {
            Utc::now().to_rfc3339()
        };
        
        serde_json::json!({
            "embeds": [{
                "title": title,
                "description": data.get("message").and_then(|v| v.as_str()).unwrap_or("No message provided"),
                "color": color,
                "fields": fields,
                "timestamp": timestamp
            }]
        })
    }
}

#[async_trait]
impl NotificationChannel for DiscordChannel {
    fn name(&self) -> &str {
        "discord"
    }
    
    async fn is_available(&self) -> bool {
        self.is_ready
    }
    
    async fn send(&self, notification_type: NotificationType, data: &JsonValue) -> Result<NotificationResult> {
        if !self.is_available().await {
            return Err(anyhow!("Discord channel not available"));
        }
        
        let payload = self.create_discord_embed(&notification_type, data);
        
        match self.client.post(&self.webhook_url).json(&payload).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    debug!("Discord notification sent successfully");
                    Ok(NotificationResult {
                        success: true,
                        error_message: None,
                        timestamp: Utc::now().timestamp(),
                        notification_type: notification_type.clone(),
                        channel: self.name().to_string(),
                    })
                } else {
                    let error = format!("Failed to send Discord notification: HTTP {}", response.status());
                    error!("{}", error);
                    Ok(NotificationResult {
                        success: false,
                        error_message: Some(error),
                        timestamp: Utc::now().timestamp(),
                        notification_type: notification_type.clone(),
                        channel: self.name().to_string(),
                    })
                }
            },
            Err(e) => {
                let error = format!("Failed to send Discord notification: {}", e);
                error!("{}", error);
                Ok(NotificationResult {
                    success: false,
                    error_message: Some(error),
                    timestamp: Utc::now().timestamp(),
                    notification_type: notification_type.clone(),
                    channel: self.name().to_string(),
                })
            }
        }
    }
    
    async fn update_config(&mut self, config: JsonValue) -> Result<()> {
        if let Some(webhook_url) = config.get("webhook_url").and_then(|v| v.as_str()) {
            self.webhook_url = webhook_url.to_string();
        }
        
        Ok(())
    }
}

/// Kênh thông báo Email
pub struct EmailChannel {
    /// SMTP server
    smtp_server: String,
    /// SMTP port
    smtp_port: u16,
    /// SMTP username
    username: String,
    /// SMTP password
    password: String,
    /// Địa chỉ email gửi
    from_email: String,
    /// Địa chỉ email nhận
    to_email: String,
    /// Có sẵn sàng không
    is_ready: bool,
}

impl EmailChannel {
    /// Tạo mới kênh thông báo Email
    pub fn new(
        smtp_server: String,
        smtp_port: u16,
        username: String,
        password: String,
        from_email: String,
        to_email: String,
    ) -> Self {
        Self {
            smtp_server,
            smtp_port,
            username,
            password,
            from_email,
            to_email,
            is_ready: true,
        }
    }
    
    /// Tạo tiêu đề email dựa trên loại thông báo
    fn get_email_subject(&self, notification_type: &NotificationType, data: &JsonValue) -> String {
        let prefix = match notification_type {
            NotificationType::CriticalAlert => "[CRITICAL]",
            NotificationType::ImportantAlert => "[IMPORTANT]",
            NotificationType::InfoAlert => "[INFO]",
            NotificationType::TradeNotification => "[TRADE]",
            NotificationType::ErrorNotification => "[ERROR]",
            NotificationType::Info => "[INFO]",
        };
        
        let subject = if let Some(alert_type) = data.get("alert_type").and_then(|v| v.as_str()) {
            format!("{} {}", prefix, alert_type.to_uppercase())
        } else {
            format!("{} SnipeBot Notification", prefix)
        };
        
        subject
    }
    
    /// Tạo nội dung email
    fn get_email_body(&self, notification_type: &NotificationType, data: &JsonValue) -> String {
        let mut body = String::new();
        
        // Add header
        body.push_str("<html><body>");
        body.push_str("<h2 style='color: #333;'>");
        
        let header_color = match notification_type {
            NotificationType::CriticalAlert => "#ff0000",
            NotificationType::ImportantAlert => "#ff9900",
            NotificationType::InfoAlert => "#00cc00",
            NotificationType::TradeNotification => "#0066cc",
            NotificationType::ErrorNotification => "#9900cc",
            NotificationType::Info => "#0099cc",
        };
        
        let header_text = if let Some(alert_type) = data.get("alert_type").and_then(|v| v.as_str()) {
            alert_type.to_uppercase()
        } else {
            format!("{:?}", notification_type)
        };
        
        body.push_str(&format!("<span style='color: {};'>{}</span>", header_color, header_text));
        body.push_str("</h2>");
        
        // Add message
        if let Some(message) = data.get("message").and_then(|v| v.as_str()) {
            body.push_str(&format!("<p>{}</p>", message.replace("\n", "<br>")));
        }
        
        // Add token info
        if let Some(token_address) = data.get("token_address").and_then(|v| v.as_str()) {
            body.push_str("<div style='margin: 10px 0; padding: 10px; background-color: #f5f5f5; border-left: 4px solid #0066cc;'>");
            body.push_str(&format!("<strong>Token:</strong> {}<br>", token_address));
            
            if let Some(chain_id) = data.get("chain_id") {
                body.push_str(&format!("<strong>Chain ID:</strong> {}<br>", chain_id));
            }
            
            if let Some(severity) = data.get("severity") {
                body.push_str(&format!("<strong>Severity:</strong> {}<br>", severity));
            }
            
            body.push_str("</div>");
        }
        
        // Add timestamp
        let timestamp = if let Some(ts) = data.get("timestamp").and_then(|v| v.as_i64()) {
            chrono::DateTime::from_timestamp(ts, 0)
                .unwrap_or_else(|| Utc::now())
                .format("%Y-%m-%d %H:%M:%S UTC")
                .to_string()
        } else {
            Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string()
        };
        
        body.push_str(&format!("<p style='color: #666; font-size: 0.8em;'>Time: {}</p>", timestamp));
        
        // Add footer
        body.push_str("<hr><p style='font-size: 0.8em; color: #999;'>This is an automated message from DiamondChain SnipeBot.</p>");
        body.push_str("</body></html>");
        
        body
    }
}

#[async_trait]
impl NotificationChannel for EmailChannel {
    fn name(&self) -> &str {
        "email"
    }
    
    async fn is_available(&self) -> bool {
        self.is_ready
    }
    
    async fn send(&self, notification_type: NotificationType, data: &JsonValue) -> Result<NotificationResult> {
        if !self.is_available().await {
            return Err(anyhow!("Email channel not available"));
        }
        
        // Note: Actual implementation would use lettre or similar to send emails
        // This is a stub implementation that just logs the attempt
        
        let subject = self.get_email_subject(&notification_type, data);
        let _body = self.get_email_body(&notification_type, data);
        
        // Log the email sending attempt
        info!(
            "Would send email: From: {}, To: {}, Subject: {}", 
            self.from_email, self.to_email, subject
        );
        
        // In a real implementation, we would send the email here
        // For now, we'll just pretend it succeeded
        
        Ok(NotificationResult {
            success: true,
            error_message: None,
            timestamp: Utc::now().timestamp(),
            notification_type: notification_type.clone(),
            channel: self.name().to_string(),
        })
    }
    
    async fn update_config(&mut self, config: JsonValue) -> Result<()> {
        if let Some(smtp_server) = config.get("smtp_server").and_then(|v| v.as_str()) {
            self.smtp_server = smtp_server.to_string();
        }
        
        if let Some(smtp_port) = config.get("smtp_port").and_then(|v| v.as_u64()) {
            self.smtp_port = smtp_port as u16;
        }
        
        if let Some(username) = config.get("username").and_then(|v| v.as_str()) {
            self.username = username.to_string();
        }
        
        if let Some(password) = config.get("password").and_then(|v| v.as_str()) {
            self.password = password.to_string();
        }
        
        if let Some(from_email) = config.get("from_email").and_then(|v| v.as_str()) {
            self.from_email = from_email.to_string();
        }
        
        if let Some(to_email) = config.get("to_email").and_then(|v| v.as_str()) {
            self.to_email = to_email.to_string();
        }
        
        Ok(())
    }
}

/// Quản lý thông báo tập trung
pub struct NotificationManager {
    /// Các kênh thông báo đã đăng ký
    channels: RwLock<HashMap<String, Box<dyn NotificationChannel>>>,
    /// Cấu hình cho từng loại thông báo
    notification_routing: RwLock<HashMap<NotificationType, Vec<String>>>,
    /// Lịch sử thông báo
    notification_history: RwLock<Vec<NotificationResult>>,
    /// Số lượng thông báo tối đa lưu trong lịch sử
    max_history_size: usize,
    /// Rate limiting - số lượng thông báo tối đa mỗi phút
    rate_limit: RwLock<HashMap<String, usize>>,
}

impl NotificationManager {
    /// Tạo mới NotificationManager
    pub fn new() -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
            notification_routing: RwLock::new(HashMap::new()),
            notification_history: RwLock::new(Vec::new()),
            max_history_size: 100,
            rate_limit: RwLock::new(HashMap::new()),
        }
    }
    
    /// Đăng ký kênh thông báo mới
    pub async fn register_channel(&self, channel: Box<dyn NotificationChannel>) -> Result<()> {
        let channel_name = channel.name().to_string();
        let mut channels = self.channels.write().await;
        channels.insert(channel_name, channel);
        Ok(())
    }
    
    /// Thiết lập routing cho loại thông báo
    pub async fn set_notification_routing(&self, notification_type: NotificationType, channel_names: Vec<String>) -> Result<()> {
        let mut routing = self.notification_routing.write().await;
        routing.insert(notification_type, channel_names);
        Ok(())
    }
    
    /// Gửi thông báo
    pub async fn send_notification(&self, notification_type: NotificationType, data: &JsonValue) -> Result<Vec<NotificationResult>> {
        // Kiểm tra rate limit
        {
            let mut rate_limit = self.rate_limit.write().await;
            let minute_key = format!("{}:{}", notification_type.clone() as u8, Utc::now().format("%Y%m%d%H%M"));
            
            let count = rate_limit.entry(minute_key.clone()).or_insert(0);
            *count += 1;
            
            if *count > 10 {
                // Quá rate limit, chỉ log và return
                warn!("Rate limit exceeded for notification type {:?}", notification_type);
                return Err(anyhow!("Rate limit exceeded"));
            }
            
            // Xóa các entry cũ
            rate_limit.retain(|k, _| k.starts_with(&format!("{}:", notification_type.clone() as u8)));
        }
        
        // Xác định các kênh cần gửi
        let channels_to_use = {
            let routing = self.notification_routing.read().await;
            routing.get(&notification_type).cloned().unwrap_or_else(|| {
                // Fallback to all channels if no specific routing
                let channels = self.channels.read().block_in_place();
                channels.keys().cloned().collect()
            })
        };
        
        let channels = self.channels.read().await;
        let mut results = Vec::new();
        
        // Gửi thông báo đến tất cả các kênh đã đăng ký
        for channel_name in channels_to_use {
            if let Some(channel) = channels.get(&channel_name) {
                if channel.is_available().await {
                    match channel.send(notification_type.clone(), data).await {
                        Ok(result) => {
                            results.push(result);
                        },
                        Err(e) => {
                            error!("Failed to send notification to channel {}: {}", channel_name, e);
                        }
                    }
                }
            }
        }
        
        // Cập nhật lịch sử thông báo
        {
            let mut history = self.notification_history.write().await;
            for result in &results {
                history.push(result.clone());
            }
            
            // Giới hạn kích thước lịch sử
            if history.len() > self.max_history_size {
                let to_remove = history.len() - self.max_history_size;
                history.drain(0..to_remove);
            }
        }
        
        Ok(results)
    }
    
    /// Khởi tạo kênh thông báo Telegram
    pub async fn init_telegram(&self, token: String, chat_id: String) -> Result<()> {
        let telegram = Box::new(TelegramChannel::new(token, chat_id)?);
        self.register_channel(telegram).await
    }
    
    /// Khởi tạo kênh thông báo Discord
    pub async fn init_discord(&self, webhook_url: String) -> Result<()> {
        let discord = Box::new(DiscordChannel::new(webhook_url));
        self.register_channel(discord).await
    }
    
    /// Khởi tạo kênh thông báo Email
    pub async fn init_email(
        &self,
        smtp_server: String,
        smtp_port: u16,
        username: String,
        password: String,
        from_email: String,
        to_email: String,
    ) -> Result<()> {
        let email = Box::new(EmailChannel::new(
            smtp_server,
            smtp_port,
            username,
            password,
            from_email,
            to_email,
        ));
        self.register_channel(email).await
    }
    
    /// Tạo NotificationManager với cấu hình mặc định
    pub async fn create_default() -> Result<Arc<Self>> {
        let manager = Arc::new(Self::new());
        
        // Thiết lập routing mặc định
        manager.set_notification_routing(
            NotificationType::CriticalAlert,
            vec!["telegram".to_string(), "discord".to_string(), "email".to_string()],
        ).await?;
        
        manager.set_notification_routing(
            NotificationType::ImportantAlert,
            vec!["telegram".to_string(), "discord".to_string()],
        ).await?;
        
        manager.set_notification_routing(
            NotificationType::InfoAlert,
            vec!["telegram".to_string()],
        ).await?;
        
        manager.set_notification_routing(
            NotificationType::TradeNotification,
            vec!["telegram".to_string(), "discord".to_string()],
        ).await?;
        
        manager.set_notification_routing(
            NotificationType::ErrorNotification,
            vec!["telegram".to_string(), "discord".to_string()],
        ).await?;
        
        manager.set_notification_routing(
            NotificationType::Info,
            vec!["telegram".to_string()],
        ).await?;
        
        Ok(manager)
    }
    
    /// Lấy lịch sử thông báo
    pub async fn get_notification_history(&self) -> Vec<NotificationResult> {
        let history = self.notification_history.read().await;
        history.clone()
    }
    
    /// Xóa lịch sử thông báo
    pub async fn clear_notification_history(&self) -> Result<()> {
        let mut history = self.notification_history.write().await;
        history.clear();
        Ok(())
    }
    
    /// Kiểm tra xem kênh thông báo có sẵn sàng không
    pub async fn is_channel_available(&self, channel_name: &str) -> bool {
        let channels = self.channels.read().await;
        if let Some(channel) = channels.get(channel_name) {
            channel.is_available().await
        } else {
            false
        }
    }
    
    /// Kiểm tra xem kênh thông báo có tồn tại không
    pub async fn has_channel(&self, channel_name: &str) -> bool {
        let channels = self.channels.read().await;
        channels.contains_key(channel_name)
    }
    
    /// Gửi thông báo kiểm tra
    pub async fn send_test_notification(&self, channel_name: &str) -> Result<NotificationResult> {
        let channels = self.channels.read().await;
        if let Some(channel) = channels.get(channel_name) {
            let test_data = serde_json::json!({
                "message": "This is a test notification",
                "alert_type": "test",
                "timestamp": Utc::now().timestamp()
            });
            
            channel.send(NotificationType::Info, &test_data).await
        } else {
            Err(anyhow!("Channel {} not found", channel_name))
        }
    }
} 