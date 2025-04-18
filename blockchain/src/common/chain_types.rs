//! Định nghĩa các loại blockchain được hỗ trợ và các hàm tiện ích liên quan
//!
//! Module này chứa enum DmdChain định nghĩa các loại blockchain được hỗ trợ
//! cùng với các phương thức để kiểm tra, chuyển đổi và xác thực địa chỉ.

use std::fmt;
use std::str::FromStr;
use regex::Regex;
use lazy_static::lazy_static;
use thiserror::Error;
use hex;
use sha3::{Digest, Keccak256};
use base58::FromBase58;
use ed25519_dalek::PublicKey;
use log::{error, debug};

/// Lỗi xảy ra khi chuyển đổi từ chuỗi sang enum DmdChain
#[derive(Debug, Error)]
#[error("Không thể chuyển đổi '{0}' thành DmdChain: {1}")]
pub struct ChainParseError(String, String);

/// Enum định nghĩa các loại blockchain được hỗ trợ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DmdChain {
    /// Ethereum Mainnet
    Ethereum,
    /// Binance Smart Chain
    BSC,
    /// Avalanche C-Chain
    Avalanche,
    /// Polygon
    Polygon,
    /// Arbitrum
    Arbitrum,
    /// Optimism
    Optimism,
    /// Base
    Base,
    /// Goerli Testnet
    Goerli,
    /// Sepolia Testnet
    Sepolia,
    /// NEAR Protocol
    NEAR,
    /// Solana
    Solana,
}

impl DmdChain {
    /// Kiểm tra xem chain có phải là chain tương thích EVM hay không
    pub fn is_evm(&self) -> bool {
        match self {
            DmdChain::NEAR | DmdChain::Solana => false,
            _ => true,
        }
    }

    /// Chuyển đổi sang chain ID của EVM
    /// 
    /// # Errors
    /// 
    /// Trả về lỗi nếu chain không phải là chain tương thích EVM
    pub fn as_evm_chain_id(&self) -> Result<u64, String> {
        match self {
            DmdChain::Ethereum => Ok(1),
            DmdChain::BSC => Ok(56),
            DmdChain::Avalanche => Ok(43114),
            DmdChain::Polygon => Ok(137),
            DmdChain::Arbitrum => Ok(42161),
            DmdChain::Optimism => Ok(10),
            DmdChain::Base => Ok(8453),
            DmdChain::Goerli => Ok(5),
            DmdChain::Sepolia => Ok(11155111),
            _ => Err(format!("{} không phải là chain tương thích EVM", self)),
        }
    }

    /// Lấy URL của blockchain explorer cho chain này
    pub fn explorer_url(&self) -> &'static str {
        match self {
            DmdChain::Ethereum => "https://etherscan.io",
            DmdChain::BSC => "https://bscscan.com",
            DmdChain::Avalanche => "https://snowtrace.io",
            DmdChain::Polygon => "https://polygonscan.com",
            DmdChain::Arbitrum => "https://arbiscan.io",
            DmdChain::Optimism => "https://optimistic.etherscan.io",
            DmdChain::Base => "https://basescan.org",
            DmdChain::Goerli => "https://goerli.etherscan.io",
            DmdChain::Sepolia => "https://sepolia.etherscan.io",
            DmdChain::NEAR => "https://explorer.near.org",
            DmdChain::Solana => "https://explorer.solana.com",
        }
    }

    /// Lấy tên chain
    pub fn name(&self) -> &'static str {
        match self {
            DmdChain::Ethereum => "Ethereum",
            DmdChain::BSC => "Binance Smart Chain",
            DmdChain::Avalanche => "Avalanche",
            DmdChain::Polygon => "Polygon",
            DmdChain::Arbitrum => "Arbitrum",
            DmdChain::Optimism => "Optimism",
            DmdChain::Base => "Base",
            DmdChain::Goerli => "Goerli",
            DmdChain::Sepolia => "Sepolia",
            DmdChain::NEAR => "NEAR Protocol",
            DmdChain::Solana => "Solana",
        }
    }

    /// Lấy tên viết tắt của đồng tiền native
    pub fn native_currency(&self) -> &'static str {
        match self {
            DmdChain::Ethereum => "ETH",
            DmdChain::BSC => "BNB",
            DmdChain::Avalanche => "AVAX",
            DmdChain::Polygon => "MATIC",
            DmdChain::Arbitrum => "ETH",
            DmdChain::Optimism => "ETH",
            DmdChain::Base => "ETH",
            DmdChain::Goerli => "GoerliETH",
            DmdChain::Sepolia => "SepoliaETH",
            DmdChain::NEAR => "NEAR",
            DmdChain::Solana => "SOL",
        }
    }

    /// Xác thực địa chỉ cho chain cụ thể
    pub fn validate_address(&self, address: &str) -> bool {
        match self {
            chain if chain.is_evm() => validate_evm_address(address),
            DmdChain::NEAR => validate_near_address(address),
            DmdChain::Solana => validate_solana_address(address),
        }
    }

    /// Chuyển đổi từ chain ID của EVM sang DmdChain
    pub fn from_evm_chain_id(chain_id: u64) -> Result<Self, String> {
        match chain_id {
            1 => Ok(DmdChain::Ethereum),
            56 => Ok(DmdChain::BSC),
            43114 => Ok(DmdChain::Avalanche),
            137 => Ok(DmdChain::Polygon),
            42161 => Ok(DmdChain::Arbitrum),
            10 => Ok(DmdChain::Optimism),
            8453 => Ok(DmdChain::Base),
            5 => Ok(DmdChain::Goerli),
            11155111 => Ok(DmdChain::Sepolia),
            _ => Err(format!("Chain ID không được hỗ trợ: {}", chain_id)),
        }
    }
}

impl FromStr for DmdChain {
    type Err = ChainParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ethereum" | "eth" => Ok(DmdChain::Ethereum),
            "bsc" | "binance" => Ok(DmdChain::BSC),
            "avalanche" | "avax" => Ok(DmdChain::Avalanche),
            "polygon" | "matic" => Ok(DmdChain::Polygon),
            "arbitrum" | "arb" => Ok(DmdChain::Arbitrum),
            "optimism" | "op" => Ok(DmdChain::Optimism),
            "base" => Ok(DmdChain::Base),
            "goerli" => Ok(DmdChain::Goerli),
            "sepolia" => Ok(DmdChain::Sepolia),
            "near" => Ok(DmdChain::NEAR),
            "solana" | "sol" => Ok(DmdChain::Solana),
            _ => {
                // Thử chuyển đổi từ chain ID nếu là số
                if let Ok(chain_id) = s.parse::<u64>() {
                    DmdChain::from_evm_chain_id(chain_id)
                        .map_err(|e| ChainParseError(s.to_string(), e))
                } else {
                    Err(ChainParseError(
                        s.to_string(),
                        "Tên chain không hợp lệ".to_string(),
                    ))
                }
            }
        }
    }
}

impl fmt::Display for DmdChain {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Xác thực địa chỉ Ethereum/EVM
pub fn validate_evm_address(address: &str) -> bool {
    lazy_static! {
        static ref EVM_ADDRESS_REGEX: Regex = Regex::new(r"^0x[a-fA-F0-9]{40}$").unwrap();
    }

    if !EVM_ADDRESS_REGEX.is_match(address) {
        return false;
    }

    // Nếu địa chỉ chỉ chứa các ký tự lowercase hoặc uppercase, không cần kiểm tra checksum
    if address[2..].chars().all(|c| c.is_ascii_lowercase()) 
        || address[2..].chars().all(|c| c.is_ascii_uppercase()) {
        return true;
    }

    // Kiểm tra EIP-55 checksum
    let normalized = normalize_evm_address(address);
    let address_without_prefix = &address[2..];
    let normalized_without_prefix = &normalized[2..];

    address_without_prefix == normalized_without_prefix
}

/// Chuẩn hóa địa chỉ EVM theo chuẩn EIP-55
pub fn normalize_evm_address(address: &str) -> String {
    // Chuyển địa chỉ về dạng lowercase
    let address_lowercase = address.to_lowercase();
    
    // Nếu địa chỉ không có prefix 0x, thêm vào
    let address_with_prefix = if !address_lowercase.starts_with("0x") {
        format!("0x{}", address_lowercase)
    } else {
        address_lowercase
    };

    // Tính hash Keccak-256 của địa chỉ không có prefix
    let address_without_prefix = &address_with_prefix[2..];
    let hash = Keccak256::digest(address_without_prefix.as_bytes());
    let hash_hex = hex::encode(hash);

    // Áp dụng EIP-55 checksum
    let mut result = String::from("0x");
    for (i, c) in address_without_prefix.char_indices() {
        if c.is_ascii_digit() {
            result.push(c);
        } else {
            // Nếu byte tương ứng trong hash >= 8, chuyển thành chữ hoa
            let nibble = u8::from_str_radix(&hash_hex[i..i+1], 16).unwrap_or(0);
            if nibble >= 8 {
                result.push(c.to_ascii_uppercase());
            } else {
                result.push(c.to_ascii_lowercase());
            }
        }
    }

    result
}

/// Xác thực địa chỉ NEAR
pub fn validate_near_address(address: &str) -> bool {
    // Kiểm tra độ dài và ký tự hợp lệ
    if address.len() < 2 || address.len() > 64 {
        return false;
    }

    lazy_static! {
        static ref NEAR_ADDRESS_REGEX: Regex = Regex::new(r"^(([a-z\d]+[-_])*[a-z\d]+\.)*([a-z\d]+[-_])*[a-z\d]+$").unwrap();
    }

    NEAR_ADDRESS_REGEX.is_match(address)
}

/// Xác thực địa chỉ Solana
pub fn validate_solana_address(address: &str) -> bool {
    // Kiểm tra độ dài
    if address.len() != 44 && address.len() != 43 {
        return false;
    }

    // Thử giải mã base58
    match address.from_base58() {
        Ok(decoded) => {
            // Địa chỉ Solana là khóa công khai Ed25519
            if decoded.len() != 32 {
                return false;
            }

            // Thử chuyển đổi thành PublicKey để xác thực
            match PublicKey::from_bytes(&decoded) {
                Ok(_) => true,
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dmd_chain_from_str() {
        assert_eq!(DmdChain::from_str("ethereum").unwrap(), DmdChain::Ethereum);
        assert_eq!(DmdChain::from_str("ETH").unwrap(), DmdChain::Ethereum);
        assert_eq!(DmdChain::from_str("bsc").unwrap(), DmdChain::BSC);
        assert_eq!(DmdChain::from_str("near").unwrap(), DmdChain::NEAR);
        assert_eq!(DmdChain::from_str("solana").unwrap(), DmdChain::Solana);
        assert_eq!(DmdChain::from_str("1").unwrap(), DmdChain::Ethereum);
        assert_eq!(DmdChain::from_str("137").unwrap(), DmdChain::Polygon);

        assert!(DmdChain::from_str("unknown").is_err());
        assert!(DmdChain::from_str("999999").is_err());
    }

    #[test]
    fn test_is_evm() {
        assert!(DmdChain::Ethereum.is_evm());
        assert!(DmdChain::BSC.is_evm());
        assert!(DmdChain::Polygon.is_evm());
        assert!(!DmdChain::NEAR.is_evm());
        assert!(!DmdChain::Solana.is_evm());
    }

    #[test]
    fn test_validate_evm_address() {
        // Địa chỉ hợp lệ
        assert!(validate_evm_address("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"));
        assert!(validate_evm_address("0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359"));
        
        // Địa chỉ không hợp lệ (sai định dạng)
        assert!(!validate_evm_address("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAe"));
        assert!(!validate_evm_address("5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"));
        assert!(!validate_evm_address("0xXaAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"));
    }

    #[test]
    fn test_validate_near_address() {
        // Địa chỉ hợp lệ
        assert!(validate_near_address("example.near"));
        assert!(validate_near_address("user.testnet"));
        assert!(validate_near_address("a-valid-account.near"));
        assert!(validate_near_address("sub.account.near"));
        
        // Địa chỉ không hợp lệ
        assert!(!validate_near_address(".near"));
        assert!(!validate_near_address("invalid..account.near"));
        assert!(!validate_near_address("Invalid@account.near"));
    }

    #[test]
    fn test_validate_solana_address() {
        // Thử với một số địa chỉ Solana (chú ý: cần thay thế bằng địa chỉ thực tế để kiểm tra)
        let valid_address = "5U3bH5b6XtG99aVWLqwVzYPVpQiFHytBD68Rz2eFPZd7";
        let invalid_address = "invalid_solana_address";
        
        assert!(validate_solana_address(valid_address));
        assert!(!validate_solana_address(invalid_address));
    }

    #[test]
    fn test_chain_id_conversion() {
        assert_eq!(DmdChain::Ethereum.as_evm_chain_id().unwrap(), 1);
        assert_eq!(DmdChain::Polygon.as_evm_chain_id().unwrap(), 137);
        
        assert!(DmdChain::NEAR.as_evm_chain_id().is_err());
        assert!(DmdChain::Solana.as_evm_chain_id().is_err());
        
        assert_eq!(DmdChain::from_evm_chain_id(1).unwrap(), DmdChain::Ethereum);
        assert_eq!(DmdChain::from_evm_chain_id(56).unwrap(), DmdChain::BSC);
        
        assert!(DmdChain::from_evm_chain_id(999999).is_err());
    }
} 