use ethers::types::Address;
use thiserror::Error;
use std::fmt;

#[derive(Error, Debug)]
pub enum WalletError {
    #[error("Invalid seed or private key: {0}")]
    InvalidSeedOrKey(String),
    #[error("Wallet already exists: {0}")]
    WalletExists(Address),
    #[error("Wallet not found: {0}")]
    WalletNotFound(Address),
    #[error("Failed to generate seed phrase: {0}")]
    SeedGenerationFailed(String),
    #[error("Encryption error: {0}")]
    EncryptionError(String),
    #[error("Decryption error: {0}")]
    DecryptionError(String),
    #[error("Invalid password")]
    InvalidPassword,
    #[error("Transaction failed: {0}")]
    TransactionFailed(String),
    #[error("Chain not supported: {0}")]
    ChainNotSupported(String),
    #[error("Provider error: {0}")]
    ProviderError(String),
    #[error("Anyhow error: {0}")]
    AnyhowError(String),
}

impl From<anyhow::Error> for WalletError {
    fn from(err: anyhow::Error) -> Self {
        // Cố gắng chuyển đổi nếu là WalletError
        if let Some(wallet_err) = err.downcast_ref::<WalletError>() {
            return wallet_err.clone();
        }
        
        // Nếu không, chuyển thành AnyhowError với message đầy đủ
        WalletError::AnyhowError(format!("{:#}", err))
    }
}

// Thêm Clone cho WalletError
impl Clone for WalletError {
    fn clone(&self) -> Self {
        match self {
            Self::InvalidSeedOrKey(s) => Self::InvalidSeedOrKey(s.clone()),
            Self::WalletExists(addr) => Self::WalletExists(*addr),
            Self::WalletNotFound(addr) => Self::WalletNotFound(*addr),
            Self::SeedGenerationFailed(s) => Self::SeedGenerationFailed(s.clone()),
            Self::EncryptionError(s) => Self::EncryptionError(s.clone()),
            Self::DecryptionError(s) => Self::DecryptionError(s.clone()),
            Self::InvalidPassword => Self::InvalidPassword,
            Self::TransactionFailed(s) => Self::TransactionFailed(s.clone()),
            Self::ChainNotSupported(s) => Self::ChainNotSupported(s.clone()),
            Self::ProviderError(s) => Self::ProviderError(s.clone()),
            Self::AnyhowError(s) => Self::AnyhowError(s.clone()),
        }
    }
}