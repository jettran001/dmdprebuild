//! 🧭 Module quản lý đăng ký và gói dịch vụ (subscription)
//! 
//! Module này cung cấp các chức năng để quản lý đăng ký và các gói dịch vụ (Free, Premium, VIP)
//! trong hệ thống DiamondChain. Triển khai các tính năng theo mô hình:
//! - manager: SubscriptionManager điều phối tổng thể
//! - user_subscription: Thông tin đăng ký của người dùng
//! - types: Định nghĩa các kiểu dữ liệu
//! - payment: Xử lý thanh toán
//! - staking: Quản lý staking DMD token
//! - nft: Kiểm tra sở hữu NFT
//! - auto_trade: Quản lý thời gian auto-trade
//! - constants: Các hằng số
//! - events: Xử lý sự kiện
//! - utils: Các hàm tiện ích
//! - tests: Kiểm thử

// External imports
use chrono::{DateTime, Utc};
use thiserror::Error;
use std::str::FromStr;
use std::fmt;

// Standard library imports
use std::collections::HashMap;

// Internal imports
use crate::users::free_user::FreeUserManager;
use crate::error::WalletError;

// Modules
mod manager;
mod user_subscription;
mod types;
mod payment;
mod staking;
mod nft;
mod auto_trade;
mod constants;
mod events;
mod utils;
#[cfg(test)]
mod tests;

// Re-exports
pub use manager::SubscriptionManager;
pub use user_subscription::UserSubscription;
pub use types::{SubscriptionType, SubscriptionStatus, Feature, PaymentToken, NonNftVipStatus};
pub use staking::{StakingManager, TokenStake, StakeStatus};
pub use nft::{NftInfo, VipNftInfo};
pub use events::{SubscriptionEvent, EventType, EventEmitter};
pub use auto_trade::{AutoTradeManager, AutoTradeStatus, AutoTradeUsage};
pub use constants::*;

/// Các lỗi liên quan đến module subscription.
///
/// Enum này định nghĩa các loại lỗi có thể xảy ra trong quá trình xử lý
/// các chức năng liên quan đến subscription trong hệ thống.
#[derive(Error, Debug)]
pub enum SubscriptionError {
    /// Lỗi khi không tìm thấy subscription của user.
    ///
    /// Xảy ra khi thực hiện các thao tác trên một subscription không tồn tại.
    /// Thường gặp khi cố gắng truy cập thông tin subscription của một người dùng
    /// chưa đăng ký hoặc khi user_id không tồn tại.
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::SubscriptionError;
    /// let error = SubscriptionError::NotFound("user123".to_string());
    /// assert!(error.to_string().contains("user123"));
    /// ```
    #[error("Không tìm thấy subscription cho người dùng: {0}")]
    NotFound(String),
    
    /// Lỗi khi không đủ quyền cho một thao tác nào đó.
    ///
    /// Xảy ra khi người dùng cố gắng sử dụng một tính năng không được phép
    /// với loại subscription hiện tại của họ.
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::SubscriptionError;
    /// let error = SubscriptionError::InsufficientPermission("Auto-trade yêu cầu gói Premium".to_string());
    /// ```
    #[error("Không đủ quyền: {0}")]
    InsufficientPermission(String),
    
    /// Lỗi khi subscription đã hết hạn.
    ///
    /// Xảy ra khi cố gắng sử dụng một tính năng với subscription đã hết hạn,
    /// nhưng chưa được gia hạn hoặc nâng cấp.
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::{SubscriptionError, SubscriptionType};
    /// let error = SubscriptionError::Expired(SubscriptionType::Premium, "2023-01-01".to_string());
    /// ```
    #[error("Subscription {0} đã hết hạn vào {1}")]
    Expired(SubscriptionType, String),
    
    /// Lỗi khi thực hiện thanh toán cho subscription.
    ///
    /// Bao gồm các lỗi như không đủ số dư, giao dịch thất bại,
    /// token không được hỗ trợ, v.v.
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::SubscriptionError;
    /// let error = SubscriptionError::PaymentError("Không đủ số dư USDC".to_string());
    /// ```
    #[error("Lỗi thanh toán: {0}")]
    PaymentError(String),
    
    /// Lỗi khi thực hiện thao tác với NFT.
    ///
    /// Xảy ra khi có vấn đề liên quan đến việc xác minh sở hữu NFT,
    /// như không tìm thấy NFT, NFT đã bị chuyển đi, v.v.
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::SubscriptionError;
    /// let error = SubscriptionError::NftError("Không sở hữu NFT VIP".to_string());
    /// ```
    #[error("Lỗi NFT: {0}")]
    NftError(String),
    
    /// Lỗi khi thực hiện thao tác với staking.
    ///
    /// Xảy ra khi có vấn đề liên quan đến việc stake/unstake token,
    /// như không đủ token để stake, lỗi khi tương tác với hợp đồng, v.v.
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::SubscriptionError;
    /// let error = SubscriptionError::StakingError("Không thể unstake token trước thời hạn".to_string());
    /// ```
    #[error("Lỗi staking: {0}")]
    StakingError(String),
    
    /// Lỗi khi thực hiện chuyển đổi giữa các loại subscription.
    ///
    /// Xảy ra khi có vấn đề trong quá trình nâng cấp hoặc hạ cấp subscription,
    /// như chuyển đổi không được phép, thiếu điều kiện tiên quyết, v.v.
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::{SubscriptionError, SubscriptionType};
    /// let error = SubscriptionError::ConversionError(
    ///     SubscriptionType::VIP,
    ///     SubscriptionType::Free,
    ///     "Không thể hạ cấp trực tiếp từ VIP xuống Free".to_string()
    /// );
    /// ```
    #[error("Lỗi khi chuyển đổi từ {0} sang {1}: {2}")]
    ConversionError(SubscriptionType, SubscriptionType, String),
    
    /// Lỗi khi subscription đã tồn tại.
    ///
    /// Xảy ra khi cố gắng tạo một subscription mới cho một người dùng
    /// đã có subscription hoạt động.
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::{SubscriptionError, SubscriptionType};
    /// let error = SubscriptionError::AlreadyExists(SubscriptionType::Premium);
    /// ```
    #[error("Subscription {0} đã tồn tại")]
    AlreadyExists(SubscriptionType),
    
    /// Lỗi khi xảy ra lỗi cơ sở dữ liệu.
    ///
    /// Xảy ra khi có vấn đề khi truy cập hoặc cập nhật dữ liệu
    /// subscription trong cơ sở dữ liệu.
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::SubscriptionError;
    /// let error = SubscriptionError::DatabaseError("Không thể kết nối đến database".to_string());
    /// ```
    #[error("Lỗi cơ sở dữ liệu: {0}")]
    DatabaseError(String),
    
    /// Lỗi khi xảy ra xung đột trong đồng bộ hóa dữ liệu.
    ///
    /// Xảy ra khi có xung đột khi nhiều thread cùng cập nhật một subscription,
    /// hoặc khi có race condition trong quá trình xử lý.
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::SubscriptionError;
    /// let error = SubscriptionError::SyncError("Xung đột khi cập nhật subscription".to_string());
    /// ```
    #[error("Lỗi đồng bộ hóa: {0}")]
    SyncError(String),
    
    /// Lỗi khi có vấn đề với tài khoản người dùng.
    ///
    /// Xảy ra khi có vấn đề liên quan đến tài khoản người dùng,
    /// như tài khoản bị khóa, chưa xác minh, v.v.
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::SubscriptionError;
    /// let error = SubscriptionError::AccountError("Tài khoản đã bị khóa".to_string());
    /// ```
    #[error("Lỗi tài khoản: {0}")]
    AccountError(String),
    
    /// Lỗi khi đối số không hợp lệ.
    ///
    /// Xảy ra khi cung cấp các tham số không hợp lệ cho các hàm
    /// trong module subscription.
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::SubscriptionError;
    /// let error = SubscriptionError::InvalidArgument("duration_days phải lớn hơn 0".to_string());
    /// ```
    #[error("Đối số không hợp lệ: {0}")]
    InvalidArgument(String),
    
    /// Các lỗi khác không thuộc các loại trên.
    ///
    /// Dùng cho các trường hợp lỗi chưa được phân loại cụ thể.
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::SubscriptionError;
    /// let error = SubscriptionError::Other("Lỗi không xác định".to_string());
    /// ```
    #[error("Lỗi khác: {0}")]
    Other(String),
}

/// Chuyển đổi từ SubscriptionError sang WalletError.
///
/// Trait implementation này cho phép chuyển đổi từ SubscriptionError sang WalletError,
/// giúp đảm bảo xử lý lỗi nhất quán trong toàn bộ hệ thống.
impl From<SubscriptionError> for WalletError {
    fn from(error: SubscriptionError) -> Self {
        match error {
            SubscriptionError::NotFound(msg) => WalletError::NotFound(msg),
            SubscriptionError::InsufficientPermission(msg) => WalletError::AccessDenied(format!("Không đủ quyền: {}", msg)),
            SubscriptionError::Expired(sub_type, date) => WalletError::Other(format!("Subscription {:?} đã hết hạn vào {}", sub_type, date)),
            SubscriptionError::PaymentError(msg) => WalletError::Other(format!("Lỗi thanh toán subscription: {}", msg)),
            SubscriptionError::NftError(msg) => WalletError::Other(format!("Lỗi NFT subscription: {}", msg)),
            SubscriptionError::StakingError(msg) => WalletError::Other(format!("Lỗi staking subscription: {}", msg)),
            SubscriptionError::ConversionError(from, to, msg) => WalletError::InvalidOperation(format!("Không thể chuyển đổi từ {:?} sang {:?}: {}", from, to, msg)),
            SubscriptionError::AlreadyExists(sub_type) => WalletError::AlreadyExists(format!("Subscription {:?} đã tồn tại", sub_type)),
            SubscriptionError::DatabaseError(msg) => WalletError::DatabaseError(msg),
            SubscriptionError::SyncError(msg) => WalletError::Other(format!("Lỗi đồng bộ hóa subscription: {}", msg)),
            SubscriptionError::AccountError(msg) => WalletError::Other(format!("Lỗi tài khoản: {}", msg)),
            SubscriptionError::InvalidArgument(msg) => WalletError::Other(format!("Đối số không hợp lệ: {}", msg)),
            SubscriptionError::Other(msg) => WalletError::Other(format!("Lỗi subscription: {}", msg)),
        }
    }
}

/// Kiểm tra quyền truy cập tính năng dựa trên loại người dùng và tính năng.
///
/// Hàm này kiểm tra xem người dùng có quyền sử dụng tính năng cụ thể hay không
/// dựa trên loại subscription và trạng thái hiện tại.
///
/// # Arguments
/// * `user` - Tham chiếu đến đối tượng User cần kiểm tra
/// * `feature` - Tính năng cần kiểm tra quyền truy cập
///
/// # Returns
/// `bool` - `true` nếu người dùng có quyền truy cập, ngược lại là `false`
///
/// # Examples
/// ```
/// use wallet::users::subscription::{has_feature_access, Feature};
/// use wallet::users::User;
///
/// let user = get_user(); // Giả sử hàm này tồn tại
/// if has_feature_access(&user, Feature::AutoTrade) {
///     // Cho phép sử dụng tính năng auto trade
/// } else {
///     // Hiển thị thông báo nâng cấp
/// }
/// ```
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
/// * `Ok(())` - Nếu cập nhật thành công
/// * `Err(AppError)` - Nếu có lỗi trong quá trình cập nhật
///
/// # Examples
/// ```
/// use wallet::users::User;
/// use wallet::users::subscription::refresh_subscription_status;
///
/// // Cập nhật trạng thái đăng ký cho người dùng
/// let mut user = get_user(); // Giả sử hàm này tồn tại
/// refresh_subscription_status(&mut user).await?;
/// ```
pub async fn refresh_subscription_status(user: &mut crate::users::User) -> Result<(), WalletError> {
    let now = Utc::now();
    
    if let Some(subscription) = &mut user.subscription {
        // Kiểm tra hết hạn
        if subscription.end_date < now && subscription.status != SubscriptionStatus::Expired {
            // Cập nhật trạng thái thành hết hạn
            subscription.status = SubscriptionStatus::Expired;
            
            // TODO: Gửi thông báo cho người dùng
            
            // TODO: Lưu thay đổi vào database
        }
        
        // Kiểm tra sắp hết hạn
        if subscription.status == SubscriptionStatus::Active {
            let days_remaining = (subscription.end_date - now).num_days();
            
            if days_remaining <= 3 && !subscription.is_expiry_notified {
                // Đánh dấu đã thông báo
                subscription.is_expiry_notified = true;
                
                // TODO: Gửi thông báo sắp hết hạn
                
                // TODO: Lưu thay đổi vào database
            }
        }
    }
    
    Ok(())
} 