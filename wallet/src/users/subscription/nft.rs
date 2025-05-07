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
}

impl NftManager {
    /// Tạo manager mới với danh sách NFT
    pub fn new(user_nfts: Vec<VipNftInfo>) -> Self {
        Self { 
            user_nfts,
            nft_cache: Arc::new(RwLock::new(HashMap::new())),
            blockchain_providers: None,
            verification_cache: Arc::new(cache::AsyncCache::new(100)), // Cache 100 kết quả xác minh
        }
    }

    /// Thiết lập blockchain providers để xác minh NFT
    pub fn set_blockchain_providers(&mut self, providers: Arc<Vec<Box<dyn BlockchainProvider>>>) {
        self.blockchain_providers = Some(providers);
    }

    /// Kiểm tra người dùng có NFT VIP đang hoạt động không
    pub fn has_active_vip_nft(&self) -> bool {
        self.user_nfts.iter().any(|nft| nft.is_active() && nft.is_vip_collection())
    }

    /// Lấy thông tin NFT VIP đang hoạt động
    pub fn get_active_vip_nft(&self) -> Option<&VipNftInfo> {
        self.user_nfts
            .iter()
            .find(|nft| nft.is_active() && nft.is_vip_collection())
    }

    /// Thêm NFT mới vào danh sách
    pub fn add_nft(&mut self, nft: VipNftInfo) {
        self.user_nfts.push(nft);
    }

    /// Kích hoạt NFT theo ID
    pub fn activate_nft(&mut self, nft_id: Uuid) -> AppResult<()> {
        match self.user_nfts.iter_mut().find(|nft| nft.nft_id == nft_id) {
            Some(nft) => nft.activate(),
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
                        errors.push(format!("Provider {} error: {}", i, e));
                    }
                }
            }
        }).await {
            Ok(_) => {
                if verification_results.is_empty() {
                    return Err(format!("Không thể xác minh NFT: {}", errors.join(", ")));
                }
                
                // Cross-check: cần ít nhất 2/3 số kết quả khớp nhau
                let truthy_count = verification_results.iter().filter(|&&r| r).count();
                let falsy_count = verification_results.len() - truthy_count;
                
                let required_threshold = (verification_results.len() as f64 * 0.66).ceil() as usize;
                let final_result = if truthy_count >= required_threshold {
                    true
                } else if falsy_count >= required_threshold {
                    false
                } else {
                    return Err("Không đủ độ tin cậy từ kết quả xác minh".to_string());
                };
                
                // Lưu kết quả vào cache
                self.verification_cache.insert(cache_key, final_result).await
                    .map_err(|e| format!("Lỗi cache: {}", e))?;
                
                Ok(final_result)
            },
            Err(_) => Err("Timeout khi xác minh NFT".to_string()),
        }
    }
} 