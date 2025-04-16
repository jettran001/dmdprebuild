// External imports
use ethers::signers::LocalWallet;
use serde::{Deserialize, Serialize};
use anyhow::{Context, Result};

// Internal imports
use crate::walletmanager::chain::ChainType;
use crate::error::WalletError;

/// Độ dài của seed phrase.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SeedLength {
    /// Seed phrase 12 từ (khuyến nghị).
    Twelve,
    /// Seed phrase 24 từ (bảo mật cao hơn).
    TwentyFour,
}

/// Loại dữ liệu bí mật của ví.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WalletSecret {
    /// Seed phrase - CHỈ sử dụng cho logic tạm thời, không lưu trữ.
    Seed(String),
    /// Private key - CHỈ sử dụng cho logic tạm thời, không lưu trữ.
    PrivateKey(String),
    /// Dữ liệu đã được mã hóa, không lưu trữ bản rõ.
    Encrypted,
}

/// Cấu hình khi tạo hoặc nhập ví.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletConfig {
    /// Seed phrase hoặc private key.
    pub seed_or_key: String,
    /// Chain ID của blockchain.
    pub chain_id: u64,
    /// Loại blockchain.
    pub chain_type: ChainType,
    /// Độ dài seed phrase (nếu có).
    pub seed_length: Option<SeedLength>,
    /// Mật khẩu để mã hóa dữ liệu.
    pub password: String,
}

impl WalletConfig {
    /// Kiểm tra tính hợp lệ của WalletConfig
    pub fn validate(&self) -> Result<(), WalletError> {
        // Kiểm tra seed_or_key
        if self.seed_or_key.trim().is_empty() {
            return Err(WalletError::InvalidSeedOrKey("Seed phrase hoặc private key không được để trống".to_string()));
        }

        // Kiểm tra mật khẩu
        if self.password.trim().is_empty() {
            return Err(WalletError::InvalidPassword("Mật khẩu không được để trống".to_string()));
        }

        // Kiểm tra độ dài mật khẩu
        if self.password.len() < 8 {
            return Err(WalletError::InvalidPassword("Mật khẩu phải có ít nhất 8 ký tự".to_string()));
        }
        
        // Kiểm tra độ phức tạp của mật khẩu
        let has_digit = self.password.chars().any(|c| c.is_digit(10));
        let has_lowercase = self.password.chars().any(|c| c.is_lowercase());
        let has_uppercase = self.password.chars().any(|c| c.is_uppercase());
        let has_special = self.password.chars().any(|c| !c.is_alphanumeric());
        
        if !(has_digit && has_lowercase && (has_uppercase || has_special)) {
            return Err(WalletError::InvalidPassword(
                "Mật khẩu phải có ít nhất 1 chữ số, 1 chữ thường, và 1 chữ hoa hoặc ký tự đặc biệt".to_string()
            ));
        }
        
        // Kiểm tra chain_id
        if self.chain_id == 0 {
            return Err(WalletError::InvalidChainId("Chain ID không hợp lệ".to_string()));
        }

        Ok(())
    }
}

/// Thông tin ví được lưu trữ.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletInfo {
    /// Đối tượng ví từ thư viện ethers.
    #[serde(skip_serializing, skip_deserializing)]
    pub wallet: LocalWallet,
    /// Loại dữ liệu bí mật (thường sẽ là Encrypted).
    pub secret: WalletSecret,
    /// Chain ID của blockchain.
    pub chain_id: u64,
    /// Loại blockchain.
    pub chain_type: ChainType,
    /// ID người dùng liên kết với ví.
    pub user_id: String,
    /// Dữ liệu bí mật đã được mã hóa.
    pub encrypted_secret: Vec<u8>,
    /// Nonce dùng trong quá trình mã hóa.
    pub nonce: Vec<u8>,
    /// Salt dùng trong quá trình mã hóa.
    pub salt: Vec<u8>, 
}