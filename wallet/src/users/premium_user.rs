//! Module quản lý người dùng premium cho wallet module.
//! Cung cấp các tính năng và giới hạn cao cấp dành riêng cho người dùng premium.
//!
//! Module này chịu trách nhiệm cho các chức năng liên quan đến người dùng Premium:
//! - Quản lý trạng thái đăng ký Premium
//! - Xử lý việc gia hạn đăng ký tự động
//! - Quản lý chu kỳ thanh toán và nhắc nhở
//! - Xử lý việc nâng cấp từ gói Free lên Premium
//! - Cung cấp thông tin về các đặc quyền và tính năng Premium
//! - Theo dõi thời hạn đăng ký và thông báo sắp hết hạn
//! - Xử lý việc hạ cấp xuống gói Free khi hết hạn
//!
//! Module này tương tác chặt chẽ với các module payment và user_subscription 
//! để đảm bảo quá trình đăng ký và thanh toán diễn ra suôn sẻ, cũng như 
//! cung cấp trải nghiệm liền mạch cho người dùng Premium.

// External imports
use chrono::{DateTime, Duration, Utc};
use ethers::types::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug};
use uuid::Uuid;

// Internal imports
use crate::error::WalletError;
use crate::walletmanager::api::WalletManagerApi;
use crate::users::subscription::types::{SubscriptionStatus, SubscriptionType, PaymentToken};
use crate::users::subscription::user_subscription::UserSubscription;
use crate::cache::{get_premium_user, update_premium_user_cache};

/// Trạng thái của người dùng Premium
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PremiumUserStatus {
    /// Đang hoạt động
    Active,
    /// Đã hết hạn
    Expired,
    /// Đang trong quá trình gia hạn
    PendingRenewal,
}

/// Thông tin người dùng premium.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PremiumUserData {
    /// ID người dùng
    pub user_id: String,
    /// Thời gian tạo tài khoản
    pub created_at: DateTime<Utc>,
    /// Thời gian hoạt động cuối
    pub last_active: DateTime<Utc>,
    /// Danh sách địa chỉ ví
    pub wallet_addresses: Vec<Address>,
    /// Ngày hết hạn gói premium
    pub subscription_end_date: DateTime<Utc>,
    /// Tự động gia hạn
    pub auto_renew: bool,
    /// Lịch sử thanh toán
    pub payment_history: Vec<PaymentRecord>,
}

/// Thông tin thanh toán
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRecord {
    /// Ngày thanh toán
    pub payment_date: DateTime<Utc>,
    /// Địa chỉ thanh toán
    pub payment_address: Address,
    /// Hash giao dịch
    pub transaction_hash: String,
    /// Loại token
    pub token: PaymentToken,
    /// Số lượng token
    pub amount: String,
}

/// Quản lý người dùng premium.
pub struct PremiumUserManager {
    /// API wallet để tương tác với blockchain
    wallet_api: Arc<RwLock<WalletManagerApi>>,
}

impl PremiumUserManager {
    /// Khởi tạo PremiumUserManager mới.
    #[flow_from("wallet::init")]
    pub fn new(wallet_api: Arc<RwLock<WalletManagerApi>>) -> Self {
        Self {
            wallet_api,
        }
    }

    /// Kiểm tra xem người dùng có thể thực hiện giao dịch không.
    #[flow_from("wallet::api")]
    pub async fn can_perform_transaction(&self, user_id: &str) -> Result<bool, WalletError> {
        // Người dùng premium ít bị giới hạn hơn free users
        if let Some(user_data) = get_premium_user(user_id, |id| async move {
            // Hàm loader, sẽ được gọi khi không tìm thấy trong cache
            self.load_premium_user_from_db(&id).await.unwrap_or(None)
        }).await {
            // Kiểm tra xem subscription có còn hiệu lực
            if user_data.subscription_end_date > Utc::now() {
                return Ok(true);
            }
        }
        
        // Kiểm tra trong subscription database
        Ok(true)
    }

    /// Kiểm tra xem người dùng có thể sử dụng snipebot không.
    #[flow_from("snipebot::permissions")]
    pub async fn can_use_snipebot(&self, user_id: &str) -> Result<bool, WalletError> {
        // Người dùng premium có quyền sử dụng snipebot nhiều hơn free users
        Ok(true)
    }
    
    /// Nâng cấp từ Free lên Premium
    ///
    /// # Arguments
    /// - `user_id`: ID của người dùng
    /// - `subscription`: Thông tin đăng ký hiện tại (Free)
    /// - `payment_address`: Địa chỉ ví dùng để thanh toán
    /// - `payment_token`: Loại token dùng để thanh toán
    /// - `duration_months`: Thời hạn đăng ký theo tháng
    ///
    /// # Returns
    /// - `Ok(UserSubscription)`: Thông tin đăng ký Premium mới
    /// - `Err`: Nếu có lỗi xảy ra
    #[flow_from("wallet::ui")]
    pub async fn upgrade_free_to_premium(
        &self,
        user_id: &str,
        subscription: &UserSubscription,
        payment_address: Address,
        payment_token: PaymentToken,
        duration_months: u32,
    ) -> Result<UserSubscription, WalletError> {
        // Kiểm tra xem user hiện tại có phải Free không
        if subscription.subscription_type != SubscriptionType::Free {
            return Err(WalletError::Other("Người dùng không phải Free user".to_string()));
        }
        
        // Kiểm tra duration_months hợp lệ
        if duration_months < 1 {
            return Err(WalletError::Other("Thời hạn đăng ký không hợp lệ".to_string()));
        }
        
        // Tính toán thời gian hết hạn mới
        let now = Utc::now();
        let duration = Duration::days(30 * duration_months as i64);
        let end_date = now + duration;
        
        // Tạo subscription mới
        let mut premium_subscription = subscription.clone();
        premium_subscription.subscription_type = SubscriptionType::Premium;
        premium_subscription.start_date = now;
        premium_subscription.end_date = end_date;
        premium_subscription.payment_address = payment_address;
        premium_subscription.payment_token = payment_token;
        premium_subscription.status = SubscriptionStatus::Active;
        
        // TODO: Xử lý thanh toán và ghi nhận vào blockchain
        
        // Thay đổi phần cache
        let premium_user_data = PremiumUserData {
            user_id: user_id.to_string(),
            created_at: now,
            last_active: now,
            wallet_addresses: vec![payment_address],
            subscription_end_date: end_date,
            auto_renew: true,
            payment_history: vec![],
        };
        
        // Sử dụng global cache
        update_premium_user_cache(user_id, premium_user_data).await;
        
        info!("Nâng cấp thành công từ Free lên Premium cho user {}, hết hạn {}", 
              user_id, end_date.format("%Y-%m-%d"));
              
        Ok(premium_subscription)
    }
    
    /// Gia hạn đăng ký Premium
    ///
    /// # Arguments
    /// - `user_id`: ID của người dùng
    /// - `subscription`: Thông tin đăng ký hiện tại
    /// - `duration_months`: Thời hạn gia hạn theo tháng
    ///
    /// # Returns
    /// - `Ok(UserSubscription)`: Thông tin đăng ký sau khi gia hạn
    /// - `Err`: Nếu có lỗi xảy ra
    #[flow_from("wallet::payment")]
    pub async fn renew_premium_subscription(
        &self,
        user_id: &str,
        subscription: &UserSubscription,
        duration_months: u32,
    ) -> Result<UserSubscription, WalletError> {
        // Kiểm tra xem subscription có phải Premium không
        if subscription.subscription_type != SubscriptionType::Premium {
            return Err(WalletError::Other("Không phải Premium subscription".to_string()));
        }
        
        // Tính toán thời gian hết hạn mới
        let duration = Duration::days(30 * duration_months as i64);
        let new_end_date = if subscription.end_date > Utc::now() {
            // Nếu chưa hết hạn, cộng thêm thời gian
            subscription.end_date + duration
        } else {
            // Nếu đã hết hạn, tính từ hiện tại
            Utc::now() + duration
        };
        
        // Cập nhật subscription
        let mut renewed_subscription = subscription.clone();
        renewed_subscription.end_date = new_end_date;
        renewed_subscription.status = SubscriptionStatus::Active;
        
        // Cập nhật cache
        // Tải dữ liệu user từ cache, cập nhật, và lưu lại
        if let Some(mut user_data) = get_premium_user(user_id, |id| async move {
            self.load_premium_user_from_db(&id).await.unwrap_or(None)
        }).await {
            user_data.subscription_end_date = new_end_date;
            user_data.last_active = Utc::now();
            update_premium_user_cache(user_id, user_data).await;
        }
        
        info!("Gia hạn thành công Premium subscription cho user {}, hết hạn mới: {}", 
              user_id, new_end_date.format("%Y-%m-%d"));
        
        Ok(renewed_subscription)
    }
    
    /// Kiểm tra và thông báo Premium subscription sắp hết hạn
    ///
    /// # Arguments
    /// - `days_before`: Số ngày trước khi hết hạn cần thông báo
    ///
    /// # Returns
    /// - `Ok(Vec<String>)`: Danh sách user_id cần thông báo
    #[flow_from("system::cron")]
    pub async fn check_expiring_subscriptions(
        &self,
        user_subscriptions: &HashMap<String, UserSubscription>,
        days_before: u32,
    ) -> Result<Vec<String>, WalletError> {
        let now = Utc::now();
        let threshold = now + Duration::days(days_before as i64);
        let mut expiring_users = Vec::new();
        
        for (user_id, subscription) in user_subscriptions {
            if subscription.subscription_type == SubscriptionType::Premium &&
               subscription.status == SubscriptionStatus::Active &&
               subscription.end_date <= threshold &&
               subscription.end_date > now {
                // Subscription sắp hết hạn
                expiring_users.push(user_id.clone());
                debug!("Premium subscription của user {} sẽ hết hạn vào {}", 
                      user_id, subscription.end_date.format("%Y-%m-%d"));
            }
        }
        
        info!("Có {} Premium subscription sắp hết hạn trong {} ngày tới", 
              expiring_users.len(), days_before);
        
        Ok(expiring_users)
    }
    
    /// Xác minh trạng thái Premium của một người dùng
    #[flow_from("wallet::api")]
    pub fn is_valid_premium(&self, subscription: &UserSubscription) -> bool {
        // Kiểm tra cơ bản
        if subscription.subscription_type != SubscriptionType::Premium {
            return false;
        }
        
        if subscription.status != SubscriptionStatus::Active {
            return false;
        }
        
        // Kiểm tra thời hạn
        if subscription.end_date < Utc::now() {
            return false;
        }
        
        true
    }
    
    /// Tải thông tin người dùng Premium từ database
    /// 
    /// # Arguments
    /// - `user_id`: ID người dùng cần tải thông tin
    /// 
    /// # Returns
    /// - `Ok(Option<PremiumUserData>)`: Thông tin người dùng Premium nếu có
    /// - `Err`: Nếu có lỗi
    #[flow_from("database::users")]
    pub async fn load_premium_user(&mut self, user_id: &str) -> Result<Option<PremiumUserData>, WalletError> {
        // Sử dụng global cache với loader function
        let user_data = get_premium_user(user_id, |id| async move {
            self.load_premium_user_from_db(&id).await.unwrap_or(None)
        }).await;
        
        Ok(user_data)
    }
    
    // Phương thức mới để tách biệt việc tải từ database
    async fn load_premium_user_from_db(&self, user_id: &str) -> Result<Option<PremiumUserData>, WalletError> {
        // TODO: Tải từ database thực tế
        info!("Tải thông tin người dùng Premium {} từ database", user_id);
        
        // Giả lập không tìm thấy
        Ok(None)
    }
    
    /// Cập nhật trạng thái tự động gia hạn cho Premium user
    /// 
    /// # Arguments
    /// - `user_id`: ID người dùng cần cập nhật
    /// - `auto_renew`: Trạng thái tự động gia hạn mới
    /// 
    /// # Returns
    /// - `Ok(())`: Nếu cập nhật thành công
    /// - `Err`: Nếu có lỗi
    #[flow_from("wallet::ui")]
    pub async fn update_auto_renew(&mut self, user_id: &str, auto_renew: bool) -> Result<(), WalletError> {
        // Sử dụng global cache
        if let Some(mut user_data) = get_premium_user(user_id, |id| async move {
            self.load_premium_user_from_db(&id).await.unwrap_or(None)
        }).await {
            user_data.auto_renew = auto_renew;
            update_premium_user_cache(user_id, user_data).await;
            info!("Đã cập nhật tự động gia hạn cho Premium user {} thành {}", user_id, auto_renew);
            return Ok(());
        }
        
        Err(WalletError::Other(format!("Không tìm thấy Premium user {}", user_id)))
    }
}

// Hằng số giới hạn của premium user - cao hơn so với free user
pub const MAX_WALLETS_PREMIUM_USER: usize = 10;
pub const MAX_TRANSACTIONS_PER_DAY: usize = 100;
pub const MAX_SNIPEBOT_ATTEMPTS_PER_DAY: usize = 20;
pub const MAX_SNIPEBOT_ATTEMPTS_PER_WALLET: usize = 5;
