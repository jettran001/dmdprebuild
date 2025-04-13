// wallet/src/users/mod.rs
//! Module quản lý người dùng và đăng ký
//! 
//! Module này bao gồm các submodule sau:
//! - free_user: Quản lý người dùng miễn phí
//! - premium_user: Quản lý người dùng premium
//! - subscription: Quản lý đăng ký và nâng cấp tài khoản

pub mod free_user;
pub mod premium_user;
pub mod subscription;

// Re-exports
pub use free_user::{FreeUserData, FreeUserManager, UserStatus};
pub use premium_user::{PremiumUserData, PremiumUserManager};

// Re-export các thành phần chính từ subscription
pub use subscription::{
    // Manager classes
    manager::SubscriptionManager,
    auto_trade::AutoTradeManager,
    vip::VipManager,
    
    // Core types
    types::{
        SubscriptionType, SubscriptionStatus, Feature, PaymentToken, 
        SubscriptionPlan, SubscriptionRecord, TransactionCheckResult
    },
    
    // Subscription data
    user_subscription::UserSubscription,
    
    // NFT related
    nft::{VipNftInfo, NftInfo, NonNftVipStatus},
    vip::{VipUserStatus, VipUserData},
    
    // Auto-trade related
    auto_trade::{AutoTradeUsage, AutoTradeStatus},
    
    // Payment related
    payment::{PaymentProcessor, TransactionInfo, TransactionStatus},
    
    // Event related
    events::{SubscriptionEvent, EventType, SubscriptionEventManager, SubscriptionEventListener},
    
    // Constants
    constants::{
        FREE_USER_AUTO_TRADE_MINUTES, PREMIUM_USER_AUTO_TRADE_HOURS, 
        VIP_USER_AUTO_TRADE_HOURS, VIP_RESET_HOUR
    }
};