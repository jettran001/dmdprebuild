//! Triển khai logic quản lý đăng ký cụ thể cho người dùng.

use chrono::{DateTime, Duration, Utc};
use ethers::types::{Address, U256};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::nft::NonNftVipStatus;
use super::types::{PaymentToken, SubscriptionStatus, SubscriptionType};
use crate::error::{AppResult, WalletError};

/// Chi tiết đăng ký của người dùng.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSubscription {
    /// ID người dùng
    pub user_id: String,
    /// Loại gói đăng ký
    pub subscription_type: SubscriptionType,
    /// Ngày bắt đầu
    pub start_date: DateTime<Utc>,
    /// Ngày kết thúc
    pub end_date: DateTime<Utc>,
    /// Địa chỉ ví thanh toán
    pub payment_address: Address,
    /// Mã giao dịch thanh toán
    pub payment_tx_hash: Option<String>,
    /// Token sử dụng để thanh toán
    pub payment_token: PaymentToken,
    /// Số lượng token đã thanh toán
    pub payment_amount: U256,
    /// Trạng thái đăng ký
    pub status: SubscriptionStatus,
    /// Thông tin NFT (chỉ cho VIP)
    pub nft_info: Option<super::nft::NftInfo>,
    /// Trạng thái nếu không có NFT (chỉ cho VIP khi không có NFT)
    pub non_nft_status: NonNftVipStatus,
}

impl UserSubscription {
    /// Tạo mới thông tin đăng ký
    pub fn new(
        user_id: &str,
        subscription_type: SubscriptionType,
        payment_address: Address,
        payment_token: PaymentToken,
        duration_days: i64,
        payment_amount: U256,
    ) -> Self {
        let now = Utc::now();
        let end_date = now + Duration::days(duration_days);
        
        Self {
            user_id: user_id.to_string(),
            subscription_type,
            start_date: now,
            end_date,
            payment_address,
            payment_tx_hash: None,
            payment_token,
            payment_amount,
            status: SubscriptionStatus::Pending,
            nft_info: None,
            non_nft_status: NonNftVipStatus::Valid,
        }
    }
    
    /// Kiểm tra nếu đăng ký đang hoạt động
    pub fn is_active(&self) -> bool {
        self.status == SubscriptionStatus::Active && self.end_date > Utc::now()
    }
    
    /// Kiểm tra nếu đăng ký đã hết hạn
    pub fn is_expired(&self) -> bool {
        self.end_date < Utc::now() || self.status == SubscriptionStatus::Expired
    }
    
    /// Kích hoạt đăng ký sau khi thanh toán thành công
    pub fn activate(&mut self, tx_hash: &str) {
        self.status = SubscriptionStatus::Active;
        self.payment_tx_hash = Some(tx_hash.to_string());
    }
    
    /// Gia hạn đăng ký thêm một số ngày
    pub fn extend(&mut self, days: i64) {
        self.end_date = self.end_date + Duration::days(days);
        if self.status == SubscriptionStatus::Expired && self.end_date > Utc::now() {
            self.status = SubscriptionStatus::Active;
        }
    }
    
    /// Hủy đăng ký
    pub fn cancel(&mut self) {
        self.status = SubscriptionStatus::Cancelled;
    }
    
    /// Cập nhật thông tin NFT
    pub fn update_nft_info(&mut self, nft_info: super::nft::NftInfo) {
        self.nft_info = Some(nft_info);
    }
    
    /// Cập nhật trạng thái khi không có NFT
    pub fn update_non_nft_status(&mut self, status: NonNftVipStatus) {
        self.non_nft_status = status;
    }
    
    /// Refresh trạng thái đăng ký theo thời gian hiện tại
    pub fn refresh_status(&mut self) -> AppResult<()> {
        // Nếu đã hết hạn, cập nhật trạng thái
        if self.end_date < Utc::now() && self.status == SubscriptionStatus::Active {
            self.status = SubscriptionStatus::Expired;
        }
        
        // Đối với VIP, cần kiểm tra NFT
        if self.subscription_type == SubscriptionType::VIP && self.status == SubscriptionStatus::Active {
            if let Some(nft_info) = &self.nft_info {
                if !nft_info.is_valid {
                    // Nếu NFT không còn hợp lệ, cập nhật trạng thái
                    self.non_nft_status = NonNftVipStatus::Pending;
                }
            }
        }
        
        Ok(())
    }
}

/// Chuyển đổi giữa các loại subscription
pub trait SubscriptionConverter {
    /// Nâng cấp từ Free lên Premium
    fn upgrade_to_premium(&self) -> AppResult<UserSubscription>;
    
    /// Nâng cấp từ Premium lên VIP
    fn upgrade_to_vip(&self, nft_info: super::nft::NftInfo) -> AppResult<UserSubscription>;
    
    /// Hạ cấp về Free
    /// 
    /// Phương thức này chuẩn bị thông tin để hạ cấp subscription xuống Free.
    /// 
    /// # Note
    /// Hiện tại chỉ trả về Ok(()) vì logic thực tế cần được triển khai khi có database.
    /// Trong triển khai đầy đủ, cần:
    /// - Cập nhật thông tin trong database
    /// - Cập nhật trạng thái đăng ký
    /// - Xóa các quyền ưu tiên của người dùng
    /// 
    /// # Returns
    /// `AppResult<()>` - Kết quả của thao tác, mã lỗi nếu có
    fn downgrade_to_free(&self) -> AppResult<()> {
        // TODO: Triển khai chi tiết logic hạ cấp khi có database
        // Trong thực tế, cần lưu thông tin vào database
        
        Ok(())
    }
}

impl SubscriptionConverter for UserSubscription {
    fn upgrade_to_premium(&self) -> AppResult<UserSubscription> {
        if self.subscription_type != SubscriptionType::Free {
            return Err(WalletError::Other("Chỉ có thể nâng cấp từ Free lên Premium".to_string()).into());
        }
        
        let mut new_subscription = self.clone();
        new_subscription.subscription_type = SubscriptionType::Premium;
        // Giữ nguyên thời hạn hiện tại
        
        Ok(new_subscription)
    }
    
    fn upgrade_to_vip(&self, nft_info: super::nft::NftInfo) -> AppResult<UserSubscription> {
        if self.subscription_type != SubscriptionType::Premium {
            return Err(WalletError::Other("Chỉ có thể nâng cấp từ Premium lên VIP".to_string()).into());
        }
        
        let mut new_subscription = self.clone();
        new_subscription.subscription_type = SubscriptionType::VIP;
        new_subscription.nft_info = Some(nft_info);
        
        Ok(new_subscription)
    }
    
    fn downgrade_to_free(&self) -> AppResult<()> {
        // Thực hiện các tác vụ cần thiết để hạ cấp về Free
        // Trong thực tế, cần lưu thông tin vào database
        
        Ok(())
    }
} 