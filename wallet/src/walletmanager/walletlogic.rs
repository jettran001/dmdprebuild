// wallet/src/walletmanager/walletlogic.rs

use std::collections::HashMap;

use ethers::signers::{LocalWallet, Signer};
use ethers::types::Address;
use hdkey::HDKey;
use tracing::{info, warn};
use uuid::Uuid;

use crate::error::WalletError;
use crate::walletmanager::types::{WalletConfig, SeedLength, WalletInfo, WalletSecret};

/// Quản lý ví với các chức năng tạo ví, nhập ví, xuất seed phrase/private key, xóa ví, cập nhật chain ID, truy xuất chain ID, kiểm tra ví và quản lý người dùng.
#[derive(Debug, Clone)]
pub struct WalletManager {
    /// Danh sách ví, ánh xạ từ địa chỉ sang thông tin ví.
    wallets: HashMap<Address, WalletInfo>,
}

impl WalletManager {
    /// Tạo một WalletManager mới.
    ///
    /// # Returns
    /// Trả về một instance rỗng của `WalletManager`.
    pub fn new() -> Self {
        Self {
            wallets: HashMap::new(),
        }
    }

    /// Tạo ID người dùng duy nhất.
    ///
    /// # Returns
    /// Trả về một UUID dưới dạng chuỗi.
    pub fn generate_user_id() -> String {
        Uuid::new_v4().to_string()
    }

    /// Kiểm tra xem config có chứa seed phrase không.
    ///
    /// # Arguments
    /// - `config`: Cấu hình ví.
    ///
    /// # Returns
    /// Trả về `true` nếu là seed phrase (12 hoặc 24 từ), `false` nếu là private key.
    pub fn is_seed_phrase(config: &WalletConfig) -> bool {
        matches!(
            config.seed_length,
            Some(SeedLength::Twelve) | Some(SeedLength::TwentyFour)
        )
    }

    pub fn create_wallet_internal(
        &mut self,
        seed_length: SeedLength,
        chain_id: u64,
    ) -> Result<(Address, String, String), WalletError> {
        let entropy_size = match seed_length {
            SeedLength::Twelve => 128,
            SeedLength::TwentyFour => 256,
        };

        info!("Creating wallet with {} words seed phrase", match seed_length {
            SeedLength::Twelve => 12,
            SeedLength::TwentyFour => 24,
        });

        let mnemonic = HDKey::generate_mnemonic(entropy_size)
            .map_err(|e| WalletError::SeedGenerationFailed(e.to_string()))?;
        let seed_phrase = mnemonic.phrase().to_string();

        let wallet = mnemonic
            .to_wallet()
            .map_err(|e| WalletError::InvalidSeedOrKey(e.to_string()))?;
        let address = wallet.address();

        if self.wallets.contains_key(&address) {
            warn!("Wallet creation failed: address {} already exists", address);
            return Err(WalletError::WalletExists(address));
        }

        let user_id = Self::generate_user_id();
        self.wallets.insert(
            address,
            WalletInfo {
                wallet,
                secret: WalletSecret::Seed(seed_phrase.clone()),
                chain_id,
                user_id: user_id.clone(),
            },
        );
        info!("Wallet created successfully: {}, user_id: {}", address, user_id);
        Ok((address, seed_phrase, user_id))
    }

    pub fn import_wallet_internal(
        &mut self,
        config: WalletConfig,
    ) -> Result<(Address, String), WalletError> {
        info!("Importing wallet with chain_id: {}", config.chain_id);

        let (wallet, secret) = if Self::is_seed_phrase(&config) {
            let mnemonic = HDKey::from_mnemonic(&config.seed_or_key)
                .map_err(|e| WalletError::InvalidSeedOrKey(e.to_string()))?;
            let wallet = mnemonic
                .to_wallet()
                .map_err(|e| WalletError::InvalidSeedOrKey(e.to_string()))?;
            (wallet, WalletSecret::Seed(config.seed_or_key.clone()))
        } else {
            let wallet = config
                .seed_or_key
                .parse::<LocalWallet>()
                .map_err(|e| WalletError::InvalidSeedOrKey(e.to_string()))?;
            (wallet, WalletSecret::PrivateKey(config.seed_or_key.clone()))
        };

        let address = wallet.address();

        if self.wallets.contains_key(&address) {
            warn!("Wallet import failed: address {} already exists", address);
            return Err(WalletError::WalletExists(address));
        }

        let user_id = Self::generate_user_id();
        self.wallets.insert(
            address,
            WalletInfo {
                wallet,
                secret,
                chain_id: config.chain_id,
                user_id: user_id.clone(),
            },
        );
        info!("Wallet imported successfully: {}, user_id: {}", address, user_id);
        Ok((address, user_id))
    }

    pub fn export_seed_phrase_internal(&self, address: Address) -> Result<String, WalletError> {
        info!("Exporting seed phrase for wallet: {}", address);

        let info = self
            .wallets
            .get(&address)
            .ok_or(WalletError::WalletNotFound(address))?;

        match &info.secret {
            WalletSecret::Seed(phrase) => Ok(phrase.clone()),
            WalletSecret::PrivateKey(_) => Err(WalletError::InvalidSeedOrKey(format!(
                "Wallet {} does not have a seed phrase",
                address
            ))),
        }
    }

    pub fn export_private_key_internal(&self, address: Address) -> Result<String, WalletError> {
        info!("Exporting private key for wallet: {}", address);

        let info = self
            .wallets
            .get(&address)
            .ok_or(WalletError::WalletNotFound(address))?;

        match &info.secret {
            WalletSecret::PrivateKey(key) => Ok(key.clone()),
            WalletSecret::Seed(_) => Err(WalletError::InvalidSeedOrKey(format!(
                "Wallet {} was imported via seed phrase; use export_seed_phrase instead",
                address
            ))),
        }
    }

    pub fn remove_wallet_internal(&mut self, address: Address) -> Result<(), WalletError> {
        if let Some(info) = self.wallets.remove(&address) {
            info!("Wallet {} removed successfully, user_id: {}", address, info.user_id);
            Ok(())
        } else {
            warn!("Attempted to remove non-existent wallet {}", address);
            Err(WalletError::WalletNotFound(address))
        }
    }

    pub fn update_chain_id_internal(
        &mut self,
        address: Address,
        new_chain_id: u64,
    ) -> Result<(), WalletError> {
        info!("Updating chain ID for wallet {} to {}", address, new_chain_id);

        let info = self
            .wallets
            .get_mut(&address)
            .ok_or(WalletError::WalletNotFound(address))?;
        info.chain_id = new_chain_id;

        info!("Chain ID updated successfully for wallet {}", address);
        Ok(())
    }

    pub fn get_chain_id_internal(&self, address: Address) -> Result<u64, WalletError> {
        self.wallets
            .get(&address)
            .map(|info| info.chain_id)
            .ok_or(WalletError::WalletNotFound(address))
    }

    pub fn has_wallet_internal(&self, address: Address) -> bool {
        self.wallets.contains_key(&address)
    }

    pub fn get_wallet_internal(&self, address: Address) -> Result<&LocalWallet, WalletError> {
        Ok(&self
            .wallets
            .get(&address)
            .ok_or(WalletError::WalletNotFound(address))?
            .wallet)
    }

    pub fn get_user_id_internal(&self, address: Address) -> Result<String, WalletError> {
        self.wallets
            .get(&address)
            .map(|info| info.user_id.clone())
            .ok_or(WalletError::WalletNotFound(address))
    }

    pub fn list_wallets_internal(&self) -> Vec<Address> {
        self.wallets.keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walletmanager::types::{SeedLength, WalletConfig};

    #[test]
    fn test_create_wallet() {
        let mut manager = WalletManager::new();
        let result = manager.create_wallet_internal(SeedLength::Twelve, 1);
        assert!(result.is_ok());
        let (address, _, user_id) = result.unwrap();
        assert!(manager.get_wallet_internal(address).is_ok());
        assert_eq!(manager.get_user_id_internal(address).unwrap(), user_id);
    }

    #[test]
    fn test_import_wallet_seed() {
        let mut manager = WalletManager::new();
        let seed = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let config = WalletConfig {
            seed_or_key: seed.to_string(),
            chain_id: 1,
            seed_length: Some(SeedLength::Twelve),
        };
        let result = manager.import_wallet_internal(config);
        assert!(result.is_ok());
        let (address, user_id) = result.unwrap();
        assert_eq!(manager.get_user_id_internal(address).unwrap(), user_id);
    }

    #[test]
    fn test_import_wallet_private_key() {
        let mut manager = WalletManager::new();
        let config = WalletConfig {
            seed_or_key: "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
            chain_id: 1,
            seed_length: None,
        };
        let result = manager.import_wallet_internal(config);
        assert!(result.is_ok());
        let (address, user_id) = result.unwrap();
        assert_eq!(manager.get_user_id_internal(address).unwrap(), user_id);
    }

    #[test]
    fn test_export_seed_phrase() {
        let mut manager = WalletManager::new();
        let (address, seed_phrase, _) = manager.create_wallet_internal(SeedLength::Twelve, 1).unwrap();
        let exported = manager.export_seed_phrase_internal(address).unwrap();
        assert_eq!(exported, seed_phrase);
    }

    #[test]
    fn test_export_private_key() {
        let mut manager = WalletManager::new();
        let private_key = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let config = WalletConfig {
            seed_or_key: private_key.to_string(),
            chain_id: 1,
            seed_length: None,
        };
        let (address, _) = manager.import_wallet_internal(config).unwrap();
        let exported = manager.export_private_key_internal(address).unwrap();
        assert_eq!(exported, private_key);
    }

    #[test]
    fn test_remove_wallet() {
        let mut manager = WalletManager::new();
        let (address, _, _) = manager.create_wallet_internal(SeedLength::Twelve, 1).unwrap();
        assert!(manager.remove_wallet_internal(address).is_ok());
        assert!(manager.get_wallet_internal(address).is_err());
    }

    #[test]
    fn test_update_chain_id() {
        let mut manager = WalletManager::new();
        let (address, _, _) = manager.create_wallet_internal(SeedLength::Twelve, 1).unwrap();
        assert!(manager.update_chain_id_internal(address, 56).is_ok());
        assert_eq!(manager.get_chain_id_internal(address).unwrap(), 56);
    }

    #[test]
    fn test_get_chain_id() {
        let mut manager = WalletManager::new();
        let (address, _, _) = manager.create_wallet_internal(SeedLength::Twelve, 1).unwrap();
        assert_eq!(manager.get_chain_id_internal(address).unwrap(), 1);
    }

    #[test]
    fn test_has_wallet() {
        let mut manager = WalletManager::new();
        let (address, _, _) = manager.create_wallet_internal(SeedLength::Twelve, 1).unwrap();
        assert!(manager.has_wallet_internal(address));
        assert!(!manager.has_wallet_internal(Address::zero()));
    }
}