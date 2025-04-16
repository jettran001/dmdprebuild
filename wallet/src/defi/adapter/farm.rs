//! Adapter cho FarmManager
//!
//! Module này cung cấp một adapter để sử dụng FarmManager
//! từ blockchain thay vì từ wallet/src/defi/farm.rs.

use ethers::types::{Address, U256};
use rust_decimal::Decimal;
use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug, warn, error};

// Import FarmManager từ blockchain
use blockchain::farm::farm_logic::FarmManager as BlockchainFarmManager;

/// Cấu hình cho farming pool
#[derive(Debug, Clone)]
pub struct FarmPoolConfig {
    pub pool_address: Address,
    pub router_address: Address,
    pub reward_token_address: Address,
    pub apy: Decimal,
}

/// Adapter cho FarmManager từ blockchain
pub struct FarmManager {
    pub(crate) inner: Arc<BlockchainFarmManager>,
}

impl FarmManager {
    /// Tạo một FarmManager mới
    pub fn new() -> Self {
        Self {
            inner: Arc::new(BlockchainFarmManager::new()),
        }
    }

    /// Thêm một farming pool mới
    pub async fn add_pool(&self, config: FarmPoolConfig) -> Result<()> {
        let blockchain_config = blockchain::farm::farm_logic::FarmPoolConfig {
            pool_address: config.pool_address,
            router_address: config.router_address,
            reward_token_address: config.reward_token_address,
            apy: config.apy,
        };

        self.inner.add_pool(blockchain_config).await?;
        Ok(())
    }

    /// Thêm liquidity vào pool
    pub async fn add_liquidity(&self, user_id: &str, pool_address: Address, amount: U256) -> Result<String> {
        self.inner.add_liquidity(user_id, pool_address, amount).await
    }

    /// Rút liquidity từ pool
    pub async fn remove_liquidity(&self, user_id: &str, pool_address: Address, amount: U256) -> Result<String> {
        self.inner.remove_liquidity(user_id, pool_address, amount).await
    }

    /// Lấy số dư liquidity của user trong pool
    pub async fn get_liquidity_balance(&self, user_id: &str, pool_address: Address) -> Result<U256> {
        self.inner.get_liquidity_balance(user_id, pool_address).await
    }

    /// Lấy reward của user trong pool
    pub async fn get_farm_rewards(&self, user_id: &str, pool_address: Address) -> Result<U256> {
        self.inner.get_rewards(user_id, pool_address).await
    }

    /// Claim rewards
    pub async fn claim_farm_rewards(&self, user_id: &str, pool_address: Address) -> Result<String> {
        self.inner.claim_rewards(user_id, pool_address).await
    }

    /// Đồng bộ hóa thông tin pools từ blockchain
    pub async fn sync_pools(&self) -> Result<()> {
        self.inner.sync_pools().await?;
        info!("Đã đồng bộ hóa farm pools từ blockchain");
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