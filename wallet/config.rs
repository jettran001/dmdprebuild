use serde::{Deserialize, Serialize};

use crate::walletmanager::chain::ChainType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletSystemConfig {
    pub default_chain_id: u64,
    pub default_chain_type: ChainType,
    pub max_wallets: usize,
    pub allow_wallet_overwrite: bool,
}

impl WalletSystemConfig {
    pub fn default() -> Self {
        Self {
            default_chain_id: 1,
            default_chain_type: ChainType::EVM,
            max_wallets: 100,
            allow_wallet_overwrite: false,
        }
    }

    pub fn can_add_wallet(&self, current_wallet_count: usize) -> bool {
        current_wallet_count < self.max_wallets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = WalletSystemConfig::default();
        assert_eq!(config.default_chain_id, 1);
        assert_eq!(config.default_chain_type, ChainType::EVM);
        assert_eq!(config.max_wallets, 100);
        assert_eq!(config.allow_wallet_overwrite, false);
    }

    #[test]
    fn test_can_add_wallet() {
        let config = WalletSystemConfig::default();
        assert!(config.can_add_wallet(50));
        assert!(!config.can_add_wallet(100));
    }
}