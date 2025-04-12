// wallet/error.rs

use ethers::types::Address;
use thiserror::Error;

#[derive(Debug, Error)]
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