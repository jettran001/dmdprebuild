//! API công khai cho wallet manager, cung cấp các chức năng quản lý ví.

// External imports
use ethers::core::types::{TransactionRequest, U256};
use ethers::signers::LocalWallet;
use ethers::types::Address;

// Standard library imports
use std::sync::Arc;

// Internal imports
use crate::error::WalletError;
use crate::config::WalletSystemConfig;
use crate::walletlogic::handler::WalletManagerHandler;
use crate::walletlogic::init::WalletManager;
use crate::walletlogic::utils::UserType;
use crate::walletmanager::chain::{ChainConfig, ChainManager, ChainType, DefaultChainManager};
use crate::walletmanager::types::{WalletConfig, SeedLength};

// Third party imports
use anyhow::Context;
use tracing::{debug, info};

// Hằng số cho thông báo lỗi
const ERR_MAX_WALLET_LIMIT: &str = "Max wallet limit reached";
const ERR_INVALID_ADDRESS_FORMAT: &str = "Invalid address format";

/// API công khai cho quản lý ví.
pub struct WalletManagerApi {
    manager: WalletManager,
    config: WalletSystemConfig,
    chain_manager: Box<dyn ChainManager + Send + Sync + 'static>,
}

impl WalletManagerApi {
    /// Khởi tạo API quản lý ví mới.
    ///
    /// # Arguments
    /// - `config`: Cấu hình hệ thống ví.
    pub fn new(config: WalletSystemConfig) -> Self {
        Self {
            manager: WalletManager::new(),
            config,
            chain_manager: Box::new(DefaultChainManager::new()),
        }
    }

    /// Tạo ví mới.
    ///
    /// # Arguments
    /// - `seed_length`: Độ dài seed phrase.
    /// - `chain_id`: ID của blockchain (nếu None sẽ dùng default_chain_id).
    /// - `chain_type`: Loại blockchain.
    /// - `password`: Mật khẩu để mã hóa seed.
    /// - `user_type`: Loại người dùng (mặc định là Free).
    ///
    /// # Returns
    /// Tuple (địa chỉ ví, seed phrase, user_id).
    #[flow_from("common::gateway")]
    pub async fn create_wallet(
        &mut self,
        seed_length: SeedLength,
        chain_id: Option<u64>,
        chain_type: ChainType,
        password: &str,
        user_type: Option<UserType>,
    ) -> Result<(Address, String, String), WalletError> 
    where Self: Send + Sync + 'static
    {
        if !self.config.can_add_wallet(self.manager.wallets.read().await.len()) {
            return Err(WalletError::InvalidSeedOrKey(ERR_MAX_WALLET_LIMIT.to_string()));
        }
        let chain_id = chain_id.unwrap_or(self.config.default_chain_id);
        self.manager.create_wallet_internal(seed_length, chain_id, chain_type, password, user_type)
            .await
            .context("Lỗi khi tạo ví mới")
            .map_err(|e| match e.downcast::<WalletError>() {
                Ok(wallet_err) => wallet_err,
                Err(e) => WalletError::SeedGenerationFailed(e.to_string()),
            })
    }

    /// Nhập ví từ seed phrase hoặc private key.
    ///
    /// # Arguments
    /// - `config`: Cấu hình ví.
    /// - `user_type`: Loại người dùng (mặc định là Free).
    ///
    /// # Returns
    /// Tuple (địa chỉ ví, user_id).
    #[flow_from("common::gateway")]
    pub async fn import_wallet(
        &mut self, 
        config: WalletConfig,
        user_type: Option<UserType>
    ) -> Result<(Address, String), WalletError> 
    where Self: Send + Sync + 'static
    {
        if !self.config.can_add_wallet(self.manager.wallets.read().await.len()) {
            return Err(WalletError::InvalidSeedOrKey(ERR_MAX_WALLET_LIMIT.to_string()));
        }
        
        // Kiểm tra địa chỉ hợp lệ và ví đã tồn tại
        let maybe_address = config.seed_or_key.parse::<Address>()
            .context("Không thể phân tích chuỗi thành địa chỉ")
            .map_err(|_| WalletError::InvalidSeedOrKey(ERR_INVALID_ADDRESS_FORMAT.to_string()));
            
        if let Ok(address) = maybe_address {
            if !self.config.allow_wallet_overwrite && self.manager.has_wallet_internal(address).await {
                return Err(WalletError::WalletExists(address));
            }
        }
        
        self.manager.import_wallet_internal(config, user_type)
            .await
            .context("Lỗi khi nhập ví")
            .map_err(|e| match e.downcast::<WalletError>() {
                Ok(wallet_err) => wallet_err,
                Err(e) => WalletError::InvalidSeedOrKey(e.to_string()),
            })
    }

    /// Xuất seed phrase từ ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    /// - `password`: Mật khẩu để giải mã.
    ///
    /// # Returns
    /// Seed phrase gốc.
    #[flow_from("common::gateway")]
    pub async fn export_seed_phrase(&self, address: Address, password: &str) -> Result<String, WalletError> 
    where Self: Send + Sync + 'static
    {
        self.manager.export_seed_phrase_internal(address, password)
            .await
            .context("Lỗi khi xuất seed phrase")
            .map_err(|e| match e.downcast::<WalletError>() {
                Ok(wallet_err) => wallet_err,
                Err(e) => WalletError::DecryptionError(e.to_string()),
            })
    }

    /// Xuất private key từ ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    /// - `password`: Mật khẩu để giải mã.
    ///
    /// # Returns
    /// Private key gốc.
    #[flow_from("common::gateway")]
    pub async fn export_private_key(&self, address: Address, password: &str) -> Result<String, WalletError> 
    where Self: Send + Sync + 'static
    {
        self.manager.export_private_key_internal(address, password)
            .await
            .context("Lỗi khi xuất private key")
            .map_err(|e| match e.downcast::<WalletError>() {
                Ok(wallet_err) => wallet_err,
                Err(e) => WalletError::DecryptionError(e.to_string()),
            })
    }

    /// Xóa ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví cần xóa.
    #[flow_from("common::gateway")]
    pub async fn remove_wallet(&mut self, address: Address) -> Result<(), WalletError> 
    where Self: Send + Sync + 'static
    {
        self.manager.remove_wallet_internal(address)
            .await
            .context("Lỗi khi xóa ví")
            .map_err(|e| match e.downcast::<WalletError>() {
                Ok(wallet_err) => wallet_err,
                Err(e) => WalletError::WalletNotFound(address),
            })
    }

    /// Cập nhật chain ID cho ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    /// - `new_chain_id`: Chain ID mới.
    /// - `new_chain_type`: Loại chain mới.
    #[flow_from("common::gateway")]
    pub async fn update_chain_id(&mut self, address: Address, new_chain_id: u64, new_chain_type: ChainType) -> Result<(), WalletError> 
    where Self: Send + Sync + 'static
    {
        self.manager.update_chain_id_internal(address, new_chain_id, new_chain_type)
            .await
            .context("Lỗi khi cập nhật chain ID")
            .map_err(|e| match e.downcast::<WalletError>() {
                Ok(wallet_err) => wallet_err,
                Err(e) => WalletError::WalletNotFound(address),
            })
    }

    /// Lấy chain ID của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    #[flow_from("common::gateway")]
    pub async fn get_chain_id(&self, address: Address) -> Result<u64, WalletError> 
    where Self: Send + Sync + 'static
    {
        self.manager.get_chain_id_internal(address)
            .await
            .context("Lỗi khi lấy chain ID")
            .map_err(|e| match e.downcast::<WalletError>() {
                Ok(wallet_err) => wallet_err,
                Err(e) => WalletError::WalletNotFound(address),
            })
    }

    /// Kiểm tra xem ví có tồn tại không.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    #[flow_from("common::gateway")]
    pub async fn has_wallet(&self, address: Address) -> bool 
    where Self: Send + Sync + 'static
    {
        self.manager.has_wallet_internal(address).await
    }

    /// Lấy đối tượng ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    #[flow_from("common::gateway")]
    pub async fn get_wallet(&self, address: Address) -> Result<LocalWallet, WalletError> 
    where Self: Send + Sync + 'static
    {
        self.manager.get_wallet_internal(address)
            .await
            .context("Lỗi khi lấy thông tin ví")
            .map_err(|e| match e.downcast::<WalletError>() {
                Ok(wallet_err) => wallet_err,
                Err(e) => WalletError::WalletNotFound(address),
            })
    }

    /// Lấy user ID của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    #[flow_from("common::gateway")]
    pub async fn get_user_id(&self, address: Address) -> Result<String, WalletError> 
    where Self: Send + Sync + 'static
    {
        self.manager.get_user_id_internal(address)
            .await
            .context("Lỗi khi lấy user ID")
            .map_err(|e| match e.downcast::<WalletError>() {
                Ok(wallet_err) => wallet_err,
                Err(e) => WalletError::WalletNotFound(address),
            })
    }

    /// Liệt kê tất cả các ví.
    #[flow_from("common::gateway")]
    pub async fn list_wallets(&self) -> Vec<Address> 
    where Self: Send + Sync + 'static
    {
        self.manager.list_wallets_internal().await
    }

    /// Thêm chain mới.
    ///
    /// # Arguments
    /// - `config`: Cấu hình chain.
    #[flow_from("common::gateway")]
    pub async fn add_chain(&mut self, config: ChainConfig) -> Result<(), WalletError> 
    where Self: Send + Sync + 'static
    {
        self.chain_manager.add_chain(config)
            .await
            .context("Lỗi khi thêm chain mới")
            .map_err(|e| match e.downcast::<WalletError>() {
                Ok(wallet_err) => wallet_err,
                Err(e) => WalletError::ChainNotSupported(e.to_string()),
            })
    }

    /// Ký giao dịch.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví dùng để ký.
    /// - `tx`: Giao dịch cần ký.
    #[flow_from("common::gateway")]
    pub async fn sign_transaction(&self, address: Address, tx: TransactionRequest) -> Result<Vec<u8>, WalletError> 
    where Self: Send + Sync + 'static
    {
        self.manager.sign_transaction(address, tx, self.chain_manager.as_ref())
            .await
            .context("Lỗi khi ký giao dịch")
            .map_err(|e| match e.downcast::<WalletError>() {
                Ok(wallet_err) => wallet_err,
                Err(e) => WalletError::TransactionFailed(e.to_string()),
            })
    }

    /// Gửi giao dịch.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví gửi giao dịch.
    /// - `tx`: Giao dịch cần gửi.
    ///
    /// # Returns
    /// - `Ok(String)`: Hash giao dịch.
    /// - `Err(WalletError)`: Nếu có lỗi.
    #[flow_from("common::gateway")]
    pub async fn send_transaction(&self, address: Address, tx: TransactionRequest) -> Result<String, WalletError> 
    where Self: Send + Sync + 'static 
    {
        // Thay đổi cách gọi send_transaction để phù hợp với ChainManager async
        let wallet = self.manager.get_wallet_internal(address).await?;
        let chain_id = self.manager.get_chain_id_internal(address).await?;
        
        // Lấy provider theo chain_id
        let provider = self.chain_manager.get_provider(chain_id).await?;
        
        // Thực hiện giao dịch
        self.manager.send_transaction_with_provider(wallet, tx, provider)
            .await
            .context("Lỗi khi gửi giao dịch")
            .map_err(|e| match e.downcast::<WalletError>() {
                Ok(wallet_err) => wallet_err,
                Err(e) => WalletError::TransactionFailed(e.to_string()),
            })
    }

    /// Lấy số dư của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    ///
    /// # Returns
    /// - `Ok(U256)`: Số dư.
    /// - `Err(WalletError)`: Nếu có lỗi.
    #[flow_from("common::gateway")]
    pub async fn get_balance(&self, address: Address) -> Result<U256, WalletError> 
    where Self: Send + Sync + 'static 
    {
        // Thay đổi cách gọi get_balance để phù hợp với ChainManager async
        let chain_id = self.manager.get_chain_id_internal(address).await?;
        
        // Lấy provider theo chain_id
        let provider = self.chain_manager.get_provider(chain_id).await?;
        
        // Lấy số dư từ provider
        self.manager.get_balance_with_provider(address, provider)
            .await
            .context("Lỗi khi lấy số dư")
            .map_err(|e| match e.downcast::<WalletError>() {
                Ok(wallet_err) => wallet_err,
                Err(e) => WalletError::ProviderError(e.to_string()),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walletmanager::types::{SeedLength, WalletConfig};
    use crate::walletmanager::chain::ChainType;

    #[tokio::test]
    async fn test_create_wallet() {
        let config = WalletSystemConfig::default();
        let mut api = WalletManagerApi::new(config);
        
        let result = api.create_wallet(SeedLength::Twelve, None, ChainType::EVM, "password", None).await;
        assert!(result.is_ok(), "Should create wallet successfully");
        
        let (address, _, user_id) = result
            .expect("Should return valid wallet information");
            
        assert!(api.has_wallet(address).await, "API should have the created wallet");
        assert_eq!(
            api.get_user_id(address).await.expect("Should get user ID"),
            user_id, 
            "User ID should match"
        );
    }

    #[tokio::test]
    async fn test_import_wallet_seed() {
        let config = WalletSystemConfig::default();
        let mut api = WalletManagerApi::new(config);
        let seed = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        
        let wallet_config = WalletConfig {
            seed_or_key: seed.to_string(),
            chain_id: 1,
            chain_type: ChainType::EVM,
            seed_length: Some(SeedLength::Twelve),
            password: "password".to_string(),
        };
        
        let result = api.import_wallet(wallet_config, None).await;
        assert!(result.is_ok(), "Should import wallet successfully");
        
        let (address, user_id) = result
            .expect("Should return valid wallet information");
            
        assert_eq!(
            api.get_user_id(address).await.expect("Should get user ID"),
            user_id, 
            "User ID should match"
        );
        
        assert_eq!(
            api.export_seed_phrase(address, "password").await.expect("Should export seed phrase"),
            seed,
            "Exported seed phrase should match original"
        );
    }

    #[tokio::test]
    async fn test_max_wallets_limit() {
        let config = WalletSystemConfig {
            default_chain_id: 1,
            default_chain_type: ChainType::EVM,
            max_wallets: 1,
            allow_wallet_overwrite: false,
        };
        
        let mut api = WalletManagerApi::new(config);
        
        let first_result = api.create_wallet(SeedLength::Twelve, None, ChainType::EVM, "password", None).await;
        assert!(first_result.is_ok(), "Should create first wallet successfully");
        first_result.expect("Should have valid first wallet result");
        
        let second_result = api.create_wallet(SeedLength::Twelve, None, ChainType::EVM, "password", None).await;
        assert!(second_result.is_err(), "Should fail to create second wallet due to limit");
    }

    #[test]
    fn test_add_chain() {
        let config = WalletSystemConfig::default();
        let mut api = WalletManagerApi::new(config);
        
        let chain_config = ChainConfig {
            chain_id: 999,
            chain_type: ChainType::EVM,
            name: "Test Chain".to_string(),
            rpc_url: "https://test-rpc.com".to_string(),
            native_token: "TEST".to_string(),
        };
        
        assert!(
            api.add_chain(chain_config).is_ok(),
            "Should add chain successfully"
        );
    }

    // Unit tests
    #[test]
    fn test_new() {
        let config = WalletSystemConfig {
            max_wallets: 5,
            default_chain_id: 1,
            allow_wallet_overwrite: false,
        };
        let api = WalletManagerApi::new(config);
        assert_eq!(api.config.max_wallets, 5);
        assert_eq!(api.config.default_chain_id, 1);
        assert_eq!(api.config.allow_wallet_overwrite, false);
    }
    
    // Integration tests
    #[tokio::test]
    async fn test_wallet_creation_and_export() {
        let config = WalletSystemConfig {
            max_wallets: 5,
            default_chain_id: 1,
            allow_wallet_overwrite: false,
        };
        let mut api = WalletManagerApi::new(config);
        
        // Tạo ví mới
        let result = api.create_wallet(
            SeedLength::Twelve,
            None,
            ChainType::EVM,
            "password123",
            None
        ).await;
        
        assert!(result.is_ok(), "Không thể tạo ví mới");
        let (address, seed_phrase, user_id) = result.unwrap();
        
        // Kiểm tra xuất seed phrase
        let export_result = api.export_seed_phrase(address, "password123").await;
        assert!(export_result.is_ok(), "Không thể xuất seed phrase");
        assert_eq!(export_result.unwrap(), seed_phrase, "Seed phrase không khớp");
        
        // Kiểm tra xuất private key
        let private_key_result = api.export_private_key(address, "password123").await;
        assert!(private_key_result.is_ok(), "Không thể xuất private key");
        
        // Kiểm tra xóa ví
        let remove_result = api.remove_wallet(address).await;
        assert!(remove_result.is_ok(), "Không thể xóa ví");
        
        // Kiểm tra ví đã bị xóa
        let export_after_remove = api.export_seed_phrase(address, "password123").await;
        assert!(export_after_remove.is_err(), "Ví vẫn tồn tại sau khi xóa");
    }
    
    #[tokio::test]
    async fn test_max_wallet_limit() {
        let config = WalletSystemConfig {
            max_wallets: 1,
            default_chain_id: 1,
            allow_wallet_overwrite: false,
        };
        let mut api = WalletManagerApi::new(config);
        
        // Tạo ví đầu tiên
        let result1 = api.create_wallet(
            SeedLength::Twelve,
            None,
            ChainType::EVM,
            "password123",
            None
        ).await;
        assert!(result1.is_ok(), "Không thể tạo ví đầu tiên");
        
        // Thử tạo ví thứ hai (vượt quá giới hạn)
        let result2 = api.create_wallet(
            SeedLength::Twelve,
            None,
            ChainType::EVM,
            "password123",
            None
        ).await;
        assert!(result2.is_err(), "Tạo được ví vượt quá giới hạn");
        
        if let Err(WalletError::InvalidSeedOrKey(msg)) = result2 {
            assert_eq!(msg, ERR_MAX_WALLET_LIMIT.to_string());
        } else {
            panic!("Lỗi không phải InvalidSeedOrKey");
        }
    }
}