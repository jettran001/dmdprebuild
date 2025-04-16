//! Module quản lý farming và liquidity pools trong DeFi
//! 
//! Module này có chức năng:
//! - Quản lý farming pools 
//! - Thêm/rút liquidity
//! - Harvest rewards
//! - Tính toán APY và rewards
//! 
//! ## Ví dụ:
//! ```
//! use blockchain::stake::farm_logic::{FarmManager, FarmPoolConfig};
//! use ethers::types::{Address, U256};
//! use rust_decimal::Decimal;
//! 
//! #[tokio::main]
//! async fn main() {
//!     // Tạo farm manager
//!     let manager = FarmManager::new();
//!     
//!     // Thêm pool mới
//!     let config = FarmPoolConfig {
//!         pool_address: Address::zero(),
//!         lp_token_address: Address::zero(),
//!         reward_token_address: Address::zero(),
//!         reward_per_block: U256::from(100),
//!         apy_estimate: Decimal::from(20),
//!     };
//!     manager.add_pool(config).await.unwrap();
//!     
//!     // Thêm liquidity
//!     let user_id = "user1";
//!     let pool_address = Address::zero();
//!     let amount = U256::from(1000);
//!     manager.add_liquidity(user_id, pool_address, amount).await.unwrap();
//!     
//!     // Harvest rewards
//!     manager.harvest(user_id, pool_address).await.unwrap();
//! }
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug};
use ethers::types::{Address, U256};
use ethers::prelude::{Provider, Http, Contract};
use rust_decimal::Decimal;
use anyhow::Result;
use prometheus::{register_counter, register_gauge, Counter, Gauge};
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use tokio::time::{timeout, Duration};
use thiserror::Error;

use crate::smartcontracts::dmd_token::DMDToken;
use crate::smartcontracts::base_contract::BaseContract;
use crate::smartcontracts::bsc_contract::BscContract;

/// Error type cho farm operations
#[derive(Debug, Error)]
pub enum FarmingError {
    #[error("Pool not found: {0}")]
    PoolNotFound(Address),
    
    #[error("User farm not found: {0}")]
    UserFarmNotFound(String),
    
    #[error("Insufficient token balance: required {0}, found {1}")]
    InsufficientBalance(U256, U256),
    
    #[error("Insufficient liquidity: required {0}, found {1}")]
    InsufficientLiquidity(U256, U256),
    
    #[error("No rewards to harvest")]
    NoRewardsToHarvest,
    
    #[error("Blockchain error: {0}")]
    BlockchainError(String),
    
    #[error("Contract error: {0}")]
    ContractError(String),
    
    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Cấu hình cho farm pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FarmPoolConfig {
    /// Địa chỉ của pool
    pub pool_address: Address,
    /// Địa chỉ của LP token
    pub lp_token_address: Address,
    /// Địa chỉ token rewards
    pub reward_token_address: Address,
    /// Rewards cho mỗi block
    pub reward_per_block: U256,
    /// Ước tính APY
    pub apy_estimate: Decimal,
}

/// Thông tin farming của user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFarmInfo {
    /// ID của user
    pub user_id: String,
    /// Địa chỉ pool
    pub pool_address: Address,
    /// Địa chỉ ví của user
    pub wallet_address: Address,
    /// Số lượng LP token đã thêm
    pub liquidity: U256,
    /// Thời điểm bắt đầu farming
    pub start_time: DateTime<Utc>,
    /// Rewards chưa harvested
    pub pending_rewards: U256,
    /// Thời gian harvest gần nhất
    pub last_harvest_time: DateTime<Utc>,
}

/// Manager quản lý farm pools và user farms
pub struct FarmManager {
    pools: Arc<RwLock<HashMap<Address, FarmPoolConfig>>>,
    user_farms: Arc<RwLock<HashMap<String, HashMap<Address, UserFarmInfo>>>>,
    total_liquidity_counter: Arc<RwLock<HashMap<Address, U256>>>,
    liquidity_gauge: Gauge,
    farms_count_gauge: Gauge,
    rewards_harvested_counter: Counter,
}

impl FarmManager {
    /// Tạo một FarmManager mới
    pub fn new() -> Self {
        let liquidity_gauge = register_gauge!("blockchain_farm_liquidity", "Total liquidity in farms").unwrap();
        let farms_count_gauge = register_gauge!("blockchain_farm_count", "Number of active farms").unwrap();
        let rewards_harvested_counter = register_counter!("blockchain_farm_rewards_harvested", "Total rewards harvested").unwrap();

        Self {
            pools: Arc::new(RwLock::new(HashMap::new())),
            user_farms: Arc::new(RwLock::new(HashMap::new())),
            total_liquidity_counter: Arc::new(RwLock::new(HashMap::new())),
            liquidity_gauge,
            farms_count_gauge,
            rewards_harvested_counter,
        }
    }

    /// Thêm pool mới
    ///
    /// # Arguments
    /// * `config` - Cấu hình của pool
    ///
    /// # Returns
    /// * `Result<(), FarmingError>` - Kết quả thành công hoặc lỗi
    pub async fn add_pool(&self, config: FarmPoolConfig) -> Result<(), FarmingError> {
        let mut pools = self.pools.write().await;
        pools.insert(config.pool_address, config);
        
        // Khởi tạo total liquidity counter cho pool
        let mut total_liquidity = self.total_liquidity_counter.write().await;
        total_liquidity.insert(config.pool_address, U256::zero());

        info!(
            pool_address = %config.pool_address,
            lp_token = %config.lp_token_address,
            reward_token = %config.reward_token_address,
            apy = %config.apy_estimate,
            "Thêm farm pool mới"
        );
        
        Ok(())
    }

    /// Thêm liquidity vào pool
    ///
    /// # Arguments
    /// * `user_id` - ID của user
    /// * `pool_address` - Địa chỉ của pool
    /// * `amount` - Số lượng LP token muốn thêm
    ///
    /// # Returns
    /// * `Result<(), FarmingError>` - Kết quả thành công hoặc lỗi
    pub async fn add_liquidity(&self, user_id: &str, pool_address: Address, amount: U256) -> Result<(), FarmingError> {
        // Kiểm tra pool tồn tại
        let pools = self.pools.read().await;
        let pool = pools.get(&pool_address).ok_or(FarmingError::PoolNotFound(pool_address))?;
        
        // Thực hiện add liquidity trên blockchain (trên thực tế sẽ gọi smart contract)
        // Mô phỏng việc gọi smart contract
        debug!(
            user_id = %user_id,
            pool_address = %pool_address,
            amount = %amount,
            "Thêm liquidity"
        );

        // Cập nhật tổng liquidity
        {
            let mut total_liquidity = self.total_liquidity_counter.write().await;
            let current = total_liquidity.get(&pool_address).unwrap_or(&U256::zero());
            total_liquidity.insert(pool_address, *current + amount);
        }

        // Cập nhật thông tin farming của user
        let now = Utc::now();
        {
            let mut user_farms = self.user_farms.write().await;
            let user_farms_map = user_farms.entry(user_id.to_string()).or_insert_with(HashMap::new);
            
            let farm_info = user_farms_map.entry(pool_address).or_insert_with(|| UserFarmInfo {
                user_id: user_id.to_string(),
                pool_address,
                wallet_address: Address::zero(), // Trong ứng dụng thực tế, lấy từ input hoặc lookup
                liquidity: U256::zero(),
                start_time: now,
                pending_rewards: U256::zero(),
                last_harvest_time: now,
            });
            
            // Cập nhật rewards trước khi thêm liquidity mới
            farm_info.pending_rewards += self.calculate_rewards(farm_info, pool).await?;
            farm_info.liquidity += amount;
            farm_info.last_harvest_time = now;
        }

        // Cập nhật metrics
        self.liquidity_gauge.add(amount.as_u64() as f64);
        self.farms_count_gauge.inc();

        info!(
            user_id = %user_id,
            pool_address = %pool_address,
            amount = %amount,
            "Đã thêm liquidity thành công"
        );

        Ok(())
    }

    /// Rút liquidity từ pool
    ///
    /// # Arguments
    /// * `user_id` - ID của user
    /// * `pool_address` - Địa chỉ của pool
    /// * `amount` - Số lượng LP token muốn rút, nếu None thì rút hết
    ///
    /// # Returns
    /// * `Result<(), FarmingError>` - Kết quả thành công hoặc lỗi
    pub async fn remove_liquidity(&self, user_id: &str, pool_address: Address, amount: Option<U256>) -> Result<(), FarmingError> {
        // Lấy thông tin farm
        let mut user_farms = self.user_farms.write().await;
        let user_farms_map = user_farms.get_mut(user_id).ok_or(FarmingError::UserFarmNotFound(user_id.to_string()))?;
        let farm_info = user_farms_map.get_mut(&pool_address).ok_or(FarmingError::UserFarmNotFound(user_id.to_string()))?;

        // Kiểm tra liquidity
        let remove_amount = amount.unwrap_or(farm_info.liquidity);
        if remove_amount > farm_info.liquidity {
            return Err(FarmingError::InsufficientLiquidity(remove_amount, farm_info.liquidity));
        }

        // Thực hiện remove liquidity trên blockchain
        debug!(
            user_id = %user_id,
            pool_address = %pool_address,
            amount = %remove_amount,
            "Removing liquidity"
        );

        // Harvest rewards trước khi rút liquidity
        let pools = self.pools.read().await;
        let pool = pools.get(&pool_address).ok_or(FarmingError::PoolNotFound(pool_address))?;
        let rewards = self.calculate_rewards(farm_info, pool).await?;
        farm_info.pending_rewards += rewards;
        
        // Cập nhật thông tin farm
        farm_info.liquidity -= remove_amount;
        let is_empty = farm_info.liquidity.is_zero();
        
        // Cập nhật tổng liquidity
        {
            let mut total_liquidity = self.total_liquidity_counter.write().await;
            let current = total_liquidity.get(&pool_address).unwrap_or(&U256::zero());
            let new_total = current.saturating_sub(remove_amount);
            total_liquidity.insert(pool_address, new_total);
        }

        // Cập nhật metrics
        self.liquidity_gauge.sub(remove_amount.as_u64() as f64);
        
        // Nếu user đã rút hết, xóa farm
        if is_empty {
            user_farms_map.remove(&pool_address);
            if user_farms_map.is_empty() {
                user_farms.remove(user_id);
            }
            self.farms_count_gauge.dec();
        }

        info!(
            user_id = %user_id,
            pool_address = %pool_address,
            amount = %remove_amount,
            "Đã rút liquidity thành công"
        );

        Ok(())
    }

    /// Harvest rewards từ farm
    ///
    /// # Arguments
    /// * `user_id` - ID của user
    /// * `pool_address` - Địa chỉ của pool
    ///
    /// # Returns
    /// * `Result<U256, FarmingError>` - Số lượng rewards đã harvest hoặc lỗi
    pub async fn harvest(&self, user_id: &str, pool_address: Address) -> Result<U256, FarmingError> {
        // Lấy thông tin farm
        let mut user_farms = self.user_farms.write().await;
        let user_farms_map = user_farms.get_mut(user_id).ok_or(FarmingError::UserFarmNotFound(user_id.to_string()))?;
        let farm_info = user_farms_map.get_mut(&pool_address).ok_or(FarmingError::UserFarmNotFound(user_id.to_string()))?;

        // Tính toán rewards
        let pools = self.pools.read().await;
        let pool = pools.get(&pool_address).ok_or(FarmingError::PoolNotFound(pool_address))?;
        
        let rewards = self.calculate_rewards(farm_info, pool).await?;
        let total_rewards = farm_info.pending_rewards + rewards;
        
        if total_rewards.is_zero() {
            return Err(FarmingError::NoRewardsToHarvest);
        }

        // Thực hiện harvest trên blockchain
        debug!(
            user_id = %user_id,
            pool_address = %pool_address,
            rewards = %total_rewards,
            "Harvesting rewards"
        );

        // Reset pending rewards
        farm_info.pending_rewards = U256::zero();
        farm_info.last_harvest_time = Utc::now();

        // Cập nhật metrics
        self.rewards_harvested_counter.inc_by(total_rewards.as_u64());

        info!(
            user_id = %user_id,
            pool_address = %pool_address,
            rewards = %total_rewards,
            "Đã harvest rewards thành công"
        );

        Ok(total_rewards)
    }

    /// Lấy thông tin pool
    ///
    /// # Arguments
    /// * `pool_address` - Địa chỉ của pool
    ///
    /// # Returns
    /// * `Result<FarmPoolConfig, FarmingError>` - Thông tin pool hoặc lỗi
    pub async fn get_pool(&self, pool_address: Address) -> Result<FarmPoolConfig, FarmingError> {
        let pools = self.pools.read().await;
        let pool = pools.get(&pool_address).ok_or(FarmingError::PoolNotFound(pool_address))?;
        Ok(pool.clone())
    }

    /// Lấy thông tin farm của user
    ///
    /// # Arguments
    /// * `user_id` - ID của user
    /// * `pool_address` - Địa chỉ của pool
    ///
    /// # Returns
    /// * `Result<UserFarmInfo, FarmingError>` - Thông tin farm của user hoặc lỗi
    pub async fn get_user_farm(&self, user_id: &str, pool_address: Address) -> Result<UserFarmInfo, FarmingError> {
        let user_farms = self.user_farms.read().await;
        let user_farms_map = user_farms.get(user_id).ok_or(FarmingError::UserFarmNotFound(user_id.to_string()))?;
        let farm_info = user_farms_map.get(&pool_address).ok_or(FarmingError::UserFarmNotFound(user_id.to_string()))?;
        Ok(farm_info.clone())
    }

    /// Lấy danh sách tất cả các pools
    ///
    /// # Returns
    /// * `Result<Vec<FarmPoolConfig>, FarmingError>` - Danh sách pools hoặc lỗi
    pub async fn get_all_pools(&self) -> Result<Vec<FarmPoolConfig>, FarmingError> {
        let pools = self.pools.read().await;
        let pool_configs = pools.values().cloned().collect();
        Ok(pool_configs)
    }

    /// Lấy danh sách tất cả các farms của user
    ///
    /// # Arguments
    /// * `user_id` - ID của user
    ///
    /// # Returns
    /// * `Result<Vec<UserFarmInfo>, FarmingError>` - Danh sách farms của user hoặc lỗi
    pub async fn get_user_farms(&self, user_id: &str) -> Result<Vec<UserFarmInfo>, FarmingError> {
        let user_farms = self.user_farms.read().await;
        let user_farms_map = user_farms.get(user_id).ok_or(FarmingError::UserFarmNotFound(user_id.to_string()))?;
        let farms = user_farms_map.values().cloned().collect();
        Ok(farms)
    }

    /// Cập nhật APY cho pool
    ///
    /// # Arguments
    /// * `pool_address` - Địa chỉ của pool
    /// * `new_apy` - APY mới
    ///
    /// # Returns
    /// * `Result<(), FarmingError>` - Kết quả thành công hoặc lỗi
    pub async fn update_apy(&self, pool_address: Address, new_apy: Decimal) -> Result<(), FarmingError> {
        let mut pools = self.pools.write().await;
        let pool = pools.get_mut(&pool_address).ok_or(FarmingError::PoolNotFound(pool_address))?;
        
        pool.apy_estimate = new_apy;

        info!(
            pool_address = %pool_address,
            new_apy = %new_apy,
            "Đã cập nhật APY cho farm pool"
        );

        Ok(())
    }

    /// Tính toán rewards của user
    ///
    /// # Arguments
    /// * `farm_info` - Thông tin farm của user
    /// * `pool` - Thông tin pool
    ///
    /// # Returns
    /// * `Result<U256, FarmingError>` - Số lượng rewards tính được hoặc lỗi
    async fn calculate_rewards(&self, farm_info: &UserFarmInfo, pool: &FarmPoolConfig) -> Result<U256, FarmingError> {
        if farm_info.liquidity.is_zero() {
            return Ok(U256::zero());
        }

        let now = Utc::now();
        let duration = now.signed_duration_since(farm_info.last_harvest_time);
        let blocks = duration.num_seconds() as u64 / 3; // Assume 3 seconds per block
        
        let total_liquidity = {
            let liquidity_counter = self.total_liquidity_counter.read().await;
            *liquidity_counter.get(&pool.pool_address).unwrap_or(&U256::one())
        };
        
        let user_share = if total_liquidity.is_zero() {
            0.0
        } else {
            farm_info.liquidity.as_u128() as f64 / total_liquidity.as_u128() as f64
        };
        
        let rewards_per_block = pool.reward_per_block.as_u128() as f64;
        let rewards = (rewards_per_block * blocks as f64 * user_share) as u128;
        
        Ok(U256::from(rewards))
    }

    /// Đồng bộ hóa thông tin pools từ blockchain
    ///
    /// # Returns
    /// * `Result<(), FarmingError>` - Kết quả thành công hoặc lỗi
    pub async fn sync_pools(&self) -> Result<(), FarmingError> {
        // Trong ứng dụng thực tế, sẽ gọi đến các contract trên blockchain để lấy thông tin mới nhất
        // Đây là phiên bản đơn giản chỉ ghi log

        info!("Bắt đầu đồng bộ hóa thông tin farm pools từ blockchain");

        let pools = self.pools.read().await;
        for pool in pools.values() {
            debug!(
                pool_address = %pool.pool_address,
                "Đồng bộ hóa farm pool"
            );
            
            // Mô phỏng việc đồng bộ dữ liệu từ blockchain
            // Trong thực tế, sẽ gọi smart contract và cập nhật thông tin
        }
        
        info!("Đã đồng bộ hóa tất cả các farm pools");
        Ok(())
    }
}

impl Default for FarmManager {
    fn default() -> Self {
        Self::new()
    }
}

// Đảm bảo FarmManager có thể sử dụng an toàn trong async context
impl Send for FarmManager {}
impl Sync for FarmManager {} 