//! Định nghĩa lỗi và kết quả cho module bridge.

use thiserror::Error;
use crate::smartcontracts::dmd_token::DmdChain;

/// Kết quả trả về của các hàm trong module bridge
pub type BridgeResult<T> = Result<T, BridgeError>;

/// Lỗi trong module bridge
#[derive(Error, Debug)]
pub enum BridgeError {
    /// Giao dịch không tìm thấy
    #[error("Không tìm thấy giao dịch: {0}")]
    TransactionNotFound(String),

    /// Route bridge không được hỗ trợ
    #[error("Route bridge không được hỗ trợ: {0}")]
    UnsupportedRoute(String),

    /// Chain không được hỗ trợ
    #[error("Chain không được hỗ trợ: {0}")]
    UnsupportedChain(String),

    /// Số lượng không hợp lệ
    #[error("Số lượng không hợp lệ: {0}")]
    InvalidAmount(String),

    /// Địa chỉ không hợp lệ
    #[error("Địa chỉ không hợp lệ: {0}")]
    InvalidAddress(String),

    /// Trạng thái không hợp lệ
    #[error("Trạng thái không hợp lệ: {0}")]
    InvalidStatus(String),

    /// Giao dịch thất bại
    #[error("Giao dịch thất bại: {0}")]
    TransactionFailed(String),

    /// Lỗi provider
    #[error("Lỗi provider: {0}")]
    ProviderError(String),

    /// Lỗi hệ thống
    #[error("Lỗi hệ thống: {0}")]
    SystemError(String),
}

/// Kiểm tra xem bridge giữa hai chain có được hỗ trợ không
pub fn is_bridge_supported(source: &DmdChain, target: &DmdChain) -> bool {
    match (source, target) {
        // NEAR <-> EVM chains được hỗ trợ
        (DmdChain::Near, target) if is_evm_chain(target) => true,
        (source, DmdChain::Near) if is_evm_chain(source) => true,
        
        // EVM <-> EVM chains được hỗ trợ
        (source, target) if is_evm_chain(source) && is_evm_chain(target) => true,
        
        // NEAR <-> Solana được hỗ trợ
        (DmdChain::Near, DmdChain::Solana) | (DmdChain::Solana, DmdChain::Near) => true,
        
        // Các trường hợp khác không được hỗ trợ
        _ => false,
    }
}

/// Kiểm tra xem chain có phải là EVM không
pub fn is_evm_chain(chain: &DmdChain) -> bool {
    matches!(
        chain,
        DmdChain::Ethereum
            | DmdChain::BinanceSmartChain
            | DmdChain::Avalanche
            | DmdChain::Polygon
            | DmdChain::Arbitrum
            | DmdChain::Optimism
            | DmdChain::Base
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_bridge_supported() {
        // NEAR <-> EVM
        assert!(is_bridge_supported(&DmdChain::Near, &DmdChain::Ethereum));
        assert!(is_bridge_supported(&DmdChain::Ethereum, &DmdChain::Near));
        assert!(is_bridge_supported(&DmdChain::Near, &DmdChain::BinanceSmartChain));
        assert!(is_bridge_supported(&DmdChain::BinanceSmartChain, &DmdChain::Near));
        
        // EVM <-> EVM
        assert!(is_bridge_supported(&DmdChain::Ethereum, &DmdChain::BinanceSmartChain));
        assert!(is_bridge_supported(&DmdChain::Polygon, &DmdChain::Avalanche));
        
        // NEAR <-> Solana
        assert!(is_bridge_supported(&DmdChain::Near, &DmdChain::Solana));
        assert!(is_bridge_supported(&DmdChain::Solana, &DmdChain::Near));
        
        // Không hỗ trợ
        assert!(!is_bridge_supported(&DmdChain::Solana, &DmdChain::Ethereum));
        assert!(!is_bridge_supported(&DmdChain::Ethereum, &DmdChain::Solana));
    }

    #[test]
    fn test_is_evm_chain() {
        assert!(is_evm_chain(&DmdChain::Ethereum));
        assert!(is_evm_chain(&DmdChain::BinanceSmartChain));
        assert!(is_evm_chain(&DmdChain::Avalanche));
        assert!(is_evm_chain(&DmdChain::Polygon));
        assert!(is_evm_chain(&DmdChain::Arbitrum));
        assert!(is_evm_chain(&DmdChain::Optimism));
        assert!(is_evm_chain(&DmdChain::Base));
        
        assert!(!is_evm_chain(&DmdChain::Near));
        assert!(!is_evm_chain(&DmdChain::Solana));
    }
} 