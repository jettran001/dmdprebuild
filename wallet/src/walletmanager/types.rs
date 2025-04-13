// External imports
use ethers::signers::LocalWallet;
use serde::{Deserialize, Serialize};

// Internal imports
use crate::walletmanager::chain::ChainType;

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

/// Thông tin ví được lưu trữ.
#[derive(Debug, Clone)]
pub struct WalletInfo {
    /// Đối tượng ví từ thư viện ethers.
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