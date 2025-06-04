//! Module cung cấp các hàm tiện ích để kiểm tra đầu vào và xác thực dữ liệu
//! 
//! Module này bao gồm các hàm để:
//! - Kiểm tra địa chỉ Ethereum
//! - Xác thực số lượng token
//! - Kiểm tra định dạng giao dịch
//! - Các hàm kiểm tra đầu vào khác
//! 
//! Mục tiêu của module này là tạo ra một nơi tập trung cho các hàm xác thực 
//! được sử dụng trong toàn bộ dự án, giúp đảm bảo tính nhất quán và chính xác.

use ethers::types::{Address, U256};
use regex::Regex;
use once_cell::sync::Lazy;
use std::str::FromStr;
use sha3::{Digest, Keccak256};
use thiserror::Error;

use crate::defi::error::DefiError;

/// Lỗi xác thực đầu vào
#[derive(Error, Debug, PartialEq)]
pub enum ValidationError {
    /// Địa chỉ không hợp lệ
    #[error("Invalid Ethereum address: {0}")]
    InvalidAddress(String),
    
    /// Số lượng token không hợp lệ
    #[error("Invalid token amount: {0}")]
    InvalidAmount(String),
    
    /// Giá trị vượt quá giới hạn
    #[error("Value exceeds limit: {0}")]
    ExceedsLimit(String),
    
    /// Định dạng không hợp lệ
    #[error("Invalid format: {0}")]
    InvalidFormat(String),
}

/// Regex cho địa chỉ Ethereum
static ETHEREUM_ADDRESS_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^0x[0-9a-fA-F]{40}$").unwrap()
});

/// Kiểm tra địa chỉ Ethereum hợp lệ với đầy đủ các bước kiểm tra
/// 
/// Hàm này thực hiện các kiểm tra:
/// 1. Kiểm tra định dạng (0x + 40 ký tự hex)
/// 2. Kiểm tra checksum EIP-55 nếu địa chỉ chứa cả chữ hoa và chữ thường
/// 
/// # Arguments
/// * `address` - Chuỗi địa chỉ Ethereum cần kiểm tra
/// 
/// # Returns
/// * `true` nếu địa chỉ hợp lệ
/// * `false` nếu địa chỉ không hợp lệ
/// 
/// # Examples
/// ```
/// use wallet::defi::utils::is_valid_ethereum_address;
/// 
/// // Địa chỉ hợp lệ không checksum
/// assert!(is_valid_ethereum_address("0x0000000000000000000000000000000000000000"));
/// 
/// // Địa chỉ hợp lệ với checksum
/// assert!(is_valid_ethereum_address("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"));
/// 
/// // Địa chỉ không hợp lệ (thiếu ký tự)
/// assert!(!is_valid_ethereum_address("0x0000"));
/// 
/// // Không phải địa chỉ Ethereum
/// assert!(!is_valid_ethereum_address("not_an_address"));
/// ```
pub fn is_valid_ethereum_address(address: &str) -> bool {
    // Kiểm tra định dạng
    if !ETHEREUM_ADDRESS_REGEX.is_match(address) {
        return false;
    }
    
    // Nếu địa chỉ không chứa cả chữ hoa và chữ thường, không cần kiểm tra checksum
    let has_upper = address.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = address.chars().any(|c| c.is_ascii_lowercase());
    
    if !has_upper || !has_lower {
        return true;
    }
    
    // Kiểm tra checksum theo EIP-55
    validate_checksum(address)
}

/// Xác thực địa chỉ Ethereum với mã lỗi cụ thể
/// 
/// # Arguments
/// * `address` - Chuỗi địa chỉ Ethereum cần xác thực
/// 
/// # Returns
/// * `Ok(Address)` - Nếu địa chỉ hợp lệ, chuyển đổi thành kiểu Address
/// * `Err(ValidationError)` - Nếu địa chỉ không hợp lệ
/// 
/// # Examples
/// ```
/// use wallet::defi::utils::validate_ethereum_address;
/// 
/// // Địa chỉ hợp lệ
/// let result = validate_ethereum_address("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed");
/// assert!(result.is_ok());
/// 
/// // Địa chỉ không hợp lệ
/// let result = validate_ethereum_address("invalid");
/// assert!(result.is_err());
/// ```
pub fn validate_ethereum_address(address: &str) -> Result<Address, ValidationError> {
    if !is_valid_ethereum_address(address) {
        return Err(ValidationError::InvalidAddress(address.to_string()));
    }
    
    // Chuyển đổi sang kiểu Address
    match Address::from_str(address) {
        Ok(addr) => Ok(addr),
        Err(_) => Err(ValidationError::InvalidAddress(
            format!("Failed to parse valid-looking address: {}", address)
        )),
    }
}

/// Kiểm tra checksum của địa chỉ Ethereum theo EIP-55
/// 
/// # Arguments
/// * `address` - Địa chỉ Ethereum có chữ hoa chữ thường
/// 
/// # Returns
/// * `true` - Nếu checksum hợp lệ
/// * `false` - Nếu checksum không hợp lệ
fn validate_checksum(address: &str) -> bool {
    // Bỏ "0x" và chuyển đổi về chữ thường
    let address_lower = address[2..].to_lowercase();
    
    // Tính hash
    let hash_bytes = Keccak256::digest(address_lower.as_bytes());
    let hash = hex::encode(hash_bytes);
    
    // Kiểm tra từng ký tự
    for (i, ch) in address_lower.chars().enumerate() {
        // Nếu là chữ cái
        if ch >= 'a' && ch <= 'f' {
            // Lấy giá trị hex từ hash
            let hash_value = u8::from_str_radix(&hash[i..i+1], 16).unwrap_or(0);
            
            // Nếu bit thứ 4 đã được set (giá trị >= 8)
            let should_be_uppercase = hash_value >= 8;
            let is_uppercase = address[i+2..i+3].chars().next().unwrap().is_uppercase();
            
            if should_be_uppercase != is_uppercase {
                return false;
            }
        }
    }
    
    true
}

/// Kiểm tra số lượng token có hợp lệ không
/// 
/// Hàm này xác minh rằng:
/// 1. Số lượng token không âm
/// 2. Số lượng token không vượt quá giới hạn tối đa (nếu được cung cấp)
/// 
/// # Arguments
/// * `amount` - Số lượng token cần kiểm tra
/// * `max_value` - Giới hạn tối đa (optional)
/// 
/// # Returns
/// * `Ok(())` - Nếu số lượng hợp lệ
/// * `Err(ValidationError)` - Nếu số lượng không hợp lệ
/// 
/// # Examples
/// ```
/// use wallet::defi::utils::validate_token_amount;
/// use ethers::types::U256;
/// 
/// // Số lượng hợp lệ
/// let amount = U256::from(100);
/// let result = validate_token_amount(amount, None);
/// assert!(result.is_ok());
/// 
/// // Số lượng vượt quá giới hạn
/// let amount = U256::from(1000);
/// let max = U256::from(500);
/// let result = validate_token_amount(amount, Some(max));
/// assert!(result.is_err());
/// ```
pub fn validate_token_amount(amount: U256, max_value: Option<U256>) -> Result<(), ValidationError> {
    // Kiểm tra không âm (thừa thãi với U256 nhưng giữ lại để rõ ràng)
    if amount.is_zero() {
        return Err(ValidationError::InvalidAmount("Amount cannot be zero".to_string()));
    }
    
    // Kiểm tra giới hạn tối đa
    if let Some(max) = max_value {
        if amount > max {
            return Err(ValidationError::ExceedsLimit(
                format!("Amount {} exceeds maximum {}", amount, max)
            ));
        }
    }
    
    Ok(())
}

/// Kiểm tra mã hash giao dịch
/// 
/// Hàm này kiểm tra xem chuỗi có phải là mã hash giao dịch hợp lệ hay không.
/// Một mã hash giao dịch hợp lệ có dạng 0x + 64 ký tự hex.
/// 
/// # Arguments
/// * `tx_hash` - Chuỗi hash giao dịch cần kiểm tra
/// 
/// # Returns
/// * `true` - Nếu hash hợp lệ
/// * `false` - Nếu hash không hợp lệ
/// 
/// # Examples
/// ```
/// use wallet::defi::utils::is_valid_transaction_hash;
/// 
/// // Hash hợp lệ
/// assert!(is_valid_transaction_hash(
///     "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
/// ));
/// 
/// // Hash không hợp lệ (thiếu 0x)
/// assert!(!is_valid_transaction_hash(
///     "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
/// ));
/// ```
pub fn is_valid_transaction_hash(tx_hash: &str) -> bool {
    if tx_hash.len() != 66 {
        return false;
    }
    
    if !tx_hash.starts_with("0x") {
        return false;
    }
    
    // Kiểm tra các ký tự còn lại là hex
    tx_hash[2..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Chuyển đổi ValidationError sang DefiError
impl From<ValidationError> for DefiError {
    fn from(error: ValidationError) -> Self {
        match error {
            ValidationError::InvalidAddress(msg) => DefiError::InvalidAddress(msg),
            ValidationError::InvalidAmount(msg) => DefiError::InvalidParameter(msg),
            ValidationError::ExceedsLimit(msg) => DefiError::InvalidParameter(msg),
            ValidationError::InvalidFormat(msg) => DefiError::InvalidParameter(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_is_valid_ethereum_address() {
        // Địa chỉ hợp lệ không chữ hoa
        assert!(is_valid_ethereum_address("0x0000000000000000000000000000000000000000"));
        
        // Địa chỉ hợp lệ với checksum
        assert!(is_valid_ethereum_address("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"));
        
        // Địa chỉ không hợp lệ
        assert!(!is_valid_ethereum_address("0x123"));
        assert!(!is_valid_ethereum_address("not_an_address"));
        assert!(!is_valid_ethereum_address("0xinvalid"));
        assert!(!is_valid_ethereum_address("0x0000000000000000000000000000000000000000000000"));
        
        // Sai checksum
        assert!(!is_valid_ethereum_address("0x5aaeb6053F3E94C9b9A09f33669435E7Ef1BeAed"));
    }
    
    #[test]
    fn test_validate_ethereum_address() {
        // Địa chỉ hợp lệ
        let result = validate_ethereum_address("0x0000000000000000000000000000000000000000");
        assert!(result.is_ok());
        
        // Địa chỉ không hợp lệ
        let result = validate_ethereum_address("invalid");
        assert!(result.is_err());
        if let Err(error) = result {
            assert_eq!(error.to_string(), "Invalid Ethereum address: invalid");
        }
    }
    
    #[test]
    fn test_validate_token_amount() {
        // Số lượng hợp lệ
        let amount = U256::from(100);
        let result = validate_token_amount(amount, None);
        assert!(result.is_ok());
        
        // Số lượng zero
        let amount = U256::zero();
        let result = validate_token_amount(amount, None);
        assert!(result.is_err());
        
        // Số lượng vượt quá giới hạn
        let amount = U256::from(1000);
        let max = U256::from(500);
        let result = validate_token_amount(amount, Some(max));
        assert!(result.is_err());
        if let Err(error) = result {
            assert!(error.to_string().contains("exceeds maximum"));
        }
    }
    
    #[test]
    fn test_is_valid_transaction_hash() {
        // Hash hợp lệ
        assert!(is_valid_transaction_hash(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        ));
        
        // Hash không hợp lệ
        assert!(!is_valid_transaction_hash("0x123"));
        assert!(!is_valid_transaction_hash(
            "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        ));
        assert!(!is_valid_transaction_hash(
            "0x123456789gabcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        ));
    }
} 