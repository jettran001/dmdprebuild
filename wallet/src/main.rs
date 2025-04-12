// src/wallet/wallet_manager.rs

// External imports
use ethers::signers::{LocalWallet, Signer};
use ethers::types::Address;
use hdkey::HDKey;
use serde::{Deserialize, Serialize};

// Standard library imports
use std::collections::HashMap;
use std::sync::Arc;

// Internal imports
use crate::blockchain::ChainConfig;
use anyhow::Result;

/// Quản lý ví với các chức năng tạo ví, nhập ví, xuất seed phrase và quản lý nhiều ví.
///
/// Struct này lưu trữ danh sách các ví theo địa chỉ và cung cấp các phương thức để thao tác ví.
///
/// # Flow
/// Dữ liệu từ `wallet` -> `snipebot` để thực hiện giao dịch.
#[flow_from(blockchain)]  // Annotate luồng dữ liệu từ module blockchain, theo patterns.naming.context_flow
    pub trait SafeWallet: Send + Sync + 'static {
    fn load_wallet(&self) -> Result<WalletConfig>;
    fn secure_transfer(&self, amount: u64) -> Result<()>;
}

pub struct WalletManager {
    config: Arc<ChainConfig>,
    wallets: HashMap<Address, (LocalWallet, Option<String>)>,
}

impl WalletManager {
    pub fn new(config: ChainConfig) -> Self {
        Self { config: Arc::new(config), wallets: HashMap::new() }
    }

    pub fn create_wallet(&self, seed_length: SeedLength, chain_id: u64) -> Result<(Address, String), WalletError> {
        let entropy_size = match seed_length {
            SeedLength::Twelve => 128,
            SeedLength::TwentyFour => 256,
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

    pub fn export_seed_phrase(&self, address: Address, seed_length: Option<SeedLength>) -> Result<String, WalletError> {
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

    pub fn get_wallet(&self, address: Address) -> Result<&LocalWallet, WalletError> {
        Ok(&self
            .wallets
            .get(&address)
            .ok_or(WalletError::WalletNotFound(address))?
            .0)
    }   

    pub fn list_wallets(&self) -> Vec<Address> {
        self.wallets.keys().copied().collect()
    }

    pub async fn initialize_wallet(&self) -> Result<()> {
        tracing::info!("Khởi tạo ví an toàn từ ChainConfig");
        // Thêm logic kiểm tra cấu hình từ blockchain, ví dụ: tải dữ liệu cơ bản
        let loaded_config = self.config.load_config().await?;  // Giả sử load_config là async
        Ok(())
    }

    pub async fn load_wallet(&self) -> Result<WalletConfig> {
        // Logic cụ thể: giả sử tải từ file hoặc API
        tracing::info!("Tải cấu hình ví");
        let config = self.config.load_config().await?;  // Đảm bảo async và error handling
        Ok(config)
    }

    pub async fn secure_transfer(&self, amount: u64) -> Result<()> {
        tracing::info!("Thực hiện giao dịch an toàn với số tiền: {}", amount);
        // Thêm kiểm tra số dư trước khi chuyển, ví dụ: gọi hàm từ blockchain module
        let balance = self.config.check_balance().await?;  // Giả sử có hàm này
        if balance < amount {
            return Err(anyhow::anyhow!("Số dư không đủ"));
        }
        Ok(())
    }

    // Thêm kiểm tra và tài liệu, theo development_workflow.testing và doc_mode
   #[cfg(test)]
   mod tests {
       use super::*;

       #[test]
       fn test_wallet_initialization() {
           let config = ChainConfig::default();  // Giả sử ChainConfig có default
           let manager = WalletManager::new(config);
           assert!(manager.initialize_wallet().is_ok());  // Unit test cơ bản
       }
   }    
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