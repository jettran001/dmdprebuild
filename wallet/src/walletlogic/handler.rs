// External imports
use anyhow::Context as AnyhowContext;
use ethers::core::types::{TransactionRequest, U256};
use ethers::signers::LocalWallet;
use ethers::types::Address;
use tracing::{info, warn};

// Internal imports
use crate::error::WalletError;
use crate::walletlogic::crypto::decrypt_data;
use crate::walletlogic::init::WalletManager;
use crate::walletmanager::chain::{ChainManager, ChainType};
use crate::walletmanager::types::WalletSecret;

// Hằng số cho thông báo lỗi
const ERR_NO_SEED_PHRASE: &str = "No seed phrase available for this wallet";
const ERR_USE_EXPORT_SEED_PHRASE: &str = "This wallet has seed phrase, use export_seed_phrase instead";
const ERR_ONLY_EVM_SIGNING: &str = "Only EVM supported for signing";
const ERR_ONLY_EVM_SENDING: &str = "Only EVM supported for sending";
const ERR_ONLY_EVM_BALANCE: &str = "Only EVM supported for balance";

/// Trait định nghĩa các thao tác với ví.
pub trait WalletManagerHandler: Send + Sync + 'static {
    async fn export_seed_phrase_internal(&self, address: Address, password: &str) -> Result<String, WalletError>;
    async fn export_private_key_internal(&self, address: Address, password: &str) -> Result<String, WalletError>;
    async fn remove_wallet_internal(&mut self, address: Address) -> Result<(), WalletError>;
    async fn update_chain_id_internal(&mut self, address: Address, new_chain_id: u64, new_chain_type: ChainType) -> Result<(), WalletError>;
    async fn get_chain_id_internal(&self, address: Address) -> Result<u64, WalletError>;
    async fn has_wallet_internal(&self, address: Address) -> bool;
    async fn get_wallet_internal(&self, address: Address) -> Result<LocalWallet, WalletError>;
    async fn get_user_id_internal(&self, address: Address) -> Result<String, WalletError>;
    async fn list_wallets_internal(&self) -> Vec<Address>;
    async fn sign_transaction(&self, address: Address, tx: TransactionRequest, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<Vec<u8>, WalletError>;
    async fn send_transaction(&self, address: Address, tx: TransactionRequest, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<String, WalletError>;
    async fn get_balance(&self, address: Address, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<U256, WalletError>;
}

impl WalletManagerHandler for WalletManager {
    /// Xuất seed phrase từ ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví cần xuất seed phrase.
    /// - `password`: Mật khẩu để giải mã.
    ///
    /// # Returns
    /// Seed phrase gốc.
    ///
    /// # Flow
    /// Được gọi từ walletmanager::api.
    #[flow_from("walletmanager::api")]
    async fn export_seed_phrase_internal(&self, address: Address, password: &str) -> Result<String, WalletError> {
        info!("Exporting seed phrase for wallet: {}", address);
        let wallets = self.wallets.read().await;
        let info = wallets.get(&address)
            .context(format!("Không tìm thấy ví với địa chỉ {}", address))
            .map_err(|_| WalletError::WalletNotFound(address))?;
        
        let decrypted = decrypt_data(&info.encrypted_secret, &info.nonce, &info.salt, password)
            .context("Lỗi giải mã seed phrase")?;
        
        match &info.secret {
            WalletSecret::Seed(_) | WalletSecret::Encrypted => Ok(decrypted),
            WalletSecret::PrivateKey(_) => Err(WalletError::InvalidSeedOrKey(ERR_NO_SEED_PHRASE.to_string())),
        }
    }
    
    /// Xuất private key từ ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví cần xuất private key.
    /// - `password`: Mật khẩu để giải mã.
    ///
    /// # Returns
    /// Private key gốc.
    ///
    /// # Flow
    /// Được gọi từ walletmanager::api.
    #[flow_from("walletmanager::api")]
    async fn export_private_key_internal(&self, address: Address, password: &str) -> Result<String, WalletError> {
        info!("Exporting private key for wallet: {}", address);
        let wallets = self.wallets.read().await;
        let info = wallets.get(&address)
            .context(format!("Không tìm thấy ví với địa chỉ {}", address))
            .map_err(|_| WalletError::WalletNotFound(address))?;
        
        let decrypted = decrypt_data(&info.encrypted_secret, &info.nonce, &info.salt, password)
            .context("Lỗi giải mã private key")?;
        
        match &info.secret {
            WalletSecret::PrivateKey(_) | WalletSecret::Encrypted => Ok(decrypted),
            WalletSecret::Seed(_) => Err(WalletError::InvalidSeedOrKey(ERR_USE_EXPORT_SEED_PHRASE.to_string())),
        }
    }

    /// Xóa ví khỏi danh sách.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví cần xóa.
    ///
    /// # Flow
    /// Được gọi từ walletmanager::api.
    #[flow_from("walletmanager::api")]
    async fn remove_wallet_internal(&mut self, address: Address) -> Result<(), WalletError> {
        let mut wallets = self.wallets.write().await;
        if let Some(info) = wallets.remove(&address) {
            info!("Wallet {} removed, user_id: {}", address, info.user_id);
            Ok(())
        } else {
            warn!("Attempted to remove non-existent wallet {}", address);
            Err(WalletError::WalletNotFound(address))
                .context(format!("Không thể xóa ví không tồn tại: {}", address))?
        }
    }

    /// Cập nhật chain ID và loại chain cho ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví cần cập nhật.
    /// - `new_chain_id`: Chain ID mới.
    /// - `new_chain_type`: Loại chain mới.
    ///
    /// # Flow
    /// Được gọi từ walletmanager::api.
    #[flow_from("walletmanager::api")]
    async fn update_chain_id_internal(&mut self, address: Address, new_chain_id: u64, new_chain_type: ChainType) -> Result<(), WalletError> {
        let mut wallets = self.wallets.write().await;
        if let Some(info) = wallets.get_mut(&address) {
            info.chain_id = new_chain_id;
            info.chain_type = new_chain_type;
            info!("Updated chain_id to {} for wallet {}", new_chain_id, address);
            Ok(())
        } else {
            warn!("Attempted to update non-existent wallet {}", address);
            Err(WalletError::WalletNotFound(address))
                .context(format!("Không thể cập nhật chain cho ví không tồn tại: {}", address))?
        }
    }

    /// Lấy chain ID của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    ///
    /// # Flow
    /// Được gọi từ walletmanager::api.
    #[flow_from("walletmanager::api")]
    async fn get_chain_id_internal(&self, address: Address) -> Result<u64, WalletError> {
        let wallets = self.wallets.read().await;
        let info = wallets.get(&address)
            .context(format!("Không tìm thấy ví với địa chỉ {} khi lấy chain ID", address))
            .map_err(|_| WalletError::WalletNotFound(address))?;
        Ok(info.chain_id)
    }

    /// Kiểm tra ví có tồn tại không.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví cần kiểm tra.
    ///
    /// # Flow
    /// Được gọi từ walletmanager::api.
    #[flow_from("walletmanager::api")]
    async fn has_wallet_internal(&self, address: Address) -> bool {
        let wallets = self.wallets.read().await;
        wallets.contains_key(&address)
    }

    /// Lấy instance LocalWallet.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    ///
    /// # Flow
    /// Được gọi từ walletmanager::api.
    #[flow_from("walletmanager::api")]
    async fn get_wallet_internal(&self, address: Address) -> Result<LocalWallet, WalletError> {
        let wallets = self.wallets.read().await;
        let info = wallets.get(&address)
            .context(format!("Không tìm thấy ví với địa chỉ {} khi lấy instance wallet", address))
            .map_err(|_| WalletError::WalletNotFound(address))?;
        Ok(info.wallet.clone())
    }

    /// Lấy user ID.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    ///
    /// # Flow
    /// Được gọi từ walletmanager::api.
    #[flow_from("walletmanager::api")]
    async fn get_user_id_internal(&self, address: Address) -> Result<String, WalletError> {
        let wallets = self.wallets.read().await;
        wallets
            .get(&address)
            .map(|info| info.user_id.clone())
            .ok_or(WalletError::WalletNotFound(address))
            .context(format!("Không tìm thấy ví với địa chỉ {} khi lấy user ID", address))?
    }

    /// Liệt kê tất cả các ví.
    ///
    /// # Flow
    /// Được gọi từ walletmanager::api.
    #[flow_from("walletmanager::api")]
    async fn list_wallets_internal(&self) -> Vec<Address> {
        let wallets = self.wallets.read().await;
        wallets.keys().copied().collect()
    }

    /// Ký giao dịch.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví dùng để ký.
    /// - `tx`: Giao dịch cần ký.
    /// - `chain_manager`: Quản lý chain.
    ///
    /// # Flow
    /// Được gọi từ walletmanager::api.
    #[flow_from("walletmanager::api")]
    async fn sign_transaction(&self, address: Address, tx: TransactionRequest, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<Vec<u8>, WalletError> {
        let wallets = self.wallets.read().await;
        let info = wallets.get(&address)
            .context(format!("Không tìm thấy ví với địa chỉ {} khi ký giao dịch", address))
            .map_err(|_| WalletError::WalletNotFound(address))?;
        
        if info.chain_type != ChainType::EVM {
            return Err(WalletError::ChainNotSupported(ERR_ONLY_EVM_SIGNING.to_string()));
        }
        
        let wallet = &info.wallet;
        let provider = chain_manager.get_provider(info.chain_id)
            .context(format!("Không thể lấy provider cho chain ID {} khi ký giao dịch", info.chain_id))?;
        
        let signed_tx = wallet
            .sign_transaction_sync(&tx, provider)
            .context("Lỗi khi ký giao dịch")
            .map_err(|e| WalletError::TransactionFailed(e.to_string()))?;
        
        Ok(signed_tx)
    }

    /// Gửi giao dịch.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví gửi giao dịch.
    /// - `tx`: Giao dịch cần gửi.
    /// - `chain_manager`: Quản lý chain.
    ///
    /// # Flow
    /// Được gọi từ walletmanager::api.
    #[flow_from("walletmanager::api")]
    async fn send_transaction(&self, address: Address, tx: TransactionRequest, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<String, WalletError> {
        let wallets = self.wallets.read().await;
        let info = wallets.get(&address)
            .context(format!("Không tìm thấy ví với địa chỉ {}", address))
            .map_err(|_| WalletError::WalletNotFound(address))?;
        
        if info.chain_type != ChainType::EVM {
            return Err(WalletError::ChainNotSupported(ERR_ONLY_EVM_SENDING.to_string()));
        }
        
        let wallet = &info.wallet;
        let provider = chain_manager.get_provider(info.chain_id)
            .context(format!("Không thể lấy provider cho chain ID {}", info.chain_id))?;
        
        let tx_hash = provider
            .send_transaction(tx, Some(wallet))
            .await
            .context("Lỗi khi gửi giao dịch")
            .map_err(|e| WalletError::TransactionFailed(e.to_string()))?
            .tx_hash()
            .to_string();
        
        Ok(tx_hash)
    }

    /// Lấy số dư của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    /// - `chain_manager`: Quản lý chain.
    ///
    /// # Flow
    /// Được gọi từ walletmanager::api.
    #[flow_from("walletmanager::api")]
    async fn get_balance(&self, address: Address, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<U256, WalletError> {
        let wallets = self.wallets.read().await;
        let info = wallets.get(&address)
            .context(format!("Không tìm thấy ví với địa chỉ {}", address))
            .map_err(|_| WalletError::WalletNotFound(address))?;
        
        if info.chain_type != ChainType::EVM {
            return Err(WalletError::ChainNotSupported(ERR_ONLY_EVM_BALANCE.to_string()));
        }
        
        let provider = chain_manager.get_provider(info.chain_id)
            .context(format!("Không thể lấy provider cho chain ID {}", info.chain_id))?;
        
        let balance = provider
            .get_balance(address, None)
            .await
            .context(format!("Lỗi khi lấy số dư cho địa chỉ {}", address))
            .map_err(|e| WalletError::ProviderError(e.to_string()))?;
        
        Ok(balance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walletlogic::init::WalletManager;
    use crate::walletmanager::types::{SeedLength, WalletConfig};
    use crate::walletmanager::chain::ChainType;

    #[tokio::test]
    async fn test_export_seed_phrase() {
        let mut manager = WalletManager::new();
        let result = manager.create_wallet_internal(SeedLength::Twelve, 1, ChainType::EVM, "password").await;
        assert!(result.is_ok(), "Should create wallet successfully");
        
        let (address, seed_phrase, _) = result.expect("Valid wallet creation result");
        let exported = manager.export_seed_phrase_internal(address, "password")
            .await
            .expect("Should export seed phrase successfully");
            
        assert_eq!(exported, seed_phrase, "Exported seed phrase should match original");
    }

    #[tokio::test]
    async fn test_remove_wallet() {
        let mut manager = WalletManager::new();
        let result = manager.create_wallet_internal(SeedLength::Twelve, 1, ChainType::EVM, "password").await;
        assert!(result.is_ok(), "Should create wallet successfully");
        
        let (address, _, _) = result.expect("Valid wallet creation result");
        assert!(
            manager.remove_wallet_internal(address).await.is_ok(),
            "Should remove wallet successfully"
        );
        assert!(
            !manager.has_wallet_internal(address).await,
            "Wallet should no longer exist after removal"
        );
    }

    #[tokio::test]
    async fn test_get_user_id_internal() {
        let mut manager = WalletManager::new();
        let result = manager.create_wallet_internal(SeedLength::Twelve, 1, ChainType::EVM, "password").await;
        assert!(result.is_ok(), "Should create wallet successfully");
        
        let (address, _, user_id) = result.expect("Valid wallet creation result");
        let retrieved_id = manager.get_user_id_internal(address)
            .await
            .expect("Should retrieve user ID successfully");
            
        assert_eq!(retrieved_id, user_id, "Retrieved user ID should match original");
    }

    #[tokio::test]
    async fn test_list_wallets_internal() {
        let mut manager = WalletManager::new();
        
        let wallet1_result = manager.create_wallet_internal(SeedLength::Twelve, 1, ChainType::EVM, "password").await;
        assert!(wallet1_result.is_ok(), "Should create first wallet successfully");
        let (address1, _, _) = wallet1_result.expect("Valid first wallet creation result");
        
        let wallet2_result = manager.create_wallet_internal(SeedLength::Twelve, 1, ChainType::EVM, "password").await;
        assert!(wallet2_result.is_ok(), "Should create second wallet successfully");
        let (address2, _, _) = wallet2_result.expect("Valid second wallet creation result");
        
        let wallets = manager.list_wallets_internal().await;
        assert_eq!(wallets.len(), 2, "Should have two wallets");
        assert!(wallets.contains(&address1), "List should contain first wallet address");
        assert!(wallets.contains(&address2), "List should contain second wallet address");
    }

    #[tokio::test]
    async fn test_sign_transaction() {
        let mut manager = WalletManager::new();
        let mut chain_manager = ChainManager::new();
        
        let result = manager.create_wallet_internal(SeedLength::Twelve, 1, ChainType::EVM, "password").await;
        assert!(result.is_ok(), "Should create wallet successfully");
        
        let (address, _, _) = result.expect("Valid wallet creation result");
        let tx = TransactionRequest::new().to(Address::zero()).value(100);
        
        let sign_result = manager.sign_transaction(address, tx, &chain_manager).await;
        assert!(sign_result.is_ok(), "Should sign transaction successfully");
    }

    #[tokio::test]
    async fn test_get_balance() {
        let mut manager = WalletManager::new();
        let chain_manager = ChainManager::new();
        
        let result = manager.create_wallet_internal(SeedLength::Twelve, 1, ChainType::EVM, "password").await;
        assert!(result.is_ok(), "Should create wallet successfully");
        
        let (address, _, _) = result.expect("Valid wallet creation result");
        
        // Chú ý: Test này yêu cầu kết nối mạng thật
        let balance_result = manager.get_balance(address, &chain_manager).await;
        assert!(balance_result.is_ok(), "Should get balance successfully");
    }
}