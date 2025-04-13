use uuid::Uuid;
use chrono::Utc;
use std::fmt;

use crate::walletmanager::types::{SeedLength, WalletConfig};

/// Enum xác định loại người dùng.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UserType {
    /// Người dùng miễn phí với giới hạn cơ bản.
    Free,
    /// Người dùng premium với tính năng nâng cao.
    Premium,
    /// Người dùng VIP với quyền đầy đủ.
    Vip,
}

impl fmt::Display for UserType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UserType::Free => write!(f, "FREE"),
            UserType::Premium => write!(f, "PREM"),
            UserType::Vip => write!(f, "VIP"),
        }
    }
}

/// Tạo ID cho người dùng với prefix theo loại user và timestamp.
///
/// # Arguments
/// - `user_type`: Loại người dùng (Free, Premium, VIP).
///
/// # Returns
/// ID người dùng định dạng "{prefix}_{timestamp}_{uuid}".
#[flow_from("walletlogic::init")]
pub fn generate_user_id(user_type: UserType) -> String {
    let timestamp = Utc::now().timestamp();
    let uuid = Uuid::new_v4();
    format!("{}_{}_{}",
        user_type,
        timestamp,
        uuid.to_string().replace("-", "")
    )
}

/// Tạo ID người dùng mặc định (Free user).
///
/// # Returns
/// ID người dùng với prefix "FREE".
pub fn generate_default_user_id() -> String {
    generate_user_id(UserType::Free)
}

/// Kiểm tra chuỗi có phải là seed phrase hay không.
///
/// # Arguments
/// - `config`: Cấu hình ví cần kiểm tra.
///
/// # Returns
/// `true` nếu là seed phrase, `false` nếu là private key.
pub fn is_seed_phrase(config: &WalletConfig) -> bool {
    matches!(
        config.seed_length,
        Some(SeedLength::Twelve) | Some(SeedLength::TwentyFour)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_user_id() {
        let id_free = generate_user_id(UserType::Free);
        let id_premium = generate_user_id(UserType::Premium);
        let id_vip = generate_user_id(UserType::Vip);
        
        // Kiểm tra cấu trúc ID
        assert!(id_free.starts_with("FREE_"));
        assert!(id_premium.starts_with("PREM_"));
        assert!(id_vip.starts_with("VIP_"));
        
        // Kiểm tra tính duy nhất
        assert_ne!(id_free, id_premium);
        assert_ne!(id_free, id_vip);
        assert_ne!(id_premium, id_vip);
        
        // Kiểm tra hàm default
        let default_id = generate_default_user_id();
        assert!(default_id.starts_with("FREE_"));
    }

    #[test]
    fn test_is_seed_phrase() {
        let seed_config = WalletConfig {
            seed_or_key: "word ".repeat(12).trim().to_string(),
            chain_id: 1,
            chain_type: crate::walletmanager::chain::ChainType::EVM,
            seed_length: Some(SeedLength::Twelve),
            password: "password".to_string(),
        };
        let key_config = WalletConfig {
            seed_or_key: "1234567890abcdef".to_string(),
            chain_id: 1,
            chain_type: crate::walletmanager::chain::ChainType::EVM,
            seed_length: None,
            password: "password".to_string(),
        };
        assert!(is_seed_phrase(&seed_config));
        assert!(!is_seed_phrase(&key_config));
    }
}