use uuid::Uuid;

use crate::walletmanager::types::{SeedLength, WalletConfig};

pub fn generate_user_id() -> String {
    Uuid::new_v4().to_string()
}

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
        let id1 = generate_user_id();
        let id2 = generate_user_id();
        assert!(!id1.is_empty());
        assert!(!id2.is_empty());
        assert_ne!(id1, id2);
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