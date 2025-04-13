//! Module quản lý các sự kiện liên quan đến đăng ký.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::types::{SubscriptionType, UserSubscription};

/// Loại sự kiện đăng ký
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    /// Tạo đăng ký mới
    Created,
    /// Kích hoạt đăng ký
    Activated,
    /// Thanh toán thành công
    PaymentSuccess,
    /// Thanh toán thất bại
    PaymentFailed,
    /// Gia hạn
    Renewed,
    /// Hủy đăng ký
    Cancelled,
    /// Hết hạn
    Expired,
    /// Nâng cấp gói
    Upgraded,
    /// Hạ cấp gói
    Downgraded,
    /// Cập nhật NFT
    NftUpdated,
    /// Xác nhận NFT
    NftVerified,
    /// NFT không hợp lệ
    NftInvalid,
}

/// Sự kiện liên quan đến đăng ký
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionEvent {
    /// ID sự kiện
    pub id: Uuid,
    /// Loại sự kiện
    pub event_type: EventType,
    /// ID người dùng
    pub user_id: String,
    /// Loại gói đăng ký
    pub subscription_type: SubscriptionType,
    /// Thông tin đăng ký (lưu trữ snapshot khi sự kiện xảy ra)
    pub subscription_data: Option<UserSubscription>,
    /// Thông tin bổ sung dạng JSON
    pub metadata: serde_json::Value,
    /// Thời điểm xảy ra sự kiện
    pub created_at: DateTime<Utc>,
}

impl SubscriptionEvent {
    /// Tạo sự kiện mới
    pub fn new(
        event_type: EventType,
        user_id: &str,
        subscription_type: SubscriptionType,
        subscription_data: Option<UserSubscription>,
        metadata: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            event_type,
            user_id: user_id.to_string(),
            subscription_type,
            subscription_data,
            metadata,
            created_at: Utc::now(),
        }
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
            event.subscription_type
        );
    }
} 