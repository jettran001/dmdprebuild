//! Module DeFi cho ví DiamondChain
//! 
//! Bao gồm các tính năng:
//! - Staking: stake DMD token để nhận rewards
//! - Farming: cung cấp liquidity cho các cặp token (DMD/BNB, DMD/ETH, v.v.)
//! - Đồng bộ hóa thông tin với blockchain
//! 
//! ## Flow:
//! ```
//! User -> Wallet -> Blockchain (stake, farm modules)
//! ```
//! 
//! ## Ví dụ:
//! ```
//! use wallet::defi::DefiManager;
//! use ethers::types::{Address, U256};
//! 
//! #[tokio::main]
//! async fn main() {
//!     // Tạo DefiManager
//!     let manager = DefiManager::new();
//!     
//!     // Stake DMD tokens
//!     let user_id = "user1";
//!     let pool_address = Address::zero();
//!     let amount = U256::from(1000);
//!     let lock_time = 86400;
//!     manager.stake_manager().stake(user_id, pool_address, amount, lock_time).await.unwrap();
//!     
//!     // Add liquidity
//!     manager.farm_manager().add_liquidity(user_id, pool_address, amount).await.unwrap();
//! }
//! ```

// External imports
use ethers::types::{Address, U256};
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::{info, warn, error, debug};
use anyhow::Result;

// Internal imports
use crate::blockchain::provider::{get_provider, ChainId};
use crate::cache::{Cache, CacheManager};

// Re-export các module
pub mod adapter;

// Re-export các component từ adapter
pub use adapter::farm::{FarmManager, FarmPoolConfig};
pub use adapter::stake::{StakeManager, StakePoolConfig};

// Các module khác
pub mod constants;
pub mod error;
pub mod chain;
pub mod blockchain;

// Re-exports
pub use constants::*;
pub use error::*;
pub use chain::ChainId;

/// Manager chính cho module DeFi
pub struct DefiManager {
    farm_manager: Arc<FarmManager>,
    stake_manager: Arc<StakeManager>,
}

impl DefiManager {
    /// Tạo một DefiManager mới
    pub fn new() -> Self {
        Self {
            farm_manager: Arc::new(FarmManager::new()),
            stake_manager: Arc::new(StakeManager::new()),
        }
    }

    /// Trả về reference đến FarmManager
    pub fn farm_manager(&self) -> &FarmManager {
        &self.farm_manager
    }

    /// Trả về reference đến StakeManager
    pub fn stake_manager(&self) -> &StakeManager {
        &self.stake_manager
    }

    /// Khởi tạo pools mặc định với các tham số đã cài đặt sẵn
    /// Logic đã được chuyển sang blockchain/stake/stake_logic.rs và blockchain/farm/farm_logic.rs
    pub async fn init_default_pools(&mut self, token_address: String) -> Result<(), String> {
        // Gọi phương thức thông qua adapter
        self.stake_manager.inner.create_default_pools(token_address.clone()).await?;

        // Khởi tạo các farm pools mặc định
        self.farm_manager.inner.create_default_pools(token_address.clone()).await?;

        info!(
            token_address = %token_address,
            "Đã khởi tạo các pools mặc định"
        );

        Ok(())
    }

    /// Đồng bộ hóa thông tin các pools từ blockchain
    ///
    /// # Returns
    /// * `Result<(), DefiError>` - Kết quả thành công hoặc lỗi
    ///
    /// # Examples
    /// ```
    /// use wallet::defi::DefiManager;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let manager = DefiManager::new();
    ///     manager.sync_all_pools().await.unwrap();
    /// }
    /// ```
    pub async fn sync_all_pools(&self) -> Result<(), DefiError> {
        // Đồng bộ hóa stake pools
        self.stake_manager.sync_pools().await?;

        // Đồng bộ hóa farm pools
        self.farm_manager.sync_pools().await?;

        info!("Đã đồng bộ hóa tất cả các pools");
        Ok(())
    }
}

// Đảm bảo DefiManager có thể sử dụng an toàn trong async context
impl Default for DefiManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Send for DefiManager {}
impl Sync for DefiManager {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_defi_manager() {
        let manager = DefiManager::new();

        // Test farm manager
        let farm_config = adapter::farm::FarmPoolConfig {
            pool_address: Address::zero(),
            router_address: Address::zero(),
            reward_token_address: Address::zero(),
            apy: Decimal::from(10),
        };
        assert!(manager.farm_manager().add_pool(farm_config).await.is_ok());

        // Test stake manager
        let stake_config = adapter::stake::StakePoolConfig {
            pool_address: Address::zero(),
            token_address: Address::zero(),
            min_lock_time: 86400,
            base_apy: Decimal::from(10),
            bonus_apy: Decimal::from(5),
        };
        assert!(manager.stake_manager().add_pool(stake_config).await.is_ok());
    }
}
