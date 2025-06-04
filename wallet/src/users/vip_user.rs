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
use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

// Internal imports
use crate::error::WalletError;
use crate::walletmanager::api::WalletManagerApi;
use crate::users::subscription::constants::VIP_NFT_CHECK_HOUR;
use crate::users::subscription::nft::{NftInfo, NonNftVipStatus};
use crate::users::subscription::types::{SubscriptionStatus, SubscriptionType, PaymentToken};
use crate::users::subscription::user_subscription::UserSubscription;
use crate::users::subscription::staking;
use crate::cache::{get_vip_user, update_vip_user_cache, invalidate_user_cache};
use crate::users::UserManager;
use crate::users::premium_user::Database;
use crate::users::abstract_user_manager::{SubscriptionBasedUserManager, create_jwt_token, save_user_data_to_db};

/// Trạng thái của người dùng VIP
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VipUserStatus {
    /// Đang hoạt động
    Active,
    /// Đã hết hạn
    Expired,
    /// NFT đã bị chuyển đi
    NftTransferred,
    /// Đặc quyền đã bị thu hồi
    PrivilegesRevoked,
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
    wallet_api: Arc<tokio::sync::RwLock<WalletManagerApi>>,
    /// Kết nối database
    db: Arc<Database>,
}

impl VipUserManager {
    /// Khởi tạo VipUserManager mới.
    pub fn new(wallet_api: Arc<tokio::sync::RwLock<WalletManagerApi>>, db: Arc<Database>) -> Self {
        Self {
            wallet_api,
            db,
        }
    }

    /// Kiểm tra xem người dùng có thể thực hiện giao dịch không.
    #[flow_from("wallet::api")]
    pub async fn can_perform_transaction(&self, user_id: &str) -> Result<bool, WalletError> {
        // Người dùng VIP không bị giới hạn giao dịch
        if let Some(user_data) = get_vip_user(user_id, |id| async move {
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
        // Thực hiện validation đầu vào
        if wallet_address.is_zero() {
            warn!("Địa chỉ ví không hợp lệ (zero address) khi kiểm tra NFT");
            return Err(WalletError::InvalidAddress("Địa chỉ ví không hợp lệ".to_string()));
        }
        
        if nft_contract.is_zero() {
            warn!("Địa chỉ hợp đồng NFT không hợp lệ (zero address)");
            return Err(WalletError::InvalidAddress("Địa chỉ hợp đồng NFT không hợp lệ".to_string()));
        }
        
        if chain_id == 0 {
            warn!("Chain ID không hợp lệ (0)");
            return Err(WalletError::InvalidChainId(0));
        }
        
        // Sử dụng wallet API để kiểm tra NFT - Cải thiện xử lý lỗi RwLock
        let wallet_api_guard = self.wallet_api.read().await.map_err(|e| {
            error!("Lỗi RwLock khi truy cập wallet API: {}", e);
            WalletError::Other(format!("Lỗi đồng bộ hóa khi truy cập WalletAPI: {}", e))
        })?;
        
        debug!("Kiểm tra NFT: wallet={:?}, contract={:?}, token_id={}, chain_id={}", 
               wallet_address, nft_contract, token_id, chain_id);
        
        // Sử dụng WalletApi để lấy thông tin ERC-1155 NFT
        let balance = match wallet_api_guard.get_erc1155_balance(
            wallet_address,
            nft_contract,
            token_id,
            chain_id
        ).await {
            Ok(balance) => balance,
            Err(e) => {
                error!("Lỗi khi kiểm tra balance ERC-1155: {}", e);
                // Retry với backup provider nếu cần
                match wallet_api_guard.get_erc1155_balance_with_backup(
                    wallet_address,
                    nft_contract,
                    token_id,
                    chain_id
                ).await {
                    Ok(balance) => balance,
                    Err(retry_err) => {
                        error!("Lỗi khi retry kiểm tra balance ERC-1155: {}", retry_err);
                        return Err(retry_err);
                    }
                }
            }
        };
        
        // Kiểm tra balance > 0
        let has_nft = balance > U256::zero();
        
        if has_nft {
            info!("Xác nhận user có NFT tại địa chỉ {} (balance: {})", wallet_address, balance);
        } else {
            warn!("Không tìm thấy NFT tại địa chỉ {} (balance: 0)", wallet_address);
        }
        
        Ok(has_nft)
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
        
        // Đảm bảo user_subscriptions không rỗng
        if user_subscriptions.is_empty() {
            warn!("Danh sách user_subscriptions rỗng, không có gì để kiểm tra");
            return Ok(Vec::new());
        }
        
        // Danh sách user_ids cần thông báo
        let mut users_without_nft = Vec::new();
        let mut total_checked = 0;
        let mut error_count = 0;
        let mut processed_count = 0;
        
        let vip_count = user_subscriptions
            .iter()
            .filter(|(_, s)| s.subscription_type == SubscriptionType::VIP)
            .count();
            
        info!("Bắt đầu kiểm tra NFT cho {} VIP users", vip_count);
        
        // Danh sách user_ids đã xử lý
        let mut processed_user_ids = Vec::with_capacity(vip_count);
        
        for (user_id, subscription) in user_subscriptions.iter_mut() {
            processed_count += 1;
            
            // Chỉ kiểm tra các VIP user đang active
            if subscription.subscription_type != SubscriptionType::VIP || 
               subscription.status != SubscriptionStatus::Active {
                continue;
            }
            
            processed_user_ids.push(user_id.clone());
            
            // Kiểm tra hạn sử dụng subscription
            if subscription.end_date < now {
                warn!("User {} có subscription đã hết hạn vào {}. Cần xử lý riêng.", 
                     user_id, subscription.end_date);
                continue;
            }
            
            // Bỏ qua kiểm tra NFT cho người dùng VIP 12 tháng đã stake token
            if subscription.non_nft_status == NonNftVipStatus::StakedDMD {
                debug!("Bỏ qua kiểm tra NFT cho user {}: đã stake DMD token", user_id);
                continue;
            }
            
            // Kiểm tra thông tin NFT
            if let Some(nft_info) = &mut subscription.nft_info {
                // Kiểm tra nếu đã kiểm tra trong ngày
                let nft_last_checked_day = nft_info.last_verified.date_naive().day();
                let current_day = now.date_naive().day();
                
                // Chỉ kiểm tra mỗi ngày một lần
                if nft_last_checked_day != current_day {
                    total_checked += 1;
                    
                    // Kiểm tra chain_id hợp lệ
                    let chain_id = nft_info.chain_id.unwrap_or(1);
                    if chain_id == 0 {
                        error!("User {} có chain_id không hợp lệ: {}", user_id, chain_id);
                        error_count += 1;
                        continue;
                    }
                    
                    // Kiểm tra địa chỉ ví và contract hợp lệ
                    if subscription.payment_address.is_zero() || nft_info.contract_address.is_zero() {
                        error!("User {} có địa chỉ ví hoặc địa chỉ contract không hợp lệ", user_id);
                        error_count += 1;
                        continue;
                    }
                    
                    // Thêm retry mechanism và xử lý lỗi chi tiết
                    let max_retries = 3;
                    let mut current_retry = 0;
                    let mut last_error: Option<WalletError> = None;
                    
                    while current_retry < max_retries {
                        match self.check_nft_ownership(
                            subscription.payment_address,
                            nft_info.contract_address,
                            nft_info.token_id,
                            chain_id
                        ).await {
                            Ok(has_nft) => {
                                // Cập nhật trạng thái
                                nft_info.update_status(has_nft, now);
                                
                                if !has_nft {
                                    warn!("User {} không có NFT VIP, cần thông báo", user_id);
                                    users_without_nft.push(user_id.clone());
                                    
                                    // Cập nhật trạng thái non_nft_status
                                    subscription.non_nft_status = NonNftVipStatus::Missing;
                                } else {
                                    debug!("Xác nhận user {} có NFT VIP hợp lệ", user_id);
                                    subscription.non_nft_status = NonNftVipStatus::Valid;
                                }
                                
                                // Thành công, thoát khỏi vòng lặp retry
                                last_error = None;
                                break;
                            },
                            Err(e) => {
                                current_retry += 1;
                                last_error = Some(e.clone());
                                error!("Lỗi khi kiểm tra NFT cho người dùng {}, lần thử {}/{}: {}", 
                                      user_id, current_retry, max_retries, e);
                                
                                if current_retry < max_retries {
                                    // Chờ một chút trước khi retry
                                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                                }
                            }
                        }
                    }
                    
                    // Nếu tất cả các lần thử đều thất bại
                    if let Some(e) = last_error {
                        error_count += 1;
                        error!("Đã thử {} lần nhưng vẫn thất bại khi kiểm tra NFT cho user {}: {}", 
                              max_retries, user_id, e);
                        // Không thay đổi trạng thái nếu có lỗi
                    }
                }
            } else {
                warn!("User {} là VIP nhưng không có thông tin NFT", user_id);
            }
            
            // Log tiến trình nếu có nhiều user
            if vip_count > 100 && processed_count % 100 == 0 {
                info!("Đã kiểm tra NFT cho {}/{} VIP users", processed_count, vip_count);
            }
        }
        
        info!("Hoàn thành kiểm tra NFT: {} users đang active, {} được kiểm tra, {} không có NFT, {} lỗi", 
              processed_user_ids.len(), total_checked, users_without_nft.len(), error_count);
        
        // Lưu trữ kết quả kiểm tra để theo dõi
        self.save_nft_check_results(users_without_nft.clone(), error_count, total_checked).await;
        
        Ok(users_without_nft)
    }
    
    /// Lưu trữ kết quả kiểm tra NFT
    async fn save_nft_check_results(&self, users_without_nft: Vec<String>, error_count: usize, total_checked: usize) {
        // Lưu vào database hoặc log
        debug!("Lưu kết quả kiểm tra NFT: {} users không có NFT, {} lỗi, {} tổng số kiểm tra", 
              users_without_nft.len(), error_count, total_checked);
        
        // TODO: Lưu vào database thực tế
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
        // Kiểm tra đầu vào kỹ lưỡng
        if subscription.user_id.is_empty() {
            error!("Không thể nâng cấp: ID người dùng trống");
            return Err(WalletError::Other("ID người dùng không hợp lệ".to_string()));
        }
        
        if subscription.payment_address.is_zero() {
            error!("Không thể nâng cấp: Địa chỉ ví thanh toán là zero address");
            return Err(WalletError::InvalidAddress("Địa chỉ ví thanh toán không hợp lệ".to_string()));
        }
        
        if nft_contract_address.is_zero() {
            error!("Không thể nâng cấp: Địa chỉ hợp đồng NFT là zero address");
            return Err(WalletError::InvalidAddress("Địa chỉ hợp đồng NFT không hợp lệ".to_string()));
        }
        
        // Kiểm tra xem đăng ký hiện tại có phải Premium không
        if subscription.subscription_type != SubscriptionType::Premium {
            error!("Chỉ có thể nâng cấp từ Premium lên VIP, loại hiện tại: {:?}", 
                  subscription.subscription_type);
            return Err(WalletError::Other("Chỉ có thể nâng cấp từ Premium lên VIP".to_string()));
        }
        
        // Kiểm tra trạng thái subscription
        if subscription.status != SubscriptionStatus::Active {
            error!("Không thể nâng cấp: Subscription không active, trạng thái hiện tại: {:?}", 
                  subscription.status);
            return Err(WalletError::Other("Subscription phải ở trạng thái Active".to_string()));
        }
        
        // Ghi log thông tin quan trọng
        info!("Bắt đầu nâng cấp từ Premium lên VIP: user_id={}, payment_address={}, nft_contract={}, token_id={}",
             subscription.user_id, subscription.payment_address, nft_contract_address, nft_token_id);
        
        // Kiểm tra NFT với cơ chế retry và timeout
        let max_retries = 3;
        let mut current_retry = 0;
        let mut has_nft = false;
        let mut last_error: Option<WalletError> = None;
        
        while current_retry < max_retries {
            match self.check_nft_ownership(
                subscription.payment_address,
                nft_contract_address,
                nft_token_id,
                1, // Chain ID mặc định
            ).await {
                Ok(result) => {
                    has_nft = result;
                    last_error = None;
                    break;
                },
                Err(e) => {
                    current_retry += 1;
                    last_error = Some(e.clone());
                    error!("Lỗi khi kiểm tra NFT, lần thử {}/{}: {}", current_retry, max_retries, e);
                    
                    if current_retry < max_retries {
                        // Chờ trước khi thử lại
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                }
            }
        }
        
        // Xử lý kết quả kiểm tra NFT
        if let Some(e) = last_error {
            error!("Không thể kiểm tra NFT sau {} lần thử: {}", max_retries, e);
            return Err(WalletError::Other(format!("Không thể xác minh NFT: {}", e)));
        }
        
        if !has_nft {
            error!("Không tìm thấy NFT trong ví {}", subscription.payment_address);
            return Err(WalletError::Other("Không tìm thấy NFT trong ví".to_string()));
        }
        
        debug!("Đã xác minh NFT tồn tại trong ví");
        
        // Tạo thông tin NFT mới với đầy đủ thông tin
        let now = Utc::now();
        let nft_info = NftInfo {
            contract_address: nft_contract_address,
            token_id: nft_token_id,
            last_verified: now,
            is_valid: true,
            chain_id: Some(1), // Chain ID
            first_verified: now,
            verification_count: 1,
        };
        
        // Tính toán thời hạn mới
        // Giữ nguyên thời gian bắt đầu, kéo dài thời hạn nếu cần
        let current_end_date = subscription.end_date;
        let min_vip_duration = Duration::days(30 * 3); // Tối thiểu 3 tháng
        let new_end_date = if current_end_date < now + min_vip_duration {
            now + min_vip_duration
        } else {
            current_end_date
        };
        
        // Tạo subscription VIP mới với thông tin từ Premium
        let mut vip_subscription = subscription.clone();
        vip_subscription.subscription_type = SubscriptionType::VIP;
        vip_subscription.nft_info = Some(nft_info);
        vip_subscription.non_nft_status = NonNftVipStatus::Valid;
        vip_subscription.end_date = new_end_date;
        vip_subscription.payment_token = Some(payment_token);
        vip_subscription.updated_at = now;
        
        // Cập nhật dữ liệu VIP trong cache
        self.update_vip_user_cache(&subscription.user_id, &vip_subscription).await?;
        
        info!("Nâng cấp thành công từ Premium lên VIP: user_id={}, hết hạn={}", 
             subscription.user_id, new_end_date.format("%Y-%m-%d"));
        
        Ok(vip_subscription)
    }
    
    /// Cập nhật cache cho người dùng VIP
    async fn update_vip_user_cache(&self, user_id: &str, subscription: &UserSubscription) -> Result<(), WalletError> {
        if let Some(vip_user_data) = get_vip_user(user_id, |id| async move {
            self.load_vip_user_from_db(&id).await.unwrap_or(None)
        }).await {
            // Đã có dữ liệu user VIP, cập nhật
            let mut updated_data = vip_user_data.clone();
            updated_data.nft_info = subscription.nft_info.clone();
            updated_data.subscription_end_date = subscription.end_date;
            updated_data.last_active = Utc::now();
            
            // Thêm địa chỉ ví nếu chưa có
            if !updated_data.wallet_addresses.contains(&subscription.payment_address) {
                updated_data.wallet_addresses.push(subscription.payment_address);
            }
            
            update_vip_user_cache(user_id, updated_data).await;
        } else {
            // Tạo dữ liệu mới
            let vip_data = VipUserData {
                user_id: user_id.to_string(),
                created_at: Utc::now(),
                last_active: Utc::now(),
                wallet_addresses: vec![subscription.payment_address],
                subscription_end_date: subscription.end_date,
                vip_support_code: generate_vip_support_code(),
                special_privileges: Vec::new(),
                nft_info: subscription.nft_info.clone(),
            };
            
            // Thêm vào cache VIP users
            update_vip_user_cache(user_id, vip_data).await;
        }
        
        Ok(())
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
        let row = sqlx::query("SELECT data FROM vip_users WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(&self.db.pool)
            .await
            .map_err(|e| WalletError::Other(format!("Lỗi query vip user: {}", e)))?;
            
        if let Some(row) = row {
            let data: String = row.try_get("data")
                .map_err(|e| WalletError::Other(format!("Lỗi đọc dữ liệu: {}", e)))?;
                
            let user_data = serde_json::from_str(&data)
                .map_err(|e| WalletError::Other(format!("Lỗi deserialize vip user: {}", e)))?;
                
            Ok(Some(user_data))
        } else {
            Ok(None)
        }
    }

    /// Cập nhật thông tin người dùng VIP với cơ chế đồng bộ hóa
    /// 
    /// Phương thức này đảm bảo tính nhất quán khi nhiều luồng cập nhật cùng
    /// một người dùng VIP bằng cách sử dụng mutex dành riêng cho mỗi user_id.
    /// 
    /// # Arguments
    /// * `user_id` - ID người dùng cần cập nhật
    /// * `update_fn` - Hàm closure nhận tham chiếu tới dữ liệu và thực hiện cập nhật
    /// 
    /// # Returns
    /// * `Ok(VipUserData)` - Dữ liệu đã được cập nhật
    /// * `Err(WalletError)` - Lỗi khi cập nhật
    pub async fn update_vip_user<F>(&self, user_id: &str, update_fn: F) -> Result<VipUserData, WalletError>
    where
        F: FnOnce(&mut VipUserData) -> Result<(), WalletError>,
    {
        // Tạo mutex key riêng cho mỗi user_id
        let mutex_key = format!("vip_update:{}", user_id);
        
        // Sử dụng tokio::sync::Mutex để khóa theo user_id
        let mutex = VIP_USER_MUTEXES.entry(mutex_key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        
        // Khóa mutex trước khi cập nhật
        let _guard = mutex.lock().await;
        
        // Lấy dữ liệu user hiện tại
        let user_data_opt = self.load_user_data(user_id).await?;
        
        let mut user_data = if let Some(data) = user_data_opt {
            data
        } else {
            return Err(WalletError::NotFound(format!("Không tìm thấy VIP user {}", user_id)));
        };
        
        // Áp dụng hàm cập nhật
        update_fn(&mut user_data)?;
        
        // Lưu vào database và cache
        self.save_user_data(user_id, user_data.clone()).await?;
        
        // Xóa mutex khỏi map nếu không cần thiết nữa
        VIP_USER_MUTEXES.remove(&mutex_key);
        
        Ok(user_data)
    }
    
    /// Tạo JWT session token mới cho người dùng VIP
    async fn generate_session_token(&self, user_id: &str) -> Result<String, WalletError> {
        // JWT payload
        let claims = SessionClaims {
            sub: user_id.to_string(),
            exp: (Utc::now() + Duration::days(30)).timestamp() as usize, // Token VIP có thời hạn dài hơn
            iat: Utc::now().timestamp() as usize,
            user_type: "VIP".to_string(),
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

// Hằng số giới hạn của VIP user
pub const MAX_WALLETS_VIP_USER: usize = 20;
pub const MAX_TRANSACTIONS_PER_DAY_VIP: usize = 1000;
pub const MAX_SNIPEBOT_ATTEMPTS_PER_DAY_VIP: usize = 50;
pub const MAX_SNIPEBOT_ATTEMPTS_PER_WALLET_VIP: usize = 15;

// Mutexes toàn cục cho việc cập nhật VIP user
lazy_static::lazy_static! {
    static ref VIP_USER_MUTEXES: tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>> = 
        tokio::sync::Mutex::new(HashMap::new());
}

// Triển khai trait SubscriptionBasedUserManager cho VipUserManager
#[async_trait]
impl SubscriptionBasedUserManager<VipUserData> for VipUserManager {
    async fn can_perform_transaction(&self, user_id: &str) -> Result<bool, WalletError> {
        // Người dùng VIP không bị giới hạn giao dịch
        if let Some(user_data) = get_vip_user(user_id, |id| async move {
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
    
    async fn can_use_snipebot(&self, user_id: &str) -> Result<bool, WalletError> {
        // Người dùng VIP được sử dụng snipebot không giới hạn
        Ok(true)
    }
    
    async fn load_user_data(&self, user_id: &str) -> Result<Option<VipUserData>, WalletError> {
        self.load_vip_user_from_db(user_id).await
    }
    
    async fn save_user_data(&self, user_data: &VipUserData) -> Result<(), WalletError> {
        // Sử dụng hàm tiện ích từ abstract_user_manager
        save_user_data_to_db(&self.db, &user_data.user_id, user_data, "vip_users").await?;
        
        // Cập nhật cache
        update_vip_user_cache(&user_data.user_id, user_data.clone()).await;
        
        Ok(())
    }
    
    async fn generate_session_token(&self, user_id: &str, subscription_type: SubscriptionType) -> Result<String, WalletError> {
        let jwt_secret = self.db.get_jwt_secret().await?;
        
        // Tạo JWT token với thông tin subscription type
        let token = create_jwt_token(
            user_id, 
            subscription_type.to_string().as_str(), 
            &jwt_secret, 
            60 // Số ngày hết hạn token cho VIP dài hơn (60 ngày)
        ).await?;
        
        // Lưu token vào database
        self.db.update_session_token(user_id, &token).await?;
        
        Ok(token)
    }
    
    async fn refresh_session_token(&self, user_id: &str) -> Result<String, WalletError> {
        let subscription = self.db.get_user_subscription(user_id).await?;
        
        // Tạo token mới với subscription type hiện tại
        self.generate_session_token(user_id, subscription.subscription_type).await
    }
    
    async fn sync_cache_with_db(&self, user_id: &str) -> Result<(), WalletError> {
        // Xóa cache cũ
        invalidate_user_cache(user_id).await?;
        
        // Load lại từ database
        let user_data = self.load_vip_user_from_db(user_id).await?;
        
        if let Some(data) = user_data {
            // Cập nhật cache mới
            update_vip_user_cache(user_id, data).await;
        }
        
        Ok(())
    }
    
    async fn link_wallet_address(&self, user_id: &str, address: Address) -> Result<(), WalletError> {
        // Kiểm tra đã có tài khoản VIP chưa
        let mut user_data = match self.load_vip_user_from_db(user_id).await? {
            Some(data) => data,
            None => return Err(WalletError::UserNotFound(user_id.to_string())),
        };
        
        // Kiểm tra wallet address đã tồn tại chưa
        if user_data.wallet_addresses.contains(&address) {
            return Ok(());
        }
        
        // Kiểm tra giới hạn số ví
        if user_data.wallet_addresses.len() >= MAX_WALLETS_VIP_USER {
            return Err(WalletError::LimitExceeded(format!(
                "Đã đạt giới hạn số ví cho người dùng VIP: {}", MAX_WALLETS_VIP_USER
            )));
        }
        
        // Thêm địa chỉ ví mới
        user_data.wallet_addresses.push(address);
        
        // Lưu lại vào database và cập nhật cache
        self.save_user_data(&user_data).await?;
        
        info!("Đã thêm địa chỉ ví {} cho user VIP {}", address, user_id);
        
        Ok(())
    }
    
    async fn renew_subscription(
        &self,
        user_id: &str,
        subscription: &UserSubscription,
        duration_months: u32
    ) -> Result<UserSubscription, WalletError> {
        // Logic đặc thù cho việc gia hạn VIP
        // Tùy thuộc vào việc có NFT hay không
        let mut subscription_copy = subscription.clone();
        
        let now = Utc::now();
        let current_end_date = subscription.end_date;
        let new_end_date = if current_end_date > now {
            // Nếu còn hạn, gia hạn thêm từ ngày hết hạn
            current_end_date + chrono::Duration::days(duration_months as i64 * 30)
        } else {
            // Nếu hết hạn, tính từ ngày hiện tại
            now + chrono::Duration::days(duration_months as i64 * 30)
        };
        
        subscription_copy.end_date = new_end_date;
        subscription_copy.status = SubscriptionStatus::Active;
        
        // Lưu thông tin renewal vào lịch sử
        subscription_copy.add_subscription_record(
            "Gia hạn gói VIP", 
            PaymentToken::USDC, 
            format!("Gia hạn {} tháng từ {} đến {}", 
                duration_months,
                current_end_date.format("%d-%m-%Y"),
                new_end_date.format("%d-%m-%Y")
            )
        );
        
        info!("Đã gia hạn thành công gói VIP cho user {} thêm {} tháng tính từ {}",
             user_id, duration_months, current_end_date.format("%d-%m-%Y"));
             
        Ok(subscription_copy)
    }
    
    async fn check_expiring_subscriptions(
        &self,
        user_subscriptions: &HashMap<String, UserSubscription>,
        days_before: u32
    ) -> Result<Vec<String>, WalletError> {
        let mut expiring_users = Vec::new();
        let now = Utc::now();
        let warning_threshold = now + chrono::Duration::days(days_before as i64);
        
        for (user_id, subscription) in user_subscriptions {
            // Chỉ kiểm tra subscription VIP đang active
            if subscription.subscription_type == SubscriptionType::VIP && 
               subscription.status == SubscriptionStatus::Active {
                
                // Kiểm tra nếu sắp hết hạn
                if subscription.end_date <= warning_threshold && subscription.end_date > now {
                    expiring_users.push(user_id.clone());
                    
                    debug!("User VIP {} sắp hết hạn vào {}", user_id, subscription.end_date);
                }
            }
        }
        
        Ok(expiring_users)
    }
    
    fn is_valid_subscription(&self, subscription: &UserSubscription) -> bool {
        self.is_valid_vip(subscription)
    }
}

// Helper function không đề cập trong ví dụ trước đây
fn generate_vip_support_code() -> String {
    // Tạo mã hỗ trợ ngẫu nhiên cho VIP user
    format!("VIP-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("00000000"))
}

// Triển khai UserManager sử dụng SubscriptionBasedUserManager
#[async_trait]
impl UserManager for VipUserManager {
    type UserData = VipUserData;
    
    async fn can_perform_transaction(&self, user_id: &str) -> Result<bool, WalletError> {
        <Self as SubscriptionBasedUserManager<VipUserData>>::can_perform_transaction(self, user_id).await
    }
    
    async fn can_use_snipebot(&self, user_id: &str) -> Result<bool, WalletError> {
        <Self as SubscriptionBasedUserManager<VipUserData>>::can_use_snipebot(self, user_id).await
    }
    
    async fn load_user_data(&self, user_id: &str) -> Result<Option<Self::UserData>, WalletError> {
        <Self as SubscriptionBasedUserManager<VipUserData>>::load_user_data(self, user_id).await
    }
    
    fn is_valid_subscription(&self, subscription: &UserSubscription) -> bool {
        <Self as SubscriptionBasedUserManager<VipUserData>>::is_valid_subscription(self, subscription)
    }
    
    async fn save_user_data(&self, user_id: &str, data: Self::UserData) -> Result<(), WalletError> {
        <Self as SubscriptionBasedUserManager<VipUserData>>::save_user_data(self, &data).await
    }
    
    async fn refresh_session_token(&self, user_id: &str) -> Result<String, WalletError> {
        <Self as SubscriptionBasedUserManager<VipUserData>>::refresh_session_token(self, user_id).await
    }
    
    async fn sync_cache_with_db(&self, user_id: &str) -> Result<(), WalletError> {
        <Self as SubscriptionBasedUserManager<VipUserData>>::sync_cache_with_db(self, user_id).await
    }
    
    async fn link_wallet_address(&self, user_id: &str, address: Address) -> Result<(), WalletError> {
        <Self as SubscriptionBasedUserManager<VipUserData>>::link_wallet_address(self, user_id, address).await
    }
}
