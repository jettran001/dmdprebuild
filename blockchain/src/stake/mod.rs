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
pub use constants::StakeConstants;
pub use validator::Validator;
pub use rewards::RewardCalculator;
pub use routers::StakingRouter;

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

/// Trait cho quản lý stake pool
#[async_trait]
pub trait StakePoolManager: Send + Sync {
    /// Tạo pool mới
    async fn create_pool(&self, config: StakePoolConfig) -> Result<Address>;
    
    /// Thêm token vào pool
    async fn add_token(&self, pool_address: Address, token_address: Address) -> Result<()>;
    
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
}

impl StakePoolCache {
    /// Tạo cache mới
    pub fn new() -> Self {
        Self {
            pools: RwLock::new(Vec::new()),
            user_stakes: RwLock::new(Vec::new()),
        }
    }
    
    /// Thêm pool vào cache
    pub async fn add_pool(&self, pool: StakePoolConfig) {
        let mut pools = self.pools.write().await;
        pools.push(pool);
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
    
    /// Lấy thông tin stake của người dùng
    pub async fn get_user_stake(&self, user_address: Address) -> Option<UserStakeInfo> {
        let stakes = self.user_stakes.read().await;
        stakes.iter().find(|s| s.user_address == user_address).cloned()
    }
}
