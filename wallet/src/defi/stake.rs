//! Module triển khai các chức năng staking cho DeFi.
//! Cung cấp các công cụ để tương tác với staking pools và rewards.
//! Hỗ trợ staking DMD trên nhiều chain khác nhau với các kỳ hạn 7, 30, 90 và 365 ngày.

use ethers::types::{Address, U256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc, Duration};
use serde::{Deserialize, Serialize};
use tracing::{info, warn, error};
use ethers::contract::{Contract, Multicall};
use ethers::providers::{Provider, Http};
use ethers::abi::{Abi, Function, Token};
use ethers::types::{Transaction};

use crate::error::WalletError;

/// Kỳ hạn staking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StakeTerm {
    /// 7 ngày với APY 15%
    SevenDays,
    /// 30 ngày với APY 20%
    ThirtyDays,
    /// 90 ngày với APY 30%
    NinetyDays,
    /// 365 ngày với APY 70%
    OneYear,
}

impl StakeTerm {
    /// Lấy số ngày của kỳ hạn
    pub fn days(&self) -> i64 {
        match self {
            StakeTerm::SevenDays => 7,
            StakeTerm::ThirtyDays => 30,
            StakeTerm::NinetyDays => 90,
            StakeTerm::OneYear => 365,
        }
    }

    /// Lấy APY mặc định của kỳ hạn
    pub fn default_apy(&self) -> f64 {
        match self {
            StakeTerm::SevenDays => 15.0,
            StakeTerm::ThirtyDays => 20.0,
            StakeTerm::NinetyDays => 30.0,
            StakeTerm::OneYear => 70.0,
        }
    }
}

/// Cấu hình cho một staking pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakePoolConfig {
    /// Địa chỉ của pool
    pub pool_address: Address,
    /// Địa chỉ của token DMD
    pub token_address: Address,
    /// Kỳ hạn staking
    pub term: StakeTerm,
    /// APY hiện tại
    pub current_apy: f64,
    /// Tổng giá trị khóa (TVL)
    pub tvl: U256,
    /// Chain ID
    pub chain_id: u64,
    /// Thời gian tạo pool
    pub created_at: DateTime<Utc>,
    /// Thời gian cập nhật cuối
    pub updated_at: DateTime<Utc>,
}

/// Thông tin về staking của người dùng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserStakeInfo {
    /// ID người dùng
    pub user_id: String,
    /// Địa chỉ ví của người dùng
    pub wallet_address: Address,
    /// Số lượng DMD đã stake
    pub staked_amount: U256,
    /// Số lượng reward đã nhận
    pub claimed_rewards: U256,
    /// Số lượng reward chưa nhận
    pub pending_rewards: U256,
    /// Thời gian bắt đầu stake
    pub start_time: DateTime<Utc>,
    /// Thời gian kết thúc stake
    pub end_time: DateTime<Utc>,
    /// Thời gian claim cuối cùng
    pub last_claim_time: DateTime<Utc>,
}

/// Quản lý các staking pool
pub struct StakeManager {
    /// Danh sách các pool đang hoạt động
    pools: Arc<RwLock<HashMap<Address, StakePoolConfig>>>,
    /// Thông tin staking của người dùng
    user_stakes: Arc<RwLock<HashMap<String, HashMap<Address, UserStakeInfo>>>>,
}

impl StakeManager {
    /// Tạo mới StakeManager
    pub fn new() -> Self {
        Self {
            pools: Arc::new(RwLock::new(HashMap::new())),
            user_stakes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Thêm một pool mới vào hệ thống
    /// 
    /// # Arguments
    /// * `config` - Cấu hình của pool mới
    /// 
    /// # Returns
    /// * `Result<(), WalletError>` - Kết quả thao tác
    pub async fn add_pool(&self, config: StakePoolConfig) -> Result<(), WalletError> {
        let mut pools = self.pools.write().await;
        pools.insert(config.pool_address, config);
        Ok(())
    }

    /// Stake DMD vào một pool
    /// 
    /// # Arguments
    /// * `pool_address` - Địa chỉ của pool
    /// * `amount` - Số lượng DMD muốn stake
    /// 
    /// # Returns
    /// * `Result<(), WalletError>` - Kết quả thao tác
    pub async fn stake(
        &self,
        pool_address: Address,
        amount: U256,
    ) -> Result<(), WalletError> {
        if amount.is_zero() {
            error!("Invalid stake amount: {}", amount);
            return Err(WalletError::InvalidAmount);
        }

        let pool = self.get_pool_info(pool_address).await?;
        info!("Staking {} DMD in pool {} for {} days", amount, pool_address, pool.term.days());

        // 1. Kiểm tra số dư token
        let provider = Provider::<Http>::try_from("https://bsc-dataseed.binance.org/")
            .map_err(|e| WalletError::ProviderError(e.to_string()))?;

        let token_contract = Contract::new(
            pool.token_address,
            Abi::load(include_bytes!("../../blockchain/abi/ERC20.json"))?,
            provider.clone(),
        );

        let balance = token_contract
            .method::<_, U256>("balanceOf", pool_address)?
            .call()
            .await?;

        if balance < amount {
            error!("Insufficient token balance: balance={}", balance);
            return Err(WalletError::InsufficientBalance);
        }

        // 2. Phê duyệt staking pool sử dụng token
        let staking_contract = Contract::new(
            pool_address,
            Abi::load(include_bytes!("../../blockchain/abi/Staking.json"))?,
            provider.clone(),
        );

        let approve_tx = token_contract
            .method::<_, Transaction>("approve", (pool_address, amount))?
            .send()
            .await?;

        // 3. Thực hiện stake
        let stake_tx = staking_contract
            .method::<_, Transaction>(
                "stake",
                (
                    amount,
                    pool.term.days(),
                ),
            )?
            .send()
            .await?;

        // 4. Cập nhật thông tin pool
        let mut pools = self.pools.write().await;
        if let Some(pool) = pools.get_mut(&pool_address) {
            pool.tvl = pool.tvl.checked_add(amount).unwrap_or(pool.tvl);
            pool.updated_at = Utc::now();
        }

        // 5. Cập nhật thông tin người dùng
        let mut user_stakes = self.user_stakes.write().await;
        let user_pools = user_stakes.entry(pool_address.to_string())
            .or_insert_with(HashMap::new);

        let now = Utc::now();
        let end_time = now + Duration::days(pool.term.days());

        if let Some(user_stake) = user_pools.get_mut(&pool_address) {
            user_stake.staked_amount = user_stake.staked_amount.checked_add(amount).unwrap_or(user_stake.staked_amount);
            user_stake.start_time = now;
            user_stake.end_time = end_time;
        } else {
            user_pools.insert(
                pool_address,
                UserStakeInfo {
                    user_id: pool_address.to_string(),
                    wallet_address: pool_address,
                    staked_amount: amount,
                    claimed_rewards: U256::zero(),
                    pending_rewards: U256::zero(),
                    start_time: now,
                    end_time: end_time,
                    last_claim_time: now,
                },
            );
        }

        Ok(())
    }

    /// Unstake DMD từ một pool
    /// 
    /// # Arguments
    /// * `pool_address` - Địa chỉ của pool
    /// * `amount` - Số lượng DMD muốn unstake
    /// 
    /// # Returns
    /// * `Result<U256, WalletError>` - Số lượng DMD nhận được (bao gồm cả reward)
    pub async fn unstake(
        &self,
        pool_address: Address,
        amount: U256,
    ) -> Result<U256, WalletError> {
        if amount.is_zero() {
            error!("Invalid unstake amount: {}", amount);
            return Err(WalletError::InvalidAmount);
        }

        let pool = self.get_pool_info(pool_address).await?;
        info!("Unstaking {} DMD from pool {}", amount, pool_address);

        // TODO: Triển khai logic unstake
        Ok(U256::zero())
    }

    /// Thu hoạch phần thưởng từ staking
    /// 
    /// # Arguments
    /// * `pool_address` - Địa chỉ của pool
    /// 
    /// # Returns
    /// * `Result<U256, WalletError>` - Số lượng DMD nhận được
    pub async fn harvest_rewards(
        &self,
        pool_address: Address,
    ) -> Result<U256, WalletError> {
        let pool = self.get_pool_info(pool_address).await?;
        info!("Harvesting rewards from pool {}", pool_address);

        // TODO: Triển khai logic thu hoạch
        Ok(U256::zero())
    }

    /// Cập nhật APY cho một pool
    /// 
    /// # Arguments
    /// * `pool_address` - Địa chỉ của pool
    /// * `new_apy` - APY mới
    /// 
    /// # Returns
    /// * `Result<(), WalletError>` - Kết quả thao tác
    pub async fn update_pool_apy(
        &self,
        pool_address: Address,
        new_apy: f64,
    ) -> Result<(), WalletError> {
        let mut pools = self.pools.write().await;
        if let Some(pool) = pools.get_mut(&pool_address) {
            pool.current_apy = new_apy;
            pool.updated_at = Utc::now();
            Ok(())
        } else {
            Err(WalletError::PoolNotFound)
        }
    }

    /// Lấy thông tin của một pool
    /// 
    /// # Arguments
    /// * `pool_address` - Địa chỉ của pool
    /// 
    /// # Returns
    /// * `Result<StakePoolConfig, WalletError>` - Thông tin pool
    pub async fn get_pool_info(
        &self,
        pool_address: Address,
    ) -> Result<StakePoolConfig, WalletError> {
        let pools = self.pools.read().await;
        pools.get(&pool_address)
            .cloned()
            .ok_or(WalletError::PoolNotFound)
    }

    /// Lấy thông tin staking của người dùng
    /// 
    /// # Arguments
    /// * `user_id` - ID người dùng
    /// * `pool_address` - Địa chỉ của pool
    /// 
    /// # Returns
    /// * `Result<UserStakeInfo, WalletError>` - Thông tin staking
    pub async fn get_user_stake_info(
        &self,
        user_id: &str,
        pool_address: Address,
    ) -> Result<UserStakeInfo, WalletError> {
        let user_stakes = self.user_stakes.read().await;
        if let Some(user_pools) = user_stakes.get(user_id) {
            user_pools.get(&pool_address)
                .cloned()
                .ok_or(WalletError::UserStakeNotFound)
        } else {
            Err(WalletError::UserStakeNotFound)
        }
    }

    /// Tính toán phần thưởng cho một khoản stake
    /// 
    /// # Arguments
    /// * `amount` - Số lượng DMD stake
    /// * `apy` - APY của pool
    /// * `days` - Số ngày stake
    /// 
    /// # Returns
    /// Số lượng DMD thưởng
    pub fn calculate_rewards(amount: U256, apy: f64, days: i64) -> U256 {
        if amount.is_zero() || apy <= 0.0 || days <= 0 {
            return U256::zero();
        }

        let daily_rate = apy / 365.0 / 100.0;
        // Tránh overflow bằng cách chia nhỏ phép tính
        let reward = amount
            .checked_mul(U256::from((daily_rate * 1e18) as u128))
            .and_then(|r| r.checked_mul(U256::from(days as u128)))
            .map(|r| r / U256::from(1e18))
            .unwrap_or(U256::zero());

        reward
    }
}
