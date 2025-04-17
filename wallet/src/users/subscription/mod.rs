//! Module quản lý đăng ký và gói dịch vụ cho người dùng
//! 
//! Module này chứa tất cả các cấu trúc dữ liệu và logic liên quan đến:
//! - Các loại gói đăng ký (Free, Premium, VIP, Lifetime)
//! - Quản lý thanh toán và giao dịch
//! - Xác thực và quản lý NFT VIP
//! - Quản lý Auto Trade
//! - Tính năng của từng gói đăng ký
//! - Quản lý token staking (DMD ERC-1155) với 30% APY hàng năm
//! - Yêu cầu NFT cho gói VIP (30 ngày) và gói VIP (12 tháng)
//! - Gói 12 tháng stake token không cần kiểm tra NFT sau khi đăng ký
//! 
//! Module này là trung tâm quản lý tất cả các đăng ký người dùng, và cung cấp
//! giao diện thống nhất cho việc nâng cấp, hạ cấp, kiểm tra các tính năng.

// Private modules
mod auto_trade;
mod constants;
mod manager;
mod nft;
mod types;
mod user_subscription;
mod utils;
mod vip;
mod staking;
mod events;
mod payment;

// Test modules
#[cfg(test)]
mod tests;

// Re-export tất cả public items từ các module con
// Cấu trúc re-export tuân theo thứ tự ưu tiên trong manifest
pub use manager::SubscriptionManager;
pub use staking::{StakingManager, TokenStake, StakeStatus, StakingError};
pub use events::{EventType, EventEmitter, SubscriptionEvent};
pub use payment::PaymentProcessor;
pub use types::{
    SubscriptionType, SubscriptionStatus, Feature, 
    PaymentToken, SubscriptionPlan, TransactionCheckResult, 
    SubscriptionRecord
};
pub use user_subscription::UserSubscription;
pub use nft::{NftInfo, VipNftInfo, NonNftVipStatus};
pub use auto_trade::{AutoTradeManager, AutoTradeUsage, AutoTradeStatus};
pub use constants::*;
pub use utils::*;
pub use vip::*;

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
    
    #[error("Lỗi khi xử lý Staking: {0}")]
    StakingError(String),
    
    #[error("Lỗi cơ sở dữ liệu: {0}")]
    DatabaseError(String),
    
    #[error("Lỗi hệ thống: {0}")]
    SystemError(String),
}

/// Kiểm tra nhanh xem người dùng có quyền sử dụng tính năng hay không
/// 
/// Hàm này kiểm tra quyền truy cập tính năng dựa trên loại gói đăng ký của người dùng
/// và tình trạng kích hoạt. Hàm đảm bảo xử lý an toàn cho mọi trường hợp đầu vào.
/// 
/// # Arguments
/// * `user` - Tham chiếu đến đối tượng User cần kiểm tra
/// * `feature` - Tính năng cần kiểm tra quyền truy cập
/// 
/// # Returns
/// Trả về `true` nếu người dùng có quyền truy cập tính năng, ngược lại trả về `false`
pub fn has_feature_access(user: &crate::users::User, feature: Feature) -> bool {
    // Kiểm tra user có hợp lệ không
    if user.status != crate::users::UserStatus::Active {
        // Người dùng không active thì không có quyền truy cập các tính năng cao cấp
        return match feature {
            Feature::RealTimeAlerts => true, // Tính năng miễn phí cho tất cả
            _ => false,
        };
    }
    
    // Kiểm tra subscription có tồn tại và còn hạn không
    match &user.subscription {
        Some(subscription) => {
            if subscription.is_active() {
                // Kiểm tra quyền truy cập dựa trên loại gói
                subscription.has_feature(feature)
            } else {
                // Gói đã hết hạn, chỉ có các tính năng miễn phí
                match feature {
                    Feature::RealTimeAlerts => true,
                    _ => false,
                }
            }
        },
        None => {
            // Người dùng không có gói đăng ký, chỉ có các tính năng miễn phí
            match feature {
                Feature::RealTimeAlerts => true,
                _ => false,
            }
        }
    }
}

/// Kiểm tra và cập nhật trạng thái đăng ký theo thời gian
/// 
/// Hàm này cập nhật trạng thái đăng ký dựa trên thời gian hiện tại, kiểm tra
/// hạn sử dụng và các yếu tố khác để đảm bảo trạng thái đăng ký luôn chính xác.
/// 
/// # Arguments
/// * `user` - Tham chiếu đến đối tượng User cần cập nhật
/// 
/// # Returns
/// Trả về `Ok(())` nếu cập nhật thành công, ngược lại trả về lỗi
pub fn refresh_subscription_status(user: &mut crate::users::User) -> crate::error::AppResult<()> {
    if user.status != crate::users::UserStatus::Active {
        return Ok(());  // Không cần cập nhật cho người dùng không active
    }
    
    if let Some(subscription) = &mut user.subscription {
        subscription.refresh_status()?;
        
        // Kiểm tra nếu subscription đã hết hạn nhưng user vẫn active
        if !subscription.is_active() {
            // Đảm bảo subscription được đặt về trạng thái chính xác
            subscription.status = SubscriptionStatus::Expired;
        }
    }
    Ok(())
} 