//! Module quản lý các sự kiện liên quan đến subscription và auto-trade
//! Được sử dụng làm nguồn chính xác cho các sự kiện trong hệ thống subscription.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::fmt;
use sqlx::{Pool, Sqlite};
use tracing::{debug, error, info, warn};
use std::sync::Arc;

use super::types::{SubscriptionType, UserSubscription};
use super::staking::StakeStatus;
use crate::error::WalletError;
use crate::users::subscription::utils::SubscriptionUtils;

/// Loại sự kiện
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    /// Đăng ký mới
    NewSubscription,
    /// Nâng cấp đăng ký
    SubscriptionUpgraded,
    /// Đăng ký hết hạn
    SubscriptionExpired,
    /// Đăng ký được gia hạn
    SubscriptionRenewed,
    /// Đăng ký bị hủy
    SubscriptionCancelled,
    /// Cảnh báo sắp hết hạn
    SubscriptionExpiryWarning,
    /// Cảnh báo sắp hết thời gian auto-trade
    AutoTradeTimeWarning,
    /// Reset thời gian auto-trade
    AutoTradeTimeReset,
    /// Đã sử dụng hết thời gian auto-trade
    AutoTradeTimeExhausted,
    /// Xác thực NFT thành công
    NFTVerificationSuccess,
    /// Xác thực NFT thất bại
    NFTVerificationFailed,
    /// Thanh toán thành công
    PaymentSuccess,
    /// Thanh toán thất bại
    PaymentFailed,
    /// Stake token thành công
    StakeSuccess,
    /// Stake token hoàn thành (unstake)
    StakeCompleted,
    /// Stake token bị hủy
    StakeCancelled,
    /// Đã phát sinh lợi nhuận staking
    StakeRewardDistributed,
    /// Subscription được tạo
    SubscriptionCreated,
    /// Subscription sắp hết hạn
    SubscriptionExpiringSoon,
    /// Subscription bị hạ cấp
    SubscriptionDowngraded,
    /// Token đã stake
    TokenStaked,
    /// Stake token đã kết thúc
    TokenStakeEnded,
    /// Stake token đã bị hủy
    TokenStakeCancelled,
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NewSubscription => write!(f, "new_subscription"),
            Self::SubscriptionUpgraded => write!(f, "subscription_upgraded"),
            Self::SubscriptionExpired => write!(f, "subscription_expired"),
            Self::SubscriptionRenewed => write!(f, "subscription_renewed"),
            Self::SubscriptionCancelled => write!(f, "subscription_cancelled"),
            Self::SubscriptionExpiryWarning => write!(f, "subscription_expiry_warning"),
            Self::AutoTradeTimeWarning => write!(f, "auto_trade_time_warning"),
            Self::AutoTradeTimeReset => write!(f, "auto_trade_time_reset"),
            Self::AutoTradeTimeExhausted => write!(f, "auto_trade_time_exhausted"),
            Self::NFTVerificationSuccess => write!(f, "nft_verification_success"),
            Self::NFTVerificationFailed => write!(f, "nft_verification_failed"),
            Self::PaymentSuccess => write!(f, "payment_success"),
            Self::PaymentFailed => write!(f, "payment_failed"),
            Self::StakeSuccess => write!(f, "stake_success"),
            Self::StakeCompleted => write!(f, "stake_completed"),
            Self::StakeCancelled => write!(f, "stake_cancelled"),
            Self::StakeRewardDistributed => write!(f, "stake_reward_distributed"),
            Self::SubscriptionCreated => write!(f, "subscription_created"),
            Self::SubscriptionExpiringSoon => write!(f, "subscription_expiring_soon"),
            Self::SubscriptionDowngraded => write!(f, "subscription_downgraded"),
            Self::TokenStaked => write!(f, "token_staked"),
            Self::TokenStakeEnded => write!(f, "token_stake_ended"),
            Self::TokenStakeCancelled => write!(f, "token_stake_cancelled"),
        }
    }
}

impl EventType {
    /// Chuyển đổi từ chuỗi sang EventType
    /// 
    /// # Arguments
    /// * `s` - Chuỗi cần chuyển đổi
    /// 
    /// # Returns
    /// * `Ok(EventType)` - Nếu chuyển đổi thành công
    /// * `Err(String)` - Nếu chuyển đổi thất bại
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "new_subscription" => Ok(Self::NewSubscription),
            "subscription_upgraded" => Ok(Self::SubscriptionUpgraded),
            "subscription_expired" => Ok(Self::SubscriptionExpired),
            "subscription_renewed" => Ok(Self::SubscriptionRenewed),
            "subscription_cancelled" => Ok(Self::SubscriptionCancelled),
            "subscription_expiry_warning" => Ok(Self::SubscriptionExpiryWarning),
            "auto_trade_time_warning" => Ok(Self::AutoTradeTimeWarning),
            "auto_trade_time_reset" => Ok(Self::AutoTradeTimeReset),
            "auto_trade_time_exhausted" => Ok(Self::AutoTradeTimeExhausted),
            "nft_verification_success" => Ok(Self::NFTVerificationSuccess),
            "nft_verification_failed" => Ok(Self::NFTVerificationFailed),
            "payment_success" => Ok(Self::PaymentSuccess),
            "payment_failed" => Ok(Self::PaymentFailed),
            "stake_success" => Ok(Self::StakeSuccess),
            "stake_completed" => Ok(Self::StakeCompleted),
            "stake_cancelled" => Ok(Self::StakeCancelled),
            "stake_reward_distributed" => Ok(Self::StakeRewardDistributed),
            "subscription_created" => Ok(Self::SubscriptionCreated),
            "subscription_expiring_soon" => Ok(Self::SubscriptionExpiringSoon),
            "subscription_downgraded" => Ok(Self::SubscriptionDowngraded),
            "token_staked" => Ok(Self::TokenStaked),
            "token_stake_ended" => Ok(Self::TokenStakeEnded),
            "token_stake_cancelled" => Ok(Self::TokenStakeCancelled),
            _ => Err(format!("Không thể chuyển đổi '{}' sang EventType", s)),
        }
    }
}

/// Sự kiện đăng ký
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionEvent {
    /// ID người dùng
    pub user_id: String,
    /// Loại sự kiện
    pub event_type: EventType,
    /// Thời gian sự kiện
    pub timestamp: DateTime<Utc>,
    /// Dữ liệu bổ sung (nếu có)
    pub data: Option<String>,
}

/// Nguồn phát các sự kiện
pub trait EventSource {
    /// Đăng ký listener để nhận sự kiện
    fn register_listener(&mut self, listener: Box<dyn EventListener>);
    /// Gỡ bỏ listener
    fn remove_listener(&mut self, id: usize);
    /// Phát sự kiện
    fn emit(&self, event: SubscriptionEvent);
}

/// Listener nhận sự kiện
pub trait EventListener: Send + Sync {
    /// ID của listener
    fn id(&self) -> usize;
    /// Xử lý sự kiện
    fn on_event(&self, event: &SubscriptionEvent);
}

/// Đối tượng phát sự kiện
#[derive(Clone)]
pub struct EventEmitter {
    // Trong phiên bản thực tế, đây sẽ là một Arc<Mutex<Vec<Box<dyn EventListener>>>>
    // nhưng để đơn giản hóa, chỉ cần một mock struct
}

impl EventEmitter {
    /// Tạo mới EventEmitter
    pub fn new() -> Self {
        Self {}
    }
    
    /// Phát sự kiện
    pub fn emit(&self, event: SubscriptionEvent) {
        // Trong môi trường thực tế, sẽ gửi sự kiện đến tất cả listeners đã đăng ký
        // Ở đây chỉ để log thông tin
        log::info!("Sự kiện [{}] từ người dùng {}: {:?}", event.event_type, event.user_id, event.data);
    }
}

impl Default for EventEmitter {
    fn default() -> Self {
        Self::new()
    }
}

/// Quản lý sự kiện đăng ký
pub struct SubscriptionEventManager {
    /// Danh sách các listener đăng ký nhận thông báo khi có sự kiện
    listeners: Vec<Box<dyn SubscriptionEventListener + Send + Sync>>,
}

impl SubscriptionEventManager {
    /// Tạo manager mới
    pub fn new() -> Self {
        Self {
            listeners: Vec::new(),
        }
    }
    
    /// Đăng ký một listener mới
    pub fn register_listener<T>(&mut self, listener: T)
    where
        T: SubscriptionEventListener + Send + Sync + 'static,
    {
        self.listeners.push(Box::new(listener));
    }
    
    /// Phát sự kiện đến tất cả listeners
    pub fn emit_event(&self, event: SubscriptionEvent) {
        for listener in &self.listeners {
            listener.on_event(&event);
        }
    }
    
    /// Lọc sự kiện theo loại và người dùng
    pub fn filter_events(
        &self, 
        events: &[SubscriptionEvent], 
        event_type: Option<EventType>, 
        user_id: Option<&str>
    ) -> Vec<&SubscriptionEvent> {
        events
            .iter()
            .filter(|event| {
                if let Some(et) = event_type {
                    if event.event_type != et {
                        return false;
                    }
                }
                
                if let Some(uid) = user_id {
                    if event.user_id != uid {
                        return false;
                    }
                }
                
                true
            })
            .collect()
    }
}

/// Trait định nghĩa một listener cho sự kiện đăng ký
pub trait SubscriptionEventListener {
    /// Xử lý khi có sự kiện xảy ra
    fn on_event(&self, event: &SubscriptionEvent);
}

/// Ví dụ về một listener ghi log
pub struct LoggingEventListener;

impl SubscriptionEventListener for LoggingEventListener {
    fn on_event(&self, event: &SubscriptionEvent) {
        tracing::info!(
            "Subscription event: type={:?}, user_id={}, subscription_type={:?}",
            event.event_type,
            event.user_id,
            event.data
        );
    }
}

/// Module quản lý sự kiện liên quan đến subscription
pub struct EventManager {
    db_pool: Arc<Pool<Sqlite>>,
}

impl EventManager {
    /// Tạo một instance mới của EventManager
    pub fn new(db_pool: Arc<Pool<Sqlite>>) -> Self {
        Self { db_pool }
    }

    /// Lưu sự kiện subscription vào database
    pub async fn save_event(&self, event: &SubscriptionEvent) -> Result<(), WalletError> {
        // Log sự kiện trước
        SubscriptionUtils::log_subscription_event(event);

        // Lưu vào database
        sqlx::query(
            r#"
            INSERT INTO subscription_events 
            (user_id, event_type, timestamp, data)
            VALUES (?, ?, ?, ?)
            "#
        )
        .bind(&event.user_id)
        .bind(event.event_type.to_string())
        .bind(event.timestamp)
        .bind(&event.data)
        .execute(&*self.db_pool)
        .await
        .map_err(|e| {
            error!("Lỗi khi lưu sự kiện subscription: {}", e);
            WalletError::DatabaseError(format!("Lỗi khi lưu sự kiện subscription: {}", e))
        })?;

        debug!("Đã lưu sự kiện subscription: {:?}", event);
        Ok(())
    }

    /// Lấy danh sách sự kiện của một người dùng
    pub async fn get_user_events(
        &self, 
        user_id: &str,
        limit: Option<i64>,
        offset: Option<i64>
    ) -> Result<Vec<SubscriptionEvent>, WalletError> {
        let limit = match limit {
            Some(value) => value,
            None => 50,
        };
        let offset = match offset {
            Some(value) => value,
            None => 0,
        };

        let events = sqlx::query_as!(
            SubscriptionEventRow,
            r#"
            SELECT user_id, event_type as "event_type: String", timestamp, data
            FROM subscription_events
            WHERE user_id = ?
            ORDER BY timestamp DESC
            LIMIT ? OFFSET ?
            "#,
            user_id,
            limit,
            offset
        )
        .fetch_all(&*self.db_pool)
        .await
        .map_err(|e| {
            error!("Lỗi khi lấy sự kiện subscription: {}", e);
            WalletError::DatabaseError(format!("Lỗi khi lấy sự kiện subscription: {}", e))
        })?;

        // Chuyển đổi từ SubscriptionEventRow sang SubscriptionEvent
        let events = events
            .into_iter()
            .filter_map(|row| {
                match EventType::from_str(&row.event_type) {
                    Ok(event_type) => Some(SubscriptionEvent {
                        user_id: row.user_id,
                        event_type,
                        timestamp: row.timestamp,
                        data: row.data,
                    }),
                    Err(_) => {
                        warn!("Không thể chuyển đổi loại sự kiện: {}", row.event_type);
                        None
                    }
                }
            })
            .collect();

        Ok(events)
    }

    /// Kiểm tra và tạo các sự kiện cho subscription sắp hết hạn
    pub async fn check_expiring_subscriptions(
        &self,
        subscriptions: Vec<UserSubscription>,
        days_threshold: i64,
    ) -> Result<(), WalletError> {
        for subscription in subscriptions {
            if SubscriptionUtils::is_expiring_soon(&subscription, days_threshold) 
                && subscription.status == SubscriptionStatus::Active 
            {
                let event = SubscriptionUtils::create_expiring_soon_event(&subscription);
                self.save_event(&event).await?;
                
                // Lấy số ngày còn lại một cách an toàn
                let days_remaining = match SubscriptionUtils::days_remaining(&subscription) {
                    Some(days) => days,
                    None => {
                        warn!("Không thể tính số ngày còn lại cho subscription user_id={}", subscription.user_id);
                        0
                    }
                };
                
                info!(
                    "Đã gửi thông báo sắp hết hạn cho user_id={}: còn {} ngày",
                    subscription.user_id,
                    days_remaining
                );
            }
        }
        
        Ok(())
    }

    /// Kiểm tra và tạo các sự kiện cho subscription đã hết hạn
    pub async fn check_expired_subscriptions(
        &self,
        subscriptions: Vec<UserSubscription>,
    ) -> Result<Vec<String>, WalletError> {
        let mut expired_user_ids = Vec::new();
        
        for mut subscription in subscriptions {
            if SubscriptionUtils::is_expired(&subscription) 
                && subscription.subscription_type != crate::users::subscription::types::SubscriptionType::Free
                && subscription.status == SubscriptionStatus::Active 
            {
                if let Some(event) = SubscriptionUtils::process_expired_subscription(&mut subscription) {
                    match self.save_event(&event).await {
                        Ok(_) => {
                            expired_user_ids.push(subscription.user_id.clone());
                        },
                        Err(e) => {
                            error!("Không thể lưu sự kiện hết hạn cho user {}: {}", subscription.user_id, e);
                            // Tiếp tục với subscription tiếp theo
                        }
                    }
                }
            }
        }
        
        Ok(expired_user_ids)
    }
}

/// Cấu trúc dữ liệu trung gian để lấy dữ liệu từ database
#[derive(Debug)]
struct SubscriptionEventRow {
    user_id: String,
    event_type: String,
    timestamp: DateTime<Utc>,
    data: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::users::subscription::types::{PaymentToken, SubscriptionType};
    use chrono::Duration;
    use sqlx::sqlite::SqlitePoolOptions;
    
    async fn setup_test_db() -> Result<Arc<Pool<Sqlite>>, sqlx::Error> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect("sqlite::memory:")
            .await?;
            
        sqlx::query(
            r#"
            CREATE TABLE subscription_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                timestamp TIMESTAMP NOT NULL,
                data TEXT
            )
            "#
        )
        .execute(&pool)
        .await?;
        
        Ok(Arc::new(pool))
    }
    
    fn create_test_subscription(user_id: &str, days: i64) -> UserSubscription {
        let now = Utc::now();
        UserSubscription {
            user_id: user_id.to_string(),
            subscription_type: SubscriptionType::Premium,
            start_date: now,
            end_date: Some(now + Duration::days(days)),
            payment_address: None,
            payment_tx_hash: None,
            payment_token: Some(PaymentToken::USDC),
            payment_amount: Some("10".to_string()),
            status: SubscriptionStatus::Active,
            nft_info: None,
            non_nft_status: None,
            created_at: now,
            updated_at: now,
        }
    }
    
    #[tokio::test]
    async fn test_save_and_get_events() -> Result<(), Box<dyn std::error::Error>> {
        let db_pool = setup_test_db().await?;
        let event_manager = EventManager::new(db_pool);
        
        let user_id = "test_user";
        let event = SubscriptionEvent {
            user_id: user_id.to_string(),
            event_type: EventType::SubscriptionCreated,
            timestamp: Utc::now(),
            data: Some(r#"{"plan":"Premium"}"#.to_string()),
        };
        
        // Lưu sự kiện
        event_manager.save_event(&event).await?;
        
        // Lấy sự kiện
        let events = event_manager.get_user_events(user_id, None, None).await?;
            
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].user_id, user_id);
        assert_eq!(events[0].event_type, EventType::SubscriptionCreated);
        
        Ok(())
    }
    
    #[tokio::test]
    async fn test_check_expiring_subscriptions() -> Result<(), Box<dyn std::error::Error>> {
        let db_pool = setup_test_db().await?;
        let event_manager = EventManager::new(db_pool);
        
        let user_id = "test_user";
        let subscription = create_test_subscription(user_id, 5);
        
        // Kiểm tra subscription sắp hết hạn
        event_manager.check_expiring_subscriptions(vec![subscription], 7).await?;
            
        // Lấy sự kiện
        let events = event_manager.get_user_events(user_id, None, None).await?;
            
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::SubscriptionExpiringSoon);
        
        Ok(())
    }
} 