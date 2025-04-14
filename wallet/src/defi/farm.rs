//! Module triển khai các chức năng farming cho DeFi.
//! Cung cấp các công cụ để tương tác với liquidity pools và farming rewards.
//! Hỗ trợ các pool: DMD/BNB, DMD/ETH, DMD/MONAD, DMD/SOL, DMD/PI, DMD/TON, DMD/SUI, DMD/NEAR

use ethers::types::{Address, U256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{info, warn, error};
use ethers::contract::{Contract, Multicall};
use ethers::providers::{Provider, Http};
use ethers::abi::{Abi, Function, Token};
use ethers::types::{Transaction};

use crate::error::WalletError;

/// Cấu hình cho một farming pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FarmPoolConfig {
    /// Địa chỉ của pool
    pub pool_address: Address,
    /// Địa chỉ của router
    pub router_address: Address,
    /// Địa chỉ của token thưởng (DMD)
    pub reward_token: Address,
    /// Địa chỉ của token pair thứ nhất (DMD)
    pub token0: Address,
    /// Địa chỉ của token pair thứ hai
    pub token1: Address,
    /// Tên của pool
    pub name: String,
    /// Chain ID
    pub chain_id: u64,
    /// APY hiện tại
    pub current_apy: f64,
    /// Tổng giá trị khóa (TVL)
    pub tvl: U256,
    /// Thời gian tạo pool
    pub created_at: DateTime<Utc>,
    /// Thời gian cập nhật cuối
    pub updated_at: DateTime<Utc>,
}

/// Thông tin về farming của người dùng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFarmInfo {
    /// ID người dùng
    pub user_id: String,
    /// Địa chỉ ví của người dùng
    pub wallet_address: Address,
    /// Số lượng LP token đã stake
    pub staked_amount: U256,
    /// Số lượng reward đã nhận
    pub claimed_rewards: U256,
    /// Số lượng reward chưa nhận
    pub pending_rewards: U256,
    /// Thời gian stake cuối cùng
    pub last_stake_time: DateTime<Utc>,
    /// Thời gian claim cuối cùng
    pub last_claim_time: DateTime<Utc>,
}

/// Quản lý các farming pool
pub struct FarmManager {
    /// Danh sách các pool đang hoạt động
    pools: Arc<RwLock<HashMap<Address, FarmPoolConfig>>>,
    /// Thông tin farming của người dùng
    user_farms: Arc<RwLock<HashMap<String, HashMap<Address, UserFarmInfo>>>>,
}

impl FarmManager {
    /// Tạo mới FarmManager
    pub fn new() -> Self {
        Self {
            pools: Arc::new(RwLock::new(HashMap::new())),
            user_farms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Thêm một pool mới vào hệ thống
    /// 
    /// # Arguments
    /// * `config` - Cấu hình của pool mới
    /// 
    /// # Returns
    /// * `Result<(), WalletError>` - Kết quả thao tác
    pub async fn add_pool(&self, config: FarmPoolConfig) -> Result<(), WalletError> {
        let mut pools = self.pools.write().await;
        pools.insert(config.pool_address, config);
        Ok(())
    }

    /// Thêm thanh khoản vào một pool
    /// 
    /// # Arguments
    /// * `pool_address` - Địa chỉ của pool
    /// * `amount0` - Số lượng token0 (DMD)
    /// * `amount1` - Số lượng token1
    /// 
    /// # Returns
    /// * `Result<U256, WalletError>` - Số lượng LP token nhận được
    pub async fn add_liquidity(
        &self,
        pool_address: Address,
        amount0: U256,
        amount1: U256,
    ) -> Result<U256, WalletError> {
        if amount0.is_zero() || amount1.is_zero() {
            error!("Invalid liquidity amounts: amount0={}, amount1={}", amount0, amount1);
            return Err(WalletError::InvalidAmount);
        }

        let pool = self.get_pool_info(pool_address).await?;
        info!("Adding liquidity to pool {}: amount0={}, amount1={}", pool.name, amount0, amount1);

        // 1. Kiểm tra số dư token
        let provider = Provider::<Http>::try_from("https://bsc-dataseed.binance.org/")
            .map_err(|e| WalletError::ProviderError(e.to_string()))?;

        let token0_contract = Contract::new(
            pool.token0,
            Abi::load(include_bytes!("../../blockchain/abi/ERC20.json"))?,
            provider.clone(),
        );

        let token1_contract = Contract::new(
            pool.token1,
            Abi::load(include_bytes!("../../blockchain/abi/ERC20.json"))?,
            provider.clone(),
        );

        let balance0 = token0_contract
            .method::<_, U256>("balanceOf", pool_address)?
            .call()
            .await?;

        let balance1 = token1_contract
            .method::<_, U256>("balanceOf", pool_address)?
            .call()
            .await?;

        if balance0 < amount0 || balance1 < amount1 {
            error!("Insufficient token balance: balance0={}, balance1={}", balance0, balance1);
            return Err(WalletError::InsufficientBalance);
        }

        // 2. Phê duyệt router sử dụng token
        let router_contract = Contract::new(
            pool.router_address,
            Abi::load(include_bytes!("../../blockchain/abi/Router.json"))?,
            provider.clone(),
        );

        let approve_tx0 = token0_contract
            .method::<_, Transaction>("approve", (pool.router_address, amount0))?
            .send()
            .await?;

        let approve_tx1 = token1_contract
            .method::<_, Transaction>("approve", (pool.router_address, amount1))?
            .send()
            .await?;

        // 3. Thêm thanh khoản
        let add_liquidity_tx = router_contract
            .method::<_, Transaction>(
                "addLiquidity",
                (
                    pool.token0,
                    pool.token1,
                    amount0,
                    amount1,
                    0, // Slippage tolerance
                    pool_address,
                    U256::from(chrono::Utc::now().timestamp() + 300), // Deadline 5 phút
                ),
            )?
            .send()
            .await?;

        // 4. Cập nhật thông tin pool
        let mut pools = self.pools.write().await;
        if let Some(pool) = pools.get_mut(&pool_address) {
            pool.tvl = pool.tvl.checked_add(amount0).unwrap_or(pool.tvl);
            pool.updated_at = Utc::now();
        }

        // 5. Cập nhật thông tin người dùng
        let mut user_farms = self.user_farms.write().await;
        let user_pools = user_farms.entry(pool_address.to_string())
            .or_insert_with(HashMap::new);

        if let Some(user_farm) = user_pools.get_mut(&pool_address) {
            user_farm.staked_amount = user_farm.staked_amount.checked_add(amount0).unwrap_or(user_farm.staked_amount);
            user_farm.last_stake_time = Utc::now();
        } else {
            user_pools.insert(
                pool_address,
                UserFarmInfo {
                    user_id: pool_address.to_string(),
                    wallet_address: pool_address,
                    staked_amount: amount0,
                    claimed_rewards: U256::zero(),
                    pending_rewards: U256::zero(),
                    last_stake_time: Utc::now(),
                    last_claim_time: Utc::now(),
                },
            );
        }

        Ok(amount0) // Trả về số lượng LP token nhận được
    }

    /// Rút thanh khoản từ một pool
    /// 
    /// # Arguments
    /// * `pool_address` - Địa chỉ của pool
    /// * `lp_amount` - Số lượng LP token muốn rút
    /// 
    /// # Returns
    /// * `Result<(U256, U256), WalletError>` - Số lượng token0 và token1 nhận được
    pub async fn remove_liquidity(
        &self,
        pool_address: Address,
        lp_amount: U256,
    ) -> Result<(U256, U256), WalletError> {
        if lp_amount.is_zero() {
            error!("Invalid LP amount: {}", lp_amount);
            return Err(WalletError::InvalidAmount);
        }

        let pool = self.get_pool_info(pool_address).await?;
        info!("Removing liquidity from pool {}: lp_amount={}", pool.name, lp_amount);

        // TODO: Triển khai logic rút thanh khoản
        Ok((U256::zero(), U256::zero()))
    }

    /// Thu hoạch phần thưởng từ farming
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
        info!("Harvesting rewards from pool {}", pool.name);

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
    /// * `Result<FarmPoolConfig, WalletError>` - Thông tin pool
    pub async fn get_pool_info(
        &self,
        pool_address: Address,
    ) -> Result<FarmPoolConfig, WalletError> {
        let pools = self.pools.read().await;
        pools.get(&pool_address)
            .cloned()
            .ok_or(WalletError::PoolNotFound)
    }

    /// Lấy thông tin farming của người dùng
    /// 
    /// # Arguments
    /// * `user_id` - ID người dùng
    /// * `pool_address` - Địa chỉ của pool
    /// 
    /// # Returns
    /// * `Result<UserFarmInfo, WalletError>` - Thông tin farming
    pub async fn get_user_farm_info(
        &self,
        user_id: &str,
        pool_address: Address,
    ) -> Result<UserFarmInfo, WalletError> {
        let user_farms = self.user_farms.read().await;
        if let Some(user_pools) = user_farms.get(user_id) {
            user_pools.get(&pool_address)
                .cloned()
                .ok_or(WalletError::UserFarmNotFound)
        } else {
            Err(WalletError::UserFarmNotFound)
        }
    }
}
