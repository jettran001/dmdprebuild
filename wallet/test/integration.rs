#[cfg(test)]
mod integration_tests {
    use ethers::core::types::{TransactionRequest, U256};
    use ethers::types::Address;
    use wallet::config::WalletSystemConfig;
    use wallet::walletmanager::api::WalletManagerApi;
    use wallet::walletmanager::types::{SeedLength, WalletConfig};
    use wallet::walletmanager::chain::ChainType;

    #[test]
    fn test_wallet_lifecycle() {
        let config = WalletSystemConfig::default();
        let mut api = WalletManagerApi::new(config);

        let (address, seed, user_id) = api.create_wallet(SeedLength::Twelve, None, ChainType::EVM, "password").unwrap();
        assert!(api.has_wallet(address));
        assert_eq!(api.get_user_id(address).unwrap(), user_id);
        assert_eq!(api.export_seed_phrase(address, "password").unwrap(), seed);
        assert!(api.export_seed_phrase(address, "wrong_password").is_err());
        api.update_chain_id(address, 56, ChainType::EVM).unwrap();
        assert_eq!(api.get_chain_id(address).unwrap(), 56);

        api.remove_wallet(address).unwrap();
        assert!(!api.has_wallet(address));
    }

    #[test]
    fn test_import_and_export() {
        let config = WalletSystemConfig::default();
        let mut api = WalletManagerApi::new(config);

        let private_key = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let wallet_config = WalletConfig {
            seed_or_key: private_key.to_string(),
            chain_id: 1,
            chain_type: ChainType::EVM,
            seed_length: None,
            password: "password".to_string(),
        };
        let (address, _) = api.import_wallet(wallet_config).unwrap();

        assert_eq!(api.export_private_key(address, "password").unwrap(), private_key);
        assert!(api.export_seed_phrase(address, "password").is_err());
    }

    #[test]
    fn test_sign_and_balance() {
        let config = WalletSystemConfig::default();
        let mut api = WalletManagerApi::new(config);
        let (address, _, _) = api.create_wallet(SeedLength::Twelve, None, ChainType::EVM, "password").unwrap();
        
        let tx = TransactionRequest::new().to(Address::zero()).value(100);
        let result = api.sign_transaction(address, tx);
        assert!(result.is_ok());
        
        let balance = api.get_balance(address);
        assert!(balance.is_ok());
    }
}