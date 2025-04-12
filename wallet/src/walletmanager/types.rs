// wallet/src/walletmanager/types.rs

use ethers::signers::LocalWallet;
use serde::{Deserialize, Serialize};

/// Cấu hình cho một ví khi nhập hoặc tạo.
#[derive(Debug, Serialize, Deserialize)]
pub struct WalletConfig {
    /// Seed phrase hoặc private key.
    pub seed_or_key: String,
    /// Chain ID mà ví sẽ hoạt động.
    pub chain_id: u64,
    /// Loại seed phrase: 12 hoặc 24 từ, hoặc None nếu là private key.
    pub seed_length: Option<SeedLength>,
}

/// Loại seed phrase: 12 hoặc 24 từ.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SeedLength {
    /// Seed phrase 12 từ.
    Twelve,
    /// Seed phrase 24 từ.
    TwentyFour,
}

/// Bí mật của ví: seed phrase hoặc private key.
#[derive(Debug, Clone)]
pub enum WalletSecret {
    /// Seed phrase (12 hoặc 24 từ).
    Seed(String),
    /// Private key (hex string).
    PrivateKey(String),
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