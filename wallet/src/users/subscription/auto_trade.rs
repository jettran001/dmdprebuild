//! Module auto_trade cung cấp chức năng giao dịch tự động dựa trên gói đăng ký
//! của người dùng trong hệ thống DiamondChain.
//! 
//! Module này cung cấp các chức năng:
//! * Theo dõi và quản lý hạn mức giao dịch tự động của người dùng
//! * Xác minh quyền sử dụng chức năng giao dịch tự động dựa trên loại đăng ký
//! * Theo dõi số lượng giao dịch đã thực hiện và giới hạn còn lại
//! * Thiết lập các giới hạn giao dịch tự động khác nhau cho mỗi loại đăng ký
//! * Cập nhật và làm mới hạn mức giao dịch khi chu kỳ đăng ký mới bắt đầu
//!
//! Module này tương tác với SubscriptionManager và tập trung vào việc duy trì
//! sự an toàn khi truy cập trong môi trường bất đồng bộ và đa luồng, tránh
//! các vấn đề như deadlock và race condition thông qua việc sử dụng thích hợp
//! các cơ chế đồng bộ hóa như Weak reference, Arc và RwLock.

// External imports
use chrono::{DateTime, Duration, Utc};
use log::{debug, error, info, warn};
use thiserror::Error;
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use tokio::sync::RwLock;
use std::time::Duration;
use tokio::time::sleep;
use std::sync::atomic::{AtomicU32, Ordering};

// Standard library imports
use std::fmt;

// Internal imports
use crate::error::WalletError;
use crate::users::subscription::{
    constants::{
        FREE_USER_AUTO_TRADE_MINUTES, FREE_USER_RESET_DAYS, PREMIUM_USER_AUTO_TRADE_HOURS,
        VIP_USER_AUTO_TRADE_HOURS, VIP_STAKE_TOTAL_AUTO_TRADE_HOURS, VIP_STAKE_BONUS_AUTO_TRADE_HOURS,
        AUTO_TRADE_TIME_WARNING_THRESHOLD, AUTO_TRADE_WARNING_MINUTES,
    },
    types::{SubscriptionType, SubscriptionStatus, UserSubscription},
    events::{EventEmitter, SubscriptionEvent, EventType},
};
use crate::cache;
use crate::database::Database;

/// Các lỗi có thể xảy ra trong quá trình xử lý auto-trade.
///
/// Enum này bao gồm tất cả các lỗi có thể xảy ra trong quá trình 
/// xử lý và quản lý thời gian auto-trade của người dùng.
#[derive(Error, Debug)]
pub enum AutoTradeError {
    /// Xảy ra khi người dùng không còn thời gian auto-trade.
    #[error("Không còn thời gian auto-trade")]
    NoRemainingTime,
    
    /// Xảy ra khi không tìm thấy auto_trade_usage cho user_id.
    #[error("Không tìm thấy auto-trade usage cho user_id: {0}")]
    NotFound(String),
    
    /// Lỗi truy cập database như kết nối bị ngắt hoặc lỗi truy vấn.
    #[error("Lỗi database: {0}")]
    Database(String),
    
    /// Lỗi đồng bộ hóa khi truy cập các tài nguyên được chia sẻ.
    #[error("Lỗi đồng bộ hóa: {0}")]
    SyncError(String),
    
    /// Lỗi xác thực như không đủ quyền hoặc token hết hạn.
    #[error("Lỗi xác thực: {0}")]
    AuthError(String),
    
    /// Lỗi khi không thể truy cập tham chiếu (reference) do đã bị giải phóng.
    #[error("Tham chiếu không hợp lệ: {0}")]
    InvalidReferenceError(String),
    
    /// Các lỗi khác không thuộc các loại trên.
    #[error("Lỗi khác: {0}")]
    Other(String),
}

/// Trạng thái của auto-trade.
///
/// Enum này biểu diễn các trạng thái có thể của auto-trade usage,
/// cho phép theo dõi chính xác trạng thái hiện tại của từng người dùng.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AutoTradeStatus {
    /// Đang hoạt động: Auto-trade đang được sử dụng.
    Active,
    
    /// Không hoạt động: Auto-trade được cấu hình nhưng không hoạt động.
    Inactive,
    
    /// Tạm dừng: Auto-trade tạm thời bị tạm dừng bởi người dùng.
    Paused,
    
    /// Hết hạn: Đã hết thời gian auto-trade được cấp phép.
    Expired,
}

/// Chuyển đổi AutoTradeStatus thành chuỗi để hiển thị.
impl fmt::Display for AutoTradeStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AutoTradeStatus::Active => write!(f, "Đang hoạt động"),
            AutoTradeStatus::Inactive => write!(f, "Không hoạt động"),
            AutoTradeStatus::Paused => write!(f, "Tạm dừng"),
            AutoTradeStatus::Expired => write!(f, "Hết hạn"),
        }
    }
}

/// Thông tin sử dụng auto-trade của người dùng.
///
/// Cấu trúc này lưu trữ thông tin chi tiết về thời gian auto-trade
/// của người dùng, bao gồm thời gian còn lại và trạng thái hiện tại.
#[derive(Debug, Clone)]
pub struct AutoTradeUsage {
    /// User ID của người dùng sở hữu auto trade usage này
    pub user_id: String,
    
    /// Trạng thái hiện tại của auto trade (Active, Inactive, Paused, Expired)
    pub status: AutoTradeStatus,
    
    /// Thời gian bắt đầu sử dụng auto trade
    pub start_time: DateTime<Utc>,
    
    /// Thời gian còn lại tính bằng phút
    pub remaining_minutes: i64,
    
    /// Thời gian sử dụng auto trade gần nhất
    pub last_used: Option<DateTime<Utc>>,
    
    /// Thời gian reset tiếp theo (chỉ áp dụng cho gói Free)
    pub reset_time: Option<DateTime<Utc>>,
    
    /// Đánh dấu đã gửi cảnh báo sắp hết thời gian chưa
    pub warning_sent: bool,
}

impl AutoTradeUsage {
    /// Tạo mới đối tượng AutoTradeUsage
    /// 
    /// Phương thức này tạo một đối tượng AutoTradeUsage mới với thời gian sử dụng auto-trade
    /// được tính dựa trên loại gói đăng ký và trạng thái VIP staking của người dùng.
    /// Mỗi loại gói đăng ký có số giờ/phút auto-trade khác nhau:
    /// - Free: số phút giới hạn (mặc định là FREE_USER_AUTO_TRADE_MINUTES)
    /// - Premium: số giờ cao hơn (mặc định là PREMIUM_USER_AUTO_TRADE_HOURS)
    /// - VIP: số giờ cao nhất (mặc định là VIP_USER_AUTO_TRADE_HOURS, hoặc 
    ///   VIP_STAKE_TOTAL_AUTO_TRADE_HOURS nếu là VIP 12 tháng)
    /// 
    /// # Arguments
    /// * `user_id` - ID người dùng
    /// * `subscription_type` - Loại gói đăng ký
    /// * `is_vip_staking` - `true` nếu người dùng đang sử dụng gói VIP 12 tháng (staking), `false` nếu không
    /// 
    /// # Returns
    /// Đối tượng AutoTradeUsage mới với thời gian sử dụng và trạng thái mặc định
    /// 
    /// # Examples
    /// ```
    /// use wallet::users::subscription::auto_trade::AutoTradeUsage;
    /// use wallet::users::subscription::types::SubscriptionType;
    /// 
    /// // Tạo auto trade usage cho người dùng Premium
    /// let usage = AutoTradeUsage::new("user123", &SubscriptionType::Premium, false);
    /// assert_eq!(usage.status, AutoTradeStatus::Inactive);
    /// ```
    pub fn new(user_id: &str, subscription_type: &SubscriptionType, is_vip_staking: bool) -> Self {
        let now = Utc::now();
        let remaining_minutes = match subscription_type {
            SubscriptionType::Free => FREE_USER_AUTO_TRADE_MINUTES,
            SubscriptionType::Premium => PREMIUM_USER_AUTO_TRADE_HOURS * 60,
            SubscriptionType::VIP => {
                if is_vip_staking {
                    // Gói VIP 12 tháng với thời gian auto-trade bổ sung
                    VIP_STAKE_TOTAL_AUTO_TRADE_HOURS * 60
                } else {
                    // Gói VIP thông thường
                    VIP_USER_AUTO_TRADE_HOURS * 60
                }
            },
            _ => FREE_USER_AUTO_TRADE_MINUTES, // Mặc định cho các gói khác
        };
        
        let reset_time = if matches!(subscription_type, SubscriptionType::Free) {
            Some(now + Duration::days(FREE_USER_RESET_DAYS))
        } else {
            None
        };
        
        Self {
            user_id: user_id.to_string(),
            status: AutoTradeStatus::Inactive,
            start_time: now,
            remaining_minutes,
            last_used: None,
            reset_time,
            warning_sent: false,
        }
    }
    
    /// Tạo mới đối tượng AutoTradeUsage với các giá trị mặc định
    /// 
    /// Phương thức này là phiên bản rút gọn của `new()`, đặt `is_vip_staking` thành `false`.
    /// Sử dụng khi không cần quan tâm đến trạng thái VIP staking của người dùng.
    /// 
    /// # Arguments
    /// * `user_id` - ID người dùng
    /// * `subscription_type` - Loại gói đăng ký
    /// 
    /// # Returns
    /// Đối tượng AutoTradeUsage mới với thời gian sử dụng và trạng thái mặc định
    pub fn new_default(user_id: &str, subscription_type: &SubscriptionType) -> Self {
        Self::new(user_id, subscription_type, false)
    }
    
    /// Cập nhật thời gian còn lại cho auto trade
    /// 
    /// Giảm thời gian còn lại của auto trade dựa trên số phút đã sử dụng.
    /// Nếu thời gian còn lại không đủ, trả về lỗi NoRemainingTime.
    /// Nếu thời gian còn lại sau khi giảm bằng 0, trạng thái sẽ chuyển thành Expired.
    /// 
    /// # Arguments
    /// * `used_minutes` - Số phút đã sử dụng
    /// 
    /// # Returns
    /// * `Ok(())` - Nếu cập nhật thành công
    /// * `Err(AutoTradeError::NoRemainingTime)` - Nếu không đủ thời gian còn lại
    pub fn update_time(&mut self, used_minutes: i64) -> Result<(), AutoTradeError> {
        if self.remaining_minutes < used_minutes {
            return Err(AutoTradeError::NoRemainingTime);
        }
        
        self.remaining_minutes -= used_minutes;
        self.last_used = Some(Utc::now());
        
        if self.remaining_minutes <= 0 {
            self.status = AutoTradeStatus::Expired;
        }
        
        Ok(())
    }
    
    /// Kiểm tra có thể sử dụng auto trade không
    /// 
    /// Xác định xem người dùng có thể sử dụng tính năng auto trade trong khoảng thời gian
    /// `duration_minutes` hay không. Điều kiện là auto trade phải đang ở trạng thái Active
    /// và thời gian còn lại phải lớn hơn hoặc bằng `duration_minutes`.
    /// 
    /// # Arguments
    /// * `duration_minutes` - Thời gian cần sử dụng (phút)
    /// 
    /// # Returns
    /// * `true` - Nếu còn đủ thời gian và đang ở trạng thái Active
    /// * `false` - Nếu không đủ thời gian hoặc không ở trạng thái Active
    pub fn can_use(&self, duration_minutes: i64) -> bool {
        match self.status {
            AutoTradeStatus::Active => self.remaining_minutes >= duration_minutes,
            _ => false,
        }
    }
    
    /// Đồng bộ trạng thái và thời gian còn lại với gói subscription hiện tại
    /// 
    /// Cập nhật thời gian còn lại và trạng thái của auto trade dựa trên thông tin
    /// subscription hiện tại của người dùng. Nếu subscription không còn hoạt động,
    /// auto trade sẽ được đặt về trạng thái Inactive.
    /// 
    /// # Arguments
    /// * `subscription` - Thông tin subscription hiện tại của người dùng
    pub fn sync_with_subscription(&mut self, subscription: &UserSubscription) {
        info!("Đồng bộ auto-trade với subscription: user={}, type={:?}", 
              self.user_id, subscription.subscription_type);
        
        // Kiểm tra xem subscription có hoạt động không
        if !subscription.is_active() {
            // Nếu subscription không còn hoạt động, đặt trạng thái thành Inactive
            self.status = AutoTradeStatus::Inactive;
            debug!("Subscription không hoạt động, đặt auto-trade thành Inactive");
            return;
        }
        
        let is_vip_staking = subscription.is_twelve_month_vip();
        
        // Đặt thời gian còn lại dựa vào loại subscription
        match subscription.subscription_type {
            SubscriptionType::Free => {
                // Không thay đổi thời gian còn lại nếu đang là Free
                // Nhưng vẫn đặt thời gian reset
                let now = Utc::now();
                if self.reset_time.is_none() {
                    self.reset_time = Some(now + Duration::days(FREE_USER_RESET_DAYS));
                }
            },
            SubscriptionType::Premium => {
                // Cập nhật lại thời gian cho người dùng Premium
                let minutes = PREMIUM_USER_AUTO_TRADE_HOURS * 60;
                if self.remaining_minutes < minutes {
                    debug!("Cập nhật thời gian cho Premium: {} phút", minutes);
                    self.remaining_minutes = minutes;
                }
                // Premium không có thời gian reset
                self.reset_time = None;
            },
            SubscriptionType::VIP => {
                // Cập nhật lại thời gian cho người dùng VIP
                let minutes = if is_vip_staking {
                    (VIP_USER_AUTO_TRADE_HOURS + VIP_STAKE_BONUS_AUTO_TRADE_HOURS) * 60
                } else {
                    VIP_USER_AUTO_TRADE_HOURS * 60
                };
                
                if self.remaining_minutes < minutes {
                    debug!("Cập nhật thời gian cho VIP: {} phút", minutes);
                    self.remaining_minutes = minutes;
                }
                // VIP không có thời gian reset
                self.reset_time = None;
            }
        }
        
        // Nếu trạng thái là Expired nhưng vẫn còn thời gian, đặt lại thành Inactive
        if self.status == AutoTradeStatus::Expired && self.remaining_minutes > 0 {
            self.status = AutoTradeStatus::Inactive;
        }
    }
    
    /// Kiểm tra và gửi cảnh báo nếu thời gian còn lại ít
    /// 
    /// Kiểm tra xem thời gian còn lại có dưới ngưỡng cảnh báo không 
    /// (mặc định là AUTO_TRADE_WARNING_MINUTES). Nếu có và chưa gửi cảnh báo trước đó,
    /// một sự kiện cảnh báo sẽ được phát ra thông qua event_emitter.
    /// 
    /// # Arguments
    /// * `event_emitter` - Đối tượng dùng để phát sự kiện, hoặc None nếu không cần phát sự kiện
    /// 
    /// # Returns
    /// * `true` - Nếu đã gửi cảnh báo trong lần gọi này
    /// * `false` - Nếu không cần gửi cảnh báo hoặc đã gửi trước đó
    pub fn check_and_send_warning(&mut self, event_emitter: Option<&EventEmitter>) -> bool {
        // Nếu đã gửi cảnh báo rồi hoặc không còn hoạt động, không gửi nữa
        if self.warning_sent || self.status != AutoTradeStatus::Active {
            return false;
        }
        
        // Kiểm tra nếu thời gian còn lại ít hơn ngưỡng cảnh báo
        if self.remaining_minutes <= AUTO_TRADE_WARNING_MINUTES {
            debug!("Gửi cảnh báo sắp hết thời gian auto-trade: user={}, còn {} phút", 
                   self.user_id, self.remaining_minutes);
            
            // Đánh dấu đã gửi cảnh báo
            self.warning_sent = true;
            
            // Gửi thông báo nếu có event_emitter
            if let Some(emitter) = event_emitter {
                let event = SubscriptionEvent {
                    user_id: self.user_id.clone(),
                    event_type: EventType::AutoTradeTimeWarning,
                    timestamp: Utc::now(),
                    data: Some(format!("Sắp hết thời gian auto-trade, chỉ còn {} phút", self.remaining_minutes)),
                };
                emitter.emit(event);
            }
            
            return true;
        }
        
        false
    }
    
    /// Reset trạng thái và thời gian auto trade
    /// 
    /// Đặt lại thời gian auto trade dựa trên loại gói đăng ký. Nếu là VIP staking,
    /// thời gian sẽ được cộng thêm giờ bonus.
    /// 
    /// # Arguments
    /// * `subscription_type` - Loại gói đăng ký
    /// * `is_vip_staking` - `true` nếu người dùng đang sử dụng gói VIP 12 tháng (staking)
    pub fn reset(&mut self, subscription_type: &SubscriptionType, is_vip_staking: bool) {
        let now = Utc::now();
        self.status = AutoTradeStatus::Inactive;
        self.start_time = now;
        self.warning_sent = false;
        
        self.remaining_minutes = match subscription_type {
            SubscriptionType::Free => FREE_USER_AUTO_TRADE_MINUTES,
            SubscriptionType::Premium => PREMIUM_USER_AUTO_TRADE_HOURS * 60,
            SubscriptionType::VIP => {
                if is_vip_staking {
                    VIP_STAKE_TOTAL_AUTO_TRADE_HOURS * 60
                } else {
                    VIP_USER_AUTO_TRADE_HOURS * 60
                }
            },
            _ => FREE_USER_AUTO_TRADE_MINUTES, // Mặc định cho các gói khác
        };
        
        self.reset_time = if matches!(subscription_type, SubscriptionType::Free) {
            Some(now + Duration::days(FREE_USER_RESET_DAYS))
        } else {
            None
        };
    }
}

/// Quản lý auto-trade của người dùng.
///
/// Lớp này quản lý thời gian auto-trade của tất cả người dùng, xử lý 
/// việc kích hoạt, tạm dừng, cập nhật thời gian và đồng bộ hóa với
/// gói subscription. Cung cấp các cơ chế thread-safe để quản lý 
/// thời gian auto-trade trong môi trường đa luồng.
#[derive(Debug)]
pub struct AutoTradeManager {
    /// Lưu trữ thông tin auto-trade của người dùng theo user_id
    auto_trade_usages: Arc<RwLock<HashMap<String, AutoTradeUsage>>>,
    
    /// Tham chiếu yếu đến SubscriptionManager để tránh circular reference
    subscription_manager: Weak<SubscriptionManager>,
    
    /// Cache LRU cho auto-trade usage để tối ưu hiệu suất truy vấn
    auto_trade_cache: AsyncCache<String, AutoTradeUsage>,
    
    /// Database để lưu trữ dữ liệu auto-trade
    db: Arc<Database>,
    
    /// Bộ đếm cho số lần làm mới auto-trade thành công
    refresh_success_counter: AtomicU32,
    
    /// Bộ đếm cho số lần làm mới auto-trade thất bại
    refresh_error_counter: AtomicU32,
}

impl AutoTradeManager {
    /// Tạo instance mới của AutoTradeManager.
    ///
    /// Khởi tạo một AutoTradeManager mới với tham chiếu yếu đến SubscriptionManager
    /// để tránh circular reference, cùng với kết nối đến database.
    ///
    /// # Arguments
    /// * `subscription_manager` - Tham chiếu đến SubscriptionManager
    /// * `db` - Kết nối database
    ///
    /// # Returns
    /// Instance mới của AutoTradeManager
    pub fn new(subscription_manager: Weak<SubscriptionManager>, db: Arc<Database>) -> Self {
        Self {
            auto_trade_usages: Arc::new(RwLock::new(HashMap::new())),
            subscription_manager,
            auto_trade_cache: create_async_lru_cache(500, AUTO_TRADE_CACHE_SECONDS),
            db,
            refresh_success_counter: AtomicU32::new(0),
            refresh_error_counter: AtomicU32::new(0),
        }
    }
    
    /// Khởi tạo AutoTradeManager với dữ liệu từ database.
    ///
    /// Tạo instance mới và tải dữ liệu auto-trade từ database.
    /// Method này thực hiện kết nối async đến DB và tải dữ liệu.
    ///
    /// # Arguments
    /// * `subscription_manager` - Tham chiếu đến SubscriptionManager
    /// * `db` - Kết nối database
    ///
    /// # Returns
    /// * `Ok(AutoTradeManager)` - Instance đã được khởi tạo
    /// * `Err(AutoTradeError)` - Nếu có lỗi trong quá trình khởi tạo
    pub async fn new_with_init(subscription_manager: Weak<SubscriptionManager>, db: Arc<Database>) -> Result<Self, AutoTradeError> {
        let manager = Self::new(subscription_manager, db.clone());
        
        // Tải dữ liệu từ database
        if let Err(e) = manager.load_from_database().await {
            error!("Không thể tải dữ liệu auto-trade từ database: {}", e);
            return Err(e);
        }
        
        Ok(manager)
    }
    
    /// Tải dữ liệu auto-trade từ database.
    ///
    /// Đọc tất cả dữ liệu auto-trade từ database và cập nhật vào memory cache.
    /// Điều này đảm bảo dữ liệu được đồng bộ sau khi khởi động lại service.
    ///
    /// # Returns
    /// * `Ok(())` - Nếu tải dữ liệu thành công
    /// * `Err(AutoTradeError)` - Nếu có lỗi trong quá trình tải dữ liệu
    pub async fn load_from_database(&self) -> Result<(), AutoTradeError> {
        debug!("Đang tải dữ liệu auto-trade từ database...");
        
        // TODO: Implement database loading logic
        
        Ok(())
    }

    /// Lấy thông tin sử dụng auto-trade của người dùng.
    ///
    /// Tìm kiếm trong cache và memory cho thông tin auto-trade của người dùng.
    /// Nếu không tìm thấy, tạo mới dựa trên thông tin đăng ký.
    ///
    /// # Arguments
    /// * `user_id` - ID của người dùng cần kiểm tra
    ///
    /// # Returns
    /// * `Ok(AutoTradeUsage)` - Thông tin sử dụng auto-trade
    /// * `Err(AutoTradeError)` - Nếu có lỗi khi lấy thông tin
    pub async fn get_auto_trade_usage(&self, user_id: &str) -> Result<AutoTradeUsage, AutoTradeError> {
        if user_id.is_empty() {
            return Err(AutoTradeError::Other("user_id không được để trống".to_string()));
        }
        
        // Tìm trong cache trước
        let auto_trade_usage_result = cache::get_or_load_with_cache(
            &self.auto_trade_cache, 
            user_id,
            |id| async {
                // Nếu không có trong cache, tìm trong memory
                let usages = self.auto_trade_usages.read().await;
                usages.get(&id).cloned()
            }
        ).await;
        
        match auto_trade_usage_result {
            Some(usage) => Ok(usage),
            None => {
                // Nếu không tìm thấy, thử tạo mới
                debug!("Không tìm thấy auto trade usage cho user {}, tạo mới", user_id);
                
                // Lấy tham chiếu mạnh từ tham chiếu yếu
                let subscription_manager = match self.subscription_manager.upgrade() {
                    Some(manager) => manager,
                    None => {
                        error!("Không thể lấy tham chiếu đến SubscriptionManager trong get_auto_trade_usage");
                        return Err(AutoTradeError::InvalidReferenceError("SubscriptionManager không còn tồn tại".to_string()));
                    }
                };
                
                // Lấy thông tin subscription
                let subscription = match subscription_manager.get_user_subscription(user_id).await {
                    Ok(sub) => sub,
                    Err(e) => {
                        error!("Không tìm thấy subscription cho user {}: {}", user_id, e);
                        return Err(AutoTradeError::Database(format!(
                            "Không tìm thấy subscription cho user {}: {}", user_id, e
                        )));
                    }
                };
                
                // Tạo auto trade usage mới
                let is_vip_staking = subscription.is_twelve_month_vip();
                let new_usage = AutoTradeUsage::new(user_id, &subscription.subscription_type, is_vip_staking);
                
                // Lưu vào memory và cache
                {
                    let mut usages = self.auto_trade_usages.write().await;
                    usages.insert(user_id.to_string(), new_usage.clone());
                }
                
                // Cập nhật cache
                self.auto_trade_cache.insert(user_id.to_string(), new_usage.clone()).await;
                
                // Lưu vào database
                if let Err(e) = self.save_auto_trade_usage(&new_usage).await {
                    error!("Không thể lưu auto trade usage cho user {} vào database: {}", user_id, e);
                }
                
                Ok(new_usage)
            }
        }
    }
    
    /// Cập nhật thông tin sử dụng auto-trade của người dùng.
    ///
    /// Cập nhật thông tin auto-trade vào memory, cache và database.
    ///
    /// # Arguments
    /// * `usage` - Thông tin auto-trade cần cập nhật
    ///
    /// # Returns
    /// * `Ok(())` - Nếu cập nhật thành công
    /// * `Err(AutoTradeError)` - Nếu có lỗi khi cập nhật
    pub async fn update_auto_trade_usage(&self, usage: AutoTradeUsage) -> Result<(), AutoTradeError> {
        // Cập nhật trong memory
        {
            let mut usages = self.auto_trade_usages.write().await;
            usages.insert(usage.user_id.clone(), usage.clone());
        }
        
        // Cập nhật cache
        self.auto_trade_cache.insert(usage.user_id.clone(), usage.clone()).await;
        
        // Lưu vào database
        self.save_auto_trade_usage(&usage).await
    }
    
    /// Lưu thông tin auto-trade vào database.
    ///
    /// # Arguments
    /// * `usage` - Thông tin auto-trade cần lưu
    ///
    /// # Returns
    /// * `Ok(())` - Nếu lưu thành công
    /// * `Err(AutoTradeError)` - Nếu có lỗi khi lưu vào database
    pub async fn save_auto_trade_usage(&self, usage: &AutoTradeUsage) -> Result<(), AutoTradeError> {
        // TODO: Implement database saving logic
        
        Ok(())
    }
    
    /// Xử lý auto-trade usage đã hết hạn.
    ///
    /// Quét toàn bộ auto-trade usage, tìm các usage đã hết hạn hoặc đến thời gian reset,
    /// và cập nhật trạng thái của chúng.
    ///
    /// # Returns
    /// * `Ok(u32)` - Số lượng auto-trade usage đã được xử lý
    /// * `Err(AutoTradeError)` - Nếu có lỗi trong quá trình xử lý
    pub async fn process_expired_auto_trades(&self) -> Result<u32, AutoTradeError> {
        debug!("Bắt đầu xử lý auto trade usage hết hạn");
        
        // Lấy danh sách user_id với auto trade usage
        let user_ids_with_usage: Vec<(String, AutoTradeUsage)>;
        {
            let auto_trade_usages = match self.auto_trade_usages.read().await {
                Ok(usages) => usages,
                Err(e) => {
                    error!("Lỗi khi đọc auto_trade_usages: {}", e);
                    return Err(AutoTradeError::Other(format!("Lỗi đọc auto_trade_usages: {}", e)));
                }
            };
            
            user_ids_with_usage = auto_trade_usages.iter()
                .map(|(user_id, usage)| (user_id.clone(), usage.clone()))
                .filter(|(_, usage)| {
                    // Lọc những usage đã hết hạn hoặc đến thời gian reset
                    usage.status == AutoTradeStatus::Active && 
                    (usage.remaining_minutes <= 0 || 
                     usage.reset_time.map_or(false, |t| t <= Utc::now()))
                })
                .collect();
        }
        
        if user_ids_with_usage.is_empty() {
            debug!("Không có auto trade usage nào hết hạn");
            return Ok(0);
        }
        
        let mut processed_count = 0;
        
        // Xử lý từng usage
        for (user_id, mut usage) in user_ids_with_usage {
            // Cập nhật trạng thái
            if usage.remaining_minutes <= 0 {
                usage.status = AutoTradeStatus::Expired;
                info!("Auto-trade cho user {} đã hết hạn (hết thời gian)", user_id);
            } else if usage.reset_time.map_or(false, |t| t <= Utc::now()) {
                // Reset thời gian cho những người dùng free
                usage = self.reset_auto_trade_time(&usage).await?;
                info!("Auto-trade cho user {} đã được reset theo lịch", user_id);
            }
            
            // Lưu các thay đổi
            if let Err(e) = self.update_auto_trade_usage(usage).await {
                error!("Lỗi khi cập nhật auto trade usage cho user {}: {}", user_id, e);
                continue;
            }
            
            processed_count += 1;
        }
        
        info!("Đã xử lý {} auto trade usage hết hạn", processed_count);
        Ok(processed_count)
    }
    
    /// Reset thời gian auto-trade cho gói Free.
    ///
    /// Đặt lại thời gian auto-trade cho người dùng Free theo chu kỳ reset.
    ///
    /// # Arguments
    /// * `usage` - Auto-trade usage cần reset
    ///
    /// # Returns
    /// * `Ok(AutoTradeUsage)` - Auto-trade usage đã được reset
    /// * `Err(AutoTradeError)` - Nếu có lỗi khi reset
    pub async fn reset_auto_trade_time(&self, usage: &AutoTradeUsage) -> Result<AutoTradeUsage, AutoTradeError> {
        let now = Utc::now();
        let mut updated_usage = usage.clone();
        
        // Xác định subscription type
        let subscription_type = {
            let subscription_manager = match self.subscription_manager.upgrade() {
                Some(manager) => manager,
                None => {
                    error!("Không thể lấy tham chiếu đến SubscriptionManager trong reset_auto_trade_time");
                    return Err(AutoTradeError::InvalidReferenceError("SubscriptionManager không còn tồn tại".to_string()));
                }
            };
            
            match subscription_manager.get_user_subscription(&usage.user_id).await {
                Ok(sub) => sub.subscription_type,
                Err(_) => SubscriptionType::Free // Mặc định là Free nếu không tìm thấy
            }
        };
        
        // Reset thời gian dựa vào loại subscription
        match subscription_type {
            SubscriptionType::Free => {
                updated_usage.remaining_minutes = FREE_USER_AUTO_TRADE_MINUTES;
                updated_usage.reset_time = Some(now + Duration::days(FREE_USER_RESET_DAYS));
                updated_usage.status = AutoTradeStatus::Inactive; // Đặt lại trạng thái
                updated_usage.warning_sent = false;
            },
            SubscriptionType::Premium => {
                updated_usage.remaining_minutes = PREMIUM_USER_AUTO_TRADE_HOURS * 60;
                updated_usage.reset_time = None;
                updated_usage.status = AutoTradeStatus::Inactive;
                updated_usage.warning_sent = false;
            },
            SubscriptionType::VIP => {
                // Kiểm tra xem có phải VIP 12 tháng không
                let is_vip_staking = {
                    let subscription_manager = self.subscription_manager.upgrade().unwrap();
                    let subscription = subscription_manager.get_user_subscription(&usage.user_id).await.unwrap_or_default();
                    subscription.is_twelve_month_vip()
                };
                
                if is_vip_staking {
                    updated_usage.remaining_minutes = VIP_STAKE_TOTAL_AUTO_TRADE_HOURS * 60;
                } else {
                    updated_usage.remaining_minutes = VIP_USER_AUTO_TRADE_HOURS * 60;
                }
                
                updated_usage.reset_time = None;
                updated_usage.status = AutoTradeStatus::Inactive;
                updated_usage.warning_sent = false;
            },
        }
        
        // Cập nhật thời gian
        updated_usage.last_used = Some(now);
        
        // Cập nhật vào memory
        {
            let mut usages = self.auto_trade_usages.write().await;
            usages.insert(usage.user_id.clone(), updated_usage.clone());
        }
        
        // Cập nhật cache và vô hiệu hóa cache cũ
        self.auto_trade_cache.invalidate(&usage.user_id).await;
        self.auto_trade_cache.insert(usage.user_id.clone(), updated_usage.clone()).await;
        
        // Lưu vào database
        if let Err(e) = self.save_auto_trade_usage(&updated_usage).await {
            warn!("Không thể lưu auto-trade usage vào database sau khi reset: {}", e);
        }
        
        // Đồng bộ với các module khác
        if let Some(subscription_manager) = self.subscription_manager.upgrade() {
            // Thông báo reset cho subscription manager
            if let Err(e) = subscription_manager.notify_auto_trade_reset(&usage.user_id).await {
                warn!("Không thể thông báo cho subscription manager về reset auto-trade: {}", e);
            }
        }
        
        info!("Đã reset auto-trade time cho user {}", usage.user_id);
        Ok(updated_usage)
    }

    /// Vô hiệu hóa cache cho một user_id cụ thể.
    ///
    /// Xóa thông tin cache của một người dùng để đảm bảo lần truy vấn tiếp theo
    /// sẽ lấy dữ liệu mới nhất.
    ///
    /// # Arguments
    /// * `user_id` - ID của người dùng cần vô hiệu hóa cache
    ///
    /// # Returns
    /// * `Ok(())` - Nếu xóa thành công
    /// * `Err(AutoTradeError)` - Nếu có lỗi xảy ra
    pub async fn invalidate_user_cache(&self, user_id: &str) -> Result<(), AutoTradeError> {
        debug!("Vô hiệu hóa cache auto trade cho user: {}", user_id);
        
        // Xóa cache
        self.auto_trade_cache.invalidate(user_id).await;
        
        // Thông báo cho subscription_manager nếu cần
        if let Some(manager) = self.subscription_manager.upgrade() {
            if let Err(e) = manager.refresh_auto_trade_status(user_id).await {
                warn!("Không thể thông báo cho subscription_manager sau khi vô hiệu hóa cache: {}", e);
            }
        }
        
        debug!("Đã xóa cache auto trade cho user: {}", user_id);
        Ok(())
    }

    /// Vô hiệu hóa toàn bộ cache auto-trade.
    /// 
    /// Xóa toàn bộ cache để đảm bảo tất cả dữ liệu sẽ được lấy mới.
    /// 
    /// # Returns
    /// * `Ok(())` - Nếu xóa thành công
    /// * `Err(AutoTradeError)` - Nếu có lỗi xảy ra
    pub async fn invalidate_all_cache(&self) -> Result<(), AutoTradeError> {
        debug!("Vô hiệu hóa toàn bộ cache auto trade");
        
        // Xóa toàn bộ cache
        self.auto_trade_cache.clear().await;
        
        info!("Đã xóa toàn bộ cache auto trade");
        Ok(())
    }

    /// Đồng bộ hóa cache với các module khác.
    ///
    /// Đảm bảo rằng tất cả các cache liên quan đến auto-trade
    /// được đồng bộ hóa giữa các module như subscription, nft, v.v.
    ///
    /// # Arguments
    /// * `user_ids` - Danh sách các user_id cần đồng bộ hóa
    ///
    /// # Returns
    /// * `Ok(())` - Nếu đồng bộ hóa thành công
    /// * `Err(AutoTradeError)` - Nếu có lỗi xảy ra
    pub async fn sync_cache_with_other_modules(&self, user_ids: &[String]) -> Result<(), AutoTradeError> {
        debug!("Đồng bộ hóa cache auto-trade với các module khác cho {} user", user_ids.len());
        
        for user_id in user_ids {
            // Vô hiệu hóa cache cho user hiện tại
            if let Err(e) = self.invalidate_user_cache(user_id).await {
                warn!("Không thể vô hiệu hóa cache cho user {}: {}", user_id, e);
            }
        }
        
        // Đồng bộ với database nếu cần
        if let Err(e) = self.sync_cache_with_database().await {
            warn!("Không thể đồng bộ cache với database: {}", e);
        }
        
        info!("Đã đồng bộ hóa cache auto-trade thành công với {} user", user_ids.len());
        Ok(())
    }

    /// Đồng bộ hóa cache với database.
    ///
    /// Tải lại dữ liệu từ database để đảm bảo cache là mới nhất.
    ///
    /// # Returns
    /// * `Ok(())` - Nếu đồng bộ hóa thành công
    /// * `Err(AutoTradeError)` - Nếu có lỗi xảy ra
    pub async fn sync_cache_with_database(&self) -> Result<(), AutoTradeError> {
        debug!("Đồng bộ hóa cache auto-trade với database");
        
        // TODO: Thực hiện tải lại dữ liệu từ database
        // Đây là nơi để tải lại auto-trade data từ database và cập nhật cache
        
        Ok(())
    }
} 