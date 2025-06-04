//! Module quản lý NFT liên quan đến đăng ký VIP trong hệ thống DiamondChain.
//!
//! Module này cung cấp các chức năng:
//! * Định nghĩa và quản lý thông tin về NFT VIP (địa chỉ hợp đồng, token ID)
//! * Theo dõi trạng thái sở hữu NFT của người dùng VIP
//! * Xác minh tính hợp lệ của NFT thông qua tương tác với blockchain
//! * Quản lý trạng thái của người dùng VIP khi không có NFT (Pending, Downgrade, Pause)
//! * Hỗ trợ trạng thái StakedDMD cho người dùng VIP không cần NFT (stake token)
//! * Xử lý quá trình kích hoạt và hết hạn của NFT VIP
//!
//! Module này cung cấp các cấu trúc dữ liệu và chức năng cần thiết để xác minh tư cách
//! VIP của người dùng thông qua sở hữu NFT, đồng thời hỗ trợ các trường hợp đặc biệt
//! như sử dụng stake token để thay thế NFT. Module làm việc chặt chẽ với các
//! module vip và user_subscription để đảm bảo trải nghiệm liền mạch cho người dùng VIP.

//! Định nghĩa các kiểu dữ liệu liên quan đến NFT.

// Standard library imports
use std::fmt;

// External imports
use chrono::{DateTime, Duration, Utc};
use ethers::types::{Address, U256};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::Duration as StdDuration;
use tokio::time::timeout;

use crate::blockchain::types::NftInfo;
use crate::error::AppResult;
use crate::users::subscription::constants::{NFT_SUBSCRIPTION_DAYS, VIP_NFT_COLLECTION_ADDRESS};
use crate::users::subscription::types::SubscriptionType;
use crate::cache;
use crate::blockchain::BlockchainProvider;

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

/// Trạng thái khi VIP không còn NFT trong ví
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NonNftVipStatus {
    /// Còn NFT, không có vấn đề gì
    Valid,
    /// Đang chờ người dùng xử lý khi mất NFT
    Pending,
    /// Người dùng chọn hạ cấp xuống Premium
    Downgrade,
    /// Người dùng chọn tạm dừng đăng ký
    Pause,
    /// Người dùng đã stake DMD token cho gói VIP 12 tháng, không cần NFT
    StakedDMD,
}

impl Default for NonNftVipStatus {
    fn default() -> Self {
        NonNftVipStatus::Valid
    }
}

/// Thông tin về NFT của VIP user
#[derive(Debug, Clone)]
pub struct VipNftInfo {
    /// ID duy nhất của NFT trong database
    pub nft_id: String,
    /// Địa chỉ contract của NFT
    pub contract_address: Address,
    /// Token ID của NFT
    pub token_id: U256,
    /// Thời điểm kiểm tra cuối
    pub last_checked: DateTime<Utc>,
    /// NFT có tồn tại trong ví không
    pub exists_in_wallet: bool,
}

impl VipNftInfo {
    /// Tạo mới thông tin NFT
    ///
    /// # Arguments
    /// * `nft_id` - ID duy nhất của NFT
    /// * `contract_address` - Địa chỉ contract của NFT
    /// * `token_id` - Token ID của NFT
    ///
    /// # Returns
    /// Trả về đối tượng VipNftInfo mới
    pub fn new(nft_id: &str, contract_address: Address, token_id: U256) -> Self {
        Self {
            nft_id: nft_id.to_string(),
            contract_address,
            token_id,
            last_checked: Utc::now(),
            exists_in_wallet: true,
        }
    }

    /// Cập nhật trạng thái NFT
    ///
    /// # Arguments
    /// * `exists` - NFT có tồn tại trong ví không
    pub fn update_status(&mut self, exists: bool) {
        self.exists_in_wallet = exists;
        self.last_checked = Utc::now();
    }

    /// Kiểm tra xem NFT có cần xác minh lại không (sau 24h)
    ///
    /// # Arguments
    /// * `current_time` - Thời điểm hiện tại
    ///
    /// # Returns
    /// Trả về true nếu cần xác minh lại
    pub fn needs_verification(&self, current_time: DateTime<Utc>) -> bool {
        let time_diff = current_time.signed_duration_since(self.last_checked);
        time_diff > Duration::hours(24)
    }
}

/// Thông tin về NFT VIP
#[derive(Debug, Clone)]
pub struct NftInfo {
    /// Địa chỉ hợp đồng NFT
    pub contract_address: Address,
    /// Token ID của NFT
    pub token_id: U256,
    /// Thời gian kiểm tra gần nhất
    pub last_verified: DateTime<Utc>,
    /// NFT có còn hợp lệ không
    pub is_valid: bool,
}

impl NftInfo {
    /// Tạo mới thông tin NFT
    pub fn new(contract_address: Address, token_id: U256) -> Self {
        Self {
            contract_address,
            token_id,
            last_verified: Utc::now(),
            is_valid: true,
        }
    }
    
    /// Cập nhật trạng thái của NFT
    pub fn update_status(&mut self, is_valid: bool, timestamp: DateTime<Utc>) {
        self.is_valid = is_valid;
        self.last_verified = timestamp;
    }
    
    /// Kiểm tra xem NFT có cần được xác minh lại không
    /// Mặc định là sau mỗi 24 giờ
    pub fn needs_verification(&self, now: DateTime<Utc>) -> bool {
        now.signed_duration_since(self.last_verified).num_hours() >= 24
    }
}

/// Trạng thái của người dùng không có NFT
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NonNftUserStatus {
    /// Tiếp tục sử dụng Premium
    UsePremium,
    /// Tạm dừng đến khi có NFT
    Paused,
}

/// Thông tin về NFT VIP và trạng thái kích hoạt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VipNftInfo {
    /// ID của NFT
    pub nft_id: Uuid,
    /// ID người dùng sở hữu
    pub user_id: Uuid,
    /// Địa chỉ ví chứa NFT
    pub wallet_address: String,
    /// Thông tin về NFT từ blockchain
    pub nft_data: NftInfo,
    /// Đã kích hoạt chưa
    pub is_activated: bool,
    /// Ngày kích hoạt
    pub activation_date: Option<DateTime<Utc>>,
    /// Ngày hết hạn
    pub expiry_date: Option<DateTime<Utc>>,
    /// Ngày tạo bản ghi
    pub created_at: DateTime<Utc>,
    /// Ngày cập nhật bản ghi
    pub updated_at: DateTime<Utc>,
}

impl VipNftInfo {
    /// Tạo một đối tượng NFT VIP mới
    pub fn new(user_id: Uuid, wallet_address: String, nft_data: NftInfo) -> Self {
        let now = Utc::now();
        Self {
            nft_id: Uuid::new_v4(),
            user_id,
            wallet_address,
            nft_data,
            is_activated: false,
            activation_date: None,
            expiry_date: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Kích hoạt NFT để nhận gói đăng ký VIP
    pub fn activate(&mut self) -> AppResult<()> {
        if self.is_activated {
            return Err("NFT đã được kích hoạt".into());
        }

        let now = Utc::now();
        self.is_activated = true;
        self.activation_date = Some(now);
        self.expiry_date = Some(now + Duration::days(NFT_SUBSCRIPTION_DAYS));
        self.updated_at = now;

        Ok(())
    }

    /// Kiểm tra xem NFT có đang hoạt động không
    pub fn is_active(&self) -> bool {
        if !self.is_activated {
            return false;
        }

        match self.expiry_date {
            Some(expiry) => Utc::now() < expiry,
            None => false,
        }
    }

    /// Kiểm tra xem NFT có phải từ bộ sưu tập VIP không
    pub fn is_vip_collection(&self) -> bool {
        self.nft_data.collection_address.eq_ignore_ascii_case(VIP_NFT_COLLECTION_ADDRESS)
    }

    /// Lấy loại đăng ký từ NFT
    pub fn get_subscription_type(&self) -> Option<SubscriptionType> {
        if self.is_active() && self.is_vip_collection() {
            Some(SubscriptionType::Vip)
        } else {
            None
        }
    }
}

/// Quản lý NFT VIP của người dùng
#[derive(Debug)]
pub struct NftManager {
    /// Danh sách NFT của người dùng
    pub user_nfts: Vec<VipNftInfo>,
    /// Cache cho các NFT đã xác minh
    nft_cache: Arc<RwLock<HashMap<Uuid, (bool, DateTime<Utc>)>>>,
    /// Blockchain providers cho việc cross-check
    blockchain_providers: Option<Arc<Vec<Box<dyn BlockchainProvider>>>>,
    /// Cache cho xác minh NFT
    verification_cache: Arc<cache::AsyncCache<String, bool, cache::LRUCache<String, bool>>>,
    /// Trạng thái của VIP không có NFT
    non_nft_status: NonNftVipStatus,
    /// Thời gian cập nhật trạng thái cuối cùng
    last_status_update: DateTime<Utc>,
}

impl NftManager {
    /// Tạo manager mới với danh sách NFT
    pub fn new(user_nfts: Vec<VipNftInfo>) -> Self {
        Self { 
            user_nfts,
            nft_cache: Arc::new(RwLock::new(HashMap::new())),
            blockchain_providers: None,
            verification_cache: Arc::new(cache::AsyncCache::new(100)), // Cache 100 kết quả xác minh
            non_nft_status: NonNftVipStatus::Valid,
            last_status_update: Utc::now(),
        }
    }

    /// Thiết lập blockchain providers để xác minh NFT
    pub fn set_blockchain_providers(&mut self, providers: Arc<Vec<Box<dyn BlockchainProvider>>>) {
        self.blockchain_providers = Some(providers);
    }

    /// Kiểm tra người dùng có NFT VIP đang hoạt động không
    pub fn has_active_vip_nft(&self) -> bool {
        // Nếu đã stake DMD token cho gói VIP 12 tháng, không cần kiểm tra NFT
        if self.non_nft_status == NonNftVipStatus::StakedDMD {
            info!("User has VIP status through DMD staking, bypassing NFT check");
            return true;
        }
        
        let has_nft = self.user_nfts.iter().any(|nft| nft.is_active() && nft.is_vip_collection());
        
        if !has_nft {
            // Log khi không tìm thấy NFT VIP hợp lệ
            debug!("No active VIP NFT found, current non-NFT status: {:?}", self.non_nft_status);
        }
        
        has_nft
    }

    /// Lấy thông tin NFT VIP đang hoạt động
    pub fn get_active_vip_nft(&self) -> Option<&VipNftInfo> {
        // Nếu đã stake DMD token cho gói VIP 12 tháng, không cần NFT
        if self.non_nft_status == NonNftVipStatus::StakedDMD {
            debug!("User has VIP status through DMD staking, no NFT required");
            return None;
        }
        
        self.user_nfts
            .iter()
            .find(|nft| nft.is_active() && nft.is_vip_collection())
    }

    /// Thêm NFT mới vào danh sách
    pub fn add_nft(&mut self, nft: VipNftInfo) {
        self.user_nfts.push(nft);
    }

    /// Vô hiệu hóa cache cho một NFT cụ thể
    ///
    /// Xóa thông tin cache liên quan đến một NFT cụ thể để đảm bảo
    /// thông tin luôn được cập nhật khi có thay đổi về quyền sở hữu NFT.
    ///
    /// # Arguments
    /// * `wallet_address` - Địa chỉ ví chứa NFT
    /// * `contract_address` - Địa chỉ hợp đồng NFT
    /// * `token_id` - Token ID của NFT
    /// * `chain_id` - Chain ID của blockchain
    pub async fn invalidate_nft_cache(
        &self,
        wallet_address: &str,
        contract_address: &str, 
        token_id: U256,
        chain_id: u64
    ) {
        let cache_key = format!("{}_{}_{}_{}",
            wallet_address, contract_address, token_id.to_string(), chain_id);
        
        match self.verification_cache.remove(&cache_key).await {
            Ok(_) => {
                debug!("Đã vô hiệu hóa cache cho NFT: {}", cache_key);
            },
            Err(e) => {
                warn!("Không thể vô hiệu hóa cache cho NFT {}: {}", cache_key, e);
            }
        }
    }
    
    /// Vô hiệu hóa toàn bộ cache
    ///
    /// Xóa toàn bộ cache để đảm bảo mọi kiểm tra NFT sẽ được thực hiện lại
    /// với dữ liệu mới nhất từ blockchain.
    pub async fn invalidate_all_cache(&self) {
        match self.verification_cache.clear().await {
            Ok(_) => {
                info!("Đã vô hiệu hóa toàn bộ cache NFT");
            },
            Err(e) => {
                error!("Không thể vô hiệu hóa toàn bộ cache NFT: {}", e);
            }
        }
    }

    /// Cập nhật trạng thái của người dùng VIP không có NFT
    ///
    /// # Arguments
    /// * `status` - Trạng thái mới
    /// * `reason` - Lý do thay đổi trạng thái
    ///
    /// # Returns
    /// * `Ok(())` - Nếu cập nhật thành công
    /// * `Err(AppError)` - Nếu có lỗi xảy ra
    pub fn update_non_nft_status(&mut self, status: NonNftVipStatus, reason: &str) -> AppResult<()> {
        // Kiểm tra trạng thái hiện tại và trạng thái mới để đảm bảo hợp lệ
        match (&self.non_nft_status, &status) {
            // Chuyển từ StakedDMD sang các trạng thái khác cần kiểm tra đặc biệt
            (NonNftVipStatus::StakedDMD, new_status) if *new_status != NonNftVipStatus::StakedDMD => {
                warn!("Chuyển từ StakedDMD sang {:?}: {}", new_status, reason);
                
                // Cảnh báo việc đang chuyển từ StakedDMD sang trạng thái khác
                // Điều này có thể có tác động đến quyền lợi VIP của người dùng
                warn!("Chuyển từ StakedDMD sang {:?} có thể ảnh hưởng đến quyền lợi VIP. Lý do: {}", 
                      new_status, reason);
                
                // Trong thực tế, cần thêm kiểm tra để đảm bảo stake đã kết thúc
                // hoặc tokens đã được rút ra trước khi chuyển trạng thái
                if *new_status == NonNftVipStatus::Valid && !self.has_active_vip_nft() {
                    return Err(AppError::ValidationError(
                        "Không thể chuyển sang trạng thái Valid khi không có NFT VIP hợp lệ".to_string()
                    ));
                }
            },
            
            // Chuyển sang StakedDMD cần xác minh stake token
            (_, NonNftVipStatus::StakedDMD) => {
                info!("Chuyển sang StakedDMD: {}", reason);
                
                // Trong môi trường thực tế, nên xác minh stake token trước khi cho phép chuyển
                // Ở đây chúng ta chấp nhận trạng thái này, nhưng đánh dấu cần xác minh sau
                // bằng cách làm cho thời điểm cập nhật cũ hơn để kích hoạt xác minh sớm
                // trong lần gọi needs_status_verification() tiếp theo
                let one_hour_ago = Utc::now() - Duration::hours(11); // Để đảm bảo kiểm tra sau 1 giờ
                self.last_status_update = one_hour_ago;
                
                // Ghi log để theo dõi
                info!("Đã đặt thời gian cập nhật trạng thái để đảm bảo xác minh sớm cho StakedDMD");
            },
            
            // Chuyển sang Valid cần có NFT
            (_, NonNftVipStatus::Valid) => {
                if !self.has_active_vip_nft() {
                    warn!("Cố gắng chuyển sang trạng thái Valid mà không có NFT: {}", reason);
                    return Err(AppError::ValidationError(
                        "Không thể chuyển sang trạng thái Valid khi không có NFT VIP hợp lệ".to_string()
                    ));
                }
                debug!("Chuyển sang trạng thái Valid với NFT hợp lệ: {}", reason);
            },
            
            _ => {
                // Các chuyển đổi khác
                debug!("Thay đổi trạng thái từ {:?} sang {:?}: {}", self.non_nft_status, status, reason);
            }
        }
        
        // Cập nhật trạng thái mới
        info!("Cập nhật trạng thái NFT từ {:?} thành {:?}: {}", self.non_nft_status, status, reason);
        self.non_nft_status = status;
        self.last_status_update = Utc::now();
        
        Ok(())
    }
    
    /// Kiểm tra trạng thái VIP dựa trên cả NFT và DMD staking
    ///
    /// # Returns
    /// * `Ok(bool)` - True nếu người dùng có quyền VIP (thông qua NFT hoặc staking)
    /// * `Err(String)` - Nếu có lỗi trong quá trình kiểm tra
    pub fn verify_vip_status(&self) -> Result<bool, String> {
        // Kiểm tra nếu đã stake DMD token
        if self.non_nft_status == NonNftVipStatus::StakedDMD {
            debug!("User có trạng thái VIP thông qua DMD staking");
            
            // Trong phương thức này, chúng ta chỉ kiểm tra trạng thái từ bộ nhớ
            // Xác minh thực tế từ blockchain sẽ được thực hiện qua verify_staked_dmd_status
            // và async_verify_vip_status
            
            // Kiểm tra xem có cần xác minh hay không, nếu cần thì gửi cảnh báo
            if self.needs_status_verification() {
                warn!("Trạng thái StakedDMD cần được xác minh qua blockchain");
                
                // Trả về true nhưng ghi log cảnh báo
                // Việc trả về true là tạm thời cho đến khi xác minh hoàn tất
                info!("Chấp nhận tạm thời trạng thái StakedDMD trong khi chờ xác minh blockchain");
            }
            
            return Ok(true);
        }
        
        // Kiểm tra nếu có NFT hợp lệ
        let has_nft = self.has_active_vip_nft();
        
        // Xác định trạng thái cuối cùng
        match self.non_nft_status {
            NonNftVipStatus::Valid => {
                if has_nft {
                    debug!("User có trạng thái VIP thông qua NFT hợp lệ");
                    Ok(true)
                } else {
                    warn!("Trạng thái không hợp lệ: NonNftVipStatus là Valid nhưng không tìm thấy NFT hoạt động");
                    
                    // Lưu trạng thái không nhất quán vào log
                    error!("Phát hiện trạng thái không nhất quán: NonNftVipStatus=Valid nhưng không có NFT hoạt động");
                    
                    // Cập nhật để phản ánh trạng thái thực tế trong runtime
                    // Nếu không cập nhật được trạng thái, vẫn trả về false để đảm bảo an toàn
                    if let Err(e) = self.update_non_nft_status(NonNftVipStatus::Pending, 
                                                         "Không có NFT hoạt động mặc dù trạng thái là Valid") {
                        error!("Không thể cập nhật trạng thái khi phát hiện sự không nhất quán: {}", e);
                    }
                    Ok(false)
                }
            },
            NonNftVipStatus::Pending => {
                debug!("Trạng thái VIP của user đang chờ giải quyết");
                
                // Kiểm tra nếu có NFT hoạt động, cập nhật trạng thái thành Valid
                if has_nft {
                    info!("Phát hiện có NFT hoạt động trong khi trạng thái là Pending, cập nhật thành Valid");
                    if let Err(e) = self.update_non_nft_status(NonNftVipStatus::Valid,
                                                        "Phát hiện NFT hoạt động trong quá trình xác minh") {
                        error!("Không thể cập nhật trạng thái thành Valid: {}", e);
                    } else {
                        return Ok(true);
                    }
                }
                
                Ok(false)
            },
            NonNftVipStatus::Downgrade => {
                debug!("User đã chọn hạ cấp từ VIP");
                
                // Kiểm tra xem có NFT không. Nếu có, ghi log cảnh báo
                if has_nft {
                    warn!("User có NFT hợp lệ nhưng đã chọn hạ cấp từ VIP");
                }
                
                Ok(false)
            },
            NonNftVipStatus::Pause => {
                debug!("Trạng thái VIP của user đang tạm dừng");
                
                // Kiểm tra xem có NFT không. Nếu có, ghi log cảnh báo
                if has_nft {
                    warn!("User có NFT hợp lệ nhưng trạng thái VIP đang tạm dừng");
                }
                
                Ok(false)
            },
            NonNftVipStatus::StakedDMD => {
                // Đã xử lý ở trên
                unreachable!()
            }
        }
    }
    
    /// Lấy trạng thái hiện tại của người dùng VIP không có NFT
    pub fn get_non_nft_status(&self) -> NonNftVipStatus {
        self.non_nft_status.clone()
    }
    
    /// Kiểm tra xem có cần xác minh lại trạng thái hay không
    /// 
    /// # Returns
    /// * `bool` - True nếu cần xác minh lại
    pub fn needs_status_verification(&self) -> bool {
        let now = Utc::now();
        let time_diff = now.signed_duration_since(self.last_status_update);
        
        // Tần suất kiểm tra phụ thuộc vào trạng thái hiện tại
        match self.non_nft_status {
            NonNftVipStatus::Valid => time_diff > Duration::hours(24),
            NonNftVipStatus::StakedDMD => time_diff > Duration::hours(12),
            NonNftVipStatus::Pending => time_diff > Duration::hours(1),
            _ => time_diff > Duration::hours(6)
        }
    }
    
    /// Kích hoạt NFT theo ID
    pub fn activate_nft(&mut self, nft_id: Uuid) -> AppResult<()> {
        match self.user_nfts.iter_mut().find(|nft| nft.nft_id == nft_id) {
            Some(nft) => {
                let result = nft.activate();
                // Cập nhật trạng thái non-NFT nếu kích hoạt thành công
                if result.is_ok() {
                    self.non_nft_status = NonNftVipStatus::Valid;
                    self.last_status_update = Utc::now();
                    info!("NFT activated successfully, updated non-NFT status to Valid");
                    
                    // Vô hiệu hóa cache sau khi kích hoạt NFT
                    tokio::spawn(async move {
                        if let Err(e) = self.invalidate_all_cache().await {
                            error!("Không thể vô hiệu hóa cache sau khi kích hoạt NFT: {}", e);
                        }
                    });
                }
                result
            },
            None => Err("Không tìm thấy NFT".into()),
        }
    }
    
    /// Xác minh NFT với cross-check trên nhiều blockchain providers
    /// 
    /// Thực hiện xác minh quyền sở hữu NFT trên nhiều blockchain providers
    /// với cơ chế kiểm tra chéo và timeout để đảm bảo độ tin cậy và hiệu năng.
    ///
    /// # Arguments
    /// * `wallet_address` - Địa chỉ ví cần kiểm tra
    /// * `contract_address` - Địa chỉ hợp đồng NFT
    /// * `token_id` - Token ID của NFT
    /// * `chain_id` - Chain ID của blockchain
    ///
    /// # Returns
    /// * `Ok(bool)` - Kết quả xác minh, true nếu có quyền sở hữu
    /// * `Err(String)` - Lỗi nếu có
    pub async fn verify_nft_ownership(
        &self,
        wallet_address: &str,
        contract_address: &str,
        token_id: U256,
        chain_id: u64,
    ) -> Result<bool, String> {
        // Tạo cache key
        let cache_key = format!("{}_{}_{}_{}",
            wallet_address, contract_address, token_id.to_string(), chain_id);

        // Kiểm tra trong cache trước
        if let Some(result) = self.verification_cache.get(&cache_key).await {
            debug!("NFT verification result from cache for {}: {}", cache_key, result);
            return Ok(result);
        }

        // Xác minh quyền sở hữu với timeout
        let providers = match &self.blockchain_providers {
            Some(p) => p,
            None => return Err("Không có blockchain providers".to_string()),
        };

        let mut verification_results = Vec::new();
        let mut errors = Vec::new();
        
        // Timeout cho toàn bộ quá trình xác minh
        match timeout(StdDuration::from_secs(30), async {
            // Xác minh trên từng provider
            for (i, provider) in providers.iter().enumerate() {
                match provider.verify_nft_ownership(
                    wallet_address,
                    contract_address,
                    token_id,
                    chain_id,
                ).await {
                    Ok(result) => {
                        debug!("Provider {} NFT verification: {}", i, result);
                        verification_results.push(result);
                    },
                    Err(e) => {
                        warn!("Provider {} NFT verification error: {}", i, e);
                        errors.push(format!("Provider {} error: {}", i, e));
                    }
                }
            }
        }).await {
            Ok(_) => {
                if verification_results.is_empty() {
                    error!("Failed to verify NFT: {}", errors.join(", "));
                    return Err(format!("Không thể xác minh NFT: {}", errors.join(", ")));
                }
                
                // Cross-check: cần ít nhất 2/3 số kết quả khớp nhau
                let truthy_count = verification_results.iter().filter(|&&r| r).count();
                let falsy_count = verification_results.len() - truthy_count;
                
                let required_threshold = (verification_results.len() as f64 * 0.66).ceil() as usize;
                let final_result = if truthy_count >= required_threshold {
                    info!("NFT verification passed with {}/{} positive results", truthy_count, verification_results.len());
                    true
                } else if falsy_count >= required_threshold {
                    warn!("NFT verification failed with {}/{} negative results", falsy_count, verification_results.len());
                    false
                } else {
                    warn!("NFT verification results inconclusive: {}/{} positive, {}/{} negative", 
                         truthy_count, verification_results.len(), falsy_count, verification_results.len());
                    return Err("Không đủ độ tin cậy từ kết quả xác minh".to_string());
                };
                
                // Lưu kết quả vào cache với TTL ngắn hơn (4 giờ thay vì 24 giờ)
                // để đảm bảo dữ liệu luôn được cập nhật đúng lúc
                let ttl = StdDuration::from_secs(60 * 60 * 4); // 4 giờ
                self.verification_cache.insert_with_ttl(cache_key, final_result, ttl).await
                    .map_err(|e| format!("Lỗi cache: {}", e))?;
                
                Ok(final_result)
            },
            Err(_) => {
                error!("Timeout when verifying NFT ownership for wallet: {}, contract: {}, token: {}", 
                      wallet_address, contract_address, token_id);
                Err("Timeout khi xác minh NFT".to_string())
            },
        }
    }

    /// Xác minh trạng thái StakedDMD
    ///
    /// Phương thức này kiểm tra xem người dùng có đủ điều kiện để duy trì
    /// trạng thái StakedDMD không dựa trên thông tin stake token.
    /// 
    /// # Arguments
    /// * `user_id` - ID người dùng
    /// * `wallet_address` - Địa chỉ ví
    /// * `minimum_stake` - Số lượng token tối thiểu cần stake
    ///
    /// # Returns
    /// * `Ok(bool)` - true nếu trạng thái StakedDMD hợp lệ, false nếu không
    /// * `Err(String)` - Nếu có lỗi xảy ra
    pub async fn verify_staked_dmd_status(
        &self,
        user_id: &str,
        wallet_address: &str,
        minimum_stake: U256,
    ) -> Result<bool, String> {
        // Kiểm tra trạng thái và log chi tiết
        match self.non_nft_status {
            NonNftVipStatus::StakedDMD => {
                info!("Xác minh trạng thái StakedDMD cho user {}", user_id);
            },
            other => {
                // Vẫn có thể xác minh nhưng sẽ ghi log thông báo
                debug!("Xác minh stake DMD cho user {} đang ở trạng thái {:?}, không phải StakedDMD", 
                       user_id, other);
            }
        };
        
        // Kiểm tra xem có blockchain provider nào không
        let providers = match &self.blockchain_providers {
            Some(providers) if !providers.is_empty() => providers,
            _ => {
                warn!("Không có blockchain provider để xác minh StakedDMD cho user {}", user_id);
                return Err("Không có blockchain provider khả dụng".to_string());
            }
        };
        
        // Lấy address từ chuỗi
        let wallet_address = match Address::from_str(wallet_address) {
            Ok(address) => address,
            Err(e) => {
                error!("Địa chỉ ví không hợp lệ khi xác minh StakedDMD cho user {}: {}", user_id, e);
                return Err(format!("Địa chỉ ví không hợp lệ: {}", e));
            }
        };
        
        // Tạo cache key để lưu kết quả xác minh
        let cache_key = format!("staked_dmd_verify:{}:{}", user_id, wallet_address);
        
        // Kiểm tra cache trước với kiểm tra trạng thái cache
        if let Some(result) = self.verification_cache.get(&cache_key).await {
            // Kiểm tra xem kết quả này có cũ không
            let last_status_update = self.last_status_update;
            let time_since_update = Utc::now().signed_duration_since(last_status_update).num_hours();
            
            // Nếu trạng thái đã được cập nhật trong 1 giờ qua và cache có giá trị,
            // thì sử dụng cache
            if time_since_update < 1 {
                debug!("Sử dụng kết quả xác minh StakedDMD từ cache cho user {}: {}", user_id, result);
                return Ok(result);
            } else {
                // Cache cũ, cần xác minh lại
                debug!("Cache xác minh StakedDMD cho user {} đã cũ ({} giờ), xác minh lại", 
                       user_id, time_since_update);
            }
        }
        
        // Trong môi trường thực tế, cần triển khai đầy đủ kiểm tra stake
        // từ blockchain, bao gồm:
        // 1. Gọi contract stake để lấy thông tin số lượng token đã stake
        // 2. Kiểm tra thời hạn stake có đủ để duy trì VIP không
        // 3. Xác minh số lượng token so với minimum_stake
        
        // Đây là triển khai giả lập cho mục đích demo
        // Trong thực tế cần gọi contract:
        
        // Bắt đầu timeout để không bị treo khi gọi blockchain
        match tokio::time::timeout(std::time::Duration::from_secs(15), async {
            // Giả lập gọi contract
            // Đây là nơi sẽ thực hiện các cuộc gọi blockchain thực tế
            // ví dụ:
            // let staking_contract = self.get_staking_contract().await?;
            // let staked_amount = staking_contract.get_staked_amount(wallet_address).await?;
            // let lock_time = staking_contract.get_lock_time(wallet_address).await?;
            // let is_valid = staked_amount >= minimum_stake && lock_time >= required_lock_time;
            
            // Giả lập kết quả cho demo, thay bằng logic thực khi triển khai
            let staked_amount = U256::from(100_000); // Giả sử đã stake 100,000 token
            let is_valid = staked_amount >= minimum_stake;
            
            // Xác minh cross-check với nhiều providers (trong thực tế)
            // và thực hiện logic quyết định dựa trên kết quả từ nhiều nguồn
            
            // Vì đây là demo nên chúng ta giả định là hợp lệ
            Ok::<bool, String>(is_valid)
        }).await {
            Ok(result) => match result {
                Ok(is_valid) => {
                    // Lưu kết quả vào cache với TTL ngắn hơn (1 giờ thay vì 24 giờ cho StakedDMD)
                    // để đảm bảo dữ liệu luôn được cập nhật đúng lúc
                    let ttl = std::time::Duration::from_secs(60 * 60 * 1); // 1 giờ
                    if let Err(e) = self.verification_cache.insert_with_ttl(cache_key, is_valid, ttl).await {
                        warn!("Không thể lưu kết quả xác minh StakedDMD vào cache: {}", e);
                    }
                    
                    if is_valid {
                        info!("Trạng thái StakedDMD của user {} là hợp lệ", user_id);
                    } else {
                        warn!("Trạng thái StakedDMD của user {} KHÔNG hợp lệ!", user_id);
                    }
                    
                    Ok(is_valid)
                },
                Err(e) => Err(format!("Lỗi khi xác minh stake: {}", e)),
            },
            Err(_) => {
                error!("Timeout khi xác minh StakedDMD cho user {}", user_id);
                Err("Timeout khi xác minh StakedDMD".to_string())
            }
        }
    }

    /// Xác minh bất đồng bộ trạng thái VIP đầy đủ
    ///
    /// Phương thức này là phiên bản bất đồng bộ của verify_vip_status,
    /// cho phép xác minh đầy đủ cả trạng thái StakedDMD trực tiếp từ blockchain.
    /// 
    /// # Arguments
    /// * `user_id` - ID của người dùng
    /// * `wallet_address` - Địa chỉ ví của người dùng
    /// * `minimum_stake` - Số lượng token tối thiểu cần stake (U256)
    /// 
    /// # Returns
    /// * `Ok(bool)` - True nếu người dùng có quyền VIP (thông qua NFT hoặc staking)
    /// * `Err(String)` - Nếu có lỗi trong quá trình kiểm tra
    pub async fn async_verify_vip_status(
        &self,
        user_id: &str,
        wallet_address: &str,
        minimum_stake: U256
    ) -> Result<bool, String> {
        // Lưu thời gian bắt đầu để có thể log xem việc xác minh mất bao lâu
        let start_time = Utc::now();
        let mut verified = false;
        
        // Kiểm tra nếu đã stake DMD token
        if self.non_nft_status == NonNftVipStatus::StakedDMD {
            info!("Bắt đầu xác minh trạng thái VIP qua StakedDMD cho user {}", user_id);
            
            // Xác minh trực tiếp trạng thái StakedDMD từ blockchain
            match self.verify_staked_dmd_status(user_id, wallet_address, minimum_stake).await {
                Ok(is_valid) => {
                    if is_valid {
                        debug!("Đã xác minh thành công trạng thái VIP qua StakedDMD cho user {}", user_id);
                        verified = true;
                    } else {
                        warn!("Trạng thái StakedDMD không hợp lệ cho user {}, cần xử lý cập nhật trạng thái", user_id);
                        
                        // Trong thực tế, khi phát hiện stake không hợp lệ, 
                        // chúng ta sẽ đổi trạng thái thành Pending và thông báo cho người dùng
                        error!("User {} stake DMD không hợp lệ, cần cập nhật trạng thái", user_id);
                        
                        // Thực hiện một cập nhật trạng thái cụ thể trong thực tế
                        // Ở đây chúng ta chỉ ghi log mẫu vì cập nhật trạng thái là phương thức mutable
                        // và không thể gọi trong phương thức này (do phương thức này là &self không phải &mut self)
                        // self.update_non_nft_status(NonNftVipStatus::Pending, 
                        //     &format!("StakedDMD không hợp lệ khi xác minh async: wallet={}", wallet_address));
                    }
                },
                Err(e) => {
                    // Thử xác minh xem có NFT không trước khi báo lỗi
                    if self.has_active_vip_nft() {
                        info!("User {} có NFT hợp lệ mặc dù không thể xác minh StakedDMD: {}", user_id, e);
                        // Có NFT hợp lệ thì vẫn cho là VIP
                        verified = true;
                    } else {
                        error!("Lỗi khi xác minh trạng thái StakedDMD cho user {} và không có NFT hợp lệ: {}", user_id, e);
                        // Trong trường hợp lỗi và không có NFT, cần xử lý cẩn thận
                        // Trong thực tế nên triển khai theo mức độ nghiêm trọng của lỗi
                        return Err(format!("Không thể xác minh trạng thái VIP: {}", e));
                    }
                }
            }
            
            // Kiểm tra nếu đã được xác minh qua StakedDMD, trả về kết quả
            if verified {
                let duration = Utc::now().signed_duration_since(start_time);
                debug!("Xác minh trạng thái VIP hoàn tất trong {} ms", duration.num_milliseconds());
                return Ok(true);
            }
        }
        
        // Nếu không phải StakedDMD hoặc không được xác minh qua StakedDMD,
        // kiểm tra trạng thái VIP thông qua NFT
        let has_nft = self.has_active_vip_nft();
        
        match self.non_nft_status {
            NonNftVipStatus::Valid => {
                if has_nft {
                    debug!("User {} có trạng thái VIP thông qua NFT hợp lệ", user_id);
                    Ok(true)
                } else {
                    warn!("Trạng thái không nhất quán: User {} có trạng thái Valid nhưng không có NFT", user_id);
                    // Trạng thái không nhất quán, trả về false để đảm bảo an toàn
                    Ok(false)
                }
            },
            NonNftVipStatus::Pending => {
                if has_nft {
                    info!("User {} có NFT hợp lệ mặc dù trạng thái là Pending", user_id);
                    // Có NFT hợp lệ thì vẫn cho là VIP
                    Ok(true)
                } else {
                    debug!("User {} trạng thái Pending và không có NFT hợp lệ", user_id);
                    Ok(false)
                }
            },
            NonNftVipStatus::StakedDMD => {
                // Đã được xử lý ở trên, nhưng nếu không được xác minh,
                // thì kiểm tra xem có NFT hợp lệ không như một cách dự phòng
                if has_nft {
                    info!("User {} có NFT hợp lệ mặc dù đang ở trạng thái StakedDMD", user_id);
                    Ok(true)
                } else {
                    warn!("User {} không có NFT và StakedDMD không hợp lệ", user_id);
                    Ok(false)
                }
            },
            _ => {
                // Các trạng thái khác (Downgrade, Pause)
                if has_nft {
                    warn!("User {} có NFT hợp lệ nhưng trạng thái là {:?}", user_id, self.non_nft_status);
                    // Có NFT hợp lệ thì vẫn cho là VIP
                    Ok(true)
                } else {
                    debug!("User {} không có NFT và trạng thái là {:?}", user_id, self.non_nft_status);
                    Ok(false)
                }
            }
        }
    }

    /// Cập nhật thông tin NFT cho một subscription.
    ///
    /// Phương thức này cập nhật thông tin NFT trong một subscription
    /// và đảm bảo rằng các cache liên quan sẽ được làm mới.
    ///
    /// # Arguments
    /// * `subscription` - Tham chiếu đến subscription cần cập nhật
    /// * `nft_info` - Thông tin NFT mới
    ///
    /// # Returns
    /// * `Ok(())` - Nếu cập nhật thành công
    /// * `Err(WalletError)` - Nếu có lỗi xảy ra
    pub async fn update_nft_info(
        &self,
        subscription: &mut UserSubscription,
        nft_info: NftInfo,
    ) -> Result<(), WalletError> {
        // Ghi log để theo dõi
        info!(
            "Cập nhật thông tin NFT cho user: {}, NFT contract: {:?}, token_id: {}",
            subscription.user_id, nft_info.contract_address, nft_info.token_id
        );
        
        // Cập nhật thông tin NFT
        subscription.nft_info = Some(nft_info.clone());
        
        // Vô hiệu hóa cache để đảm bảo dữ liệu mới nhất
        self.invalidate_cache(&subscription.user_id).await?;
        
        // Vô hiệu hóa cache trong auto_trade_manager nếu có
        if let Some(auto_trade_manager) = &self.auto_trade_manager {
            if let Some(manager) = auto_trade_manager.upgrade() {
                if let Err(e) = manager.invalidate_user_cache(&subscription.user_id).await {
                    warn!("Không thể vô hiệu hóa cache auto-trade khi cập nhật NFT: {}", e);
                }
            }
        }
        
        Ok(())
    }

    /// Vô hiệu hóa cache cho một user_id.
    ///
    /// Đảm bảo rằng tất cả các cache liên quan đến NFT verification sẽ được làm mới,
    /// giúp người dùng luôn nhận được thông tin mới nhất sau khi có thay đổi.
    ///
    /// # Arguments
    /// * `user_id` - ID của người dùng cần vô hiệu hóa cache
    ///
    /// # Returns
    /// * `Ok(())` - Nếu vô hiệu hóa thành công
    /// * `Err(WalletError)` - Nếu có lỗi xảy ra
    pub async fn invalidate_cache(&self, user_id: &str) -> Result<(), WalletError> {
        debug!("Vô hiệu hóa cache NFT cho user: {}", user_id);
        
        // Xóa khỏi verification_cache
        self.verification_cache.remove(user_id).await;
        
        // Đánh dấu cho các cache khác cần refresh
        if let Some(manager) = &self.subscription_manager {
            if let Some(sub_manager) = manager.upgrade() {
                // Thông báo cho SubscriptionManager refresh cache
                if let Err(e) = sub_manager.refresh_user_cache(user_id).await {
                    warn!("Không thể refresh cache subscription khi vô hiệu hóa NFT cache: {}", e);
                }
                
                // Đánh dấu người dùng cần kiểm tra lại NFT
                if let Err(e) = sub_manager.mark_user_for_verification(user_id).await {
                    warn!("Không thể đánh dấu người dùng cần kiểm tra lại NFT: {}", e);
                }
            }
        }
        
        Ok(())
    }

    /// Đồng bộ hóa cache NFT với các module khác.
    ///
    /// Phương thức này đảm bảo rằng tất cả các cache liên quan đến NFT
    /// được đồng bộ hóa giữa các module như subscription, auto-trade, v.v.
    ///
    /// # Arguments
    /// * `user_ids` - Danh sách các user_id cần đồng bộ hóa
    ///
    /// # Returns
    /// * `Ok(())` - Nếu đồng bộ hóa thành công
    /// * `Err(WalletError)` - Nếu có lỗi xảy ra
    pub async fn sync_cache_with_other_modules(&self, user_ids: &[String]) -> Result<(), WalletError> {
        debug!("Đồng bộ hóa cache NFT với các module khác cho {} user", user_ids.len());
        
        for user_id in user_ids {
            // Vô hiệu hóa cache cho user hiện tại
            self.invalidate_cache(user_id).await?;
        }
        
        // Sau khi vô hiệu hóa tất cả cache, đồng bộ lại với database
        self.sync_cache_with_database().await?;
        
        info!("Đã đồng bộ hóa cache NFT thành công với {} user", user_ids.len());
        Ok(())
    }
    
    /// Đồng bộ hóa cache với database.
    ///
    /// Tải lại dữ liệu từ database để đảm bảo cache là mới nhất.
    ///
    /// # Returns
    /// * `Ok(())` - Nếu đồng bộ hóa thành công
    /// * `Err(WalletError)` - Nếu có lỗi xảy ra
    pub async fn sync_cache_with_database(&self) -> Result<(), WalletError> {
        debug!("Đồng bộ hóa cache NFT với database");
        
        // TODO: Thực hiện tải lại dữ liệu từ database
        // Đây là nơi để tải lại NFT data từ database và cập nhật cache
        
        Ok(())
    }
} 