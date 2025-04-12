// wallet/src/walletmanager/walletlogic.rs

use std::collections::HashMap;

use ethers::signers::{LocalWallet, Signer};
use ethers::types::Address;
use hdkey::HDKey;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{info, warn};
use uuid::Uuid;

use crate::error::WalletError;
use crate::walletmanager::types::{WalletConfig, SeedLength, WalletSecret};

/// Quản lý ví với các chức năng tạo ví, nhập ví, xuất seed phrase/private key, xóa ví, cập nhật chain ID, truy xuất chain ID, kiểm tra ví và quản lý người dùng.
///
/// Struct này lưu trữ danh sách các ví theo địa chỉ và cung cấp các phương thức để thao tác ví.
///
/// # Flow
/// Dữ liệu từ `wallet` -> `snipebot` để thực hiện giao dịch.
#[derive(Debug, Clone)]
pub struct WalletManager {
    /// Danh sách ví, ánh xạ từ địa chỉ sang thông tin ví.
    wallets: HashMap<Address, WalletInfo>,
}

/// Thông tin của một ví, bao gồm ví, bí mật, chain ID và ID người dùng.
#[derive(Debug, Clone)]
pub struct WalletInfo {
    /// Ví Ethereum.
    pub wallet: LocalWallet,
    /// Bí mật của ví (seed phrase hoặc private key).
    pub secret: WalletSecret,
    /// Chain ID mà ví hoạt động.
    pub chain_id: u64,
    /// ID duy nhất của người dùng sở hữu ví.
    pub user_id: String,
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
    fn generate_user_id() -> String {
        Uuid::new_v4().to_string()
    }

    /// Kiểm tra xem config có chứa seed phrase không.
    ///
    /// # Arguments
    /// - `config`: Cấu hình ví.
    ///
    /// # Returns
    /// Trả về `true` nếu là seed phrase (12 hoặc 24 từ), `false` nếu là private key.
    fn is_seed_phrase(config: &WalletConfig) -> bool {
        matches!(
            config.seed_length,
            Some(SeedLength::Twelve) | Some(SeedLength::TwentyFour)
        )
    }

    /// Tạo ví mới với seed phrase 12 hoặc 24 từ.
    ///
    /// # Arguments
    /// - `seed_length`: Loại seed phrase (12 hoặc 24 từ).
    /// - `chain_id`: Chain ID mà ví sẽ hoạt động.
    ///
    /// # Returns
    /// Trả về địa chỉ của ví, seed phrase được tạo và ID người dùng.
    ///
    /// # Errors
    /// Trả về lỗi nếu không thể tạo seed phrase hoặc ví đã tồn tại.
    ///
    /// # Flow
    /// Hàm này tạo ví mới và lưu vào `wallet` để sử dụng trong `snipebot`.
    #[flow_from("user_request")]
    pub fn create_wallet(
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

    /// Nhập ví từ seed phrase hoặc private key.
    ///
    /// # Arguments
    /// - `config`: Cấu hình ví bao gồm seed phrase/private key, chain ID, và loại seed phrase.
    ///
    /// # Returns
    /// Trả về địa chỉ của ví và ID người dùng nếu nhập thành công.
    ///
    /// # Errors
    /// Trả về lỗi nếu seed/key không hợp lệ hoặc ví đã tồn tại.
    ///
    /// # Flow
    /// Hàm này nhận dữ liệu từ người dùng và lưu vào `wallet` để sử dụng trong `snipebot`.
    #[flow_from("user_input")]
    pub fn import_wallet(&mut self, config: WalletConfig) -> Result<(Address, String), WalletError> {
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

    /// Xuất seed phrase của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví cần xuất.
    ///
    /// # Returns
    /// Trả về seed phrase dưới dạng chuỗi.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại hoặc không có seed phrase.
    ///
    /// # Flow
    /// Hàm này lấy dữ liệu từ `wallet` để cung cấp cho người dùng hoặc `snipebot`.
    #[flow_from("wallet")]
    pub fn export_seed_phrase(&self, address: Address) -> Result<String, WalletError> {
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

    /// Xuất private key của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví cần xuất.
    ///
    /// # Returns
    /// Trả về private key dưới dạng chuỗi hex.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại hoặc không có private key trực tiếp.
    ///
    /// # Flow
    /// Hàm này lấy dữ liệu từ `wallet` để cung cấp cho người dùng hoặc `snipebot`.
    #[flow_from("wallet")]
    pub fn export_private_key(&self, address: Address) -> Result<String, WalletError> {
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

    /// Xóa ví khỏi danh sách quản lý.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví cần xóa.
    ///
    /// # Returns
    /// Trả về `Ok(())` nếu xóa thành công.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại.
    ///
    /// # Flow
    /// Hàm này cập nhật `wallet` để loại bỏ ví trước khi sử dụng trong `snipebot`.
    #[flow_from("user_request")]
    pub fn remove_wallet(&mut self, address: Address) -> Result<(), WalletError> {
        if let Some(info) = self.wallets.remove(&address) {
            info!("Wallet {} removed successfully, user_id: {}", address, info.user_id);
            Ok(())
        } else {
            warn!("Attempted to remove non-existent wallet {}", address);
            Err(WalletError::WalletNotFound(address))
        }
    }

    /// Cập nhật chain ID cho ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví cần cập nhật.
    /// - `new_chain_id`: Chain ID mới.
    ///
    /// # Returns
    /// Trả về `Ok(())` nếu cập nhật thành công.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại.
    ///
    /// # Flow
    /// Hàm này cập nhật `wallet` để sử dụng chain ID mới trong `snipebot`.
    #[flow_from("user_request")]
    pub fn update_chain_id(&mut self, address: Address, new_chain_id: u64) -> Result<(), WalletError> {
        info!("Updating chain ID for wallet {} to {}", address, new_chain_id);

        let info = self
            .wallets
            .get_mut(&address)
            .ok_or(WalletError::WalletNotFound(address))?;
        info.chain_id = new_chain_id;

        info!("Chain ID updated successfully for wallet {}", address);
        Ok(())
    }

    /// Lấy chain ID của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví.
    ///
    /// # Returns
    /// Trả về chain ID nếu ví tồn tại.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại.
    ///
    /// # Flow
    /// Hàm này cung cấp chain ID từ `wallet` để sử dụng trong `snipebot`.
    #[flow_from("wallet")]
    pub fn get_chain_id(&self, address: Address) -> Result<u64, WalletError> {
        self.wallets
            .get(&address)
            .map(|info| info.chain_id)
            .ok_or(WalletError::WalletNotFound(address))
    }

    /// Kiểm tra xem ví đã được quản lý chưa.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví.
    ///
    /// # Returns
    /// `true` nếu ví tồn tại, `false` nếu không.
    ///
    /// # Flow
    /// Hàm này kiểm tra trạng thái `wallet` để hỗ trợ frontend hoặc automation trong `snipebot`.
    #[flow_from("wallet")]
    pub fn has_wallet(&self, address: Address) -> bool {
        self.wallets.contains_key(&address)
    }

    /// Lấy ví theo địa chỉ.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví.
    ///
    /// # Returns
    /// Trả về tham chiếu đến `LocalWallet` nếu tồn tại.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại.
    ///
    /// # Flow
    /// Hàm này cung cấp ví cho `snipebot` để thực hiện giao dịch.
    #[flow_from("wallet")]
    pub fn get_wallet(&self, address: Address) -> Result<&LocalWallet, WalletError> {
        Ok(&self
            .wallets
            .get(&address)
            .ok_or(WalletError::WalletNotFound(address))?
            .wallet)
    }

    /// Lấy ID người dùng của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví.
    ///
    /// # Returns
    /// Trả về ID người dùng nếu ví tồn tại.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại.
    ///
    /// # Flow
    /// Hàm này cung cấp ID người dùng từ `wallet` để sử dụng trong `snipebot`.
    #[flow_from("wallet")]
    pub fn get_user_id(&self, address: Address) -> Result<String, WalletError> {
        self.wallets
            .get(&address)
            .map(|info| info.user_id.clone())
            .ok_or(WalletError::WalletNotFound(address))
    }

    /// Liệt kê tất cả các ví đang quản lý.
    ///
    /// # Returns
    /// Trả về danh sách địa chỉ của các ví.
    pub fn list_wallets(&self) -> Vec<Address> {
        self.wallets.keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_wallet() {
        let mut manager = WalletManager::new();
        let result = manager.create_wallet(SeedLength::Twelve, 1);
        assert!(result.is_ok());
        let (address, _, user_id) = result.unwrap();
        assert!(manager.get_wallet(address).is_ok());
        assert_eq!(manager.get_user_id(address).unwrap(), user_id);
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
        let result = manager.import_wallet(config);
        assert!(result.is_ok());
        let (address, user_id) = result.unwrap();
        assert_eq!(manager.get_user_id(address).unwrap(), user_id);
    }

    #[test]
    fn test_import_wallet_private_key() {
        let mut manager = WalletManager::new();
        let config = WalletConfig {
            seed_or_key: "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
            chain_id: 1,
            seed_length: None,
        };
        let result = manager.import_wallet(config);
        assert!(result.is_ok());
        let (address, user_id) = result.unwrap();
        assert_eq!(manager.get_user_id(address).unwrap(), user_id);
    }

    #[test]
    fn test_export_seed_phrase() {
        let mut manager = WalletManager::new();
        let (address, seed_phrase, _) = manager.create_wallet(SeedLength::Twelve, 1).unwrap();
        let exported = manager.export_seed_phrase(address).unwrap();
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
        let (address, _) = manager.import_wallet(config).unwrap();
        let exported = manager.export_private_key(address).unwrap();
        assert_eq!(exported, private_key);
    }

    #[test]
    fn test_remove_wallet() {
        let mut manager = WalletManager::new();
        let (address, _, _) = manager.create_wallet(SeedLength::Twelve, 1).unwrap();
        assert!(manager.remove_wallet(address).is_ok());
        assert!(manager.get_wallet(address).is_err());
    }

    #[test]
    fn test_update_chain_id() {
        let mut manager = WalletManager::new();
        let (address, _, _) = manager.create_wallet(SeedLength::Twelve, 1).unwrap();
        assert!(manager.update_chain_id(address, 56).is_ok());
        assert_eq!(manager.get_chain_id(address).unwrap(), 56);
    }

    #[test]
    fn test_get_chain_id() {
        let mut manager = WalletManager::new();
        let (address, _, _) = manager.create_wallet(SeedLength::Twelve, 1).unwrap();
        assert_eq!(manager.get_chain_id(address).unwrap(), 1);
    }

    #[test]
    fn test_has_wallet() {
        let mut manager = WalletManager::new();
        let (address, _, _) = manager.create_wallet(SeedLength::Twelve, 1).unwrap();
        assert!(manager.has_wallet(address));
        assert!(!manager.has_wallet(Address::zero()));
    }
}