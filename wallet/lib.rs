// wallet/lib.rs

//! Ví Diamond Chain - Module ngoại vi cho DiamondChain
//!
//! Cung cấp các chức năng quản lý ví và tương tác với blockchain.
//! Thiết kế modular giúp dễ dàng mở rộng và bảo trì.

// Re-export toàn bộ API công khai
pub use crate::walletmanager::api::WalletManagerApi;
pub use crate::walletmanager::types::{WalletConfig, WalletInfo, SeedLength, WalletSecret};
pub use crate::walletmanager::chain::{ChainConfig, ChainType, ChainManager, DefaultChainManager};
pub use crate::error::{WalletError, Result};
pub use crate::config::WalletSystemConfig;
pub use crate::users::subscription::manager::SubscriptionManager;
pub use crate::users::subscription::staking::{StakingManager, TokenStake, StakeStatus};
pub use crate::defi::api::DefiApi;
pub use crate::defi::error::DefiError;
pub use crate::cache::{CacheSystem, CacheConfig, CacheKey};
pub use crate::contracts::erc20::{Erc20Contract, Erc20Interface};
pub use crate::defi::blockchain::non_evm::diamond::DiamondBlockchainProvider;

// Export các module chính
pub mod walletlogic;
pub mod walletmanager;
pub mod defi;
pub mod users;
pub mod contracts;
pub mod error;
pub mod config;
pub mod cache;
pub mod bridge;  // Module mới cho xử lý bridge

// Một số config mẫu
pub const DEFAULT_CHAIN_TYPE: ChainType = ChainType::EVM;
pub const DEFAULT_NETWORK_ID: u64 = 1; // Ethereum Mainnet
pub const MAX_WALLETS_PER_USER: usize = 10;

// Enum quản lý Mnemonic Seeds
pub enum MnemonicLength {
    Words12,
    Words24,
}

impl From<MnemonicLength> for SeedLength {
    fn from(length: MnemonicLength) -> Self {
        match length {
            MnemonicLength::Words12 => SeedLength::Words12,
            MnemonicLength::Words24 => SeedLength::Words24,
        }
    }
}

// Các helper functions
pub fn init_wallet_system() -> WalletSystemConfig {
    WalletSystemConfig {
        default_chain_id: DEFAULT_NETWORK_ID,
        max_wallets: MAX_WALLETS_PER_USER,
    }
}

pub fn create_chain_config(chain_type: ChainType, chain_id: u64) -> ChainConfig {
    ChainConfig { chain_type, chain_id }
}

// Re-export các types quan trọng từ nhiều module
pub mod types {
    pub use crate::walletmanager::types::*;
    pub use crate::walletlogic::utils::UserType;
    pub use crate::users::subscription::types::{SubscriptionType, SubscriptionStatus, Feature, PaymentToken};
    pub use crate::defi::adapter::farm::FarmAdapter;
    pub use crate::defi::adapter::stake::StakeAdapter;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_wallet_system() {
        let config = init_wallet_system();
        assert_eq!(config.default_chain_id, DEFAULT_NETWORK_ID);
        assert_eq!(config.max_wallets, MAX_WALLETS_PER_USER);
    }

    #[test]
    fn test_create_chain_config() {
        let config = create_chain_config(ChainType::EVM, 1);
        assert_eq!(config.chain_type, ChainType::EVM);
        assert_eq!(config.chain_id, 1);
    }
}