//! Integration tests cho wallet module.
//! Kiểm tra các tính năng cơ bản của wallet API dựa trên quy tắc trong .rules.
//! Tuân thủ quy tắc "Viết integration test cho module" từ development_workflow.testing.

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

        let create_result = api.create_wallet(SeedLength::Twelve, None, ChainType::EVM, "password", None);
        assert!(create_result.is_ok(), "Không thể tạo ví: {:?}", create_result);
        
        let (address, seed, user_id) = match create_result {
            Ok(wallet_info) => wallet_info,
            Err(e) => panic!("Không thể tạo ví: {:?}", e),
        };
        
        assert!(api.has_wallet(address));
        
        let user_id_result = api.get_user_id(address);
        assert!(user_id_result.is_ok(), "Không thể lấy user ID: {:?}", user_id_result);
        if let Ok(id) = user_id_result {
            assert_eq!(id, user_id, "User ID không khớp");
        }
        
        let seed_result = api.export_seed_phrase(address, "password");
        assert!(seed_result.is_ok(), "Không thể xuất seed phrase: {:?}", seed_result);
        if let Ok(exported_seed) = seed_result {
            assert_eq!(exported_seed, seed, "Seed phrase không khớp");
        }
        
        assert!(api.export_seed_phrase(address, "wrong_password").is_err());
        
        let update_result = api.update_chain_id(address, 56, ChainType::EVM);
        assert!(update_result.is_ok(), "Không thể cập nhật chain ID: {:?}", update_result);
        
        let chain_id_result = api.get_chain_id(address);
        assert!(chain_id_result.is_ok(), "Không thể lấy chain ID: {:?}", chain_id_result);
        if let Ok(chain_id) = chain_id_result {
            assert_eq!(chain_id, 56, "Chain ID không khớp");
        }

        let remove_result = api.remove_wallet(address);
        assert!(remove_result.is_ok(), "Không thể xóa ví: {:?}", remove_result);
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
        
        let import_result = api.import_wallet(wallet_config, None);
        assert!(import_result.is_ok(), "Không thể import ví: {:?}", import_result);
        
        let (address, _) = match import_result {
            Ok(wallet_info) => wallet_info,
            Err(e) => panic!("Không thể import ví: {:?}", e),
        };

        let export_result = api.export_private_key(address, "password");
        assert!(export_result.is_ok(), "Không thể xuất private key: {:?}", export_result);
        if let Ok(exported_key) = export_result {
            assert_eq!(exported_key, private_key, "Private key không khớp");
        }
        
        assert!(api.export_seed_phrase(address, "password").is_err());
    }

    #[test]
    fn test_sign_and_balance() {
        let config = WalletSystemConfig::default();
        let mut api = WalletManagerApi::new(config);
        
        let create_result = api.create_wallet(SeedLength::Twelve, None, ChainType::EVM, "password", None);
        assert!(create_result.is_ok(), "Không thể tạo ví: {:?}", create_result);
        
        let (address, _, _) = match create_result {
            Ok(wallet_info) => wallet_info,
            Err(e) => panic!("Không thể tạo ví: {:?}", e),
        };
        
        let tx = TransactionRequest::new().to(Address::zero()).value(100);
        let result = api.sign_transaction(address, tx);
        assert!(result.is_ok());
        
        let balance = api.get_balance(address);
        assert!(balance.is_ok());
    }
}