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
//!     let manager = DefiManager::new(ChainId::EthereumMainnet).await.unwrap();
//!     
//!     // Stake DMD tokens
//!     let user_id = "user1";
//!     let pool_address = Address::zero();
//!     let amount = U256::from(1000);
//!     let lock_time = 86400;
//!     manager.provider().stake(user_id, pool_address, amount, lock_time).await.unwrap();
//!     
//!     // Add liquidity
//!     manager.provider().add_liquidity(user_id, pool_address, amount).await.unwrap();
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

// Các module
pub mod provider;
pub mod constants;
pub mod error;
pub mod chain;
pub mod blockchain;
pub mod erc20;
pub mod erc721;
pub mod erc1155;
pub mod contracts;
pub mod security;
pub mod api;
pub mod crypto;
pub mod utils;
pub mod connection_monitor;
pub mod blockchain_listener;

// Re-export từ module mới
pub use provider::{DefiProvider, DefiProviderImpl, DefiProviderFactory, FarmPoolConfig, StakePoolConfig};
pub use constants::*;
pub use error::*;
pub use chain::ChainId;
pub use erc20::Erc20Contract;
pub use erc721::Erc721Contract;
pub use erc1155::Erc1155Contract;
pub use contracts::{
    ContractInterface, ContractMetadata, ContractType, ContractError,
    ContractRegistry, ContractFactory, get_contract_registry
};
pub use api::DefiApi;
pub use crypto::{encrypt_data, decrypt_data};
pub use utils::{
    is_valid_ethereum_address, validate_ethereum_address, validate_token_amount, 
    is_valid_transaction_hash, ValidationError
};

/// Manager chính cho module DeFi
pub struct DefiManager {
    provider: Arc<Box<dyn DefiProvider>>,
}

impl DefiManager {
    /// Tạo một DefiManager mới
    pub async fn new(chain_id: ChainId) -> Result<Self, DefiError> {
        let provider = DefiProviderFactory::create_provider(chain_id).await?;
        
        Ok(Self {
            provider: Arc::new(provider),
        })
    }

    /// Trả về reference đến DefiProvider
    pub fn provider(&self) -> &Box<dyn DefiProvider> {
        &self.provider
    }

    /// Khởi tạo pools mặc định với các tham số đã cài đặt sẵn
    pub async fn init_default_pools(&mut self, token_address: String) -> Result<(), String> {
        info!(
            token_address = %token_address,
            "Bắt đầu khởi tạo pools mặc định"
        );
        
        // Gọi phương thức thông qua provider
        // Logic đã được chuyển sang blockchain/stake/stake_logic.rs và blockchain/farm/farm_logic.rs
        let blockchain_farm_manager = self.provider.farm_manager();
        let blockchain_stake_manager = self.provider.stake_manager();
        
        // Cơ chế fallback: Nếu lỗi khi tạo một loại pool, vẫn tiếp tục tạo loại pool khác
        let farm_result = blockchain_farm_manager.create_default_pools(token_address.clone()).await;
        if let Err(e) = &farm_result {
            warn!(
                token_address = %token_address,
                error = %e,
                "Lỗi khi khởi tạo farm pools, tiếp tục với stake pools"
            );
        }
        
        let stake_result = blockchain_stake_manager.create_default_pools(token_address.clone()).await;
        if let Err(e) = &stake_result {
            warn!(
                token_address = %token_address,
                error = %e,
                "Lỗi khi khởi tạo stake pools"
            );
        }

        // Chỉ báo lỗi nếu cả hai đều thất bại
        if farm_result.is_err() && stake_result.is_err() {
            let farm_err = farm_result.err().map(|e| format!("Farm error: {:?}", e)).unwrap_or_default();
            let stake_err = stake_result.err().map(|e| format!("Stake error: {:?}", e)).unwrap_or_default();
            return Err(format!(
                "Không thể khởi tạo pools mặc định. {} {}",
                farm_err,
                stake_err
            ));
        }

        info!(
            token_address = %token_address,
            "Đã khởi tạo các pools mặc định thành công"
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
    ///     let manager = DefiManager::new(ChainId::EthereumMainnet).await.unwrap();
    ///     manager.sync_all_pools().await.unwrap();
    /// }
    /// ```
    pub async fn sync_all_pools(&self) -> Result<(), DefiError> {
        self.provider.sync_pools().await?;
        Ok(())
    }
}

// Đảm bảo DefiManager có thể sử dụng an toàn trong async context
impl Send for DefiManager {}
impl Sync for DefiManager {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_defi_manager() {
        let manager_result = DefiManager::new(ChainId::EthereumMainnet).await;
        assert!(manager_result.is_ok(), "Khởi tạo DefiManager thất bại: {:?}", manager_result.err());
        let manager = manager_result.unwrap();
        // Test provider
        let farm_config = FarmPoolConfig {
            pool_address: Address::zero(),
            router_address: Address::zero(),
            reward_token_address: Address::zero(),
            apy: Decimal::from(10),
        };
        assert!(manager.provider().add_farm_pool(farm_config).await.is_ok());
        // Test stake
        let stake_config = StakePoolConfig {
            pool_address: Address::zero(),
            token_address: Address::zero(),
            min_lock_time: 86400,
            base_apy: Decimal::from(10),
            bonus_apy: Decimal::from(5),
        };
        assert!(manager.provider().add_stake_pool(stake_config).await.is_ok());
    }
}
