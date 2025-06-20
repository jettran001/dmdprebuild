/// Module cung cấp các trait và thành phần trừu tượng hóa cho quản lý người dùng dựa trên subscription.
/// 
/// Module này giúp giảm sự trùng lặp code giữa các lớp quản lý người dùng như
/// PremiumUserManager và VipUserManager, cung cấp các phương thức chung và giao diện
/// thống nhất cho các loại người dùng dựa trên subscription.

// External imports
use chrono::{DateTime, Duration, Utc};
use ethers::types::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn, error, debug};
use uuid::Uuid;
use async_trait::async_trait;
use serde_json;
use jsonwebtoken::{self, EncodingKey, Header};

// Internal imports
use crate::error::WalletError;
use crate::walletmanager::api::WalletManagerApi;
use crate::users::subscription::types::{SubscriptionStatus, SubscriptionType, PaymentToken};
use crate::users::subscription::user_subscription::UserSubscription;
use crate::users::premium_user::Database;
use crate::cache::{invalidate_user_cache};
use crate::users::UserManager;

/// Trait cho các loại người dùng dựa trên gói đăng ký (Premium/VIP)
#[async_trait]
pub trait SubscriptionBasedUserManager<T>: Send + Sync + 'static {
    /// Kiểm tra xem người dùng có thể thực hiện giao dịch không dựa trên loại subscription
    async fn can_perform_transaction(&self, user_id: &str) -> Result<bool, WalletError>;
    
    /// Kiểm tra xem người dùng có thể sử dụng snipebot không dựa trên loại subscription
    async fn can_use_snipebot(&self, user_id: &str) -> Result<bool, WalletError>;
    
    /// Nạp dữ liệu người dùng từ database
    async fn load_user_data(&self, user_id: &str) -> Result<Option<T>, WalletError>;
    
    /// Lưu dữ liệu người dùng vào database
    async fn save_user_data(&self, user_data: &T) -> Result<(), WalletError>;
    
    /// Tạo token phiên mới cho người dùng
    async fn generate_session_token(&self, user_id: &str, subscription_type: SubscriptionType) -> Result<String, WalletError>;
    
    /// Làm mới token phiên cho người dùng
    async fn refresh_session_token(&self, user_id: &str) -> Result<String, WalletError>;
    
    /// Đồng bộ hóa cache với database
    async fn sync_cache_with_db(&self, user_id: &str) -> Result<(), WalletError>;
    
    /// Liên kết địa chỉ ví mới với người dùng
    async fn link_wallet_address(&self, user_id: &str, address: Address) -> Result<(), WalletError>;
    
    /// Cập nhật subscription trước khi hết hạn
    async fn renew_subscription(
        &self,
        user_id: &str,
        subscription: &UserSubscription,
        duration_months: u32
    ) -> Result<UserSubscription, WalletError>;
    
    /// Kiểm tra các subscription sắp hết hạn
    async fn check_expiring_subscriptions(
        &self,
        user_subscriptions: &HashMap<String, UserSubscription>,
        days_before: u32
    ) -> Result<Vec<String>, WalletError>;
    
    /// Kiểm tra tính hợp lệ của subscription
    fn is_valid_subscription(&self, subscription: &UserSubscription) -> bool;
}

/// Tạo JWT token cho phiên người dùng
///
/// Hàm tiện ích để tạo JWT token cho phiên người dùng với các thông tin cơ bản
///
/// # Arguments
/// * `user_id` - ID của người dùng
/// * `user_type` - Loại người dùng (premium, vip)
/// * `jwt_secret` - JWT secret key để ký token
/// * `expiration_days` - Số ngày token có hiệu lực
///
/// # Returns
/// * `Result<String, WalletError>` - JWT token hoặc lỗi
pub async fn create_jwt_token(
    user_id: &str,
    user_type: &str,
    jwt_secret: &str,
    expiration_days: u64
) -> Result<String, WalletError> {
    let now = chrono::Utc::now();
    let expiry = now + chrono::Duration::days(expiration_days as i64);
    
    // Claims cho JWT token
    let claims = serde_json::json!({
        "sub": user_id,
        "exp": expiry.timestamp() as usize,
        "iat": now.timestamp() as usize,
        "user_type": user_type,
        "version": 1 // Có thể sử dụng để vô hiệu hóa token nếu cần
    });
    
    // Tạo token JWT
    let token = jsonwebtoken::encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes())
    ).map_err(|e| WalletError::Other(format!("Không thể tạo JWT token: {}", e)))?;
    
    Ok(token)
}

/// Lưu dữ liệu người dùng với khả năng serialize
///
/// Hàm tiện ích để lưu dữ liệu người dùng có thể serialize vào database
///
/// # Arguments
/// * `db` - Kết nối database
/// * `user_id` - ID của người dùng
/// * `user_data` - Dữ liệu người dùng cần lưu
/// * `table_name` - Tên bảng trong database
///
/// # Returns
/// * `Result<(), WalletError>` - Thành công hoặc lỗi
pub async fn save_user_data_to_db<T: serde::Serialize>(
    db: &Database,
    user_id: &str,
    user_data: &T,
    table_name: &str
) -> Result<(), WalletError> {
    // Serialize dữ liệu người dùng
    let data = serde_json::to_string(user_data)
        .map_err(|e| WalletError::Other(format!("Lỗi serialize dữ liệu người dùng: {}", e)))?;
    
    // Tạo câu lệnh SQL động dựa trên tên bảng
    let query = format!(
        "INSERT OR REPLACE INTO {} (user_id, data, updated_at) VALUES (?, ?, ?)",
        table_name
    );
    
    // Thực thi query
    sqlx::query(&query)
        .bind(user_id)
        .bind(&data)
        .bind(Utc::now().timestamp())
        .execute(&db.pool)
        .await
        .map_err(|e| WalletError::Other(format!("Lỗi lưu dữ liệu người dùng: {}", e)))?;
    
    Ok(())
}

/// Kiểm tra subscription đã hết hạn chưa
///
/// # Arguments
/// * `subscription` - Subscription cần kiểm tra
///
/// # Returns
/// * `bool` - true nếu subscription còn hạn, false nếu đã hết hạn
pub fn is_subscription_valid(subscription: &UserSubscription) -> bool {
    // Kiểm tra trạng thái
    if subscription.status != SubscriptionStatus::Active {
        return false;
    }
    
    // Kiểm tra ngày hết hạn
    subscription.end_date > Utc::now()
} 