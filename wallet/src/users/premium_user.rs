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
use async_trait::async_trait;
use serde_json;
use jsonwebtoken;
use sqlx::{Row, SqlitePool, query};

// Internal imports
use crate::error::WalletError;
use crate::walletmanager::api::WalletManagerApi;
use crate::users::subscription::types::{SubscriptionStatus, SubscriptionType, PaymentToken};
use crate::users::subscription::user_subscription::UserSubscription;
use crate::cache::{get_premium_user, update_premium_user_cache, invalidate_user_cache};
use crate::users::UserManager;
use crate::users::abstract_user_manager::{SubscriptionBasedUserManager, create_jwt_token, save_user_data_to_db};

/// Database connection wrapper
pub struct Database {
    /// Connection pool
    pool: SqlitePool,
    /// JWT secret
    jwt_secret: String,
}

impl Database {
    /// Tạo database connection mới
    pub async fn new(connection_string: &str) -> Result<Self, WalletError> {
        let pool = SqlitePool::connect(connection_string)
            .await
            .map_err(|e| WalletError::Other(format!("Không thể kết nối database: {}", e)))?;
        
        Ok(Self {
            pool,
            jwt_secret: "default_secret_should_be_loaded_from_env".to_string(), // Trong thực tế nên load từ env
        })
    }
    
    /// Thực thi câu lệnh SQL
    pub async fn execute(&self, query: &str, params: &[&(dyn sqlx::encode::Encode<'_, sqlx::Sqlite> + Send + Sync)]) -> Result<(), WalletError> {
        sqlx::query(query)
            .bind_all_owned(params.to_vec())
            .execute(&self.pool)
            .await
            .map_err(|e| WalletError::Other(format!("Lỗi thực thi SQL: {}", e)))?;
        
        Ok(())
    }
    
    /// Lấy JWT secret
    pub async fn get_jwt_secret(&self) -> Result<String, WalletError> {
        Ok(self.jwt_secret.clone())
    }
    
    /// Cập nhật token session trong database
    pub async fn update_session_token(&self, user_id: &str, token: &str) -> Result<(), WalletError> {
        sqlx::query("INSERT OR REPLACE INTO user_sessions (user_id, token, created_at) VALUES (?, ?, ?)")
            .bind(user_id)
            .bind(token)
            .bind(Utc::now().timestamp())
            .execute(&self.pool)
            .await
            .map_err(|e| WalletError::Other(format!("Không thể cập nhật session token: {}", e)))?;
            
        Ok(())
    }
    
    /// Lấy thông tin subscription từ database
    pub async fn get_user_subscription(&self, user_id: &str) -> Result<UserSubscription, WalletError> {
        let row = sqlx::query("SELECT data FROM user_subscriptions WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| WalletError::Other(format!("Lỗi query subscription: {}", e)))?;
            
        if let Some(row) = row {
            let data: String = row.try_get("data")
                .map_err(|e| WalletError::Other(format!("Lỗi đọc dữ liệu: {}", e)))?;
                
            let subscription = serde_json::from_str(&data)
                .map_err(|e| WalletError::Other(format!("Lỗi deserialize subscription: {}", e)))?;
                
            Ok(subscription)
        } else {
            Err(WalletError::Other(format!("Không tìm thấy subscription cho user {}", user_id)))
        }
    }
    
    /// Load user data từ database
    pub async fn load_premium_user(&self, user_id: &str) -> Result<Option<PremiumUserData>, WalletError> {
        let row = sqlx::query("SELECT data FROM premium_users WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| WalletError::Other(format!("Lỗi query premium user: {}", e)))?;
            
        if let Some(row) = row {
            let data: String = row.try_get("data")
                .map_err(|e| WalletError::Other(format!("Lỗi đọc dữ liệu: {}", e)))?;
                
            let user_data = serde_json::from_str(&data)
                .map_err(|e| WalletError::Other(format!("Lỗi deserialize premium user: {}", e)))?;
                
            Ok(Some(user_data))
        } else {
            Ok(None)
        }
    }
}

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
    wallet_api: Arc<tokio::sync::RwLock<WalletManagerApi>>,
    /// Kết nối database
    db: Arc<Database>,
}

// Triển khai constructors
impl PremiumUserManager {
    /// Tạo một PremiumUserManager mới.
    ///
    /// # Arguments
    /// * `wallet_api` - API quản lý ví
    /// * `db` - Kết nối database
    pub fn new(wallet_api: Arc<tokio::sync::RwLock<WalletManagerApi>>, db: Arc<Database>) -> Self {
        Self {
            wallet_api,
            db,
        }
    }

    /// Kiểm tra xem người dùng có thể thực hiện giao dịch không.
    #[flow_from("wallet::api")]
    pub async fn can_perform_transaction(&self, user_id: &str) -> Result<bool, WalletError> {
        // Người dùng premium ít bị giới hạn hơn free users
        match get_premium_user(user_id, |id| async move {
            // Hàm loader, sẽ được gọi khi không tìm thấy trong cache
            match self.load_premium_user_from_db(&id).await {
                Ok(data) => data,
                Err(e) => {
                    warn!("Không thể tải thông tin premium user từ DB: {}", e);
                    None
                }
            }
        }).await {
            Some(user_data) => {
                // Kiểm tra xem subscription có còn hiệu lực
                if user_data.subscription_end_date > Utc::now() {
                    return Ok(true);
                }
            },
            None => {
                debug!("Không tìm thấy thông tin premium user cho ID: {}", user_id);
            }
        }
        
        // Kiểm tra trong subscription database - mặc định cho phép
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
        
        // Lấy lock để đồng bộ hóa các thao tác cập nhật
        // Đảm bảo việc cập nhật cache và database là atomic
        let _guard = PREMIUM_USER_LOCK.lock().await;
        
        // Cập nhật database trước để dữ liệu không bị mất nếu xảy ra lỗi
        let mut premium_user_data = match get_premium_user(user_id, |id| async move {
            self.load_premium_user_from_db(&id).await.unwrap_or(None)
        }).await {
            Some(data) => data,
            None => {
                // Nếu không tìm thấy trong cache, tải trực tiếp từ database
                match self.load_premium_user_from_db(user_id).await {
                    Ok(Some(data)) => data,
                    Ok(None) => {
                        // Nếu không tồn tại trong database, tạo mới
                        PremiumUserData {
                            user_id: user_id.to_string(),
                            created_at: Utc::now(),
                            last_active: Utc::now(),
                            wallet_addresses: vec![subscription.payment_address],
                            subscription_end_date: new_end_date,
                            auto_renew: true,
                            payment_history: vec![],
                        }
                    },
                    Err(e) => {
                        error!("Lỗi khi tải dữ liệu premium user từ database: {}", e);
                        return Err(e);
                    }
                }
            }
        };
        
        // Cập nhật thông tin premium user
        premium_user_data.subscription_end_date = new_end_date;
        premium_user_data.last_active = Utc::now();
        
        // Cập nhật vào database
        if let Err(e) = self.save_premium_user_to_db(&premium_user_data).await {
            error!("Lỗi khi lưu premium user vào database: {}", e);
            return Err(e);
        }
        
        // Sau khi cập nhật database thành công, cập nhật cache
        update_premium_user_cache(user_id, premium_user_data.clone()).await;
        
        // Ghi log
        info!("Gia hạn thành công Premium subscription cho user {}, hết hạn mới: {}", 
              user_id, new_end_date.format("%Y-%m-%d"));
        
        // Đồng bộ với các module liên quan
        if let Err(e) = self.sync_related_modules(user_id).await {
            warn!("Có lỗi khi đồng bộ hóa với các module liên quan: {}", e);
            // Không return error ở đây để tiếp tục luồng xử lý chính
        }
        
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
        // Tải từ database thật
        self.db.load_premium_user(user_id).await
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
    pub async fn update_auto_renew(&self, user_id: &str, auto_renew: bool) -> Result<(), WalletError> {
        // Sử dụng phương thức từ UserManager trait để đảm bảo đồng bộ với database
        let user_data_opt = self.load_user_data(user_id).await?;
        
        if let Some(mut user_data) = user_data_opt {
            user_data.auto_renew = auto_renew;
            // Cập nhật cả database và cache
            self.save_user_data(user_id, user_data).await?;
            
            info!("Đã cập nhật tự động gia hạn cho Premium user {} thành {}", user_id, auto_renew);
            
            // Nếu thông tin đăng ký thay đổi, refresh session
            self.refresh_session_token(user_id).await?;
            
            return Ok(());
        }
        
        Err(WalletError::Other(format!("Không tìm thấy Premium user {}", user_id)))
    }

    /// Lưu thông tin người dùng Premium vào database
    async fn save_premium_user_to_db(&self, user_data: &PremiumUserData) -> Result<(), WalletError> {
        // Dữ liệu cần lưu
        let serialized = serde_json::to_string(&user_data)
            .map_err(|e| WalletError::Other(format!("Lỗi serialize dữ liệu: {}", e)))?;

        // Lưu vào database
        self.db.execute(
            "INSERT OR REPLACE INTO premium_users (user_id, data) VALUES (?, ?)",
            &[&user_data.user_id, &serialized],
        ).await.map_err(|e| WalletError::Other(format!("Lỗi database: {}", e)))?;
        
        info!("Đã lưu dữ liệu Premium user {} vào database", user_data.user_id);
        Ok(())
    }

    /// Tạo JWT session token mới cho người dùng
    async fn generate_session_token(&self, user_id: &str, subscription_type: SubscriptionType) -> Result<String, WalletError> {
        // JWT payload
        let claims = SessionClaims {
            sub: user_id.to_string(),
            exp: (Utc::now() + Duration::days(7)).timestamp() as usize,
            iat: Utc::now().timestamp() as usize,
            user_type: subscription_type.to_string(),
            version: 1,
        };
        
        // Tải JWT secret
        let jwt_secret = self.db.get_jwt_secret().await
            .map_err(|e| WalletError::Other(format!("Không thể lấy JWT secret: {}", e)))?;
        
        // Tạo token
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(jwt_secret.as_bytes()),
        ).map_err(|e| WalletError::Other(format!("Lỗi tạo JWT token: {}", e)))?;
        
        Ok(token)
    }

    /// Đồng bộ hóa thông tin với các module liên quan.
    ///
    /// Thông báo cho các module khác như auto_trade, nft, v.v. khi có thay đổi
    /// trong thông tin premium user để đảm bảo tính nhất quán của dữ liệu.
    ///
    /// # Arguments
    /// * `user_id` - ID của người dùng cần đồng bộ hóa
    ///
    /// # Returns
    /// * `Ok(())` - Nếu đồng bộ hóa thành công
    /// * `Err(WalletError)` - Nếu có lỗi xảy ra
    async fn sync_related_modules(&self, user_id: &str) -> Result<(), WalletError> {
        // TODO: Thêm logic để thông báo cho các module khác
        // Ví dụ:
        // - Thông báo cho AutoTradeManager
        // - Thông báo cho VipNftManager
        // - Thông báo cho SubscriptionManager
        
        Ok(())
    }
}

// Cấu trúc dữ liệu JWT claims
#[derive(Debug, Serialize, Deserialize)]
struct SessionClaims {
    sub: String,  // user_id
    exp: usize,   // Thời gian hết hạn
    iat: usize,   // Thời gian tạo
    user_type: String, // Loại người dùng
    version: usize, // Version để có thể invalidate tất cả các token khi cần
}

// Triển khai trait UserManager cho PremiumUserManager
#[async_trait]
impl UserManager for PremiumUserManager {
    type UserData = PremiumUserData;
    
    async fn can_perform_transaction(&self, user_id: &str) -> Result<bool, WalletError> {
        <Self as SubscriptionBasedUserManager<PremiumUserData>>::can_perform_transaction(self, user_id).await
    }
    
    async fn can_use_snipebot(&self, user_id: &str) -> Result<bool, WalletError> {
        <Self as SubscriptionBasedUserManager<PremiumUserData>>::can_use_snipebot(self, user_id).await
    }
    
    async fn load_user_data(&self, user_id: &str) -> Result<Option<Self::UserData>, WalletError> {
        <Self as SubscriptionBasedUserManager<PremiumUserData>>::load_user_data(self, user_id).await
    }
    
    fn is_valid_subscription(&self, subscription: &UserSubscription) -> bool {
        <Self as SubscriptionBasedUserManager<PremiumUserData>>::is_valid_subscription(self, subscription)
    }
    
    async fn save_user_data(&self, user_id: &str, data: Self::UserData) -> Result<(), WalletError> {
        <Self as SubscriptionBasedUserManager<PremiumUserData>>::save_user_data(self, &data).await
    }
    
    async fn refresh_session_token(&self, user_id: &str) -> Result<String, WalletError> {
        <Self as SubscriptionBasedUserManager<PremiumUserData>>::refresh_session_token(self, user_id).await
    }
    
    async fn sync_cache_with_db(&self, user_id: &str) -> Result<(), WalletError> {
        <Self as SubscriptionBasedUserManager<PremiumUserData>>::sync_cache_with_db(self, user_id).await
    }
    
    async fn link_wallet_address(&self, user_id: &str, address: Address) -> Result<(), WalletError> {
        <Self as SubscriptionBasedUserManager<PremiumUserData>>::link_wallet_address(self, user_id, address).await
    }
}

// Hằng số giới hạn của premium user - cao hơn so với free user
pub const MAX_WALLETS_PREMIUM_USER: usize = 10;
pub const MAX_TRANSACTIONS_PER_DAY: usize = 100;
pub const MAX_SNIPEBOT_ATTEMPTS_PER_DAY: usize = 20;
pub const MAX_SNIPEBOT_ATTEMPTS_PER_WALLET: usize = 5;

// Thêm mutex để đồng bộ hóa các thao tác cập nhật premium user
lazy_static! {
    static ref PREMIUM_USER_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::new(());
}
