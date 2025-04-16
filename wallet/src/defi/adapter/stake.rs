//! Adapter cho StakeManager
//!
//! Module này cung cấp một adapter để sử dụng StakeManager
//! từ blockchain thay vì từ wallet/src/defi/stake.rs.

use ethers::types::{Address, U256};
use rust_decimal::Decimal;
use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug, warn, error};

// Import StakeManager từ blockchain
use blockchain::stake::stake_logic::StakeManager as BlockchainStakeManager;

/// Cấu hình cho staking pool
#[derive(Debug, Clone)]
pub struct StakePoolConfig {
    pub pool_address: Address,
    pub token_address: Address,
    pub min_lock_time: u64,
    pub base_apy: Decimal,
    pub bonus_apy: Decimal,
}

/// Adapter cho StakeManager từ blockchain
pub struct StakeManager {
    pub(crate) inner: Arc<BlockchainStakeManager>,
}

impl StakeManager {
    /// Tạo một StakeManager mới
    pub fn new() -> Self {
        Self {
            inner: Arc::new(BlockchainStakeManager::new()),
        }
    }

    /// Thêm một staking pool mới
    pub async fn add_pool(&self, config: StakePoolConfig) -> Result<()> {
        let blockchain_config = blockchain::stake::stake_logic::StakePoolConfig {
            pool_address: config.pool_address,
            token_address: config.token_address,
            min_lock_time: config.min_lock_time,
            base_apy: config.base_apy,
            bonus_apy: config.bonus_apy,
        };

        self.inner.add_pool(blockchain_config).await?;
        Ok(())
    }

    /// Stake tokens vào pool
    pub async fn stake(&self, user_id: &str, pool_address: Address, amount: U256, lock_time: u64) -> Result<String> {
        self.inner.stake(user_id, pool_address, amount, lock_time).await
    }

    /// Unstake tokens từ pool
    pub async fn unstake(&self, user_id: &str, pool_address: Address, amount: U256) -> Result<String> {
        self.inner.unstake(user_id, pool_address, amount).await
    }

    /// Lấy số dư stake của user trong pool
    pub async fn get_staked_balance(&self, user_id: &str, pool_address: Address) -> Result<U256> {
        self.inner.get_staked_balance(user_id, pool_address).await
    }

    /// Lấy reward của user trong pool
    pub async fn get_rewards(&self, user_id: &str, pool_address: Address) -> Result<U256> {
        self.inner.get_rewards(user_id, pool_address).await
    }

    /// Claim rewards
    pub async fn claim_rewards(&self, user_id: &str, pool_address: Address) -> Result<String> {
        self.inner.claim_rewards(user_id, pool_address).await
    }

    /// Đồng bộ hóa thông tin pools từ blockchain
    pub async fn sync_pools(&self) -> Result<()> {
        self.inner.sync_pools().await?;
        info!("Đã đồng bộ hóa stake pools từ blockchain");
        Ok(())
    }
}

impl Default for StakeManager {
    fn default() -> Self {
        Self::new()
    }
}

// Đảm bảo StakeManager có thể sử dụng an toàn trong async context
impl Send for StakeManager {}
impl Sync for StakeManager {} 