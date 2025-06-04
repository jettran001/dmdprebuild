//! Module xử lý người dùng VIP.
//! 
//! Module này cung cấp các chức năng quản lý người dùng VIP, bao gồm:
//! - Kiểm tra và xác thực NFT của người dùng VIP
//! - Quản lý trạng thái VIP khi người dùng không còn NFT
//! - Xử lý các trường hợp stake DMD token thay thế NFT
//! - Quản lý chu kỳ đăng ký VIP và gia hạn tự động
//! - Cung cấp thông tin về đặc quyền VIP
//! 
//! Module này làm việc chặt chẽ với các module khác như user_subscription, nft, 
//! và staking để đảm bảo người dùng VIP nhận được đầy đủ quyền lợi.

// Standard library imports
use std::collections::HashMap;
use std::sync::Arc;

// External imports
use chrono::{DateTime, Duration, Utc};
use ethers::types::{Address, U256};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug};
use uuid::Uuid;

// Internal imports
use crate::error::WalletError;
use crate::walletmanager::api::WalletManagerApi;

use super::constants::VIP_NFT_CHECK_HOUR;
use super::nft::{NftInfo, NonNftVipStatus};
use super::types::{SubscriptionStatus, SubscriptionType, PaymentToken};
use super::user_subscription::UserSubscription;

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

/// Quản lý người dùng VIP và NFT
pub struct VipManager {
    // API wallet để xác thực NFT
    wallet_api: Arc<RwLock<WalletManagerApi>>,
}

impl VipManager {
    /// Khởi tạo VipManager mới
    pub fn new(wallet_api: Arc<RwLock<WalletManagerApi>>) -> Self {
        Self {
            wallet_api,
        }
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
        
        // Tạo NFT info
        let nft_id = Uuid::new_v4().to_string();
        let nft_info = NftInfo {
            contract_address: nft_contract_address,
            token_id: nft_token_id,
            last_verified: Utc::now(),
            is_valid: true,
        };
        
        // Nếu người dùng có NFT nhưng chưa đăng ký VIP, tạo đăng ký mới
        let mut vip_subscription = subscription.clone();
        vip_subscription.subscription_type = SubscriptionType::VIP;
        vip_subscription.nft_info = Some(nft_info.clone());
        vip_subscription.status = SubscriptionStatus::Pending; // Cần thanh toán phí nâng cấp
        vip_subscription.payment_token = payment_token;
        
        Ok(vip_subscription)
    }

    /// Kiểm tra xem người dùng có đăng ký VIP hợp lệ không
    ///
    /// # Arguments
    /// - `subscription`: Thông tin đăng ký cần kiểm tra
    ///
    /// # Returns
    /// - `true` nếu là VIP hợp lệ
    /// - `false` nếu không phải VIP hoặc không hợp lệ
    pub fn is_valid_vip(&self, subscription: &UserSubscription) -> bool {
        // Kiểm tra loại đăng ký
        if subscription.subscription_type != SubscriptionType::VIP {
            return false;
        }
        
        // Kiểm tra trạng thái
        if subscription.status != SubscriptionStatus::Active {
            return false;
        }
        
        // Kiểm tra thời hạn
        if Utc::now() > subscription.end_date {
            return false;
        }
        
        // Kiểm tra NFT
        if let Some(nft_info) = &subscription.nft_info {
            if !nft_info.is_valid {
                return false;
            }
        } else {
            return false;
        }
        
        true
    }
}

/// Dữ liệu VIP user
#[derive(Debug, Clone)]
pub struct VipUserData {
    /// ID người dùng
    pub user_id: String,
    /// Thời gian tạo
    pub created_at: DateTime<Utc>,
    /// Thời gian hoạt động gần nhất
    pub last_active: DateTime<Utc>,
    /// Danh sách địa chỉ ví
    pub wallet_addresses: Vec<Address>,
    /// Thời điểm hết hạn đăng ký
    pub subscription_end_date: DateTime<Utc>,
    /// Thông tin NFT
    pub nft_info: NftInfo,
} 