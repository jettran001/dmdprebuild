//! Module staking quản lý việc đặt cọc (staking) token DMD cho các đăng ký VIP.
//! 
//! Module này cung cấp các chức năng:
//! * Đặt cọc token DMD để nâng cấp hoặc duy trì tư cách VIP
//! * Quản lý thông tin đặt cọc của người dùng (StakeInfo)
//! * Tính toán phần thưởng dựa trên số lượng token đặt cọc và thời gian
//! * Hoàn thành quá trình đặt cọc và rút token sau khi hết hạn
//! * Theo dõi thời gian còn lại cho mỗi khoản đặt cọc
//! * Xử lý lỗi cho các tương tác với blockchain
//! * Xác minh số dư tối thiểu cho việc đặt cọc
//! 
//! Module này tương tác với các hợp đồng thông minh trên blockchain để 
//! thực hiện các hành động đặt cọc và rút token, đồng thời duy trì
//! trạng thái của người dùng VIP mà không cần NFT.

// External imports
use ethers::types::{Address, U256};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

// Standard library imports
use std::fmt;
use std::sync::Arc;
use std::collections::HashMap;

// Tokio imports
use tokio::sync::RwLock;

// Time related imports
use chrono::{DateTime, Duration, Utc};
use tracing::{info, warn, error, debug};

// Internal imports
use crate::error::WalletError;
use crate::users::subscription::types::PaymentToken;
use crate::walletmanager::chain::ChainType;
use crate::walletmanager::api::WalletManagerApi;

use super::constants::{
    STAKED_DMD_APY_PERCENTAGE,
    TWELVE_MONTH_STAKE_DAYS,
    MIN_DMD_STAKE_AMOUNT,
    STAKING_REWARD_INTERVAL_DAYS,
    DMD_STAKING_POOL_ADDRESS,
    DMD_STAKING_TOKEN_ID,
};

// Thay thế đoạn định nghĩa enum StakeStatus với việc import từ module cha
use super::types::StakeStatus;

/// Lỗi khi thực hiện stake token
#[derive(Debug, Error)]
pub enum StakingError {
    /// Số lượng token không đủ
    #[error("Số lượng token không đủ, cần tối thiểu {}", .0)]
    InsufficientAmount(Decimal),
    
    /// Không tìm thấy thông tin stake
    #[error("Không tìm thấy thông tin stake với ID: {}", .0)]
    StakeNotFound(String),
    
    /// Không thể hủy stake đang hoạt động
    #[error("Không thể hủy stake đang hoạt động")]
    CannotCancelActiveStake,
    
    /// Lỗi từ blockchain
    #[error("Lỗi blockchain: {}", .0)]
    BlockchainError(String),
    
    /// NFT không được tìm thấy
    #[error("NFT không được tìm thấy trong ví")]
    NftNotFound,
    
    /// Lỗi khác
    #[error("Lỗi: {}", .0)]
    Other(String),
}

impl From<StakingError> for WalletError {
    fn from(err: StakingError) -> Self {
        WalletError::Other(err.to_string())
    }
}

/// Thông tin về token đã stake
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStake {
    /// ID của stake
    pub stake_id: String,
    /// ID người dùng
    pub user_id: String,
    /// Địa chỉ ví
    pub wallet_address: Address,
    /// Loại blockchain
    pub chain_type: ChainType,
    /// Số lượng token
    pub amount: U256,
    /// Loại token
    pub token: PaymentToken,
    /// Ngày bắt đầu
    pub start_date: DateTime<Utc>,
    /// Ngày kết thúc
    pub end_date: DateTime<Utc>,
    /// Trạng thái
    pub status: StakeStatus,
    /// Hash giao dịch stake
    pub stake_tx_hash: Option<String>,
    /// Hash giao dịch unstake
    pub unstake_tx_hash: Option<String>,
    /// Lợi nhuận dự kiến
    pub expected_rewards: Decimal,
    /// Lợi nhuận đã nhận
    pub received_rewards: Decimal,
    /// Đã kích hoạt cho gói VIP 12 tháng
    pub is_vip_subscription: bool,
    /// Token ID của NFT ERC-1155
    pub erc1155_token_id: Option<u64>,
    /// Địa chỉ hợp đồng của NFT ERC-1155
    pub erc1155_contract_address: Option<Address>,
}

impl TokenStake {
    /// Tạo mới thông tin stake
    pub fn new(
        user_id: &str,
        wallet_address: Address,
        chain_type: ChainType,
        amount: U256,
        duration_days: i64,
    ) -> Result<Self, StakingError> {
        if amount < MIN_DMD_STAKE_AMOUNT {
            return Err(StakingError::InsufficientAmount(Decimal::from(MIN_DMD_STAKE_AMOUNT)));
        }

        let start_date = Utc::now();
        let end_date = start_date + Duration::days(duration_days);
        let amount_decimal = Decimal::from_i128_with_scale(amount.as_u128() as i128, 2);
        
        // Tính toán lợi nhuận dự kiến
        let apy_rate = STAKED_DMD_APY_PERCENTAGE / Decimal::from(100);
        let year_fraction = Decimal::from(duration_days) / Decimal::from(365);
        let expected_rewards = amount_decimal.checked_mul(apy_rate)
            .and_then(|r| r.checked_mul(year_fraction))
            .ok_or_else(|| StakingError::Other("Lỗi tính toán lợi nhuận".to_string()))?;
        
        Ok(Self {
            stake_id: Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            wallet_address,
            chain_type,
            amount,
            token: PaymentToken::DMD,
            start_date,
            end_date,
            status: StakeStatus::Active,
            stake_tx_hash: None,
            unstake_tx_hash: None,
            expected_rewards,
            received_rewards: Decimal::new(0, 0),
            is_vip_subscription: false,
            erc1155_token_id: Some(DMD_STAKING_TOKEN_ID),
            erc1155_contract_address: None,
        })
    }
    
    /// Cập nhật hash giao dịch stake
    pub fn set_stake_tx_hash(&mut self, tx_hash: &str) {
        self.stake_tx_hash = Some(tx_hash.to_string());
    }
    
    /// Kiểm tra nếu stake đã hết hạn
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.end_date
    }
    
    /// Hoàn thành stake khi hết hạn
    pub fn complete(&mut self, unstake_tx_hash: &str) -> Result<(), StakingError> {
        if !self.is_expired() && self.status == StakeStatus::Active {
            return Err(StakingError::CannotCancelActiveStake);
        }
        
        self.status = StakeStatus::Completed;
        self.unstake_tx_hash = Some(unstake_tx_hash.to_string());
        
        Ok(())
    }
    
    /// Tính toán lợi nhuận hiện tại
    pub fn calculate_current_rewards(&self) -> Result<Decimal, StakingError> {
        if self.status != StakeStatus::Active {
            return Ok(self.received_rewards);
        }
        
        let now = Utc::now();
        let elapsed = now.signed_duration_since(self.start_date);
        let total_duration = self.end_date.signed_duration_since(self.start_date);
        
        if elapsed <= chrono::Duration::zero() {
            return Ok(Decimal::new(0, 0));
        }
        
        let elapsed_fraction = Decimal::from(elapsed.num_days()) / Decimal::from(total_duration.num_days());
        let current_rewards = self.expected_rewards.checked_mul(elapsed_fraction)
            .ok_or_else(|| StakingError::Other("Lỗi tính toán lợi nhuận".to_string()))?;
        
        Ok(current_rewards)
    }
    
    /// Đánh dấu stake này là cho gói VIP 12 tháng
    pub fn mark_as_vip_subscription(&mut self) {
        self.is_vip_subscription = true;
    }
    
    /// Kiểm tra xem stake có phải cho gói VIP 12 tháng không
    pub fn is_vip_subscription(&self) -> bool {
        self.is_vip_subscription
    }
    
    /// Cập nhật thông tin ERC-1155
    pub fn set_erc1155_info(&mut self, contract_address: Address, token_id: u64) {
        self.erc1155_contract_address = Some(contract_address);
        self.erc1155_token_id = Some(token_id);
    }
}

/// Quản lý stake token
pub struct StakingManager {
    /// API quản lý ví
    wallet_api: Arc<tokio::sync::RwLock<WalletManagerApi>>,
    /// Địa chỉ của staking pool
    staking_pool_address: Address,
    /// Danh sách các stake đang hoạt động
    active_stakes: Arc<tokio::sync::RwLock<HashMap<String, TokenStake>>>,
}

impl StakingManager {
    /// Tạo mới StakingManager
    pub fn new(wallet_api: Arc<tokio::sync::RwLock<WalletManagerApi>>) -> Self {
        let staking_pool_address = match DMD_STAKING_POOL_ADDRESS.parse::<Address>() {
            Ok(address) => address,
            Err(e) => {
                warn!("Không thể parse địa chỉ staking pool: {}, sử dụng địa chỉ zero", e);
                Address::zero()
            }
        };
            
        Self {
            wallet_api,
            staking_pool_address,
            active_stakes: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }
    
    /// Stake token DMD
    pub async fn stake_tokens(
        &self,
        user_id: &str,
        wallet_address: Address,
        chain_type: ChainType,
        amount: U256,
    ) -> Result<TokenStake, StakingError> {
        // Kiểm tra số dư tối thiểu
        if amount < MIN_DMD_STAKE_AMOUNT {
            error!("Số lượng stake không đủ: user_id={}, amount={}, minimum={}", 
                  user_id, amount, MIN_DMD_STAKE_AMOUNT);
            return Err(StakingError::InsufficientAmount(Decimal::from(MIN_DMD_STAKE_AMOUNT)));
        }

        info!("Bắt đầu stake token cho user_id={}, amount={}", user_id, amount);
        
        // Kiểm tra địa chỉ ví có hợp lệ không
        if wallet_address.is_zero() {
            error!("Địa chỉ ví không hợp lệ (zero address): user_id={}", user_id);
            return Err(StakingError::Other("Địa chỉ ví không hợp lệ".to_string()));
        }

        // Tạo thông tin stake mới
        let stake = match TokenStake::new(
            user_id,
            wallet_address,
            chain_type,
            amount,
            TWELVE_MONTH_STAKE_DAYS,
        ) {
            Ok(stake) => stake,
            Err(e) => {
                error!("Không thể tạo thông tin stake: user_id={}, error={}", user_id, e);
                return Err(e);
            }
        };

        // Thực hiện stake trên blockchain với retry
        const MAX_RETRIES: u8 = 3;
        let mut last_error = None;
        
        // Cải thiện cách xử lý RwLock
        let wallet_api = self.wallet_api.read().await.map_err(|e| {
            error!("Lỗi RwLock khi truy cập wallet API: {}", e);
            StakingError::Other(format!("Lỗi đồng bộ hóa khi truy cập WalletAPI: {}", e))
        })?;
        
        let mut tx_hash = String::new();
        
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                info!("Thử lại stake lần {}/{} cho user_id={}", attempt, MAX_RETRIES, user_id);
                // Đợi một khoảng thời gian trước khi thử lại
                tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
            }
            
            match wallet_api.stake_tokens(
                wallet_address,
                amount,
                TWELVE_MONTH_STAKE_DAYS,
            ).await {
                Ok(hash) => {
                    info!("Stake thành công: user_id={}, tx_hash={}, attempt={}", user_id, hash, attempt + 1);
                    tx_hash = hash;
                    break;
                },
                Err(e) => {
                    warn!("Lỗi khi stake token lần {}/{}: user_id={}, error={}", 
                         attempt + 1, MAX_RETRIES, user_id, e);
                    last_error = Some(e);
                    
                    // Nếu là lỗi không thể retry, thoát ngay
                    if matches!(e, WalletError::InsufficientBalance | WalletError::InvalidAddress) {
                        error!("Lỗi không thể retry khi stake: user_id={}, error={}", user_id, e);
                        return Err(StakingError::BlockchainError(format!("Lỗi không thể retry: {}", e)));
                    }
                    
                    // Nếu đã hết số lần thử
                    if attempt == MAX_RETRIES {
                        error!("Đã hết số lần thử stake token: user_id={}, attempts={}", user_id, MAX_RETRIES + 1);
                        return Err(StakingError::BlockchainError(format!(
                            "Lỗi sau {} lần thử: {}", 
                            MAX_RETRIES + 1, 
                            last_error.as_ref().map_or_else(|| "Unknown error".to_string(), |e| e.to_string())
                        )));
                    }
                }
            }
        }
        
        if tx_hash.is_empty() {
            error!("Không thể stake token sau khi thử lại: user_id={}", user_id);
            return Err(StakingError::BlockchainError(format!(
                "Không thể stake token sau {} lần thử", MAX_RETRIES + 1
            )));
        }

        // Cập nhật thông tin stake
        let mut stake = stake;
        stake.set_stake_tx_hash(&tx_hash);

        // Lưu vào danh sách active stakes
        let mut active_stakes = self.active_stakes.write().await;
        active_stakes.insert(stake.stake_id.clone(), stake.clone());

        // Ghi log thành công
        info!("Stake hoàn tất: user_id={}, stake_id={}, amount={}, tx_hash={}", 
             user_id, stake.stake_id, amount, tx_hash);
        
        Ok(stake)
    }
    
    /// Lấy danh sách stake của người dùng
    pub async fn get_user_stakes(&self, user_id: &str) -> Result<Vec<TokenStake>, StakingError> {
        if user_id.is_empty() {
            error!("User ID không hợp lệ khi lấy danh sách stake");
            return Err(StakingError::Other("User ID không được để trống".to_string()));
        }
        
        // TODO: Thực hiện truy vấn database
        // Trong môi trường thực tế, sẽ gọi database và xử lý lỗi
        // Ví dụ:
        /*
        let mongodb = match get_mongodb_client().await {
            Ok(client) => client,
            Err(e) => {
                error!("Lỗi kết nối database: {}", e);
                return Err(StakingError::Database(format!("Lỗi kết nối: {}", e)));
            }
        };
        
        let stakes = mongodb.collection("token_stakes");
        match stakes.find(doc! { "user_id": user_id }).await {
            Ok(cursor) => {
                match cursor.try_collect().await {
                    Ok(stakes) => Ok(stakes),
                    Err(e) => {
                        error!("Lỗi khi đọc dữ liệu stake từ cursor: {}", e);
                        Err(StakingError::Database(format!("Lỗi đọc dữ liệu: {}", e)))
                    }
                }
            },
            Err(e) => {
                error!("Lỗi khi truy vấn thông tin stake từ database: {}", e);
                Err(StakingError::Database(format!("Lỗi truy vấn: {}", e)))
            }
        }
        */
        
        // Giả lập cho mục đích demo
        info!("Lấy danh sách stake cho user {}", user_id);
        Ok(vec![])
    }
    
    /// Hoàn thành stake khi đủ điều kiện
    pub async fn complete_stake(
        &self,
        user_id: &str,
        stake_id: &str,
    ) -> Result<TokenStake, StakingError> {
        // Validate đầu vào
        if user_id.is_empty() {
            error!("User ID không hợp lệ khi hoàn thành stake");
            return Err(StakingError::Other("User ID không được để trống".to_string()));
        }
        
        if stake_id.is_empty() {
            error!("Stake ID không hợp lệ khi hoàn thành stake");
            return Err(StakingError::Other("Stake ID không được để trống".to_string()));
        }
        
        info!("Bắt đầu hoàn thành stake ID {} cho user {}", stake_id, user_id);
        
        // Lấy thông tin stake từ active stakes trước
        let stake_from_active = {
            let active_stakes = self.active_stakes.read().await;
            active_stakes.get(stake_id).cloned()
        };
        
        // Nếu tìm thấy trong active stakes, sử dụng ngay
        let stake = if let Some(stake) = stake_from_active {
            debug!("Tìm thấy stake ID {} trong active stakes", stake_id);
            stake
        } else {
            // Nếu không tìm thấy, tìm trong database
            debug!("Không tìm thấy stake ID {} trong active stakes, tìm trong database", stake_id);
            let stakes = match self.get_user_stakes(user_id).await {
                Ok(stakes) => stakes,
                Err(e) => {
                    error!("Lỗi khi lấy danh sách stake: {}", e);
                    return Err(e);
                }
            };
            
            if stakes.is_empty() {
                warn!("Không tìm thấy stake nào cho user {}", user_id);
                return Err(StakingError::StakeNotFound(format!("User {} không có stake nào", user_id)));
            }
            
            stakes.iter()
                .find(|s| s.stake_id == stake_id)
                .cloned()
                .ok_or_else(|| {
                    warn!("Không tìm thấy stake với ID {} cho user {}", stake_id, user_id);
                    StakingError::StakeNotFound(stake_id.to_string())
                })?
        };
        
        // Kiểm tra điều kiện unstake
        if !stake.is_expired() {
            warn!("Stake ID {} chưa hết hạn, không thể hoàn thành", stake_id);
            return Err(StakingError::CannotCancelActiveStake);
        }
        
        if stake.status != StakeStatus::Active {
            warn!("Stake ID {} không trong trạng thái Active, không thể hoàn thành", stake_id);
            return Err(StakingError::CannotCancelActiveStake);
        }
        
        // Gọi smart contract để unstake token với retry
        const MAX_RETRIES: u8 = 3;
        let mut last_error: Option<String> = None;
        let mut tx_hash = String::new();
        
        // Cải thiện cách xử lý RwLock
        let wallet_api = self.wallet_api.read().await.map_err(|e| {
            error!("Lỗi RwLock khi truy cập wallet API: {}", e);
            StakingError::Other(format!("Lỗi đồng bộ hóa khi truy cập WalletAPI: {}", e))
        })?;
        
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                info!("Thử lại unstake lần {}/{} cho stake ID {}", attempt, MAX_RETRIES, stake_id);
                tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
            }
            
            // Kiểm tra nếu địa chỉ staking pool hợp lệ
            if self.staking_pool_address.is_zero() {
                warn!("Địa chỉ staking pool chưa được cấu hình đúng, không thể unstake");
                return Err(StakingError::BlockchainError("Địa chỉ staking pool không hợp lệ".to_string()));
            }
            
            // Thực hiện gọi hợp đồng để unstake
            let result = if let Some(token_id) = stake.erc1155_token_id {
                debug!("Unstake với token ERC-1155 ID {}", token_id);
                
                wallet_api.call_staking_contract(
                    stake.wallet_address, 
                    self.staking_pool_address,
                    "unstakeAndBurnNft",
                    vec![
                        token_id.into(),
                        stake.amount,
                    ],
                    ChainType::EVM, // Sử dụng ChainType từ stake.chain_type trong code thực tế
                ).await
            } else {
                debug!("Unstake không có token ERC-1155");
                
                wallet_api.call_staking_contract(
                    stake.wallet_address,
                    self.staking_pool_address,
                    "unstake",
                    vec![stake.amount],
                    ChainType::EVM, // Sử dụng ChainType từ stake.chain_type trong code thực tế
                ).await
            };
            
            match result {
                Ok(hash) => {
                    info!("Unstake thành công: stake_id={}, tx_hash={}, attempt={}", stake_id, hash, attempt + 1);
                    tx_hash = hash;
                    break;
                },
                Err(e) => {
                    warn!("Lỗi khi unstake token lần {}/{}: stake_id={}, error={}", 
                         attempt + 1, MAX_RETRIES, stake_id, e);
                    last_error = Some(e.to_string());
                    
                    // Nếu đã hết số lần thử
                    if attempt == MAX_RETRIES {
                        error!("Đã hết số lần thử unstake token: stake_id={}, attempts={}", stake_id, MAX_RETRIES + 1);
                        return Err(StakingError::BlockchainError(format!(
                            "Lỗi sau {} lần thử unstake: {}", 
                            MAX_RETRIES + 1, 
                            last_error.as_deref().unwrap_or("Unknown error")
                        )));
                    }
                }
            }
        }
        
        if tx_hash.is_empty() {
            error!("Không thể unstake sau khi thử lại: stake_id={}", stake_id);
            return Err(StakingError::BlockchainError(
                format!("Không thể unstake sau {} lần thử", MAX_RETRIES + 1)
            ));
        }
        
        // Cập nhật thông tin stake
        let mut updated_stake = stake.clone();
        match updated_stake.complete(&tx_hash) {
            Ok(_) => {},
            Err(e) => {
                error!("Lỗi khi cập nhật trạng thái stake: {}", e);
                return Err(e);
            }
        }
        
        // Cập nhật vào active stakes
        {
            let mut active_stakes = self.active_stakes.write().await;
            active_stakes.remove(stake_id);
        }
        
        // Ghi log thành công
        info!("Hoàn thành stake thành công: user_id={}, stake_id={}, tx_hash={}", 
             user_id, stake_id, tx_hash);
        
        // Cập nhật vào database
        // TODO: Thực hiện cập nhật database
        // Trong môi trường thực tế, đoạn code này sẽ thực hiện cập nhật database
        
        Ok(updated_stake)
    }
    
    /// Tính toán và cộng lợi nhuận cho người dùng
    pub async fn calculate_rewards(
        &self,
        user_id: &str,
    ) -> Result<Decimal, StakingError> {
        let stakes = self.get_user_stakes(user_id).await?;
        let active_stakes = stakes.iter()
            .filter(|s| s.status == StakeStatus::Active);
            
        let mut total_rewards = Decimal::new(0, 0);
        
        for stake in active_stakes {
            let current_rewards = stake.calculate_current_rewards()?;
            total_rewards += current_rewards;
        }
        
        Ok(total_rewards)
    }
    
    /// Tính số ngày còn lại cho stake
    pub fn days_remaining(stake: &TokenStake) -> i64 {
        if stake.status != StakeStatus::Active {
            return 0;
        }
        
        let now = Utc::now();
        if now > stake.end_date {
            return 0;
        }
        
        (stake.end_date - now).num_days()
    }
} 