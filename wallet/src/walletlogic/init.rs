// wallet/src/walletlogic/init.rs

use std::collections::HashMap;

use ethers::signers::LocalWallet;
use ethers::types::Address;
use hdkey::HDKey;
use tracing::{info, warn};

use crate::error::WalletError;
use crate::walletlogic::utils::{generate_user_id, is_seed_phrase};
use crate::walletmanager::types::{WalletConfig, SeedLength, WalletInfo, WalletSecret};

/// Quản lý ví với HashMap lưu trữ thông tin ví.
#[derive(Debug, Clone)]
pub struct WalletManager {
    /// Danh sách ví, ánh xạ từ địa chỉ sang thông tin ví.
    pub wallets: HashMap<Address, WalletInfo>,
}

impl WalletManager {
    /// Tạo một WalletManager mới.
    ///
    /// # Returns
    /// Trả về instance rỗng của `WalletManager`.
    pub fn new() -> Self {
        Self {
            wallets: HashMap::new(),
        }
    }

    /// Tạo ví mới với seed phrase 12 hoặc 24 từ.
    ///
    /// # Arguments
    /// - `seed_length`: Loại seed phrase (12 hoặc 24 từ).
    /// - `chain_id`: Chain ID mà ví sẽ hoạt động.
    ///
    /// # Returns
    /// Trả về địa chỉ ví, seed phrase, và ID người dùng.
    ///
    /// # Errors
    /// Trả về lỗi nếu không thể tạo seed hoặc ví đã tồn tại.
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

        let user_id = generate_user_id();
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

    /// Nhập ví từ seed phrase hoặc private key.
    ///
    /// # Arguments
    /// - `config`: Cấu hình ví (seed phrase/private key, chain ID, seed length).
    ///
    /// # Returns
    /// Trả về địa chỉ ví và ID người dùng.
    ///
    /// # Errors
    /// Trả về lỗi nếu seed/key không hợp lệ hoặc ví đã tồn tại.
    pub fn import_wallet_internal(
        &mut self,
        config: WalletConfig,
    ) -> Result<(Address, String), WalletError> {
        info!("Importing wallet with chain_id: {}", config.chain_id);

        let (wallet, secret) = if is_seed_phrase(&config) {
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

        let user_id = generate_user_id();
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
}