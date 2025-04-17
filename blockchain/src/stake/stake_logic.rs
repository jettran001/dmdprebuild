//! Module logic staking chính
//!
//! Module này cung cấp các chức năng:
//! - Quản lý stake pools
//! - Xử lý stake/unstake
//! - Tính toán rewards
//! - Tương tác với smart contracts

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use async_trait::async_trait;
use ethers::types::{Address, U256};
use tracing::{info, warn, error, debug, trace};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

use crate::stake::{
    StakePoolConfig,
    UserStakeInfo,
    StakePoolManager,
    StakePoolCache,
    TokenParameters,
};
use super::constants::*;

/// Định nghĩa các loại lỗi của stake manager
#[derive(Error, Debug)]
pub enum StakeError {
    /// Lỗi khi địa chỉ không hợp lệ
    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    /// Lỗi khi pool không tồn tại
    #[error("Pool not found: {0}")]
    PoolNotFound(Address),

    /// Lỗi khi token không hợp lệ
    #[error("Invalid token: {0}")]
    InvalidToken(Address),

    /// Lỗi khi token đã tồn tại trong pool
    #[error("Token already exists in pool: {0}")]
    TokenAlreadyExists(Address),

    /// Lỗi khi đạt giới hạn token trong pool
    #[error("Maximum number of tokens reached for pool: {0}")]
    MaxTokensReached(Address),

    /// Lỗi khi thời gian lock quá ngắn
    #[error("Lock time too short: {0} < {1}")]
    LockTimeTooShort(u64, u64),

    /// Lỗi khi số validator không đủ
    #[error("Too few validators: {0} < {1}")]
    TooFewValidators(u32, u32),

    /// Lỗi khi dữ liệu pool không hợp lệ
    #[error("Invalid pool data: {0}")]
    InvalidPoolData(String),

    /// Lỗi khi chưa tìm thấy dữ liệu stake
    #[error("No stake found for user: {0}")]
    NoStakeFound(Address),

    /// Lỗi khi thời gian lock chưa hết
    #[error("Lock time not expired: {0} < {1}")]
    LockTimeNotExpired(u64, u64),

    /// Lỗi khi số lượng stake quá thấp
    #[error("Stake amount too small: {0} < {1}")]
    StakeAmountTooSmall(U256, U256),

    /// Lỗi khi transaction thất bại
    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    /// Lỗi khi không thể lấy thông tin từ blockchain
    #[error("Failed to fetch data from blockchain: {0}")]
    BlockchainError(String),

    /// Lỗi khi thời gian không hợp lệ
    #[error("Invalid time: {0}")]
    InvalidTime(String),
    
    /// Lỗi khác
    #[error("Other error: {0}")]
    Other(String),
}

/// Chuyển đổi từ anyhow::Error sang StakeError
impl From<anyhow::Error> for StakeError {
    fn from(err: anyhow::Error) -> Self {
        StakeError::Other(err.to_string())
    }
}

/// Chuyển đổi từ StakeError sang anyhow::Error
impl From<StakeError> for anyhow::Error {
    fn from(err: StakeError) -> Self {
        anyhow::anyhow!("{}", err)
    }
}

/// Implement StakePoolManager
pub struct StakeManager {
    /// Cache cho stake pools
    cache: Arc<StakePoolCache>,
    /// Validator
    validator: Arc<dyn crate::stake::Validator>,
    /// Reward calculator
    reward_calculator: Arc<dyn crate::stake::RewardCalculator>,
    /// Staking router
    router: Arc<dyn crate::stake::StakingRouter>,
    /// Cache hết hạn (giây)
    cache_ttl: u64,
    /// Thời gian cập nhật cache cuối cùng
    last_cache_update: RwLock<std::collections::HashMap<Address, u64>>,
}

impl StakeManager {
    /// Tạo manager mới
    pub fn new(
        validator: Arc<dyn crate::stake::Validator>,
        reward_calculator: Arc<dyn crate::stake::RewardCalculator>,
        router: Arc<dyn crate::stake::StakingRouter>,
    ) -> Self {
        Self {
            cache: Arc::new(StakePoolCache::new()),
            validator,
            reward_calculator,
            router,
            cache_ttl: 300, // 5 phút mặc định
            last_cache_update: RwLock::new(std::collections::HashMap::new()),
        }
    }
    
    /// Tạo manager với TTL tùy chỉnh
    pub fn with_cache_ttl(
        validator: Arc<dyn crate::stake::Validator>,
        reward_calculator: Arc<dyn crate::stake::RewardCalculator>,
        router: Arc<dyn crate::stake::StakingRouter>,
        cache_ttl: u64,
    ) -> Self {
        Self {
            cache: Arc::new(StakePoolCache::new()),
            validator,
            reward_calculator,
            router,
            cache_ttl,
            last_cache_update: RwLock::new(std::collections::HashMap::new()),
        }
    }
    
    /// Lấy thời gian hiện tại (giây)
    fn get_current_time() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
    
    /// Kiểm tra và cập nhật cache nếu cần
    async fn refresh_cache_if_needed(&self, pool_address: Address) -> Result<bool, StakeError> {
        // Kiểm tra địa chỉ pool hợp lệ
        if pool_address == Address::zero() {
            return Err(StakeError::InvalidAddress("zero address".to_string()));
        }

        // Validate thời gian hiện tại
        let current_time = Self::get_current_time();
        if current_time == 0 {
            return Err(StakeError::InvalidTime("current time is zero".to_string()));
        }

        let needs_refresh = {
            let last_updates = self.last_cache_update.read().await;
            match last_updates.get(&pool_address) {
                Some(last_update) => {
                    // Kiểm tra thời gian hợp lệ
                    if *last_update > current_time {
                        warn!("Last update time is in the future for pool: {:?}", pool_address);
                        true
                    } else {
                        current_time - last_update > self.cache_ttl
                    }
                },
                None => true, // Chưa có trong cache, cần refresh
            }
        };
        
        if needs_refresh {
            debug!("Refreshing cache for pool: {:?}", pool_address);
            
            // Thực hiện cập nhật dữ liệu từ blockchain
            let pool_data = match self.router.get_pool_config(pool_address).await {
                Ok(data) => {
                    // Validate dữ liệu từ blockchain trước khi cập nhật vào cache
                    if data.min_lock_time < *MIN_LOCK_TIME {
                        warn!("Received invalid min_lock_time from blockchain for pool: {:?}", pool_address);
                        return Err(StakeError::InvalidPoolData(format!("min_lock_time too low: {} < {}", data.min_lock_time, *MIN_LOCK_TIME)));
                    }
                    
                    if data.max_validators < *MIN_VALIDATORS || data.max_validators > *MAX_VALIDATORS {
                        warn!("Received invalid max_validators from blockchain for pool: {:?}", pool_address);
                        return Err(StakeError::InvalidPoolData(format!("max_validators out of range: {} not in range [{}, {}]", 
                            data.max_validators, *MIN_VALIDATORS, *MAX_VALIDATORS)));
                    }
                    
                    // Cập nhật cache với dữ liệu đã validate
                    self.cache.add_pool(data).await;
                    
                    true
                },
                Err(e) => {
                    warn!("Failed to fetch pool data from blockchain: {:?}", e);
                    return Err(StakeError::BlockchainError(e.to_string()));
                }
            };
            
            // Chỉ cập nhật timestamp nếu lấy dữ liệu thành công
            if pool_data {
                let mut last_updates = self.last_cache_update.write().await;
                last_updates.insert(pool_address, current_time);
                info!("Cache updated for pool: {:?}", pool_address);
            }
            
            Ok(true)
        } else {
            trace!("Using cached data for pool: {:?}", pool_address);
            Ok(false)
        }
    }
    
    /// Xóa cache cho pool cụ thể
    pub async fn invalidate_cache(&self, pool_address: Address) {
        let mut last_updates = self.last_cache_update.write().await;
        last_updates.remove(&pool_address);
        self.cache.remove_pool(pool_address).await;
    }
    
    /// Xóa toàn bộ cache 
    pub async fn clear_all_cache(&self) {
        let mut last_updates = self.last_cache_update.write().await;
        last_updates.clear();
        self.cache.clear_all().await;
    }
}

#[async_trait]
impl StakePoolManager for StakeManager {
    async fn create_pool(&self, config: StakePoolConfig) -> Result<Address> {
        // Validate địa chỉ
        if config.address == Address::zero() {
            return Err(anyhow::anyhow!("Invalid pool address: zero address"));
        }
        
        if config.token_address == Address::zero() {
            return Err(anyhow::anyhow!("Invalid token address: zero address"));
        }
        
        // Validate địa chỉ token hợp lệ
        if !self.validator.is_valid_token(config.token_address).await? {
            return Err(anyhow::anyhow!("Invalid token contract at address: {:?}", config.token_address));
        }
        
        // Kiểm tra xem pool đã tồn tại chưa
        if let Some(_) = self.cache.get_pool(config.address).await {
            return Err(anyhow::anyhow!("Pool already exists with address: {:?}", config.address));
        }
        
        // Kiểm tra điều kiện tạo pool
        if config.min_lock_time < *MIN_LOCK_TIME {
            return Err(anyhow::anyhow!("Lock time too short, minimum required: {}", *MIN_LOCK_TIME));
        }
        
        if config.max_validators < *MIN_VALIDATORS {
            return Err(anyhow::anyhow!("Too few validators, minimum required: {}", *MIN_VALIDATORS));
        }
        
        // Gửi giao dịch tạo pool đến blockchain
        match self.router.create_pool(config.clone()).await {
            Ok(tx_hash) => {
                info!("Pool creation transaction sent: {:?}", tx_hash);
                
                // Lưu pool vào cache
                self.cache.add_pool(config.clone()).await;
                
                // Cập nhật timestamp cache
                let mut last_updates = self.last_cache_update.write().await;
                last_updates.insert(config.address, Self::get_current_time());
                
                info!("Created new stake pool: {:?}", config.address);
                Ok(config.address)
            },
            Err(e) => {
                error!("Failed to create pool on blockchain: {:?}", e);
                Err(anyhow::anyhow!("Failed to create pool: {}", e))
            }
        }
    }

    async fn add_token(&self, pool_address: Address, token_address: Address, token_parameters: TokenParameters) -> Result<()> {
        // Validate địa chỉ
        if pool_address == Address::zero() {
            return Err(anyhow::anyhow!("Invalid pool address: zero address"));
        }
        
        if token_address == Address::zero() {
            return Err(anyhow::anyhow!("Invalid token address: zero address"));
        }
        
        // Validate tham số token
        if token_parameters.apy > 1000 {
            return Err(anyhow::anyhow!("Invalid APY: too high (>1000%)"));
        }
        
        if token_parameters.min_stake_amount == U256::zero() {
            return Err(anyhow::anyhow!("Invalid minimum stake amount: cannot be zero"));
        }
        
        if token_parameters.token_decimals > 18 {
            return Err(anyhow::anyhow!("Invalid token decimals: exceeds 18"));
        }
        
        // Cập nhật cache nếu cần
        self.refresh_cache_if_needed(pool_address).await?;
        
        // Kiểm tra xem token đã tồn tại trong pool chưa
        let is_token_in_pool = self.cache.is_token_in_pool(pool_address, token_address).await;
        if is_token_in_pool {
            return Err(anyhow::anyhow!("Token is already in the pool"));
        }
        
        // Kiểm tra số lượng token trong pool đã đạt giới hạn chưa
        let token_count = self.cache.get_token_count(pool_address).await;
        if token_count >= MAX_TOKENS_PER_POOL {
            return Err(anyhow::anyhow!("Maximum number of tokens reached for this pool"));
        }
        
        // Gửi transaction để thêm token vào pool
        let tx_result = self.router.add_token_to_pool(pool_address, token_address, token_parameters.clone()).await;
        match tx_result {
            Ok(tx_hash) => {
                info!("Token added to pool. Transaction hash: {:?}", tx_hash);
                
                // Cập nhật cache
                self.cache.add_token_to_pool(pool_address, token_address, token_parameters).await;
                
                // Log thông tin thành công
                info!("Successfully added token {:?} to pool {:?}", token_address, pool_address);
                
                Ok(())
            },
            Err(e) => {
                error!("Failed to add token to pool: {:?}", e);
                Err(anyhow::anyhow!("Failed to add token to pool: {}", e))
            }
        }
    }

    async fn get_pool_info(&self, pool_address: Address) -> Result<StakePoolConfig> {
        // Kiểm tra và cập nhật cache nếu cần
        let cache_refreshed = self.refresh_cache_if_needed(pool_address).await?;
        
        // Lấy từ cache
        if let Some(pool) = self.cache.get_pool(pool_address).await {
            return Ok(pool);
        }
        
        // Nếu không có trong cache và đã làm mới cache, pool không tồn tại
        if cache_refreshed {
            return Err(anyhow::anyhow!("Pool not found"));
        }
        
        // Thử lấy từ blockchain nếu chưa làm mới cache
        // TODO: Implement gọi blockchain để lấy thông tin pool
        // Ví dụ:
        // let pool_config = self.router.get_pool_config(pool_address).await?;
        // self.cache.add_pool(pool_config.clone()).await;
        
        Err(anyhow::anyhow!("Pool not found"))
    }

    async fn stake(&self, pool_address: Address, user_address: Address, amount: U256, lock_time: u64) -> Result<()> {
        // Kiểm tra điều kiện stake
        if amount < MIN_STAKE_AMOUNT {
            return Err(anyhow::anyhow!("Stake amount too small"));
        }
        
        if lock_time < MIN_LOCK_TIME || lock_time > MAX_LOCK_TIME {
            return Err(anyhow::anyhow!("Invalid lock time"));
        }
        
        // Lấy thông tin pool
        let pool = self.get_pool_info(pool_address).await?;
        
        // Tạo thông tin stake
        let stake_info = UserStakeInfo {
            user_address,
            staked_amount: amount,
            start_time: Self::get_current_time(),
            lock_time,
            claimed_rewards: U256::zero(),
            pending_rewards: U256::zero(),
        };
        
        // Lưu vào cache
        self.cache.update_user_stake(stake_info).await;
        
        // TODO: Implement logic stake
        // 1. Gọi smart contract
        // 2. Xác thực giao dịch
        // 3. Cập nhật rewards
        
        info!("User staked: {:?}, amount: {:?}", user_address, amount);
        Ok(())
    }

    async fn unstake(&self, pool_address: Address, user_address: Address) -> Result<()> {
        // Refresh cache để đảm bảo dữ liệu mới nhất
        self.refresh_cache_if_needed(pool_address).await?;
        
        // Lấy thông tin stake
        let stake_info = if let Some(info) = self.cache.get_user_stake(user_address).await {
            info
        } else {
            return Err(anyhow::anyhow!("No stake found"));
        };
        
        // Kiểm tra thời gian lock
        let current_time = Self::get_current_time();
        if current_time < stake_info.start_time + stake_info.lock_time {
            return Err(anyhow::anyhow!("Lock time not expired"));
        }
        
        // TODO: Implement logic unstake
        // 1. Gọi smart contract
        // 2. Xác thực giao dịch
        // 3. Cập nhật cache
        
        // Xóa stake khỏi cache
        self.cache.remove_user_stake(user_address).await;
        
        info!("User unstaked: {:?}", user_address);
        Ok(())
    }

    async fn claim_rewards(&self, pool_address: Address, user_address: Address) -> Result<U256> {
        // Refresh cache để đảm bảo dữ liệu mới nhất
        self.refresh_cache_if_needed(pool_address).await?;
        
        // Lấy thông tin stake
        let stake_info = if let Some(info) = self.cache.get_user_stake(user_address).await {
            info
        } else {
            return Err(anyhow::anyhow!("No stake found"));
        };
        
        // Tính toán rewards
        let pool = self.get_pool_info(pool_address).await?;
        let rewards = self.reward_calculator
            .calculate_pending_rewards(&pool, &stake_info)
            .await?;
        
        if rewards == U256::zero() {
            return Ok(U256::zero());
        }
        
        // TODO: Implement logic claim rewards
        // 1. Gọi smart contract
        // 2. Xác thực giao dịch
        
        // Cập nhật cache với rewards đã claim
        let mut updated_stake_info = stake_info.clone();
        updated_stake_info.claimed_rewards = updated_stake_info.claimed_rewards.checked_add(rewards).unwrap_or(updated_stake_info.claimed_rewards);
        updated_stake_info.pending_rewards = U256::zero();
        self.cache.update_user_stake(updated_stake_info).await;
        
        info!("User claimed rewards: {:?}, amount: {:?}", user_address, rewards);
        Ok(rewards)
    }

    async fn get_user_stake_info(&self, pool_address: Address, user_address: Address) -> Result<UserStakeInfo> {
        // Refresh cache nếu cần
        self.refresh_cache_if_needed(pool_address).await?;
        
        if let Some(info) = self.cache.get_user_stake(user_address).await {
            // Cập nhật pending rewards trong cache
            let pool = self.get_pool_info(pool_address).await?;
            let pending_rewards = self.reward_calculator
                .calculate_pending_rewards(&pool, &info)
                .await?;
                
            if pending_rewards != info.pending_rewards {
                let mut updated_info = info.clone();
                updated_info.pending_rewards = pending_rewards;
                self.cache.update_user_stake(updated_info.clone()).await;
                return Ok(updated_info);
            }
            
            Ok(info)
        } else {
            Err(anyhow::anyhow!("No stake found"))
        }
    }
}
