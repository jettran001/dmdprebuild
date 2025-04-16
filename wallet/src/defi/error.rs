//! Các error type cho module DeFi
//! 
//! Bao gồm các error type cho:
//! - Cấu hình và tham số
//! - Pool và user
//! - Staking và farming
//! - Blockchain
//! - Transaction
//! - Support

use thiserror::Error;
use ethers::types::{Address, U256};

/// Các loại lỗi có thể xảy ra trong module DeFi
#[derive(Error, Debug)]
pub enum DefiError {
    /// Cấu hình và tham số
    #[error("Cấu hình không hợp lệ: {0}")]
    InvalidConfig(String),

    /// Số lượng không hợp lệ
    #[error("Số lượng không hợp lệ: {0}")]
    InvalidAmount(U256),

    /// Số dư không đủ
    #[error("Số dư không đủ: {0}")]
    InsufficientBalance(U256),

    /// Pool không tồn tại
    #[error("Không tìm thấy pool: {0}")]
    PoolNotFound(Address),

    /// Stake của người dùng không tồn tại
    #[error("Không tìm thấy stake của user: {0}")]
    UserStakeNotFound(String),

    /// Farm của người dùng không tồn tại
    #[error("Không tìm thấy farm của user: {0}")]
    UserFarmNotFound(String),

    /// Kỳ hạn stake không hợp lệ
    #[error("Thời gian stake không hợp lệ: {0}")]
    InvalidStakeTerm(u64),

    /// APY không hợp lệ
    #[error("APY không hợp lệ: {0}")]
    InvalidApy(f64),

    /// Lỗi khi stake chưa đến hạn
    #[error("Stake chưa đến hạn: {0}")]
    StakeNotMature(u64),

    /// Lỗi khi không có reward để claim
    #[error("Không có rewards để claim")]
    NoRewardsToClaim,

    /// Lỗi khi không có liquidity để remove
    #[error("Không có liquidity để remove")]
    NoLiquidityToRemove,

    /// Lỗi từ provider blockchain
    #[error("Lỗi provider: {0}")]
    ProviderError(String),

    /// Lỗi từ contract blockchain
    #[error("Lỗi contract: {0}")]
    ContractError(String),

    /// Lỗi khi timeout
    #[error("Timeout")]
    Timeout,

    /// Lỗi khi thử lại quá nhiều lần
    #[error("Vượt quá số lần thử lại tối đa")]
    MaxRetriesExceeded,

    /// Lỗi khi slippage quá cao
    #[error("Slippage quá cao: {0}")]
    SlippageTooHigh(f64),

    /// Lỗi khi deadline đã hết
    #[error("Quá hạn")]
    DeadlineExceeded,

    /// Lỗi khi chain không được hỗ trợ
    #[error("Chain không được hỗ trợ: {0}")]
    ChainNotSupported(String),
    
    /// Lỗi khi Chain ID không hợp lệ
    #[error("Chain ID không hợp lệ: {0}")]
    InvalidChainId(String),

    /// Lỗi khi token không được hỗ trợ
    #[error("Token không được hỗ trợ: {0}")]
    TokenNotSupported(Address),

    /// Lỗi khi router không được hỗ trợ
    #[error("Router không được hỗ trợ: {0}")]
    RouterNotSupported(Address),

    /// Lỗi bảo mật (mã hóa, giải mã, xác thực)
    #[error("Lỗi bảo mật: {0}")]
    SecurityError(String),
    
    /// Lỗi khi vượt quá giới hạn tốc độ request
    #[error("Vượt quá giới hạn tốc độ request")]
    RateLimitExceeded,
    
    /// Lỗi khi không tìm thấy Chain
    #[error("Không tìm thấy chain với ID: {0}")]
    ChainNotFound(u64),
    
    /// Lỗi khi chức năng chưa được triển khai
    #[error("Chức năng chưa được triển khai: {0}")]
    NotImplemented(String),
    
    /// Lỗi khi giao dịch thất bại
    #[error("Giao dịch thất bại: {0}")]
    TransactionFailed(String),
    
    /// Lỗi khi giải mã thất bại
    #[error("Giải mã thất bại: {0}")]
    DecryptionError(String),
    
    /// Lỗi khi đạt giới hạn số lượng ví
    #[error("Đã đạt giới hạn số lượng ví tối đa: {0}")]
    MaxWalletLimitReached(String),
    
    /// Lỗi khi không tìm thấy ví
    #[error("Không tìm thấy ví với địa chỉ: {0:?}")]
    WalletNotFound(Address),
} 