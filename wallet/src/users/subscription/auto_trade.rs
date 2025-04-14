//! Định nghĩa các kiểu dữ liệu và phương thức liên quan đến tính năng giao dịch tự động (Auto Trade).

// External imports
use chrono::{DateTime, Duration, Utc};
use log::{debug, error, info, warn};
use thiserror::Error;
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use tokio::sync::RwLock;

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

/// Lỗi liên quan đến tính năng auto trade
#[derive(Debug, Error)]
pub enum AutoTradeError {
    /// Đã hết thời gian sử dụng auto trade
    #[error("Đã hết thời gian sử dụng auto trade")]
    NoRemainingTime,
    
    /// Không có quyền sử dụng auto trade
    #[error("Không có quyền sử dụng tính năng auto trade")]
    NoPermission,
    
    /// Database error
    #[error("Lỗi database: {0}")]
    Database(String),
    
    /// Lỗi khác
    #[error("Lỗi: {0}")]
    Other(String),
}

// Chuyển đổi từ AutoTradeError sang WalletError để đảm bảo xử lý lỗi nhất quán
impl From<AutoTradeError> for WalletError {
    fn from(error: AutoTradeError) -> Self {
        match error {
            AutoTradeError::NoRemainingTime => WalletError::Other("Đã hết thời gian sử dụng auto trade".to_string()),
            AutoTradeError::NoPermission => WalletError::Other("Không có quyền sử dụng tính năng auto trade".to_string()),
            AutoTradeError::Database(err) => WalletError::Other(format!("Lỗi database auto trade: {}", err)),
            AutoTradeError::Other(err) => WalletError::Other(format!("Lỗi auto trade: {}", err)),
        }
    }
}

/// Trạng thái của tính năng auto trade
#[derive(Debug, Clone)]
pub enum AutoTradeStatus {
    /// Chưa kích hoạt
    Inactive,
    /// Đang hoạt động
    Active,
    /// Tạm dừng
    Paused,
    /// Đã hết hạn
    Expired,
}

impl fmt::Display for AutoTradeStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AutoTradeStatus::Inactive => write!(f, "Inactive"),
            AutoTradeStatus::Active => write!(f, "Active"),
            AutoTradeStatus::Paused => write!(f, "Paused"),
            AutoTradeStatus::Expired => write!(f, "Expired"),
        }
    }
}

/// Thông tin về việc sử dụng auto trade
#[derive(Debug, Clone)]
pub struct AutoTradeUsage {
    /// User ID
    pub user_id: String,
    /// Trạng thái auto trade
    pub status: AutoTradeStatus,
    /// Thời gian bắt đầu
    pub start_time: DateTime<Utc>,
    /// Thời gian còn lại (phút)
    pub remaining_minutes: i64,
    /// Thời gian sử dụng cuối
    pub last_used: Option<DateTime<Utc>>,
    /// Thời gian reset
    pub reset_time: Option<DateTime<Utc>>,
    /// Đã gửi cảnh báo sắp hết thời gian chưa
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
}

/// Quản lý tính năng auto trade
/// 
/// AutoTradeManager chịu trách nhiệm quản lý việc sử dụng tính năng auto trade của người dùng,
/// bao gồm theo dõi thời gian sử dụng, đồng bộ với thông tin subscription, và gửi cảnh báo
/// khi thời gian sử dụng gần hết.
#[derive(Debug)]
pub struct AutoTradeManager {
    /// Lưu trữ thông tin sử dụng auto-trade của người dùng
    auto_trade_usages: Arc<RwLock<HashMap<String, AutoTradeUsage>>>,
    /// Đối tượng phát sự kiện
    event_emitter: Option<EventEmitter>,
    /// Tham chiếu yếu đến subscription manager để tránh circular reference
    subscription_manager: Weak<super::manager::SubscriptionManager>,
}

impl AutoTradeManager {
    /// Tạo mới AutoTradeManager
    /// 
    /// Khởi tạo một AutoTradeManager mới với tham chiếu đến SubscriptionManager.
    /// Sử dụng Weak reference để tránh circular references và deadlock giữa
    /// AutoTradeManager và SubscriptionManager.
    /// 
    /// # Arguments
    /// * `subscription_manager` - Tham chiếu đến SubscriptionManager
    /// 
    /// # Returns
    /// Một instance mới của AutoTradeManager với các giá trị mặc định
    /// 
    /// # Examples
    /// ```
    /// use wallet::users::subscription::auto_trade::AutoTradeManager;
    /// use wallet::users::subscription::manager::SubscriptionManager;
    /// use std::sync::Arc;
    ///
    /// let subscription_manager = Arc::new(SubscriptionManager::new());
    /// let auto_trade_manager = AutoTradeManager::new(subscription_manager);
    /// ```
    pub fn new(subscription_manager: Arc<super::manager::SubscriptionManager>) -> Self {
        Self {
            auto_trade_usages: Arc::new(RwLock::new(HashMap::new())),
            event_emitter: None,
            subscription_manager: Arc::downgrade(&subscription_manager),
        }
    }
    
    /// Thiết lập event emitter
    /// 
    /// Cài đặt event emitter để AutoTradeManager có thể gửi thông báo khi có
    /// sự kiện liên quan đến auto trade, như sắp hết thời gian hoặc reset thời gian.
    /// 
    /// # Arguments
    /// * `emitter` - EventEmitter dùng để phát sự kiện
    pub fn set_event_emitter(&mut self, emitter: EventEmitter) {
        self.event_emitter = Some(emitter);
    }
    
    /// Đồng bộ tất cả auto-trade với subscription
    /// 
    /// Đồng bộ thông tin auto-trade của tất cả người dùng với thông tin subscription tương ứng.
    /// Nếu subscription không còn active, auto-trade sẽ được đặt về trạng thái Inactive.
    /// Ngược lại, thời gian còn lại sẽ được điều chỉnh theo loại subscription hiện tại.
    /// 
    /// # Returns
    /// * `Ok(())` - Nếu đồng bộ thành công
    /// * `Err(AutoTradeError)` - Nếu có lỗi xảy ra trong quá trình đồng bộ
    pub async fn sync_all_with_subscriptions(&self) -> Result<(), AutoTradeError> {
        info!("Đồng bộ tất cả auto-trade với subscription");
        
        // Lấy danh sách auto-trade
        let auto_trade_usages = {
            let usages = self.auto_trade_usages.read().await;
            usages.clone()
        };
        
        // Lấy tham chiếu mạnh từ tham chiếu yếu
        let subscription_manager = match self.subscription_manager.upgrade() {
            Some(manager) => manager,
            None => {
                error!("Không thể lấy tham chiếu đến SubscriptionManager, đã bị giải phóng");
                return Err(AutoTradeError::Other("SubscriptionManager không còn tồn tại".to_string()));
            }
        };
        
        let mut updated_count = 0;
        
        // Đồng bộ từng auto-trade với subscription tương ứng
        for (user_id, mut usage) in auto_trade_usages {
            // Lấy thông tin subscription
            match subscription_manager.get_user_subscription(&user_id).await {
                Ok(subscription) => {
                    // Đồng bộ auto-trade với subscription
                    usage.sync_with_subscription(&subscription);
                    
                    // Cập nhật lại vào danh sách
                    {
                        let mut usages = self.auto_trade_usages.write().await;
                        usages.insert(user_id.clone(), usage);
                    }
                    
                    updated_count += 1;
                },
                Err(e) => {
                    error!("Lỗi khi lấy subscription cho user {}: {}", user_id, e);
                }
            }
        }
        
        info!("Đã đồng bộ {} auto-trade với subscription", updated_count);
        Ok(())
    }
    
    /// Kiểm tra định kỳ auto-trade
    /// 
    /// Thực hiện kiểm tra định kỳ tất cả auto-trade, bao gồm đồng bộ với subscription,
    /// kiểm tra và reset nếu cần, đồng thời gửi cảnh báo nếu thời gian còn lại gần hết.
    /// 
    /// # Returns
    /// * `Ok(())` - Nếu kiểm tra thành công
    /// * `Err(AutoTradeError)` - Nếu có lỗi xảy ra trong quá trình kiểm tra
    pub async fn periodic_check(&self) -> Result<(), AutoTradeError> {
        info!("Thực hiện kiểm tra định kỳ auto-trade");
        
        // Đồng bộ với subscription
        if let Err(e) = self.sync_all_with_subscriptions().await {
            error!("Lỗi khi đồng bộ auto-trade với subscription: {}", e);
        }
        
        // Lấy danh sách auto-trade
        let auto_trade_usages = {
            let usages = self.auto_trade_usages.read().await;
            usages.clone()
        };
        
        // Lấy tham chiếu mạnh từ tham chiếu yếu
        let subscription_manager = match self.subscription_manager.upgrade() {
            Some(manager) => manager,
            None => {
                error!("Không thể lấy tham chiếu đến SubscriptionManager trong periodic_check");
                return Err(AutoTradeError::Other("SubscriptionManager không còn tồn tại".to_string()));
            }
        };
        
        let mut reset_count = 0;
        let mut warning_count = 0;
        
        // Kiểm tra từng auto-trade
        for (user_id, mut usage) in auto_trade_usages {
            // Kiểm tra và reset nếu cần
            match subscription_manager.get_user_subscription(&user_id).await {
                Ok(subscription) => {
                    // Kiểm tra và reset nếu cần
                    if usage.check_and_reset_if_needed(&subscription.subscription_type) {
                        debug!("Đã reset auto-trade cho user {}", user_id);
                        reset_count += 1;
                        
                        // Cập nhật lại vào danh sách
                        {
                            let mut usages = self.auto_trade_usages.write().await;
                            usages.insert(user_id.clone(), usage.clone());
                        }
                        
                        // Gửi thông báo đã reset
                        if let Some(emitter) = &self.event_emitter {
                            let event = SubscriptionEvent {
                                user_id: user_id.clone(),
                                event_type: EventType::AutoTradeTimeReset,
                                timestamp: Utc::now(),
                                data: Some(format!("Đã reset thời gian auto-trade, còn {} phút", 
                                                   FREE_USER_AUTO_TRADE_MINUTES)),
                            };
                            emitter.emit(event);
                        }
                    }
                    
                    // Kiểm tra và gửi cảnh báo nếu sắp hết thời gian
                    if usage.check_and_send_warning(self.event_emitter.as_ref()) {
                        warning_count += 1;
                        
                        // Cập nhật lại vào danh sách
                        {
                            let mut usages = self.auto_trade_usages.write().await;
                            usages.insert(user_id, usage);
                        }
                    }
                },
                Err(e) => {
                    error!("Lỗi khi lấy subscription cho user {}: {}", user_id, e);
                }
            }
        }
        
        info!("Hoàn thành kiểm tra định kỳ. Reset: {}, Cảnh báo: {}", reset_count, warning_count);
        Ok(())
    }
    
    /// Kiểm tra quyền sử dụng auto-trade và thời gian còn lại
    /// 
    /// Kiểm tra xem người dùng có quyền sử dụng auto-trade trong khoảng thời gian
    /// `duration_minutes` hay không. Điều kiện là subscription phải đang active,
    /// auto-trade phải ở trạng thái Active, và thời gian còn lại phải đủ.
    /// 
    /// # Arguments
    /// * `user_id` - ID của người dùng cần kiểm tra
    /// * `duration_minutes` - Số phút dự kiến sẽ sử dụng
    /// 
    /// # Returns
    /// * `Ok(true)` - Nếu người dùng có quyền sử dụng với thời gian yêu cầu
    /// * `Err(AutoTradeError)` - Nếu không có quyền hoặc không đủ thời gian
    pub async fn can_use_auto_trade_with_time(
        &self, 
        user_id: &str,
        duration_minutes: i64
    ) -> Result<bool, AutoTradeError> {
        // Lấy tham chiếu mạnh từ tham chiếu yếu
        let subscription_manager = match self.subscription_manager.upgrade() {
            Some(manager) => manager,
            None => {
                error!("Không thể lấy tham chiếu đến SubscriptionManager");
                return Err(AutoTradeError::Other("SubscriptionManager không còn tồn tại".to_string()));
            }
        };
        
        // Lấy thông tin subscription
        let subscription = match subscription_manager.get_user_subscription(user_id).await {
            Ok(sub) => sub,
            Err(e) => {
                error!("Không thể lấy thông tin subscription cho user {}: {}", user_id, e);
                return Err(AutoTradeError::Other(format!("Lỗi khi lấy subscription: {}", e)));
            }
        };
        
        // Kiểm tra xem subscription có còn active không
        if !subscription.is_active() {
            debug!("Subscription không hoạt động, từ chối auto-trade: user={}", user_id);
            return Err(AutoTradeError::NoPermission);
        }
        
        // Lấy hoặc tạo auto-trade usage
        let mut usage = match self.get_auto_trade_usage(user_id).await {
            Ok(u) => u,
            Err(AutoTradeError::Database(_)) => {
                // Nếu không tìm thấy, tạo mới
                let is_vip_staking = subscription.is_twelve_month_vip();
                match self.create_auto_trade_usage(user_id, &subscription.subscription_type, is_vip_staking).await {
                    Ok(u) => u,
                    Err(e) => {
                        error!("Không thể tạo auto-trade usage cho user {}: {}", user_id, e);
                        return Err(e);
                    }
                }
            }
            Err(e) => {
                error!("Lỗi khi lấy auto-trade usage cho user {}: {}", user_id, e);
                return Err(e);
            }
        };
        
        // Đồng bộ với subscription
        usage.sync_with_subscription(&subscription);
        
        // Kiểm tra xem có đủ thời gian không
        let can_use = usage.can_use(duration_minutes);
        
        // Cập nhật lại vào danh sách
        {
            let mut usages = self.auto_trade_usages.write().await;
            usages.insert(user_id.to_string(), usage);
        }
        
        if can_use {
            debug!("Cho phép sử dụng auto-trade: user={}, duration={} phút", user_id, duration_minutes);
            Ok(true)
        } else {
            debug!("Từ chối sử dụng auto-trade do không đủ thời gian: user={}", user_id);
            Err(AutoTradeError::NoRemainingTime)
        }
    }
    
    /// Lấy thông tin auto-trade usage của user
    /// 
    /// Truy xuất thông tin sử dụng auto-trade của người dùng từ bộ nhớ.
    /// Nếu không tìm thấy, sẽ trả về lỗi Database.
    /// 
    /// # Arguments
    /// * `user_id` - ID của người dùng cần lấy thông tin
    /// 
    /// # Returns
    /// * `Ok(AutoTradeUsage)` - Nếu tìm thấy thông tin
    /// * `Err(AutoTradeError)` - Nếu không tìm thấy hoặc có lỗi xảy ra
    pub async fn get_auto_trade_usage(&self, user_id: &str) -> Result<AutoTradeUsage, AutoTradeError> {
        let usages = self.auto_trade_usages.read().await;
        
        if let Some(usage) = usages.get(user_id) {
            return Ok(usage.clone());
        }
        
        // Nếu không tìm thấy, thử tạo mới
        drop(usages); // Giải phóng lock trước khi tạo mới
        
        // Lấy tham chiếu mạnh từ tham chiếu yếu
        let subscription_manager = match self.subscription_manager.upgrade() {
            Some(manager) => manager,
            None => {
                error!("Không thể lấy tham chiếu đến SubscriptionManager trong get_auto_trade_usage");
                return Err(AutoTradeError::Other("SubscriptionManager không còn tồn tại".to_string()));
            }
        };
        
        let subscription = match subscription_manager.get_user_subscription(user_id).await {
            Ok(sub) => sub,
            Err(e) => {
                return Err(AutoTradeError::Database(format!(
                    "Không tìm thấy subscription cho user {}: {}", user_id, e
                )));
            }
        };
        
        let is_vip_staking = subscription.is_twelve_month_vip();
        self.create_auto_trade_usage(user_id, &subscription.subscription_type, is_vip_staking).await
    }
    
    /// Tạo mới một AutoTradeUsage cho người dùng
    /// 
    /// Khởi tạo thông tin sử dụng auto-trade mới cho người dùng dựa trên
    /// loại subscription và trạng thái VIP staking. Thông tin này sẽ được
    /// lưu vào bộ nhớ và trả về.
    /// 
    /// # Arguments
    /// * `user_id` - ID của người dùng
    /// * `subscription_type` - Loại subscription của người dùng
    /// * `is_vip_staking` - Có phải là gói VIP 12 tháng không
    /// 
    /// # Returns
    /// * `Ok(AutoTradeUsage)` - Thông tin auto-trade mới đã được tạo
    /// * `Err(AutoTradeError)` - Nếu có lỗi xảy ra trong quá trình tạo
    pub async fn create_auto_trade_usage(
        &self,
        user_id: &str,
        subscription_type: &SubscriptionType,
        is_vip_staking: bool,
    ) -> Result<AutoTradeUsage, AutoTradeError> {
        let mut usage = AutoTradeUsage::new(user_id, subscription_type, is_vip_staking);
        usage.warning_sent = false;
        
        {
            let mut auto_trade_usages = self.auto_trade_usages.write().await;
            auto_trade_usages.insert(user_id.to_string(), usage.clone());
        }
        
        Ok(usage)
    }
    
    /// Tạo mới một AutoTradeUsage với các giá trị mặc định
    /// 
    /// Phiên bản rút gọn của `create_auto_trade_usage()`, đặt `is_vip_staking` thành `false`.
    /// 
    /// # Arguments
    /// * `user_id` - ID của người dùng
    /// * `subscription_type` - Loại subscription của người dùng
    /// 
    /// # Returns
    /// * `Ok(AutoTradeUsage)` - Thông tin auto-trade mới đã được tạo
    /// * `Err(AutoTradeError)` - Nếu có lỗi xảy ra trong quá trình tạo
    pub async fn create_default_auto_trade_usage(
        &self,
        user_id: &str,
        subscription_type: &SubscriptionType,
    ) -> Result<AutoTradeUsage, AutoTradeError> {
        self.create_auto_trade_usage(user_id, subscription_type, false).await
    }

    /// Reset thời gian auto trade cho người dùng
    /// 
    /// Đặt lại thời gian sử dụng auto-trade cho người dùng theo loại subscription
    /// và trạng thái VIP staking hiện tại. Nếu người dùng chưa có thông tin
    /// auto-trade, sẽ tạo mới.
    /// 
    /// # Arguments
    /// * `user_id` - ID của người dùng
    /// * `subscription_type` - Loại subscription của người dùng
    /// * `is_vip_staking` - Có phải là gói VIP 12 tháng không
    /// 
    /// # Returns
    /// * `Ok(())` - Nếu reset thành công
    /// * `Err(AutoTradeError)` - Nếu có lỗi xảy ra trong quá trình reset
    pub async fn reset_auto_trade(
        &self,
        user_id: &str,
        subscription_type: &SubscriptionType,
        is_vip_staking: bool,
    ) -> Result<(), AutoTradeError> {
        let mut auto_trade_usages = self.auto_trade_usages.write().await;
        
        if let Some(auto_trade_usage) = auto_trade_usages.get_mut(user_id) {
            auto_trade_usage.reset(subscription_type, is_vip_staking);
            auto_trade_usage.warning_sent = false;
            
            info!("Đã reset thời gian auto-trade cho user {}", user_id);
            
            // Gửi sự kiện thông báo
            if let Some(ref emitter) = self.event_emitter {
                let event = SubscriptionEvent {
                    user_id: user_id.to_string(),
                    event_type: EventType::AutoTradeTimeReset,
                    timestamp: Utc::now(),
                    data: Some(format!("Reset thời gian auto-trade cho gói {:?}", subscription_type)),
                };
                debug!("Gửi thông báo reset thời gian auto-trade: {:#?}", event);
            }
            
            Ok(())
        } else {
            // Không tìm thấy thông tin của người dùng, tạo mới
            let mut usage = AutoTradeUsage::new(user_id, subscription_type, is_vip_staking);
            usage.warning_sent = false;
            usage.activate();
            
            auto_trade_usages.insert(user_id.to_string(), usage);
            
            info!("Đã tạo và kích hoạt auto-trade mới cho user {}", user_id);
            Ok(())
        }
    }
    
    /// Reset thời gian auto trade với các giá trị mặc định
    /// 
    /// Phiên bản rút gọn của `reset_auto_trade()`, đặt `is_vip_staking` thành `false`.
    /// 
    /// # Arguments
    /// * `user_id` - ID của người dùng
    /// * `subscription_type` - Loại subscription của người dùng
    /// 
    /// # Returns
    /// * `Ok(())` - Nếu reset thành công
    /// * `Err(AutoTradeError)` - Nếu có lỗi xảy ra trong quá trình reset
    pub async fn reset_default_auto_trade(
        &self,
        user_id: &str,
        subscription_type: &SubscriptionType,
    ) -> Result<(), AutoTradeError> {
        self.reset_auto_trade(user_id, subscription_type, false).await
    }

    /// Lấy thông tin sử dụng auto trade của người dùng
    /// Nếu chưa có thì tạo mới với thông tin gói hiện tại
    /// 
    /// Truy xuất thông tin sử dụng auto-trade của người dùng. Nếu không tìm thấy,
    /// sẽ tạo mới dựa trên thông tin subscription hiện tại. Nếu tìm thấy, sẽ
    /// đồng bộ với subscription và reset nếu cần.
    /// 
    /// # Arguments
    /// * `user_id` - ID của người dùng
    /// 
    /// # Returns
    /// * `Ok(AutoTradeUsage)` - Thông tin auto-trade đã được lấy hoặc tạo mới
    /// * `Err(AutoTradeError)` - Nếu có lỗi xảy ra trong quá trình xử lý
    pub async fn get_or_create_auto_trade_usage(
        &self,
        user_id: &str,
    ) -> Result<AutoTradeUsage, AutoTradeError> {
        // Lấy tham chiếu mạnh từ tham chiếu yếu
        let subscription_manager = match self.subscription_manager.upgrade() {
            Some(manager) => manager,
            None => {
                error!("Không thể lấy tham chiếu đến SubscriptionManager trong get_or_create_auto_trade_usage");
                return Err(AutoTradeError::Other("SubscriptionManager không còn tồn tại".to_string()));
            }
        };
        
        // Lấy thông tin từ subscription manager
        let subscription = subscription_manager.get_user_subscription(user_id).await
            .map_err(|e| AutoTradeError::Other(format!("Không thể lấy thông tin subscription: {}", e)))?;
        
        // Xác định loại gói và có phải VIP staking không
        let subscription_type = subscription.subscription_type;
        let is_vip_staking = subscription.is_twelve_month_vip();
        
        {
            let auto_trade_usages = self.auto_trade_usages.read().await;
            if let Some(auto_trade_usage) = auto_trade_usages.get(user_id) {
                // Đồng bộ trạng thái với subscription
                let mut usage = auto_trade_usage.clone();
                usage.sync_with_subscription(&subscription);
                
                // Kiểm tra nếu là Free user thì cần reset theo định kỳ
                if usage.check_and_reset_if_needed(&subscription_type) {
                    // Đã reset, cập nhật lại vào map
                    drop(auto_trade_usages);
                    let mut auto_trade_usages = self.auto_trade_usages.write().await;
                    auto_trade_usages.insert(user_id.to_string(), usage.clone());
                }
                return Ok(usage);
            }
        }
        
        // Không tìm thấy, tạo mới
        self.create_auto_trade_usage(user_id, &subscription_type, is_vip_staking).await
    }
} 