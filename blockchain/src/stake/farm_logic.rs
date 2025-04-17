//! Module logic farming liên quan đến staking
//!
//! Module này cung cấp các chức năng:
//! - Quản lý farm pools
//! - Tính toán farming rewards
//! - Tương tác với smart contracts

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use async_trait::async_trait;
use ethers::types::{Address, U256};
use tracing::{info, warn, error};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::stake::{
    StakePoolConfig,
    UserStakeInfo,
    StakePoolManager,
    StakePoolCache,
};
use super::constants::*;

/// Cấu hình cho farm pool
#[derive(Debug, Clone)]
pub struct FarmPoolConfig {
    /// Địa chỉ của pool
    pub address: Address,
    /// Token được farm
    pub token_address: Address,
    /// Token reward
    pub reward_token_address: Address,
    /// APY hiện tại
    pub current_apy: f64,
    /// Tổng số token đã farm
    pub total_farmed: U256,
    /// Tổng rewards đã phân phối
    pub total_rewards_distributed: U256,
    /// Thời gian bắt đầu farm
    pub start_time: u64,
    /// Thời gian kết thúc farm
    pub end_time: u64,
}

/// Thông tin farm của người dùng
#[derive(Debug, Clone)]
pub struct UserFarmInfo {
    /// Địa chỉ người dùng
    pub user_address: Address,
    /// Số lượng token đã farm
    pub farmed_amount: U256,
    /// Thời gian bắt đầu farm
    pub start_time: u64,
    /// Rewards đã nhận
    pub claimed_rewards: U256,
    /// Rewards đang chờ
    pub pending_rewards: U256,
}

/// Trait cho quản lý farm pool
#[async_trait]
pub trait FarmManager: Send + Sync {
    /// Tạo farm pool mới
    async fn create_farm_pool(&self, config: FarmPoolConfig) -> Result<Address>;
    
    /// Thêm liquidity vào farm
    async fn add_liquidity(&self, pool_address: Address, user_address: Address, amount: U256) -> Result<()>;
    
    /// Rút liquidity từ farm
    async fn remove_liquidity(&self, pool_address: Address, user_address: Address) -> Result<()>;
    
    /// Claim farming rewards
    async fn claim_farming_rewards(&self, pool_address: Address, user_address: Address) -> Result<U256>;
    
    /// Lấy thông tin farm pool
    async fn get_farm_pool_info(&self, pool_address: Address) -> Result<FarmPoolConfig>;
    
    /// Lấy thông tin farm của người dùng
    async fn get_user_farm_info(&self, pool_address: Address, user_address: Address) -> Result<UserFarmInfo>;
}

/// Implement FarmManager
pub struct FarmManagerImpl {
    /// Cache cho farm pools
    farm_pools: Arc<RwLock<Vec<FarmPoolConfig>>>,
    /// Cache cho user farm info
    user_farms: Arc<RwLock<Vec<UserFarmInfo>>>,
    /// Stake manager
    stake_manager: Arc<dyn StakePoolManager>,
}

impl FarmManagerImpl {
    /// Tạo manager mới
    pub fn new(stake_manager: Arc<dyn StakePoolManager>) -> Self {
        Self {
            farm_pools: Arc::new(RwLock::new(Vec::new())),
            user_farms: Arc::new(RwLock::new(Vec::new())),
            stake_manager,
        }
    }
    
    /// Lấy thời gian hiện tại (giây)
    fn get_current_time() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}

#[async_trait]
impl FarmManager for FarmManagerImpl {
    async fn create_farm_pool(&self, config: FarmPoolConfig) -> Result<Address> {
        // Kiểm tra điều kiện tạo pool
        if config.start_time >= config.end_time {
            return Err(anyhow::anyhow!("Invalid time range"));
        }
        
        // Lưu pool vào cache
        let mut pools = self.farm_pools.write().await;
        pools.push(config.clone());
        
        info!("Created new farm pool: {:?}", config.address);
        Ok(config.address)
    }

    async fn add_liquidity(&self, pool_address: Address, user_address: Address, amount: U256) -> Result<()> {
        // Kiểm tra điều kiện add liquidity
        if amount < MIN_STAKE_AMOUNT {
            return Err(anyhow::anyhow!("Amount too small"));
        }
        
        // Lấy thông tin pool
        let pool = self.get_farm_pool_info(pool_address).await?;
        
        // Kiểm tra thời gian farm
        let current_time = Self::get_current_time();
        if current_time < pool.start_time || current_time > pool.end_time {
            return Err(anyhow::anyhow!("Farm not active"));
        }
        
        // Tạo thông tin farm
        let farm_info = UserFarmInfo {
            user_address,
            farmed_amount: amount,
            start_time: current_time,
            claimed_rewards: U256::zero(),
            pending_rewards: U256::zero(),
        };
        
        // Lưu vào cache
        let mut farms = self.user_farms.write().await;
        farms.push(farm_info);
        
        // TODO: Implement logic add liquidity
        // 1. Gọi smart contract
        // 2. Xác thực giao dịch
        // 3. Cập nhật rewards
        
        info!("User added liquidity: {:?}, amount: {:?}", user_address, amount);
        Ok(())
    }

    async fn remove_liquidity(&self, pool_address: Address, user_address: Address) -> Result<()> {
        // Lấy thông tin farm
        let user_farms = self.user_farms.read().await;
        let farm_info = user_farms.iter()
            .find(|f| f.user_address == user_address)
            .cloned();
            
        drop(user_farms); // Giải phóng lock read trước khi dùng write lock
        
        let farm_info = if let Some(info) = farm_info {
            info
        } else {
            return Err(anyhow::anyhow!("No farm found for user"));
        };
        
        // Lấy thông tin pool
        let pool = self.get_farm_pool_info(pool_address).await?;
        
        // Tương tác với smart contract để remove liquidity
        // Trong triển khai thực tế, cần gọi smart contract để thực hiện unstake
        info!("Executing remove_liquidity transaction for user: {:?}", user_address);
        
        let transaction_result = self.execute_remove_liquidity_transaction(
            pool_address, 
            user_address, 
            farm_info.farmed_amount
        ).await;
        
        match transaction_result {
            Ok(tx_hash) => {
        info!(
                    "Remove liquidity transaction successful: txHash={:?}, user={:?}, amount={:?}", 
                    tx_hash, user_address, farm_info.farmed_amount
                );
                
                // Cập nhật state sau khi transaction thành công
                let mut user_farms = self.user_farms.write().await;
                
                // Tìm và xóa farm info
                if let Some(index) = user_farms.iter().position(|f| f.user_address == user_address) {
                    user_farms.remove(index);
                    
                    // Cập nhật tổng liquidity của pool
                    let mut pools = self.farm_pools.write().await;
                    if let Some(pool_index) = pools.iter().position(|p| p.address == pool_address) {
                        if let Some(new_total) = pools[pool_index].total_farmed.checked_sub(farm_info.farmed_amount) {
                            pools[pool_index].total_farmed = new_total;
                        }
                    }
                }
                
                Ok(())
            },
            Err(e) => {
                error!("Failed to execute remove_liquidity transaction: {:?}", e);
                Err(anyhow::anyhow!("Transaction failed: {:?}", e))
            }
        }
    }

    /// Execute transaction for removing liquidity
    async fn execute_remove_liquidity_transaction(
        &self, 
        pool_address: Address, 
        user_address: Address, 
        amount: U256
    ) -> Result<String> {
        // Triển khai thực tế - gọi contract để thực hiện unstake
        // Đây là giả lập để development
        
        // Giả lập delay của blockchain
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        // Trả về tx_hash giả
        Ok(format!("0x{:x}{:x}{:x}", pool_address, user_address, amount))
    }

    async fn claim_farming_rewards(&self, pool_address: Address, user_address: Address) -> Result<U256> {
        // Lấy thông tin farm
        let user_farms = self.user_farms.read().await;
        let farm_info = user_farms.iter()
            .find(|f| f.user_address == user_address)
            .cloned();
            
        drop(user_farms); // Giải phóng lock read
        
        let farm_info = if let Some(info) = farm_info {
            info
        } else {
            return Err(anyhow::anyhow!("No farm found for user"));
        };
        
        if farm_info.pending_rewards == U256::zero() {
            return Ok(U256::zero());
        }

        // Thực hiện giao dịch claim rewards
        info!("Executing claim_rewards transaction for user: {:?}", user_address);
        
        let rewards_amount = farm_info.pending_rewards;
        let transaction_result = self.execute_claim_rewards_transaction(
            pool_address, 
            user_address, 
            rewards_amount
        ).await;
        
        match transaction_result {
            Ok(tx_hash) => {
                info!(
                    "Claim rewards transaction successful: txHash={:?}, user={:?}, amount={:?}", 
                    tx_hash, user_address, rewards_amount
                );
                
                // Cập nhật state sau khi transaction thành công
                let mut user_farms = self.user_farms.write().await;
                
                // Tìm và cập nhật farm info
                if let Some(index) = user_farms.iter().position(|f| f.user_address == user_address) {
                    // Cập nhật rewards đã claim và đang pending
                    user_farms[index].claimed_rewards = user_farms[index].claimed_rewards
                        .checked_add(rewards_amount)
                        .unwrap_or(user_farms[index].claimed_rewards);
                    user_farms[index].pending_rewards = U256::zero();
                }
                
                // Cập nhật thông tin pool
                let mut pools = self.farm_pools.write().await;
                if let Some(pool_index) = pools.iter().position(|p| p.address == pool_address) {
                    pools[pool_index].total_rewards_distributed = pools[pool_index].total_rewards_distributed
                        .checked_add(rewards_amount)
                        .unwrap_or(pools[pool_index].total_rewards_distributed);
                }
                
                Ok(rewards_amount)
            },
            Err(e) => {
                error!("Failed to execute claim_rewards transaction: {:?}", e);
                Err(anyhow::anyhow!("Transaction failed: {:?}", e))
            }
        }
    }

    /// Execute transaction for claiming rewards
    async fn execute_claim_rewards_transaction(
        &self, 
        pool_address: Address, 
        user_address: Address, 
        amount: U256
    ) -> Result<String> {
        // Triển khai thực tế - gọi contract để thực hiện claim rewards
        // Đây là giả lập để development
        
        // Giả lập delay của blockchain
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        // Trả về tx_hash giả
        Ok(format!("0x{:x}{:x}{:x}", pool_address, user_address, amount))
    }

    async fn get_farm_pool_info(&self, pool_address: Address) -> Result<FarmPoolConfig> {
        let pools = self.farm_pools.read().await;
        if let Some(pool) = pools.iter().find(|p| p.address == pool_address) {
            Ok(pool.clone())
        } else {
            Err(anyhow::anyhow!("Pool not found"))
        }
    }

    async fn get_user_farm_info(&self, pool_address: Address, user_address: Address) -> Result<UserFarmInfo> {
        let farms = self.user_farms.read().await;
        if let Some(farm) = farms.iter().find(|f| f.user_address == user_address) {
            Ok(farm.clone())
        } else {
            Err(anyhow::anyhow!("No farm found"))
        }
    }
} 