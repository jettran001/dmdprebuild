//! Module quản lý thông tin đăng ký người dùng và các chức năng chuyển đổi giữa 
//! các loại gói đăng ký trong hệ thống DiamondChain.
//!
//! Module này cung cấp các chức năng:
//! * Định nghĩa cấu trúc dữ liệu UserSubscription cốt lõi cho đăng ký của người dùng
//! * Quản lý trạng thái đăng ký (đang hoạt động, hết hạn, đang chờ, đã hủy)
//! * Cập nhật và theo dõi thông tin về NFT cho đăng ký VIP
//! * Thực hiện nâng cấp giữa các loại đăng ký (Free → Premium → VIP)
//! * Xử lý hạ cấp khi hết hạn đăng ký hoặc theo yêu cầu người dùng
//! * Quản lý thông tin thanh toán liên quan đến đăng ký
//! * Gia hạn đăng ký và tính toán thời hạn mới
//!
//! Module này đóng vai trò trung tâm trong hệ thống đăng ký, cung cấp các cấu trúc dữ liệu
//! và chức năng được sử dụng bởi nhiều module khác như manager, payment, staking, và auto_trade.
//! Nó định nghĩa trait SubscriptionConverter cho phép chuyển đổi linh hoạt giữa các loại
//! đăng ký, hỗ trợ nhiều luồng nâng cấp/hạ cấp khác nhau trong hệ thống.

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
            } else if self.non_nft_status != NonNftVipStatus::StakedDMD {
                // Nếu không có NFT và cũng không phải là staked DMD, đánh dấu là pending
                self.non_nft_status = NonNftVipStatus::Pending;
            }
        }
        
        Ok(())
    }
    
    /// Kiểm tra nếu là gói VIP 12 tháng
    pub fn is_twelve_month_vip(&self) -> bool {
        // Chỉ áp dụng cho VIP
        if self.subscription_type != SubscriptionType::VIP {
            return false;
        }
        
        // Kiểm tra thời hạn đăng ký (12 tháng là khoảng 365 ngày)
        let duration = self.end_date.signed_duration_since(self.start_date);
        let days = duration.num_days();
        
        // Có thể hơn 365 ngày một chút do thời gian xử lý đăng ký
        days >= 360 && days <= 370
    }
}

/// Chuyển đổi giữa các loại subscription
pub trait SubscriptionConverter {
    /// Nâng cấp từ Free lên Premium
    /// 
    /// # Arguments
    /// Không cần tham số vì thao tác này chỉ chuyển đổi từ Free lên Premium
    /// 
    /// # Returns
    /// `AppResult<UserSubscription>` - Thông tin subscription sau khi nâng cấp
    fn upgrade_to_premium(&self) -> AppResult<UserSubscription>;
    
    /// Nâng cấp từ Premium lên VIP
    /// 
    /// # Arguments
    /// * `nft_info` - Thông tin NFT cần thiết để xác thực VIP
    /// 
    /// # Returns
    /// `AppResult<UserSubscription>` - Thông tin subscription sau khi nâng cấp
    fn upgrade_to_vip(&self, nft_info: super::nft::NftInfo) -> AppResult<UserSubscription>;
    
    /// Nâng cấp trực tiếp từ Free lên VIP
    /// 
    /// # Arguments
    /// * `nft_info` - Thông tin NFT cần thiết để xác thực VIP
    /// * `is_twelve_month_plan` - Có đăng ký gói 12 tháng hay không
    /// 
    /// # Returns
    /// `AppResult<UserSubscription>` - Thông tin subscription sau khi nâng cấp
    fn upgrade_free_to_vip(&self, nft_info: super::nft::NftInfo, is_twelve_month_plan: bool) -> AppResult<UserSubscription>;
    
    /// Đăng ký gói 12 tháng với staking token
    /// 
    /// # Arguments
    /// * `staking_amount` - Số lượng DMD token stake
    /// * `is_vip` - Có phải là gói VIP không, nếu false thì là Premium
    /// 
    /// # Returns
    /// `AppResult<UserSubscription>` - Thông tin subscription sau khi đăng ký
    fn upgrade_with_staking(&self, staking_amount: U256, is_vip: bool, nft_info: Option<super::nft::NftInfo>) -> AppResult<UserSubscription>;
    
    /// Hạ cấp về Free khi hết hạn subscription hoặc người dùng yêu cầu
    /// 
    /// # Returns
    /// `AppResult<UserSubscription>` - Thông tin subscription sau khi hạ cấp
    fn downgrade_to_free(&self) -> AppResult<UserSubscription>;
    
    /// Kiểm tra nếu subscription cần được hạ cấp về Free do hết hạn
    /// 
    /// # Returns
    /// `true` nếu cần hạ cấp, `false` nếu không
    fn needs_downgrade(&self) -> bool;
}

impl SubscriptionConverter for UserSubscription {
    fn upgrade_to_premium(&self) -> AppResult<UserSubscription> {
        if self.subscription_type != SubscriptionType::Free {
            return Err(WalletError::Other("Chỉ có thể nâng cấp từ Free lên Premium".to_string()).into());
        }
        
        let mut new_subscription = self.clone();
        new_subscription.subscription_type = SubscriptionType::Premium;
        
        // Thực hiện cập nhật dữ liệu vào MongoDB
        // TODO: Thực hiện kết nối và cập nhật MongoDB
        // Ví dụ:
        // async {
        //    let mongodb = get_mongodb_client();
        //    let collection = mongodb.collection("subscriptions");
        //    collection.update_one(
        //        doc! { "user_id": &self.user_id },
        //        doc! { "$set": { 
        //            "subscription_type": "Premium",
        //            "updated_at": Utc::now()
        //        }}
        //    ).await?;
        //    info!("Đã cập nhật dữ liệu nâng cấp lên Premium cho user_id: {}", self.user_id);
        // }
        
        Ok(new_subscription)
    }
    
    fn upgrade_to_vip(&self, nft_info: super::nft::NftInfo) -> AppResult<UserSubscription> {
        if self.subscription_type != SubscriptionType::Premium {
            return Err(WalletError::Other("Chỉ có thể nâng cấp từ Premium lên VIP".to_string()).into());
        }
        
        // Kiểm tra tính hợp lệ của NFT
        if !nft_info.is_valid {
            return Err(WalletError::Other("NFT không hợp lệ hoặc đã hết hạn".to_string()).into());
        }
        
        let mut new_subscription = self.clone();
        new_subscription.subscription_type = SubscriptionType::VIP;
        new_subscription.nft_info = Some(nft_info);
        
        // Thực hiện cập nhật dữ liệu vào MongoDB
        // TODO: Thực hiện kết nối và cập nhật MongoDB
        // Ví dụ:
        // async {
        //    let mongodb = get_mongodb_client();
        //    let collection = mongodb.collection("subscriptions");
        //    let nft_bson = bson::to_bson(&nft_info)?;
        //    
        //    collection.update_one(
        //        doc! { "user_id": &self.user_id },
        //        doc! { "$set": { 
        //            "subscription_type": "VIP",
        //            "nft_info": nft_bson,
        //            "updated_at": Utc::now()
        //        }}
        //    ).await?;
        //    info!("Đã cập nhật dữ liệu nâng cấp lên VIP cho user_id: {}", self.user_id);
        // }
        
        Ok(new_subscription)
    }
    
    fn upgrade_free_to_vip(&self, nft_info: super::nft::NftInfo, is_twelve_month_plan: bool) -> AppResult<UserSubscription> {
        if self.subscription_type != SubscriptionType::Free {
            return Err(WalletError::Other("Chỉ có thể nâng cấp từ Free lên VIP với phương thức này".to_string()).into());
        }
        
        // Kiểm tra tính hợp lệ của NFT
        if !nft_info.is_valid {
            return Err(WalletError::Other("NFT không hợp lệ hoặc đã hết hạn".to_string()).into());
        }
        
        let mut new_subscription = self.clone();
        new_subscription.subscription_type = SubscriptionType::VIP;
        new_subscription.nft_info = Some(nft_info);
        
        // Nếu là gói 12 tháng, cập nhật thời hạn
        if is_twelve_month_plan {
            new_subscription.end_date = Utc::now() + Duration::days(365);
        }
        
        // Thực hiện cập nhật dữ liệu vào MongoDB
        // TODO: Thực hiện kết nối và cập nhật MongoDB
        // Ví dụ:
        // async {
        //    let mongodb = get_mongodb_client();
        //    let collection = mongodb.collection("subscriptions");
        //    let nft_bson = bson::to_bson(&nft_info)?;
        //    
        //    collection.update_one(
        //        doc! { "user_id": &self.user_id },
        //        doc! { "$set": { 
        //            "subscription_type": "VIP",
        //            "nft_info": nft_bson,
        //            "end_date": new_subscription.end_date,
        //            "is_twelve_month_plan": is_twelve_month_plan,
        //            "updated_at": Utc::now()
        //        }}
        //    ).await?;
        //    info!("Đã cập nhật dữ liệu nâng cấp trực tiếp từ Free lên VIP cho user_id: {}", self.user_id);
        // }
        
        Ok(new_subscription)
    }
    
    fn upgrade_with_staking(&self, staking_amount: U256, is_vip: bool, nft_info: Option<super::nft::NftInfo>) -> AppResult<UserSubscription> {
        use super::constants::TWELVE_MONTH_STAKE_DAYS;
        use super::constants::MIN_DMD_STAKE_AMOUNT;
        use rust_decimal::Decimal;
        
        // Kiểm tra staking_amount có đủ không
        if staking_amount < U256::from(MIN_DMD_STAKE_AMOUNT.mantissa()) {
            return Err(WalletError::Other(format!(
                "Số lượng DMD stake không đủ, tối thiểu: {}",
                MIN_DMD_STAKE_AMOUNT
            )).into());
        }
        
        let mut new_subscription = self.clone();
        
        // Cập nhật loại đăng ký và thời hạn
        new_subscription.subscription_type = if is_vip {
            // Nếu là VIP, kiểm tra NFT hoặc sử dụng trạng thái StakedDMD
            if let Some(nft) = nft_info {
                if !nft.is_valid {
                    return Err(WalletError::Other("NFT không hợp lệ hoặc đã hết hạn".to_string()).into());
                }
                new_subscription.nft_info = Some(nft);
            } else {
                // Nếu không có NFT, đánh dấu là sử dụng staking DMD cho VIP
                new_subscription.non_nft_status = NonNftVipStatus::StakedDMD;
            }
            SubscriptionType::VIP
        } else {
            SubscriptionType::Premium
        };
        
        // Cập nhật thời hạn 12 tháng
        new_subscription.end_date = Utc::now() + Duration::days(TWELVE_MONTH_STAKE_DAYS);
        
        // Ghi lại số lượng token stake
        new_subscription.payment_token = PaymentToken::DMD;
        new_subscription.payment_amount = staking_amount;
        
        // Trạng thái đăng ký
        new_subscription.status = SubscriptionStatus::Pending;
        
        // TODO: Thực hiện cập nhật dữ liệu vào MongoDB
        // Ví dụ:
        // async {
        //    let mongodb = get_mongodb_client();
        //    let collection = mongodb.collection("subscriptions");
        //    
        //    // Lưu thông tin đăng ký mới
        //    let subscription_doc = bson::to_document(&new_subscription)?;
        //    collection.update_one(
        //        doc! { "user_id": &self.user_id },
        //        doc! { "$set": subscription_doc },
        //    ).await?;
        //    
        //    // Lưu thông tin staking
        //    let stake_collection = mongodb.collection("token_stakes");
        //    let stake_info = StakeInfo {
        //        user_id: self.user_id.clone(),
        //        wallet_address: self.payment_address.to_string(),
        //        amount: staking_amount.as_u128(),
        //        token: "DMD".to_string(),
        //        start_date: Utc::now(),
        //        end_date: new_subscription.end_date,
        //        is_active: true,
        //    };
        //    
        //    stake_collection.insert_one(stake_info).await?;
        //    
        //    info!("Đã cập nhật đăng ký với staking cho user_id: {}", self.user_id);
        // }
        
        Ok(new_subscription)
    }
    
    fn downgrade_to_free(&self) -> AppResult<UserSubscription> {
        if self.subscription_type == SubscriptionType::Free {
            return Err(WalletError::Other("Đã là Free User, không thể hạ cấp".to_string()).into());
        }
        
        let mut new_subscription = self.clone();
        new_subscription.subscription_type = SubscriptionType::Free;
        new_subscription.nft_info = None;
        new_subscription.non_nft_status = NonNftVipStatus::Pending;
        new_subscription.status = SubscriptionStatus::Expired;
        
        // Đặt thời hạn mới cho tài khoản Free
        let now = Utc::now();
        new_subscription.start_date = now;
        new_subscription.end_date = now + Duration::days(365 * 10); // Free user không có hạn sử dụng
        
        // Thực hiện cập nhật dữ liệu vào MongoDB
        // TODO: Thực hiện kết nối và cập nhật MongoDB
        // Ví dụ:
        // async {
        //    let mongodb = get_mongodb_client();
        //    let collection = mongodb.collection("subscriptions");
        //    
        //    collection.update_one(
        //        doc! { "user_id": &self.user_id },
        //        doc! { "$set": { 
        //            "subscription_type": "Free",
        //            "nft_info": null,
        //            "non_nft_status": "Pending",
        //            "status": "Expired",
        //            "start_date": now,
        //            "end_date": new_subscription.end_date,
        //            "updated_at": now
        //        }}
        //    ).await?;
        //    
        //    // Cập nhật auto_trade_usage
        //    let auto_trade_collection = mongodb.collection("auto_trade_usage");
        //    auto_trade_collection.update_one(
        //        doc! { "user_id": &self.user_id },
        //        doc! { "$set": {
        //            "status": "Inactive",
        //            "remaining_minutes": 0
        //        }}
        //    ).await?;
        //    
        //    info!("Đã cập nhật dữ liệu hạ cấp về Free cho user_id: {}", self.user_id);
        // }
        
        Ok(new_subscription)
    }
    
    fn needs_downgrade(&self) -> bool {
        // Kiểm tra nếu subscription đã hết hạn
        if self.is_expired() {
            return true;
        }
        
        // Kiểm tra đặc biệt cho VIP với NFT
        if self.subscription_type == SubscriptionType::VIP {
            // Nếu có NFT, kiểm tra tính hợp lệ của NFT
            if let Some(nft_info) = &self.nft_info {
                if !nft_info.is_valid {
                    return true;
                }
            } else {
                // VIP mà không có NFT thì cũng cần hạ cấp
                return true;
            }
        }
        
        false
    }
} 