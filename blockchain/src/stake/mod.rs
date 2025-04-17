//! Module stake - Quản lý logic staking và farming
//!
//! Module này cung cấp các chức năng:
//! - Quản lý stake pools
//! - Tính toán và phân phối rewards
//! - Validator cho proof-of-stake
//! - Routers cho các pool staking
//! - Tương tác với smart contracts

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use async_trait::async_trait;
use ethers::types::{Address, U256};
use tracing::{info, warn, error};

mod stake_logic;
mod farm_logic;
mod constants;
mod validator;
mod rewards;
mod routers;

pub use stake_logic::StakeManager;
pub use farm_logic::FarmManager;
pub use constants::{
    MIN_LOCK_TIME, MAX_LOCK_TIME, MIN_VALIDATORS, MAX_VALIDATORS,
    MIN_CLAIM_INTERVAL, EARLY_UNSTAKE_FEE_PERCENT, MIN_BLOCKS_BETWEEN_APY_UPDATE,
    MIN_BLOCKS_BETWEEN_REWARD_DISTRIBUTION, MAX_TRANSACTION_RETRIES,
    MIN_STAKE_AMOUNT, MAX_STAKE_AMOUNT, MIN_VALIDATOR_REWARD_PERCENT,
    MAX_VALIDATOR_REWARD_PERCENT, POOL_INFO_CACHE_TIME, USER_STAKE_CACHE_TIME,
    REWARDS_CACHE_TIME, DEFAULT_TOKEN_DECIMALS, DEFAULT_APY,
    MIN_UNSTAKE_FEE_PERCENT, MAX_UNSTAKE_FEE_PERCENT, MAX_ALLOWED_PENALTIES,
    MAX_TOKENS_PER_POOL, transaction_timeout, retry_delay,
    default_staking_contract, default_token_address
};
pub use validator::Validator;
pub use rewards::RewardCalculator;
pub use routers::StakingRouter;

/// Cấu hình tokenomics cho token trong pool
#[derive(Debug, Clone)]
pub struct TokenParameters {
    /// APY cho token
    pub apy: f64,
    /// Số lượng tối thiểu để stake
    pub min_stake_amount: U256,
    /// Decimals của token
    pub token_decimals: u8,
}

/// Cấu hình cho stake pool
#[derive(Debug, Clone)]
pub struct StakePoolConfig {
    /// Địa chỉ của pool
    pub address: Address,
    /// Token được stake
    pub token_address: Address,
    /// Thời gian lock tối thiểu (giây)
    pub min_lock_time: u64,
    /// APY hiện tại
    pub current_apy: f64,
    /// Tổng số token đã stake
    pub total_staked: U256,
    /// Số validator tối đa
    pub max_validators: u32,
}

/// Thông tin stake của người dùng
#[derive(Debug, Clone)]
pub struct UserStakeInfo {
    /// Địa chỉ người dùng
    pub user_address: Address,
    /// Số lượng token đã stake
    pub staked_amount: U256,
    /// Thời gian bắt đầu stake
    pub start_time: u64,
    /// Thời gian lock
    pub lock_time: u64,
    /// Rewards đã nhận
    pub claimed_rewards: U256,
    /// Rewards đang chờ
    pub pending_rewards: U256,
}

/// Factory để tạo ra các stake manager
pub trait StakeManagerFactory: Send + Sync {
    /// Tạo stake manager mới
    fn create_stake_manager(&self) -> Arc<dyn StakePoolManager>;
    
    /// Tạo farm manager mới
    fn create_farm_manager(&self) -> Arc<dyn StakePoolManager>;
    
    /// Tạo validator mới
    fn create_validator(&self) -> Arc<dyn Validator>;
    
    /// Tạo reward calculator mới
    fn create_reward_calculator(&self) -> Arc<dyn RewardCalculator>;
    
    /// Tạo staking router mới
    fn create_staking_router(&self) -> Arc<dyn StakingRouter>;
}

/// Triển khai factory mặc định
pub struct DefaultStakeManagerFactory {
    /// Config chung
    config: Arc<dyn StakeConfig>,
}

impl DefaultStakeManagerFactory {
    /// Tạo factory mới với config mặc định
    pub fn new() -> Self {
        Self {
            config: Arc::new(DefaultStakeConfig::new()),
        }
    }
    
    /// Tạo factory với config tùy chỉnh
    pub fn with_config(config: Arc<dyn StakeConfig>) -> Self {
        Self {
            config,
        }
    }
}

impl StakeManagerFactory for DefaultStakeManagerFactory {
    fn create_stake_manager(&self) -> Arc<dyn StakePoolManager> {
        let validator = self.create_validator();
        let reward_calculator = self.create_reward_calculator();
        let router = self.create_staking_router();
        
        let cache_ttl = self.config.get_cache_ttl();
        
        Arc::new(StakeManager::with_cache_ttl(
            validator,
            reward_calculator,
            router,
            cache_ttl,
        ))
    }
    
    fn create_farm_manager(&self) -> Arc<dyn StakePoolManager> {
        // Tùy chỉnh factory để trả về farm manager thay vì stake manager
        let validator = self.create_validator();
        let reward_calculator = self.create_reward_calculator();
        let router = self.create_staking_router();
        
        Arc::new(FarmManager::new(validator, reward_calculator, router))
    }
    
    fn create_validator(&self) -> Arc<dyn Validator> {
        // Triển khai tạo validator
        Arc::new(validator::ValidatorImpl::new())
    }
    
    fn create_reward_calculator(&self) -> Arc<dyn RewardCalculator> {
        // Triển khai tạo reward calculator
        Arc::new(rewards::DefaultRewardCalculator::new())
    }
    
    fn create_staking_router(&self) -> Arc<dyn StakingRouter> {
        // Triển khai tạo staking router
        let chain_id = self.config.get_chain_id();
        let rpc_url = self.config.get_rpc_url();
        
        Arc::new(routers::EthereumStakingRouter::new(chain_id, rpc_url))
    }
}

/// Trait cho cấu hình stake
#[async_trait]
pub trait StakeConfig: Send + Sync {
    /// Lấy thời gian TTL cache
    fn get_cache_ttl(&self) -> u64;
    
    /// Lấy chain ID
    fn get_chain_id(&self) -> u64;
    
    /// Lấy RPC URL
    fn get_rpc_url(&self) -> String;
    
    /// Lấy địa chỉ ví mặc định
    fn get_default_wallet(&self) -> String;
    
    /// Lấy path file keystore
    fn get_keystore_path(&self) -> String;
    
    /// Lấy gas price
    async fn get_gas_price(&self) -> Result<U256>;
    
    /// Lấy gas limit
    fn get_gas_limit(&self) -> U256;
}

/// Config mặc định
pub struct DefaultStakeConfig {
    /// Chain ID
    chain_id: u64,
    /// RPC URL
    rpc_url: String,
    /// Thời gian TTL cache
    cache_ttl: u64,
}

impl DefaultStakeConfig {
    /// Tạo config mặc định
    pub fn new() -> Self {
        Self {
            chain_id: 1, // Ethereum mainnet
            rpc_url: "https://eth-mainnet.alchemyapi.io/v2/YOUR_API_KEY".to_string(),
            cache_ttl: 300, // 5 phút
        }
    }
    
    /// Tạo config với các tham số tùy chỉnh
    pub fn with_params(chain_id: u64, rpc_url: String, cache_ttl: u64) -> Self {
        Self {
            chain_id,
            rpc_url,
            cache_ttl,
        }
    }
}

#[async_trait]
impl StakeConfig for DefaultStakeConfig {
    fn get_cache_ttl(&self) -> u64 {
        self.cache_ttl
    }
    
    fn get_chain_id(&self) -> u64 {
        self.chain_id
    }
    
    fn get_rpc_url(&self) -> String {
        self.rpc_url.clone()
    }
    
    fn get_default_wallet(&self) -> String {
        std::env::var("STAKE_DEFAULT_WALLET")
            .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string())
    }
    
    fn get_keystore_path(&self) -> String {
        std::env::var("STAKE_KEYSTORE_PATH")
            .unwrap_or_else(|_| "./keystore".to_string())
    }
    
    async fn get_gas_price(&self) -> Result<U256> {
        // TODO: Implement gas price fetching
        Ok(U256::from(5_000_000_000u64)) // 5 Gwei
    }
    
    fn get_gas_limit(&self) -> U256 {
        U256::from(3_000_000u64)
    }
}

/// Trait cho quản lý stake pool
#[async_trait]
pub trait StakePoolManager: Send + Sync {
    /// Tạo pool mới
    async fn create_pool(&self, config: StakePoolConfig) -> Result<Address>;
    
    /// Thêm token vào pool
    async fn add_token(&self, pool_address: Address, token_address: Address, token_parameters: TokenParameters) -> Result<()>;
    
    /// Lấy thông tin pool
    async fn get_pool_info(&self, pool_address: Address) -> Result<StakePoolConfig>;
    
    /// Stake token vào pool
    async fn stake(&self, pool_address: Address, user_address: Address, amount: U256, lock_time: u64) -> Result<()>;
    
    /// Unstake token từ pool
    async fn unstake(&self, pool_address: Address, user_address: Address) -> Result<()>;
    
    /// Claim rewards
    async fn claim_rewards(&self, pool_address: Address, user_address: Address) -> Result<U256>;
    
    /// Lấy thông tin stake của người dùng
    async fn get_user_stake_info(&self, pool_address: Address, user_address: Address) -> Result<UserStakeInfo>;
}

/// Cache cho stake pools
#[derive(Debug, Default)]
pub struct StakePoolCache {
    pools: RwLock<Vec<StakePoolConfig>>,
    user_stakes: RwLock<Vec<UserStakeInfo>>,
    tokens: RwLock<std::collections::HashMap<Address, Vec<(Address, TokenParameters)>>>,
}

impl StakePoolCache {
    /// Tạo cache mới
    pub fn new() -> Self {
        Self {
            pools: RwLock::new(Vec::new()),
            user_stakes: RwLock::new(Vec::new()),
            tokens: RwLock::new(std::collections::HashMap::new()),
        }
    }
    
    /// Thêm pool vào cache
    pub async fn add_pool(&self, pool: StakePoolConfig) {
        let mut pools = self.pools.write().await;
        if let Some(index) = pools.iter().position(|p| p.address == pool.address) {
            pools[index] = pool;
        } else {
            pools.push(pool);
        }
    }
    
    /// Xóa pool khỏi cache
    pub async fn remove_pool(&self, address: Address) {
        let mut pools = self.pools.write().await;
        pools.retain(|p| p.address != address);
        
        // Xóa tokens của pool
        let mut tokens = self.tokens.write().await;
        tokens.remove(&address);
    }
    
    /// Xóa tất cả dữ liệu cache
    pub async fn clear_all(&self) {
        let mut pools = self.pools.write().await;
        pools.clear();
        
        let mut user_stakes = self.user_stakes.write().await;
        user_stakes.clear();
        
        let mut tokens = self.tokens.write().await;
        tokens.clear();
    }
    
    /// Lấy pool từ cache
    pub async fn get_pool(&self, address: Address) -> Option<StakePoolConfig> {
        let pools = self.pools.read().await;
        pools.iter().find(|p| p.address == address).cloned()
    }
    
    /// Cập nhật thông tin stake của người dùng
    pub async fn update_user_stake(&self, stake_info: UserStakeInfo) {
        let mut stakes = self.user_stakes.write().await;
        if let Some(index) = stakes.iter().position(|s| s.user_address == stake_info.user_address) {
            stakes[index] = stake_info;
        } else {
            stakes.push(stake_info);
        }
    }
    
    /// Xóa thông tin stake của người dùng
    pub async fn remove_user_stake(&self, user_address: Address) {
        let mut stakes = self.user_stakes.write().await;
        stakes.retain(|s| s.user_address != user_address);
    }
    
    /// Lấy thông tin stake của người dùng
    pub async fn get_user_stake(&self, user_address: Address) -> Option<UserStakeInfo> {
        let stakes = self.user_stakes.read().await;
        stakes.iter().find(|s| s.user_address == user_address).cloned()
    }
    
    /// Thêm token vào pool
    pub async fn add_token_to_pool(&self, pool_address: Address, token_address: Address, parameters: TokenParameters) {
        let mut tokens = self.tokens.write().await;
        let pool_tokens = tokens.entry(pool_address).or_insert_with(Vec::new);
        
        if let Some(index) = pool_tokens.iter().position(|(addr, _)| *addr == token_address) {
            pool_tokens[index] = (token_address, parameters);
        } else {
            pool_tokens.push((token_address, parameters));
        }
    }
    
    /// Kiểm tra token đã tồn tại trong pool chưa
    pub async fn is_token_in_pool(&self, pool_address: Address, token_address: Address) -> bool {
        let tokens = self.tokens.read().await;
        if let Some(pool_tokens) = tokens.get(&pool_address) {
            pool_tokens.iter().any(|(addr, _)| *addr == token_address)
        } else {
            false
        }
    }
    
    /// Lấy số lượng token trong pool
    pub async fn get_token_count(&self, pool_address: Address) -> u32 {
        let tokens = self.tokens.read().await;
        if let Some(pool_tokens) = tokens.get(&pool_address) {
            pool_tokens.len() as u32
        } else {
            0
        }
    }
}
