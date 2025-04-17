//! Module định nghĩa các lỗi cho Oracle

/// Kết quả truy vấn Oracle
pub type OracleResult<T> = Result<T, OracleError>;

/// Lỗi Oracle
#[derive(Debug, thiserror::Error)]
pub enum OracleError {
    /// Dữ liệu không tìm thấy
    #[error("Dữ liệu không tìm thấy: {0}")]
    DataNotFound(String),
    
    /// Lỗi quyền truy cập
    #[error("Không có quyền truy cập: {0}")]
    AccessDenied(String),
    
    /// Lỗi kết nối
    #[error("Lỗi kết nối: {0}")]
    ConnectionError(String),
    
    /// Dữ liệu không hợp lệ
    #[error("Dữ liệu không hợp lệ: {0}")]
    InvalidData(String),
    
    /// Lỗi xác thực
    #[error("Lỗi xác thực: {0}")]
    AuthenticationError(String),

    /// Lỗi đồng bộ
    #[error("Lỗi đồng bộ: {0}")]
    SynchronizationError(String),
    
    /// Lỗi ngoại lệ
    #[error("Lỗi ngoại lệ: {0}")]
    InternalError(String),
    
    /// Lỗi từ Bridge
    #[error("Lỗi từ Bridge: {0}")]
    BridgeError(String),
    
    /// Lỗi từ Stake
    #[error("Lỗi từ Stake: {0}")]
    StakeError(String),
    
    /// Lỗi từ Farm
    #[error("Lỗi từ Farm: {0}")]
    FarmError(String),
    
    /// Lỗi từ Exchange
    #[error("Lỗi từ Exchange: {0}")]
    ExchangeError(String),
    
    /// Lỗi từ Smart Contract
    #[error("Lỗi từ Smart Contract: {0}")]
    SmartContractError(String),
} 