// Standard library imports
use std::collections::HashMap;
use std::sync::Arc;

// External imports
use anyhow::Context as AnyhowContext;
use ethers::signers::{LocalWallet, Signer};
use ethers::types::Address;
use hdkey::HDKey;
use tokio::sync::RwLock;
use tracing::{info, warn};

// Internal imports
use crate::error::WalletError;
use crate::walletlogic::crypto::{encrypt_data, decrypt_data};
use crate::walletlogic::utils::{generate_user_id, is_seed_phrase};
use crate::walletmanager::chain::ChainType;
use crate::walletmanager::types::{WalletConfig, SeedLength, WalletInfo, WalletSecret};

/// Quản lý ví, lưu trữ và thao tác với danh sách ví.
#[derive(Debug, Clone)]
pub struct WalletManager {
    pub wallets: Arc<RwLock<HashMap<Address, WalletInfo>>>,
}

impl WalletManager {
    /// Tạo một instance WalletManager mới.
    pub fn new() -> Self {
        Self {
            wallets: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Tạo ví mới với seed phrase được sinh ngẫu nhiên.
    ///
    /// # Arguments
    /// - `seed_length`: Độ dài của seed phrase (12 hoặc 24 từ).
    /// - `chain_id`: ID của blockchain.
    /// - `chain_type`: Loại blockchain (EVM, Solana, v.v.).
    /// - `password`: Mật khẩu để mã hóa seed phrase.
    ///
    /// # Returns
    /// Tuple (địa chỉ ví, seed phrase, user_id).
    ///
    /// # Flow
    /// Dữ liệu được tạo từ api gọi vào đây.
    #[flow_from("walletmanager::api")]
    pub async fn create_wallet_internal(
        &mut self,
        seed_length: SeedLength,
        chain_id: u64,
        chain_type: ChainType,
        password: &str,
    ) -> Result<(Address, String, String), WalletError> {
        let entropy_size = match seed_length {
            SeedLength::Twelve => 128,
            SeedLength::TwentyFour => 256,
        };

        info!("Creating wallet with {} words", match seed_length {
            SeedLength::Twelve => 12,
            SeedLength::TwentyFour => 24,
        });

        let mnemonic = HDKey::generate_mnemonic(entropy_size)
            .context("Không thể tạo seed phrase ngẫu nhiên")
            .map_err(|e| WalletError::SeedGenerationFailed(e.to_string()))?;
        
        let seed_phrase = mnemonic.phrase().to_string();

        let wallet = mnemonic
            .to_wallet()
            .context("Không thể tạo ví từ seed phrase")
            .map_err(|e| WalletError::InvalidSeedOrKey(e.to_string()))?;
        
        let address = wallet.address();

        let contains_wallet = self.wallets.read().await.contains_key(&address);
        if contains_wallet {
            warn!("Wallet creation failed: address {} exists", address);
            return Err(WalletError::WalletExists(address));
        }

        let (encrypted_secret, nonce, salt) = encrypt_data(&seed_phrase, password)
            .context("Không thể mã hóa seed phrase")?;
        
        let user_id = generate_user_id();
        
        self.wallets.write().await.insert(
            address,
            WalletInfo {
                wallet,
                secret: WalletSecret::Encrypted, // Không lưu seed phrase không mã hóa
                chain_id,
                chain_type,
                user_id: user_id.clone(),
                encrypted_secret,
                nonce,
                salt,
            },
        );
        
        info!("Wallet created: {}, user_id: {}", address, user_id);
        Ok((address, seed_phrase, user_id))
    }

    /// Nhập ví từ seed phrase hoặc private key.
    ///
    /// # Arguments
    /// - `config`: Cấu hình ví bao gồm seed/key, chain_id và password.
    ///
    /// # Returns
    /// Tuple (địa chỉ ví, user_id).
    ///
    /// # Flow
    /// Dữ liệu được tạo từ api gọi vào đây.
    #[flow_from("walletmanager::api")]
    pub async fn import_wallet_internal(
        &mut self,
        config: WalletConfig,
    ) -> Result<(Address, String), WalletError> {
        info!("Importing wallet with chain_id: {}", config.chain_id);

        let (wallet, secret_type) = if is_seed_phrase(&config) {
            let mnemonic = HDKey::from_mnemonic(&config.seed_or_key)
                .context("Không thể tạo mnemonic từ seed phrase")
                .map_err(|e| WalletError::InvalidSeedOrKey(e.to_string()))?;
            
            let wallet = mnemonic
                .to_wallet()
                .context("Không thể tạo ví từ mnemonic")
                .map_err(|e| WalletError::InvalidSeedOrKey(e.to_string()))?;
            
            (wallet, WalletSecret::Encrypted)
        } else {
            let wallet = config
                .seed_or_key
                .parse::<LocalWallet>()
                .context("Không thể tạo ví từ private key")
                .map_err(|e| WalletError::InvalidSeedOrKey(e.to_string()))?;
            
            (wallet, WalletSecret::Encrypted)
        };

        let address = wallet.address();
        
        let contains_wallet = self.wallets.read().await.contains_key(&address);
        if contains_wallet {
            warn!("Wallet import failed: address {} exists", address);
            return Err(WalletError::WalletExists(address));
        }

        let (encrypted_secret, nonce, salt) = encrypt_data(&config.seed_or_key, &config.password)
            .context("Không thể mã hóa seed phrase hoặc private key")?;
        
        let user_id = generate_user_id();
        
        self.wallets.write().await.insert(
            address,
            WalletInfo {
                wallet,
                secret: secret_type,
                chain_id: config.chain_id,
                chain_type: config.chain_type,
                user_id: user_id.clone(),
                encrypted_secret,
                nonce,
                salt,
            },
        );
        
        info!("Wallet imported: {}, user_id: {}", address, user_id);
        Ok((address, user_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walletmanager::types::{SeedLength, WalletConfig};
    use crate::walletmanager::chain::ChainType;

    #[tokio::test]
    async fn test_create_wallet() {
        let mut manager = WalletManager::new();
        let result = manager.create_wallet_internal(SeedLength::Twelve, 1, ChainType::EVM, "password").await;
        assert!(result.is_ok(), "Failed to create wallet");
        
        let (address, seed_phrase, user_id) = result
            .expect("Should return valid wallet information");
        
        assert!(!seed_phrase.is_empty(), "Seed phrase should not be empty");
        assert!(manager.wallets.read().await.contains_key(&address), "Address should exist in wallets map");
        let wallets = manager.wallets.read().await;
        let wallet_info = wallets.get(&address).unwrap();
        assert_eq!(wallet_info.user_id, user_id, "User IDs should match");
    }

    #[tokio::test]
    async fn test_import_wallet_seed() {
        let mut manager = WalletManager::new();
        let seed = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let config = WalletConfig {
            seed_or_key: seed.to_string(),
            chain_id: 1,
            chain_type: ChainType::EVM,
            seed_length: Some(SeedLength::Twelve),
            password: "password".to_string(),
        };
        
        let result = manager.import_wallet_internal(config).await;
        assert!(result.is_ok(), "Failed to import wallet");
        
        let (address, user_id) = result
            .expect("Should return valid wallet information");
         
        let wallets = manager.wallets.read().await;
        let wallet_info = wallets.get(&address).unwrap();
        assert_eq!(wallet_info.user_id, user_id, "User IDs should match");
    }
}