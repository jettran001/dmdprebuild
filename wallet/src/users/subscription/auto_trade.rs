//! Định nghĩa các kiểu dữ liệu và phương thức liên quan đến tính năng giao dịch tự động (Auto Trade).

// External imports
use chrono::{DateTime, Duration, Utc};
use log::{debug, error, info, warn};
use thiserror::Error;

// Standard library imports
use std::fmt;

// Internal imports
use crate::error::WalletError;
use crate::users::subscription::{
    constants::{
        FREE_USER_AUTO_TRADE_MINUTES, FREE_USER_RESET_DAYS, PREMIUM_USER_AUTO_TRADE_HOURS,
        VIP_USER_AUTO_TRADE_HOURS,
    },
    types::SubscriptionType,
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
}

impl AutoTradeUsage {
    /// Tạo mới đối tượng AutoTradeUsage
    pub fn new(user_id: &str, subscription_type: &SubscriptionType) -> Self {
        let now = Utc::now();
        let remaining_minutes = match subscription_type {
            SubscriptionType::Free => FREE_USER_AUTO_TRADE_MINUTES,
            SubscriptionType::Premium => PREMIUM_USER_AUTO_TRADE_HOURS * 60,
            SubscriptionType::VIP => VIP_USER_AUTO_TRADE_HOURS * 60,
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
        }
    }
    
    /// Cập nhật thời gian còn lại cho auto trade
    /// 
    /// # Arguments
    /// * `used_minutes` - Số phút đã sử dụng
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
    /// # Arguments
    /// * `duration_minutes` - Thời gian cần sử dụng (phút)
    /// 
    /// # Returns
    /// `true` nếu còn đủ thời gian sử dụng
    pub fn can_use(&self, duration_minutes: i64) -> bool {
        match self.status {
            AutoTradeStatus::Active => self.remaining_minutes >= duration_minutes,
            _ => false,
        }
    }
    
    /// Kích hoạt tính năng auto trade
    pub fn activate(&mut self) {
        self.status = AutoTradeStatus::Active;
        debug!("Auto trade activated for user {}", self.user_id);
    }
    
    /// Tạm dừng tính năng auto trade
    pub fn pause(&mut self) {
        self.status = AutoTradeStatus::Paused;
        debug!("Auto trade paused for user {}", self.user_id);
    }
    
    /// Reset thời gian sử dụng auto trade (chỉ dành cho free user)
    pub fn reset(&mut self, subscription_type: &SubscriptionType) {
        let now = Utc::now();
        
        match subscription_type {
            SubscriptionType::Free => {
                if let Some(reset_time) = self.reset_time {
                    if now >= reset_time {
                        self.remaining_minutes = FREE_USER_AUTO_TRADE_MINUTES;
                        self.reset_time = Some(now + Duration::days(FREE_USER_RESET_DAYS));
                        debug!("Auto trade reset for free user {}", self.user_id);
                    }
                }
            }
            SubscriptionType::Premium => {
                self.remaining_minutes = PREMIUM_USER_AUTO_TRADE_HOURS * 60;
                self.reset_time = None;
                debug!("Auto trade reset for premium user {}", self.user_id);
            }
            SubscriptionType::VIP => {
                self.remaining_minutes = VIP_USER_AUTO_TRADE_HOURS * 60;
                self.reset_time = None;
                debug!("Auto trade reset for VIP user {}", self.user_id);
            }
        }
        
        // Nếu đã hết hạn và được reset, kích hoạt lại
        if matches!(self.status, AutoTradeStatus::Expired) {
            self.status = AutoTradeStatus::Active;
        }
    }
    
    /// Kiểm tra và cập nhật trạng thái reset
    pub fn check_reset(&mut self, subscription_type: &SubscriptionType) -> bool {
        if let SubscriptionType::Free = subscription_type {
            if let Some(reset_time) = self.reset_time {
                if Utc::now() >= reset_time {
                    self.reset(subscription_type);
                    return true;
                }
            }
        }
        false
    }
}

/// Quản lý tính năng auto trade
#[derive(Debug)]
pub struct AutoTradeManager {
    /// Database storage connector
    pub db_connector: String,
}

impl AutoTradeManager {
    /// Tạo mới AutoTradeManager
    pub fn new(db_connector: &str) -> Self {
        Self {
            db_connector: db_connector.to_string(),
        }
    }
    
    /// Kiểm tra người dùng có thể sử dụng auto trade trong khoảng thời gian chỉ định không
    /// 
    /// # Arguments
    /// * `user_id` - ID của người dùng
    /// * `duration_minutes` - Thời gian cần sử dụng (phút)
    /// 
    /// # Returns
    /// `true` nếu có thể sử dụng
    #[flow_from(wallet_service, request_handler)]
    pub fn can_use_auto_trade_with_time(
        &self, 
        user_id: &str,
        duration_minutes: i64
    ) -> Result<bool, AutoTradeError> {
        // Giả lập truy vấn database
        info!("Checking auto trade availability for user {} with duration {} minutes", user_id, duration_minutes);
        
        // Trong thực tế, sẽ cần truy vấn database để lấy thông tin usage
        // Đây chỉ là code mẫu
        let usage = self.get_auto_trade_usage(user_id)?;
        
        Ok(usage.can_use(duration_minutes))
    }
    
    /// Lấy thông tin sử dụng auto trade của người dùng
    /// 
    /// # Arguments
    /// * `user_id` - ID của người dùng
    /// 
    /// # Returns
    /// Thông tin sử dụng auto trade
    pub fn get_auto_trade_usage(&self, user_id: &str) -> Result<AutoTradeUsage, AutoTradeError> {
        // Giả lập truy vấn database
        debug!("Getting auto trade usage for user {}", user_id);
        
        // Trong thực tế, sẽ truy vấn database
        // Đây chỉ là code mẫu
        Err(AutoTradeError::Database("Not implemented".to_string()))
    }
    
    /// Cập nhật thời gian sử dụng auto trade
    /// 
    /// # Arguments
    /// * `user_id` - ID của người dùng
    /// * `used_minutes` - Số phút đã sử dụng
    #[flow_from(transaction_service, wallet_activity)]
    pub fn update_auto_trade_usage(
        &self,
        user_id: &str,
        used_minutes: i64
    ) -> Result<(), AutoTradeError> {
        // Giả lập cập nhật database
        info!("Updating auto trade usage for user {}: used {} minutes", user_id, used_minutes);
        
        // Lấy thông tin hiện tại
        let mut usage = self.get_auto_trade_usage(user_id)?;
        
        // Cập nhật
        usage.update_time(used_minutes)?;
        
        // Lưu lại vào database
        // Đây chỉ là code mẫu
        debug!("Updated auto trade usage for user {}", user_id);
        
        Ok(())
    }
} 