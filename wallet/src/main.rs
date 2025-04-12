// src/wallet/wallet_manager.rs

use std::collections::HashMap;

use ethers::signers::{LocalWallet, Signer};
use ethers::types::Address;
use hdkey::HDKey;
use serde::{Deserialize, Serialize};

/// Quản lý ví với các chức năng tạo ví, nhập ví, xuất seed phrase và quản lý nhiều ví.
///
/// Struct này lưu trữ danh sách các ví theo địa chỉ và cung cấp các phương thức để thao tác ví.
///
/// # Flow
/// Dữ liệu từ `wallet` -> `snipebot` để thực hiện giao dịch.
#[derive(Debug, Clone)]
pub struct WalletManager {
    /// Danh sách ví, ánh xạ từ địa chỉ sang ví và seed phrase.
    wallets: HashMap<Address, (LocalWallet, Option<String>)>,
}

/// Cấu hình cho một ví khi nhập hoặc tạo.
#[derive(Debug, Serialize, Deserialize)]
pub struct WalletConfig {
    /// Seed phrase hoặc private key.
    pub seed_or_key: String,
    /// Chain ID mà ví sẽ hoạt động.
    pub chain_id: u64,
    /// Loại seed phrase: 12 hoặc 24 từ.
    pub seed_length: SeedLength,
}

/// Loại seed phrase: 12 hoặc 24 từ.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SeedLength {
    /// Seed phrase 12 từ.
    Twelve,
    /// Seed phrase 24 từ.
    TwentyFour,
}

/// Lỗi tùy chỉnh cho WalletManager.
#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    /// Lỗi khi parse seed phrase hoặc private key.
    #[error("Invalid seed or key: {0}")]
    InvalidSeedOrKey(String),
    /// Lỗi khi ví đã tồn tại.
    #[error("Wallet already exists: {0}")]
    WalletExists(Address),
    /// Lỗi khi ví không tồn tại.
    #[error("Wallet not found: {0}")]
    WalletNotFound(Address),
    /// Lỗi khi tạo seed phrase.
    #[error("Failed to generate seed phrase: {0}")]
    SeedGenerationFailed(String),
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

    /// Tạo ví mới với seed phrase 12 hoặc 24 từ.
    ///
    /// # Arguments
    /// - `seed_length`: Loại seed phrase (12 hoặc 24 từ).
    /// - `chain_id`: Chain ID mà ví sẽ hoạt động.
    ///
    /// # Returns
    /// Trả về địa chỉ của ví và seed phrase được tạo.
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
    ) -> Result<(Address, String), WalletError> {
        let entropy_size = match seed_length {
            SeedLength::Twelve => 128,   // 12 từ cần 128 bit entropy
            SeedLength::TwentyFour => 256, // 24 từ cần 256 bit entropy
        };

        let mnemonic = HDKey::generate_mnemonic(entropy_size)
            .map_err(|e| WalletError::SeedGenerationFailed(e.to_string()))?;
        let seed_phrase = mnemonic.phrase().to_string();

        let wallet = mnemonic
            .to_wallet()
            .map_err(|e| WalletError::InvalidSeedOrKey(e.to_string()))?;
        let address = wallet.address();

        if self.wallets.contains_key(&address) {
            return Err(WalletError::WalletExists(address));
        }

        self.wallets.insert(address, (wallet, Some(seed_phrase.clone())));
        Ok((address, seed_phrase))
    }

    /// Nhập ví từ seed phrase hoặc private key.
    ///
    /// # Arguments
    /// - `config`: Cấu hình ví bao gồm seed phrase/private key, chain ID, và loại seed phrase.
    ///
    /// # Returns
    /// Trả về địa chỉ của ví nếu nhập thành công.
    ///
    /// # Errors
    /// Trả về lỗi nếu seed/key không hợp lệ hoặc ví đã tồn tại.
    ///
    /// # Flow
    /// Hàm này nhận dữ liệu từ người dùng và lưu vào `wallet` để sử dụng trong `snipebot`.
    #[flow_from("user_input")]
    pub fn import_wallet(&mut self, config: WalletConfig) -> Result<Address, WalletError> {
        let wallet = if config.seed_length == SeedLength::Twelve
            || config.seed_length == SeedLength::TwentyFour
        {
            HDKey::from_mnemonic(&config.seed_or_key)
                .map_err(|e| WalletError::InvalidSeedOrKey(e.to_string()))?
                .to_wallet()
                .map_err(|e| WalletError::InvalidSeedOrKey(e.to_string()))?
        } else {
            config
                .seed_or_key
                .parse::<LocalWallet>()
                .map_err(|e| WalletError::InvalidSeedOrKey(e.to_string()))?
        };

        let address = wallet.address();

        if self.wallets.contains_key(&address) {
            return Err(WalletError::WalletExists(address));
        }

        let seed_phrase = if config.seed_length == SeedLength::Twelve
            || config.seed_length == SeedLength::TwentyFour
        {
            Some(config.seed_or_key)
        } else {
            None
        };

        self.wallets.insert(address, (wallet, seed_phrase));
        Ok(address)
    }

    /// Xuất seed phrase hoặc private key của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví cần xuất.
    /// - `seed_length`: Loại seed phrase (12 hoặc 24 từ) hoặc None nếu xuất private key.
    ///
    /// # Returns
    /// Trả về seed phrase hoặc private key dưới dạng chuỗi.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại hoặc không có seed phrase.
    ///
    /// # Flow
    /// Hàm này lấy dữ liệu từ `wallet` để cung cấp cho người dùng hoặc `snipebot`.
    #[flow_from("wallet")]
    pub fn export_seed_phrase(
        &self,
        address: Address,
        seed_length: Option<SeedLength>,
    ) -> Result<String, WalletError> {
        let (wallet, seed_phrase) = self
            .wallets
            .get(&address)
            .ok_or(WalletError::WalletNotFound(address))?;

        match (seed_length, seed_phrase) {
            (Some(SeedLength::Twelve) | Some(SeedLength::TwentyFour), Some(phrase)) => Ok(phrase.clone()),
            (Some(_), None) => Err(WalletError::InvalidSeedOrKey(
                "No seed phrase available for this wallet".to_string(),
            )),
            (None, _) => Ok(format!("{:?}", wallet.signer())),
        }
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
            .0)
    }

    /// Liệt kê tất cả các ví đang quản lý.
    ///
    /// # Returns
    /// Trả về danh sách địa chỉ của các ví.
    pub fn list_wallets(&self) -> Vec<Address> {
        self.wallets.keys().copied().collect()
    }
}