// wallet/src/users/mod.rs
//! Module quản lý người dùng và đăng ký
//! 
//! Module này bao gồm các submodule sau:
//! - abstract_user_manager: Cung cấp các trait và thành phần trừu tượng hóa cho quản lý người dùng
//! - free_user: Quản lý người dùng miễn phí
//! - premium_user: Quản lý người dùng premium
//! - vip_user: Quản lý người dùng VIP với các tính năng cao cấp
//! - subscription: Quản lý đăng ký và nâng cấp tài khoản

pub mod abstract_user_manager;
pub mod free_user;
pub mod premium_user;
pub mod vip_user;
pub mod subscription;

use std::sync::Arc;
use async_trait::async_trait;
use ethers::types::Address;
use chrono::{DateTime, Utc};

use crate::error::WalletError;
use crate::walletmanager::api::WalletManagerApi;
use crate::users::subscription::user_subscription::UserSubscription;

/// Trait chung cho tất cả các loại user manager
/// Giúp chuẩn hóa các phương thức cơ bản và giảm trùng lặp mã nguồn
#[async_trait]
pub trait UserManager: Send + Sync + 'static {
    /// Kiểu dữ liệu người dùng mà manager quản lý
    type UserData;
    
    /// Kiểm tra xem người dùng có thể thực hiện giao dịch không
    async fn can_perform_transaction(&self, user_id: &str) -> Result<bool, WalletError>;
    
    /// Kiểm tra xem người dùng có thể sử dụng snipebot không
    async fn can_use_snipebot(&self, user_id: &str) -> Result<bool, WalletError>;
    
    /// Lấy thông tin người dùng từ cache hoặc database
    async fn load_user_data(&self, user_id: &str) -> Result<Option<Self::UserData>, WalletError>;
    
    /// Kiểm tra tính hợp lệ của subscription
    fn is_valid_subscription(&self, subscription: &UserSubscription) -> bool;
    
    /// Cập nhật dữ liệu người dùng đồng bộ với database và cache
    async fn save_user_data(&self, user_id: &str, data: Self::UserData) -> Result<(), WalletError>;
    
    /// Tạo token session mới khi trạng thái subscription thay đổi
    async fn refresh_session_token(&self, user_id: &str) -> Result<String, WalletError>;
    
    /// Bắt buộc đồng bộ cache với database
    async fn sync_cache_with_db(&self, user_id: &str) -> Result<(), WalletError>;
    
    /// Liên kết địa chỉ ví mới với người dùng
    async fn link_wallet_address(&self, user_id: &str, address: Address) -> Result<(), WalletError>;
}

// Chỉ export các API chính và ẩn chi tiết cài đặt
// Re-exports có kiểm soát
pub use free_user::manager::FreeUserManager;
pub use premium_user::PremiumUserManager;
pub use vip_user::VipUserManager;
pub use abstract_user_manager::SubscriptionBasedUserManager;

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
    pub use crate::users::abstract_user_manager::{
        SubscriptionBasedUserManager, create_jwt_token, save_user_data_to_db,
        is_subscription_valid
    };
}

// Re-export các hằng số chung quan trọng
pub mod constants {
    pub use crate::users::subscription::constants::{
        FREE_USER_AUTO_TRADE_MINUTES, PREMIUM_USER_AUTO_TRADE_HOURS, 
        VIP_USER_AUTO_TRADE_HOURS, VIP_RESET_HOUR
    };
}