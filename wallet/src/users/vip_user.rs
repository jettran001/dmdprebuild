//! Module quản lý người dùng VIP cho wallet module.
//! Cung cấp các tính năng không giới hạn và đặc quyền cao cấp nhất cho người dùng VIP.
//!
//! Module này cung cấp các chức năng quản lý người dùng VIP, bao gồm:
//! - Kiểm tra và xác thực NFT của người dùng VIP
//! - Quản lý trạng thái VIP khi người dùng không còn NFT
//! - Xử lý các trường hợp stake DMD token thay thế NFT
//! - Quản lý chu kỳ đăng ký VIP và gia hạn tự động
//! - Cung cấp thông tin về đặc quyền VIP
//! - Hỗ trợ kích hoạt đặc quyền đặc biệt cho người dùng VIP
//! - Kiểm tra tính hợp lệ của NFT VIP
//! 
//! Module này làm việc chặt chẽ với các module khác như user_subscription, nft, 
//! và staking để đảm bảo người dùng VIP nhận được đầy đủ quyền lợi.

// External imports
use chrono::{DateTime, Duration, Utc};
use ethers::types::{Address, U256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug};
use uuid::Uuid;

// Internal imports
use crate::error::WalletError;
use crate::walletmanager::api::WalletManagerApi;
use crate::users::subscription::constants::VIP_NFT_CHECK_HOUR;
use crate::users::subscription::nft::{NftInfo, NonNftVipStatus};
use crate::users::subscription::types::{SubscriptionStatus, SubscriptionType, PaymentToken};
use crate::users::subscription::user_subscription::UserSubscription;
use crate::users::subscription::staking;
use crate::cache::{get_vip_user, update_vip_user_cache};

/// Trạng thái của VIP user
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VipUserStatus {
    /// Đang hoạt động bình thường
    Active,
    /// Tạm dừng (không có NFT)
    Suspended,
    /// Đã hết hạn
    Expired,
}

/// Thông tin người dùng VIP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VipUserData {
    /// ID người dùng
    pub user_id: String,
    /// Thời gian tạo tài khoản
    pub created_at: DateTime<Utc>,
    /// Thời gian hoạt động cuối
    pub last_active: DateTime<Utc>,
    /// Danh sách địa chỉ ví
    pub wallet_addresses: Vec<Address>,
    /// Ngày hết hạn gói VIP
    pub subscription_end_date: DateTime<Utc>,
    /// Mã hỗ trợ VIP riêng
    pub vip_support_code: String,
    /// Đặc quyền bổ sung
    pub special_privileges: Vec<String>,
    /// Thông tin NFT
    pub nft_info: Option<NftInfo>,
}

/// Quản lý người dùng VIP.
pub struct VipUserManager {
    /// API wallet để xác thực NFT
    wallet_api: Arc<RwLock<WalletManagerApi>>,
}

impl VipUserManager {
    /// Khởi tạo VipUserManager mới.
    #[flow_from("wallet::init")]
    pub fn new(wallet_api: Arc<RwLock<WalletManagerApi>>) -> Self {
        Self {
            wallet_api,
        }
    }

    /// Kiểm tra xem người dùng có thể thực hiện giao dịch không.
    #[flow_from("wallet::api")]
    pub async fn can_perform_transaction(&self, user_id: &str) -> Result<bool, WalletError> {
        // Người dùng VIP không bị giới hạn giao dịch
        if let Some(user_data) = get_vip_user(user_id, |id| async move {
            // Hàm loader, sẽ được gọi khi không tìm thấy trong cache
            self.load_vip_user_from_db(&id).await.unwrap_or(None)
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
        // Người dùng VIP được sử dụng snipebot không giới hạn
        Ok(true)
    }

    /// Kích hoạt đặc quyền VIP đặc biệt.
    #[flow_from("wallet::ui")]
    pub async fn activate_special_privilege(
        &self,
        user_id: &str,
        privilege_name: &str
    ) -> Result<(), WalletError> {
        // Đây là phần thực hiện cho đặc quyền đặc biệt của người dùng VIP
        info!("Kích hoạt đặc quyền '{}' cho người dùng VIP {}", privilege_name, user_id);
        
        // TODO: Triển khai logic kích hoạt đặc quyền thực tế
        Ok(())
    }
    
    /// Kiểm tra xem NFT có tồn tại trong ví của user không
    ///
    /// # Arguments
    /// - `wallet_address`: Địa chỉ ví cần kiểm tra
    /// - `nft_contract`: Địa chỉ smart contract của NFT
    /// - `token_id`: ID của token (chuẩn ERC-1155)
    /// - `chain_id`: ID của chain chứa NFT
    ///
    /// # Returns
    /// - `Ok(true)`: Nếu NFT tồn tại trong ví
    /// - `Ok(false)`: Nếu NFT không tồn tại trong ví
    /// - `Err`: Nếu có lỗi khi kiểm tra
    #[flow_from("wallet::nft")]
    pub async fn check_nft_ownership(
        &self,
        wallet_address: Address,
        nft_contract: Address,
        token_id: U256,
        chain_id: u64,
    ) -> Result<bool, WalletError> {
        // Sử dụng RwLock an toàn trong async context
        let wallet_api_guard = match self.wallet_api.read().await {
            guard => guard
        };
        
        // Thêm validation trước khi kiểm tra
        if wallet_address.is_zero() {
            warn!("Địa chỉ ví không hợp lệ (zero address) khi kiểm tra NFT");
            return Err(WalletError::Other("Địa chỉ ví không hợp lệ".to_string()));
        }
        
        if nft_contract.is_zero() {
            warn!("Địa chỉ hợp đồng NFT không hợp lệ (zero address)");
            return Err(WalletError::Other("Địa chỉ hợp đồng NFT không hợp lệ".to_string()));
        }
        
        // TODO: Triển khai thực tế để kiểm tra NFT trên blockchain
        // Đây là ví dụ đơn giản, cần kết nối đến blockchain thực tế để kiểm tra balance của NFT
        
        // Trong thực tế, cần gọi hàm balanceOf từ smart contract ERC-1155
        // Và kiểm tra nếu balance > 0
        
        info!("Kiểm tra NFT: address={:?}, contract={:?}, token_id={}, chain_id={}", 
              wallet_address, nft_contract, token_id, chain_id);
        
        // Giả lập trả về true
        Ok(true)
    }

    /// Kiểm tra NFT của tất cả VIP users
    /// Hàm này nên được gọi định kỳ sau VIP_NFT_CHECK_HOUR UTC mỗi ngày
    #[flow_from("system::cron")]
    pub async fn check_all_vip_nft_status(
        &self,
        user_subscriptions: &mut HashMap<String, UserSubscription>
    ) -> Result<Vec<String>, WalletError> {
        let now = Utc::now();
        
        // Chỉ chạy kiểm tra sau VIP_NFT_CHECK_HOUR UTC
        if now.hour() < VIP_NFT_CHECK_HOUR {
            info!("Bỏ qua kiểm tra NFT, chưa đến giờ kiểm tra ({} UTC)", VIP_NFT_CHECK_HOUR);
            return Ok(Vec::new());
        }
        
        // Danh sách user_ids cần thông báo
        let mut users_without_nft = Vec::new();
        let mut total_checked = 0;
        let mut error_count = 0;
        
        info!("Bắt đầu kiểm tra NFT cho {} VIP users", 
              user_subscriptions.iter().filter(|(_, s)| s.subscription_type == SubscriptionType::VIP).count());
        
        for (user_id, subscription) in user_subscriptions.iter_mut() {
            // Chỉ kiểm tra các VIP user đang active
            if subscription.subscription_type == SubscriptionType::VIP && 
               subscription.status == SubscriptionStatus::Active {
                
                // Bỏ qua kiểm tra NFT cho người dùng VIP 12 tháng đã stake token
                if subscription.non_nft_status == NonNftVipStatus::StakedDMD {
                    debug!("Bỏ qua kiểm tra NFT cho user {}: đã stake DMD token", user_id);
                    continue;
                }
                
                if let Some(nft_info) = &mut subscription.nft_info {
                    // Kiểm tra nếu đã kiểm tra trong ngày
                    let nft_last_checked_day = nft_info.last_verified.date_naive().day();
                    let current_day = now.date_naive().day();
                    
                    // Chỉ kiểm tra mỗi ngày một lần
                    if nft_last_checked_day != current_day {
                        total_checked += 1;
                        
                        // Kiểm tra NFT trên blockchain
                        match self.check_nft_ownership(
                            subscription.payment_address,
                            nft_info.contract_address,
                            nft_info.token_id,
                            nft_info.chain_id.unwrap_or(1) // Sử dụng chain_id từ nft_info hoặc mặc định
                        ).await {
                            Ok(has_nft) => {
                                // Cập nhật trạng thái
                                nft_info.update_status(has_nft, now);
                                
                                if !has_nft {
                                    warn!("User {} không có NFT VIP, cần thông báo", user_id);
                                    users_without_nft.push(user_id.clone());
                                } else {
                                    debug!("Xác nhận user {} có NFT VIP hợp lệ", user_id);
                                }
                            },
                            Err(e) => {
                                error_count += 1;
                                error!("Lỗi khi kiểm tra NFT cho người dùng {}: {}", user_id, e);
                                // Không thay đổi trạng thái nếu có lỗi
                            }
                        }
                    }
                } else {
                    warn!("User {} là VIP nhưng không có thông tin NFT", user_id);
                }
            }
        }
        
        info!("Hoàn thành kiểm tra NFT: {} users được kiểm tra, {} không có NFT, {} lỗi", 
              total_checked, users_without_nft.len(), error_count);
        
        Ok(users_without_nft)
    }

    /// Cập nhật trạng thái cho VIP user không có NFT
    ///
    /// # Arguments
    /// - `subscription`: Đăng ký cần cập nhật
    /// - `status`: Trạng thái mới (UsePremium hoặc Paused)
    ///
    /// # Returns
    /// - `Ok(())`: Nếu cập nhật thành công
    /// - `Err`: Nếu có lỗi
    #[flow_from("wallet::ui")]
    pub fn update_non_nft_vip_status(
        &self,
        subscription: &mut UserSubscription,
        status: NonNftVipStatus,
    ) -> Result<(), WalletError> {
        if subscription.subscription_type != SubscriptionType::VIP || 
           subscription.status != SubscriptionStatus::Active {
            return Err(WalletError::Other("Người dùng không phải là VIP đang hoạt động".to_string()));
        }
        
        // Cập nhật trạng thái
        subscription.non_nft_status = status;
        
        Ok(())
    }

    /// Nâng cấp tài khoản Premium lên VIP với NFT
    ///
    /// # Arguments
    /// - `subscription`: Đăng ký hiện tại (Premium)
    /// - `nft_contract_address`: Địa chỉ hợp đồng NFT
    /// - `nft_token_id`: ID token của NFT
    /// - `payment_token`: Loại token thanh toán
    ///
    /// # Returns
    /// - `Ok(UserSubscription)`: Đăng ký VIP mới
    /// - `Err`: Nếu có lỗi
    #[flow_from("wallet::ui")]
    pub async fn upgrade_premium_to_vip(
        &self,
        subscription: &UserSubscription,
        nft_contract_address: Address,
        nft_token_id: U256,
        payment_token: PaymentToken,
    ) -> Result<UserSubscription, WalletError> {
        // Kiểm tra xem đăng ký hiện tại có phải Premium không
        if subscription.subscription_type != SubscriptionType::Premium {
            return Err(WalletError::Other("Chỉ có thể nâng cấp từ Premium lên VIP".to_string()));
        }
        
        // Kiểm tra NFT
        let has_nft = self.check_nft_ownership(
            subscription.payment_address,
            nft_contract_address,
            nft_token_id,
            1, // Chain ID mặc định
        ).await?;
        
        if !has_nft {
            return Err(WalletError::Other("Không tìm thấy NFT trong ví".to_string()));
        }
        
        // Tạo thông tin NFT mới
        let nft_info = NftInfo::new(
            nft_contract_address,
            nft_token_id,
            Utc::now(),
            Some(1), // Chain ID
        );
        
        // TODO: Triển khai nâng cấp từ Premium lên VIP thực tế
        
        // Tạo subscription VIP mới với thông tin từ Premium
        let mut vip_subscription = subscription.clone();
        vip_subscription.subscription_type = SubscriptionType::VIP;
        vip_subscription.nft_info = Some(nft_info);
        vip_subscription.non_nft_status = NonNftVipStatus::Valid;
        
        // Thêm logic cập nhật cache ở đây nếu cần
        if let Some(vip_user_data) = get_vip_user(&subscription.user_id, |id| async move {
            self.load_vip_user_from_db(&id).await.unwrap_or(None)
        }).await {
            // Đã có dữ liệu user VIP, cập nhật
            let mut updated_data = vip_user_data.clone();
            updated_data.nft_info = Some(NftInfo::new(nft_contract_address, nft_token_id));
            update_vip_user_cache(&subscription.user_id, updated_data).await;
        } else {
            // Tạo dữ liệu mới
            let vip_data = VipUserData {
                user_id: subscription.user_id.clone(),
                created_at: Utc::now(),
                last_active: Utc::now(),
                wallet_addresses: vec![subscription.payment_address],
                subscription_end_date: vip_subscription.end_date,
                vip_support_code: generate_vip_support_code(),
                special_privileges: Vec::new(),
                nft_info: Some(NftInfo::new(nft_contract_address, nft_token_id)),
            };
            // Thêm vào cache VIP users
            update_vip_user_cache(&subscription.user_id, vip_data).await;
        }
        
        // Cập nhật thông tin khác
        // TODO: Thêm logic tính thời hạn mới và các thông tin liên quan
        
        Ok(vip_subscription)
    }
    
    /// Nâng cấp tài khoản lên VIP sử dụng stake DMD token thay vì NFT
    /// 
    /// # Arguments
    /// - `user_id`: ID người dùng
    /// - `subscription`: Đăng ký hiện tại
    /// - `wallet_address`: Địa chỉ ví dùng để stake
    /// - `stake_amount`: Số lượng DMD token để stake
    /// - `duration_months`: Thời hạn (thường là 12 tháng)
    /// 
    /// # Returns
    /// - `Ok(UserSubscription)`: Đăng ký VIP mới với trạng thái StakedDMD
    /// - `Err`: Nếu có lỗi
    #[flow_from("wallet::staking")]
    pub async fn upgrade_with_staking(
        &self,
        user_id: &str,
        subscription: &UserSubscription,
        wallet_address: Address,
        stake_amount: U256,
        duration_months: u32,
    ) -> Result<UserSubscription, WalletError> {
        // Import hoặc định nghĩa MIN_DMD_STAKE_AMOUNT từ constants
        let min_stake_amount = crate::users::subscription::constants::MIN_DMD_STAKE_AMOUNT;
        
        // Kiểm tra số lượng token stake có đủ không
        if stake_amount < min_stake_amount {
            return Err(WalletError::Other(format!("Số lượng DMD token stake không đủ. Cần ít nhất {}", min_stake_amount)));
        }
        
        // TODO: Gọi hàm stake_tokens thực tế từ StakingManager
        
        // Tính toán thời gian hết hạn mới
        let now = Utc::now();
        let duration = Duration::days(30 * duration_months as i64);
        let end_date = now + duration;
        
        // Cập nhật subscription
        let mut vip_subscription = subscription.clone();
        vip_subscription.subscription_type = SubscriptionType::VIP;
        vip_subscription.start_date = now;
        vip_subscription.end_date = end_date;
        vip_subscription.payment_address = wallet_address;
        vip_subscription.non_nft_status = NonNftVipStatus::StakedDMD;
        vip_subscription.status = SubscriptionStatus::Active;
        
        // Thêm vào cache VIP users
        let vip_user_data = VipUserData {
            user_id: user_id.to_string(),
            created_at: now,
            last_active: now,
            wallet_addresses: vec![wallet_address],
            subscription_end_date: end_date,
            vip_support_code: Uuid::new_v4().to_string(),
            special_privileges: vec!["stake_dmd".to_string()],
            nft_info: None,
        };
        
        // Thêm vào cache VIP users
        update_vip_user_cache(&user_id, vip_user_data).await;
        
        info!("Nâng cấp thành công lên VIP cho user {} bằng stake token, hết hạn {}", 
              user_id, end_date.format("%Y-%m-%d"));
        
        Ok(vip_subscription)
    }
    
    /// Kiểm tra xem VIP subscription có hợp lệ hay không
    #[flow_from("wallet::api")]
    pub fn is_valid_vip(&self, subscription: &UserSubscription) -> bool {
        // Kiểm tra cơ bản
        if subscription.subscription_type != SubscriptionType::VIP {
            return false;
        }
        
        if subscription.status != SubscriptionStatus::Active {
            return false;
        }
        
        // Kiểm tra thời hạn
        if subscription.end_date < Utc::now() {
            return false;
        }
        
        // Kiểm tra NFT hoặc staked DMD
        if subscription.non_nft_status == NonNftVipStatus::StakedDMD {
            // Nếu đã stake DMD, không cần kiểm tra NFT
            return true;
        }
        
        // Kiểm tra NFT
        if let Some(nft_info) = &subscription.nft_info {
            return nft_info.is_valid();
        }
        
        false
    }

    /// Tải thông tin người dùng VIP từ database
    /// 
    /// # Arguments
    /// - `user_id`: ID người dùng cần tải thông tin
    /// 
    /// # Returns
    /// - `Ok(Option<VipUserData>)`: Thông tin người dùng VIP nếu có
    /// - `Err`: Nếu có lỗi
    #[flow_from("database::users")]
    pub async fn load_vip_user(&mut self, user_id: &str) -> Result<Option<VipUserData>, WalletError> {
        // Sử dụng global cache với loader function
        let user_data = get_vip_user(user_id, |id| async move {
            self.load_vip_user_from_db(&id).await.unwrap_or(None)
        }).await;
        
        Ok(user_data)
    }
    
    // Phương thức mới để tách biệt việc tải từ database
    async fn load_vip_user_from_db(&self, user_id: &str) -> Result<Option<VipUserData>, WalletError> {
        // TODO: Tải từ database thực tế
        info!("Tải thông tin người dùng VIP {} từ database", user_id);
        
        // Giả lập không tìm thấy
        Ok(None)
    }
}

// Hằng số giới hạn của VIP user - hầu như không giới hạn
pub const MAX_WALLETS_VIP_USER: usize = 50;
pub const MAX_TRANSACTIONS_PER_DAY: usize = 1000;
pub const MAX_SNIPEBOT_ATTEMPTS_PER_DAY: usize = 100;
pub const MAX_SNIPEBOT_ATTEMPTS_PER_WALLET: usize = 20;
pub const VIP_PRIORITY_LEVEL: u8 = 10; // Mức độ ưu tiên cao nhất

// Helper function không đề cập trong ví dụ trước đây
fn generate_vip_support_code() -> String {
    // Tạo mã hỗ trợ ngẫu nhiên cho VIP user
    format!("VIP-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("00000000"))
}
