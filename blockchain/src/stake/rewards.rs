//! Module tính toán và phân phối rewards
//!
//! Module này cung cấp các chức năng:
//! - Tính toán rewards dựa trên thời gian stake và APY
//! - Phân phối rewards cho người dùng
//! - Quản lý rewards pool

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use async_trait::async_trait;
use ethers::types::{Address, U256};
use tracing::{info, warn, error};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::stake::{StakePoolConfig, UserStakeInfo};
use super::constants::*;

/// Thông tin rewards pool
#[derive(Debug, Clone)]
pub struct RewardsPool {
    /// Địa chỉ pool
    pub pool_address: Address,
    /// Tổng rewards đã phân phối
    pub total_distributed: U256,
    /// Tổng rewards còn lại
    pub remaining_rewards: U256,
    /// Thời gian cập nhật cuối cùng
    pub last_update_time: u64,
    /// APY hiện tại
    pub current_apy: f64,
}

/// Trait cho tính toán rewards
#[async_trait]
pub trait RewardCalculator: Send + Sync {
    /// Tính toán rewards đang chờ
    async fn calculate_pending_rewards(
        &self,
        pool: &StakePoolConfig,
        user_stake: &UserStakeInfo,
    ) -> Result<U256>;

    /// Cập nhật APY cho pool
    async fn update_pool_apy(&self, pool_address: Address) -> Result<f64>;

    /// Phân phối rewards cho người dùng
    async fn distribute_rewards(&self, pool_address: Address) -> Result<()>;

    /// Lấy thông tin rewards pool
    async fn get_rewards_pool(&self, pool_address: Address) -> Result<RewardsPool>;
}

/// Implement RewardCalculator
pub struct RewardCalculatorImpl {
    /// Cache cho rewards pools
    rewards_pools: Arc<RwLock<Vec<RewardsPool>>>,
}

impl RewardCalculatorImpl {
    /// Tạo calculator mới
    pub fn new() -> Self {
        Self {
            rewards_pools: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Lấy thời gian hiện tại (giây)
    fn get_current_time() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    /// Tính toán rewards dựa trên công thức:
    /// rewards = staked_amount * (APY / 100) * (time_elapsed / 31536000)
    fn calculate_rewards(
        staked_amount: U256,
        apy: f64,
        start_time: u64,
        current_time: u64,
    ) -> U256 {
        let time_elapsed = current_time - start_time;
        let apy_decimal = apy / 100.0;
        let time_factor = time_elapsed as f64 / 31536000.0; // 1 năm = 31536000 giây
        
        // Chuyển đổi sang U256 và tính toán
        let rewards = staked_amount
            .checked_mul(U256::from((apy_decimal * time_factor * 1e18) as u128))
            .unwrap_or(U256::zero())
            .checked_div(U256::from(1e18))
            .unwrap_or(U256::zero());
            
        rewards
    }
}

#[async_trait]
impl RewardCalculator for RewardCalculatorImpl {
    async fn calculate_pending_rewards(
        &self,
        pool: &StakePoolConfig,
        user_stake: &UserStakeInfo,
    ) -> Result<U256> {
        let current_time = Self::get_current_time();
        
        // Kiểm tra thời gian lock
        if current_time < user_stake.start_time + user_stake.lock_time {
            return Ok(U256::zero());
        }
        
        // Tính toán rewards
        let rewards = Self::calculate_rewards(
            user_stake.staked_amount,
            pool.current_apy,
            user_stake.start_time,
            current_time,
        );
        
        // Trừ đi rewards đã claim
        let pending_rewards = rewards
            .checked_sub(user_stake.claimed_rewards)
            .unwrap_or(U256::zero());
            
        Ok(pending_rewards)
    }

    async fn update_pool_apy(&self, pool_address: Address) -> Result<f64> {
        // TODO: Implement logic cập nhật APY dựa trên:
        // - Tổng số token đã stake
        // - Tổng rewards còn lại
        // - Thời gian lock trung bình
        // - Số lượng validator
        Ok(DEFAULT_APY)
    }

    async fn distribute_rewards(&self, pool_address: Address) -> Result<()> {
        // TODO: Implement logic phân phối rewards:
        // 1. Lấy danh sách người dùng đang stake
        // 2. Tính toán rewards cho từng người dùng
        // 3. Cập nhật rewards pool
        // 4. Gửi rewards cho người dùng
        Ok(())
    }

    async fn get_rewards_pool(&self, pool_address: Address) -> Result<RewardsPool> {
        let pools = self.rewards_pools.read().await;
        if let Some(pool) = pools.iter().find(|p| p.pool_address == pool_address) {
            return Ok(pool.clone());
        }
        
        // Nếu không tìm thấy, tạo pool mới
        Ok(RewardsPool {
            pool_address,
            total_distributed: U256::zero(),
            remaining_rewards: U256::zero(),
            last_update_time: Self::get_current_time(),
            current_apy: DEFAULT_APY,
        })
    }
}
