//! Module quản lý đăng ký và gói dịch vụ cho người dùng
//! 
//! Module này chứa tất cả các cấu trúc dữ liệu và logic liên quan đến:
//! - Các loại gói đăng ký (Free, Premium, VIP, Lifetime)
//! - Quản lý thanh toán và giao dịch
//! - Xác thực và quản lý NFT VIP
//! - Quản lý Auto Trade
//! - Tính năng của từng gói đăng ký

mod auto_trade;
mod constants;
mod manager;
mod nft;
mod types;
mod user_subscription;
mod utils;
mod vip;
pub mod payment;
pub mod events;

#[cfg(test)]
mod tests;

// Re-export tất cả public items từ các module con
pub use auto_trade::*;
pub use constants::*;
pub use manager::*;
pub use nft::*;
pub use types::{SubscriptionType, SubscriptionStatus, Feature, PaymentToken, SubscriptionPlan, TransactionCheckResult, SubscriptionRecord};
pub use user_subscription::*;
pub use utils::*;
pub use vip::*;
pub use payment::*;
pub use events::*;

// Các type alias và utility functions
pub type SubscriptionResult<T> = Result<T, SubscriptionError>;

/// Errors liên quan đến đăng ký
#[derive(Debug, thiserror::Error)]
pub enum SubscriptionError {
    #[error("Gói đăng ký đã hết hạn")]
    Expired,
    
    #[error("Gói đăng ký không hợp lệ")]
    InvalidSubscription,
    
    #[error("Không tìm thấy gói đăng ký")]
    NotFound,
    
    #[error("Đã đạt giới hạn tính năng")]
    FeatureLimitReached,
    
    #[error("Tính năng không có sẵn cho gói đăng ký này")]
    FeatureNotAvailable,
    
    #[error("Lỗi khi xử lý thanh toán: {0}")]
    PaymentError(String),
    
    #[error("Lỗi khi xử lý NFT: {0}")]
    NftError(String),
    
    #[error("Lỗi khi xử lý Auto Trade: {0}")]
    AutoTradeError(String),
    
    #[error("Lỗi cơ sở dữ liệu: {0}")]
    DatabaseError(String),
    
    #[error("Lỗi hệ thống: {0}")]
    SystemError(String),
}

/// Kiểm tra nhanh xem người dùng có quyền sử dụng tính năng hay không
pub fn has_feature_access(user: &crate::users::User, feature: Feature) -> bool {
    if let Some(subscription) = &user.subscription {
        if subscription.is_active() {
            return subscription.has_feature(feature);
        }
    }
    
    // Mặc định cho các tính năng miễn phí
    match feature {
        Feature::RealTimeAlerts => true,
        _ => false,
    }
}

/// Kiểm tra và cập nhật trạng thái đăng ký theo thời gian
pub fn refresh_subscription_status(user: &mut crate::users::User) -> crate::error::AppResult<()> {
    if let Some(subscription) = &mut user.subscription {
        subscription.refresh_status()?;
    }
    Ok(())
} 