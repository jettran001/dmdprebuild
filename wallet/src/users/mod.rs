// wallet/src/users/mod.rs
//! Module quản lý người dùng và đăng ký
//! 
//! Module này bao gồm các submodule sau:
//! - free_user: Quản lý người dùng miễn phí
//! - premium_user: Quản lý người dùng premium
//! - vip_user: Quản lý người dùng VIP với các tính năng cao cấp
//! - subscription: Quản lý đăng ký và nâng cấp tài khoản

pub mod free_user;
pub mod premium_user;
pub mod vip_user;
pub mod subscription;

// Chỉ export các API chính và ẩn chi tiết cài đặt
// Re-exports có kiểm soát
pub use free_user::manager::FreeUserManager;
pub use premium_user::PremiumUserManager;
pub use vip_user::VipUserManager;

// Re-export các kiểu dữ liệu chung sử dụng bởi API công khai
pub mod types {
    pub use crate::users::free_user::types::{UserStatus, FreeUserData};
    pub use crate::users::premium_user::{PremiumUserStatus, PremiumUserData, PaymentRecord};
    pub use crate::users::vip_user::{VipUserStatus, VipUserData};
    pub use crate::users::subscription::types::{
        SubscriptionType, SubscriptionStatus, Feature, PaymentToken, 
        SubscriptionPlan, SubscriptionRecord, TransactionCheckResult
    };
    pub use crate::users::subscription::user_subscription::UserSubscription;
    pub use crate::users::subscription::nft::{VipNftInfo, NftInfo, NonNftVipStatus};
    pub use crate::users::subscription::auto_trade::{AutoTradeUsage, AutoTradeStatus};
}

// Re-export các manager classes có kiểm soát
pub mod managers {
    pub use crate::users::subscription::manager::SubscriptionManager;
    pub use crate::users::subscription::auto_trade::AutoTradeManager;
    pub use crate::users::subscription::payment::PaymentProcessor;
    pub use crate::users::subscription::events::{
        SubscriptionEventManager, SubscriptionEventListener
    };
}

// Re-export các hằng số chung quan trọng
pub mod constants {
    pub use crate::users::subscription::constants::{
        FREE_USER_AUTO_TRADE_MINUTES, PREMIUM_USER_AUTO_TRADE_HOURS, 
        VIP_USER_AUTO_TRADE_HOURS, VIP_RESET_HOUR
    };
}