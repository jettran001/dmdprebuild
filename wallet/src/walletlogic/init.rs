// Standard library imports
use std::collections::HashMap;
use std::sync::Arc;

// External imports
use anyhow::{Context, Result};
use ethers::signers::{LocalWallet, Signer};
use ethers::types::Address;
use hdkey::HDKey;
use tokio::sync::RwLock;
use tracing::{info, warn};
use bip39::{Language, Mnemonic, Seed};
use bitcoin_bip32::{ExtendedPrivKey, ExtendedPubKey, Network};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::fs;

// Internal imports
use crate::error::WalletError;
use crate::defi::crypto::{encrypt_data, decrypt_data};
use crate::walletlogic::utils::{generate_user_id, generate_default_user_id, UserType, is_seed_phrase};
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
    /// - `user_type`: Loại người dùng (mặc định là Free).
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
        user_type: Option<UserType>,
    ) -> Result<(Address, String, String), WalletError> {
        let entropy_size = match seed_length {
            SeedLength::Twelve => 128,
            SeedLength::TwentyFour => 256,
        };

        info!("Creating wallet with {} words", match seed_length {
            SeedLength::Twelve => 12,
            SeedLength::TwentyFour => 24,
        });

        let (mnemonic_phrase, public_key) = generate_wallet(entropy_size)?;
        
        let wallet = LocalWallet::from_mnemonic(&mnemonic_phrase)
            .context("Không thể tạo ví từ mnemonic phrase")
            .map_err(|e| WalletError::InvalidSeedOrKey(e.to_string()))?;
        
        let address = wallet.address();

        let contains_wallet = self.wallets.read().await.contains_key(&address);
        if contains_wallet {
            warn!("Wallet creation failed: address {} exists", address);
            return Err(WalletError::WalletExists(address));
        }

        let (encrypted_secret, nonce, salt) = encrypt_data(&public_key, password)
            .context("Không thể mã hóa seed phrase")?;
        
        // Tạo user_id với loại người dùng cụ thể hoặc mặc định là Free
        let user_id = match user_type {
            Some(utype) => generate_user_id(utype),
            None => generate_default_user_id(),
        };
        
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
        Ok((address, mnemonic_phrase, user_id))
    }

    /// Nhập ví từ seed phrase hoặc private key.
    ///
    /// # Arguments
    /// - `config`: Cấu hình ví bao gồm seed/key, chain_id và password.
    /// - `user_type`: Loại người dùng (mặc định là Free).
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
        user_type: Option<UserType>,
    ) -> Result<(Address, String), WalletError> {
        info!("Importing wallet with chain_id: {}", config.chain_id);

        // Validate seed phrase hoặc private key trước khi xử lý
        let (wallet, secret_type) = if is_seed_phrase(&config) {
            // Validate mnemonic phrase format và tính hợp lệ
            if !validate_seed_phrase(&config.seed_or_key) {
                tracing::error!("Invalid seed phrase format or words");
                return Err(WalletError::InvalidSeedOrKey("Seed phrase không hợp lệ: kiểm tra định dạng, từ vựng và độ dài".to_string()));
            }
            
            // Verify checksum của mnemonic
            if !verify_seed_phrase_checksum(&config.seed_or_key) {
                tracing::error!("Invalid seed phrase checksum");
                return Err(WalletError::InvalidSeedOrKey("Seed phrase không hợp lệ: checksum sai".to_string()));
            }
            
            // Log an toàn về độ dài seed mà không tiết lộ nội dung
            let words_count = config.seed_or_key.split_whitespace().count();
            tracing::debug!("Seed phrase validation passed: {} words", words_count);
            
            let mnemonic = Mnemonic::from_phrase(&config.seed_or_key, Language::English)?;
            
            let wallet = LocalWallet::from_mnemonic(&mnemonic.to_string())
                .context("Không thể tạo ví từ mnemonic")
                .map_err(|e| WalletError::InvalidSeedOrKey(e.to_string()))?;
            
            (wallet, WalletSecret::Seed(words_count))
        } else {
            // Validate private key format
            if !validate_private_key(&config.seed_or_key) {
                tracing::error!("Invalid private key format");
                return Err(WalletError::InvalidSeedOrKey("Private key không hợp lệ: kiểm tra định dạng".to_string()));
            }
            
            let wallet = config
                .seed_or_key
                .parse::<LocalWallet>()
                .context("Không thể tạo ví từ private key")
                .map_err(|e| WalletError::InvalidSeedOrKey(e.to_string()))?;
            
            (wallet, WalletSecret::PrivateKey(64))
        };

        let address = wallet.address();
        
        let contains_wallet = self.wallets.read().await.contains_key(&address);
        if contains_wallet {
            warn!("Wallet import failed: address {} exists", address);
            return Err(WalletError::WalletExists(address));
        }

        // Tăng cường bảo mật lưu trữ với salt riêng và tham số mã hóa mạnh
        let (encrypted_secret, nonce, salt) = encrypt_data_enhanced(&config.seed_or_key, &config.password)
            .context("Không thể mã hóa seed phrase hoặc private key")?;
        
        // Tạo user_id với loại người dùng cụ thể hoặc mặc định là Free
        let user_id = match user_type {
            Some(utype) => generate_user_id(utype),
            None => generate_default_user_id(),
        };
        
        // Lưu metadata về bảo mật cho audit log, không bao gồm dữ liệu nhạy cảm
        tracing::info!(
            "Wallet security: Address={}, EncryptionMethod=AES-256-GCM, PBKDF2Iterations=100000, SecretType={:?}", 
            address, 
            secret_type
        );
        
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

/// Hàm mở rộng cho encrypt_data với tham số bảo mật mạnh hơn
fn encrypt_data_enhanced(data: &str, password: &str) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), WalletError> {
    use crate::defi::crypto::encrypt_data_with_params;
    
    // Tăng số vòng lặp PBKDF2 và sử dụng salt ngẫu nhiên đủ lớn
    const PBKDF2_ITERATIONS: u32 = 310_000; // OWASP recommendation for 2023
    const SALT_SIZE: usize = 32; // 256 bits
    
    encrypt_data_with_params(data, password, PBKDF2_ITERATIONS, SALT_SIZE)
        .context("Encryption failed with enhanced parameters")
        .map_err(|e| WalletError::EncryptionError(e.to_string()))
}

/// Kiểm tra định dạng và tính hợp lệ của seed phrase (mnemonic)
fn validate_seed_phrase(seed: &str) -> bool {
    use bip39::{Mnemonic, Language};
    
    // Kiểm tra độ dài và từ vựng
    let words: Vec<&str> = seed.split_whitespace().collect();
    let word_count = words.len();
    
    // BIP39 seed phrase phải có 12, 15, 18, 21 hoặc 24 từ
    if ![12, 15, 18, 21, 24].contains(&word_count) {
        tracing::warn!("Invalid seed phrase word count: {}", word_count);
        return false;
    }
    
    // Kiểm tra mỗi từ phải nằm trong wordlist BIP39
    for word in &words {
        if !Language::English.wordlist().contains(word) {
            tracing::warn!("Word not in BIP39 wordlist");
            return false;
        }
    }
    
    // Kiểm tra định dạng cơ bản đúng không
    match Mnemonic::from_phrase(seed, Language::English) {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!("Mnemonic validation failed: {}", e);
            false
        }
    }
}

/// Kiểm tra checksum của seed phrase
fn verify_seed_phrase_checksum(seed: &str) -> bool {
    use bip39::{Mnemonic, Language};
    
    match Mnemonic::from_phrase(seed, Language::English) {
        Ok(_) => true,
        Err(_) => false,
    }
}

/// Kiểm tra định dạng private key
fn validate_private_key(private_key: &str) -> bool {
    // Private key phải là chuỗi hex 64 ký tự (không tính 0x prefix)
    let private_key = private_key.trim_start_matches("0x");
    
    // Kiểm tra độ dài
    if private_key.len() != 64 {
        tracing::warn!("Invalid private key length: {}", private_key.len());
        return false;
    }
    
    // Kiểm tra định dạng hex
    private_key.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn generate_wallet(entropy_size: usize) -> Result<(String, String)> {
    // Tạo mnemonic phrase sử dụng bip39
    let mnemonic = Mnemonic::generate_in(Language::English, entropy_size)?;
    let mnemonic_phrase = mnemonic.to_string();
    
    // Tạo seed từ mnemonic
    let seed = Seed::new(&mnemonic, "");
    
    // Tạo HD wallet từ seed
    let root_key = ExtendedPrivKey::new_master(Network::Bitcoin, &seed)?;
    let account_key = root_key.derive_child(0)?;
    let public_key = account_key.public_key();
    
    Ok((mnemonic_phrase, public_key.to_string()))
}

pub fn restore_wallet(seed_or_key: &str) -> Result<String> {
    // Khôi phục mnemonic từ seed
    let mnemonic = Mnemonic::from_phrase(seed_or_key, Language::English)?;
    let seed = Seed::new(&mnemonic, "");
    
    // Tạo HD wallet từ seed
    let root_key = ExtendedPrivKey::new_master(Network::Bitcoin, &seed)?;
    let account_key = root_key.derive_child(0)?;
    let public_key = account_key.public_key();
    
    Ok(public_key.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walletmanager::types::{SeedLength, WalletConfig};
    use crate::walletmanager::chain::ChainType;
    use crate::walletlogic::utils::UserType;

    #[tokio::test]
    async fn test_create_wallet() {
        let mut manager = WalletManager::new();
        let result = manager.create_wallet_internal(
            SeedLength::Twelve, 1, ChainType::EVM, "password", Some(UserType::Free)
        ).await;
        assert!(result.is_ok(), "Failed to create wallet");
        
        let (address, seed_phrase, user_id) = result
            .expect("Should return valid wallet information");
        
        assert!(!seed_phrase.is_empty(), "Seed phrase should not be empty");
        assert!(manager.wallets.read().await.contains_key(&address), "Address should exist in wallets map");
        let wallets = manager.wallets.read().await;
        let wallet_info = wallets.get(&address).unwrap();
        assert_eq!(wallet_info.user_id, user_id, "User IDs should match");
        assert!(user_id.starts_with("FREE_"), "User ID should have FREE prefix");
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
        
        let result = manager.import_wallet_internal(config, Some(UserType::Free)).await;
        assert!(result.is_ok(), "Failed to import wallet");
        
        let (address, user_id) = result
            .expect("Should return valid wallet information");
         
        let wallets = manager.wallets.read().await;
        let wallet_info = wallets.get(&address).unwrap();
        assert_eq!(wallet_info.user_id, user_id, "User IDs should match");
    }
}