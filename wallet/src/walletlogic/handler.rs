// External imports
use anyhow::Context as AnyhowContext;
use ethers::core::types::{TransactionRequest, U256};
use ethers::signers::LocalWallet;
use ethers::types::Address;
use tracing::{info, warn};
use std::sync::Arc;

// Internal imports
use crate::error::WalletError;
use crate::defi::crypto::decrypt_data;
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
    async fn get_balance(&self, address: Address, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<String, WalletError>;
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
        
        // Logging transaction request details
        tracing::debug!("Signing transaction for address: {}, chain type: {:?}, chain id: {}", 
            address, info.chain_type, info.chain_id);
        
        // Check if wallet is available before proceeding
        let wallet = &info.wallet;
        
        match info.chain_type {
            ChainType::EVM | ChainType::Polygon | ChainType::BSC | ChainType::Arbitrum | 
            ChainType::Optimism | ChainType::Fantom | ChainType::Avalanche => {
                self.sign_transaction_evm(wallet, address, tx, info.chain_id, chain_manager).await
            },
            ChainType::Solana => {
                tracing::warn!("Solana transaction signing chưa được hỗ trợ. Đang phát triển theo roadmap Q3 2023. Vui lòng kiểm tra lại sau hoặc liên hệ support nếu cần gấp.");
                let error_msg = "Solana transaction signing chưa được hỗ trợ. Xem roadmap Q3 2023.";
                Err(WalletError::NotImplemented(error_msg.to_string()))
            },
            ChainType::Tron => {
                tracing::warn!("Tron transaction signing chưa được hỗ trợ. Đang phát triển theo roadmap Q3 2023. Vui lòng kiểm tra lại sau hoặc liên hệ support nếu cần gấp.");
                let error_msg = "Tron transaction signing chưa được hỗ trợ. Xem roadmap Q3 2023.";
                Err(WalletError::NotImplemented(error_msg.to_string()))
            },
            ChainType::Diamond => {
                // Diamond chain sử dụng cơ chế ký tương tự EVM
                self.sign_transaction_evm(wallet, address, tx, info.chain_id, chain_manager).await
            },
            ChainType::TON => {
                tracing::warn!("TON transaction signing chưa được hỗ trợ. Đang phát triển theo roadmap Q4 2023. Vui lòng kiểm tra lại sau hoặc liên hệ support nếu cần gấp.");
                let error_msg = "TON transaction signing chưa được hỗ trợ. Xem roadmap Q4 2023.";
                Err(WalletError::NotImplemented(error_msg.to_string()))
            },
            ChainType::NEAR => {
                tracing::warn!("NEAR transaction signing chưa được hỗ trợ. Đang phát triển theo roadmap Q4 2023. Vui lòng kiểm tra lại sau hoặc liên hệ support nếu cần gấp.");
                let error_msg = "NEAR transaction signing chưa được hỗ trợ. Xem roadmap Q4 2023.";
                Err(WalletError::NotImplemented(error_msg.to_string()))
            },
            ChainType::Stellar => {
                tracing::warn!("Stellar transaction signing chưa được hỗ trợ. Đang phát triển theo roadmap Q1 2024. Vui lòng kiểm tra lại sau hoặc liên hệ support nếu cần gấp.");
                let error_msg = "Stellar transaction signing chưa được hỗ trợ. Xem roadmap Q1 2024.";
                Err(WalletError::NotImplemented(error_msg.to_string()))
            },
            ChainType::Sui => {
                tracing::warn!("Sui transaction signing chưa được hỗ trợ. Đang phát triển theo roadmap Q1 2024. Vui lòng kiểm tra lại sau hoặc liên hệ support nếu cần gấp.");
                let error_msg = "Sui transaction signing chưa được hỗ trợ. Xem roadmap Q1 2024.";
                Err(WalletError::NotImplemented(error_msg.to_string()))
            },
            ChainType::Cosmos => {
                tracing::warn!("Cosmos transaction signing chưa được hỗ trợ. Đang phát triển theo roadmap Q4 2023. Vui lòng kiểm tra lại sau hoặc liên hệ support nếu cần gấp.");
                let error_msg = "Cosmos transaction signing chưa được hỗ trợ. Xem roadmap Q4 2023.";
                Err(WalletError::NotImplemented(error_msg.to_string()))
            },
            ChainType::Hedera => {
                tracing::warn!("Hedera transaction signing chưa được hỗ trợ. Đang phát triển theo roadmap Q1 2024. Vui lòng kiểm tra lại sau hoặc liên hệ support nếu cần gấp.");
                let error_msg = "Hedera transaction signing chưa được hỗ trợ. Xem roadmap Q1 2024.";
                Err(WalletError::NotImplemented(error_msg.to_string()))
            },
            ChainType::Aptos => {
                tracing::warn!("Aptos transaction signing chưa được hỗ trợ. Đang phát triển theo roadmap Q1 2024. Vui lòng kiểm tra lại sau hoặc liên hệ support nếu cần gấp.");
                let error_msg = "Aptos transaction signing chưa được hỗ trợ. Xem roadmap Q1 2024.";
                Err(WalletError::NotImplemented(error_msg.to_string()))
            },
            ChainType::BTC | ChainType::Bitcoin => {
                tracing::warn!("Bitcoin transaction signing chưa được hỗ trợ. Đang phát triển theo roadmap Q2 2024. Vui lòng kiểm tra lại sau hoặc liên hệ support nếu cần gấp.");
                let error_msg = "Bitcoin transaction signing chưa được hỗ trợ. Xem roadmap Q2 2024.";
                Err(WalletError::NotImplemented(error_msg.to_string()))
            },
            _ => {
                let error_msg = format!(
                    "Blockchain {} chưa được hỗ trợ cho việc ký giao dịch. Lộ trình phát triển: [Q3 2023: Solana, Tron] [Q4 2023: TON, NEAR, Cosmos] [Q1 2024: Các blockchain khác]",
                    info.chain_type
                );
                tracing::error!("{}", error_msg);
                Err(WalletError::ChainNotSupported(error_msg))
            }
        }
    }
    
    /// Hàm hỗ trợ để ký giao dịch EVM và các chain tương thích
    async fn sign_transaction_evm(&self, wallet: &LocalWallet, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<Vec<u8>, WalletError> {
        let provider = chain_manager.get_provider(chain_id)
            .await
            .context(format!("Không thể lấy provider cho chain ID {} khi ký giao dịch", chain_id))?;
        
        // Verify the connection to provider
        let _ = provider.get_block_number()
            .await
            .context(format!("Không thể kết nối đến blockchain, kiểm tra lại mạng"))
            .map_err(|e| {
                tracing::error!("Network connectivity issue: {}", e);
                WalletError::ProviderError(format!("Không thể kết nối đến blockchain: {}", e))
            })?;
        
        // Handle transaction signing with proper retry and error handling
        let mut retries = 0;
        const MAX_RETRIES: u8 = 3;
        let mut last_error = None;
        
        while retries < MAX_RETRIES {
            match wallet.sign_transaction_sync(&tx, provider.as_ref()) {
                Ok(signed_tx) => {
                    tracing::debug!("Transaction signed successfully for address: {}", address);
                    return Ok(signed_tx);
                },
                Err(e) => {
                    retries += 1;
                    last_error = Some(e);
                    
                    if retries < MAX_RETRIES {
                        tracing::warn!("Transaction signing failed (attempt {}/{}): {}", retries, MAX_RETRIES, last_error.as_ref().unwrap());
                        tokio::time::sleep(tokio::time::Duration::from_millis(500 * retries as u64)).await;
                    }
                }
            }
        }
        
        let error_msg = format!("Lỗi khi ký giao dịch EVM: {}", last_error.unwrap());
        tracing::error!("{}", error_msg);
        Err(WalletError::TransactionFailed(error_msg))
    }
    
    /// Hàm hỗ trợ để ký giao dịch Solana
    async fn sign_transaction_solana(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<Vec<u8>, WalletError> {
        #[cfg(feature = "solana")]
        {
            let wallets = self.wallets.read().await;
            let info = wallets.get(&address)
                .context(format!("Không tìm thấy ví với địa chỉ {}", address))
                .map_err(|_| WalletError::WalletNotFound(address))?;
            
            let wallet = &info.wallet;
            
            // TODO: Triển khai ký giao dịch Solana với solana-sdk
            // Solana transaction signing sẽ được triển khai trong Q3 2023
            tracing::info!("Solana transaction signing (roadmap: Q3 2023)");
            
            // Mã giả cho quá trình ký giao dịch Solana
            // 1. Lấy Solana keypair từ wallet
            // 2. Tạo Solana transaction từ tx parameter
            // 3. Ký transaction với keypair
            // 4. Trả về transaction đã ký
        }
        
        let error_msg = "Solana transaction signing not yet implemented (roadmap: Q3 2023)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }
    
    /// Hàm hỗ trợ để ký giao dịch Tron
    async fn sign_transaction_tron(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<Vec<u8>, WalletError> {
        #[cfg(feature = "tron")]
        {
            let wallets = self.wallets.read().await;
            let info = wallets.get(&address)
                .context(format!("Không tìm thấy ví với địa chỉ {}", address))
                .map_err(|_| WalletError::WalletNotFound(address))?;
            
            let wallet = &info.wallet;
            
            // TODO: Triển khai ký giao dịch Tron với tron-sdk
            // Tron transaction signing sẽ được triển khai trong Q3 2023
            tracing::info!("Tron transaction signing (roadmap: Q3 2023)");
            
            // Mã giả cho quá trình ký giao dịch Tron
            // 1. Convert EVM wallet sang Tron format
            // 2. Tạo Tron transaction từ tx parameter
            // 3. Ký transaction
            // 4. Trả về transaction đã ký
        }
        
        let error_msg = "Tron transaction signing not yet implemented (roadmap: Q3 2023)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }
    
    /// Hàm hỗ trợ để ký giao dịch TON
    async fn sign_transaction_ton(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<Vec<u8>, WalletError> {
        let error_msg = "TON transaction signing not yet implemented (roadmap: Q4 2023)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }
    
    /// Hàm hỗ trợ để ký giao dịch NEAR
    async fn sign_transaction_near(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<Vec<u8>, WalletError> {
        let error_msg = "NEAR transaction signing not yet implemented (roadmap: Q4 2023)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }
    
    /// Hàm hỗ trợ để ký giao dịch Stellar
    async fn sign_transaction_stellar(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<Vec<u8>, WalletError> {
        let error_msg = "Stellar transaction signing not yet implemented (roadmap: Q1 2024)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }
    
    /// Hàm hỗ trợ để ký giao dịch Sui
    async fn sign_transaction_sui(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<Vec<u8>, WalletError> {
        let error_msg = "Sui transaction signing not yet implemented (roadmap: Q1 2024)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }
    
    /// Hàm hỗ trợ để ký giao dịch Cosmos
    async fn sign_transaction_cosmos(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<Vec<u8>, WalletError> {
        let error_msg = "Cosmos transaction signing not yet implemented (roadmap: Q4 2023)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }
    
    /// Hàm hỗ trợ để ký giao dịch Hedera
    async fn sign_transaction_hedera(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<Vec<u8>, WalletError> {
        let error_msg = "Hedera transaction signing not yet implemented (roadmap: Q1 2024)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }
    
    /// Hàm hỗ trợ để ký giao dịch Aptos
    async fn sign_transaction_aptos(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<Vec<u8>, WalletError> {
        let error_msg = "Aptos transaction signing not yet implemented (roadmap: Q1 2024)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }
    
    /// Hàm hỗ trợ để ký giao dịch Bitcoin
    async fn sign_transaction_bitcoin(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<Vec<u8>, WalletError> {
        let error_msg = "Bitcoin transaction signing not yet implemented (roadmap: Q1 2024)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
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
        
        // Transaction logging
        tracing::info!(
            "Sending transaction from address: {}, chain_type: {:?}, chain_id: {}", 
            address, info.chain_type, info.chain_id
        );
        
        // General validation for all transaction types
        if let Some(value) = tx.value {
            if value.is_zero() {
                tracing::warn!("Transaction has zero value - this may be intended for contract interactions");
            }
        }
        
        match info.chain_type {
            ChainType::EVM | ChainType::Polygon | ChainType::BSC | ChainType::Arbitrum | 
            ChainType::Optimism | ChainType::Fantom | ChainType::Avalanche => {
                self.send_transaction_evm(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::Solana => {
                self.send_transaction_solana(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::Tron => {
                self.send_transaction_tron(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::Diamond => {
                self.send_transaction_diamond(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::BTC | ChainType::Bitcoin => {
                self.send_transaction_bitcoin(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::TON => {
                self.send_transaction_ton(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::NEAR => {
                self.send_transaction_near(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::Stellar => {
                self.send_transaction_stellar(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::Sui => {
                self.send_transaction_sui(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::Cosmos => {
                self.send_transaction_cosmos(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::Hedera => {
                self.send_transaction_hedera(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::Aptos => {
                self.send_transaction_aptos(address, tx, info.chain_id, chain_manager).await
            },
            _ => {
                // Blockchain chưa được hỗ trợ - cung cấp thông tin về lộ trình phát triển
                let error_msg = format!(
                    "Blockchain {:?} chưa được hỗ trợ cho việc gửi giao dịch. Lộ trình phát triển: [Q3 2023: Solana, Tron] [Q4 2023: TON, NEAR, Cosmos] [Q1 2024: Các blockchain khác]",
                    info.chain_type
                );
                tracing::error!("{}", error_msg);
                Err(WalletError::ChainNotSupported(error_msg))
            }
        }
    }
    
    /// Hàm hỗ trợ để gửi giao dịch EVM
    async fn send_transaction_evm(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<String, WalletError> {
        // Lấy thông tin cần thiết
        let wallet = match self.get_wallet_internal(address).await {
            Ok(wallet) => wallet,
            Err(e) => {
                tracing::error!("Không thể lấy ví {}: {}", address, e);
                return Err(e);
            }
        };
        
        let provider = chain_manager.get_provider(chain_id)
                    .await
            .context(format!("Không thể lấy provider cho chain ID {}", chain_id))
            .map_err(|e| {
                tracing::error!("Provider error: {}", e);
                WalletError::ProviderError(e.to_string())
            })?;
        
        // Kiểm tra kết nối mạng trước khi tiếp tục
        match provider.get_block_number().await {
            Ok(_) => tracing::debug!("Network connection verified for chain_id: {}", chain_id),
            Err(e) => {
                let error_msg = format!("Không thể kết nối đến blockchain, vui lòng kiểm tra lại mạng: {}", e);
                tracing::error!("{}", error_msg);
                return Err(WalletError::ProviderError(error_msg));
            }
        }
                
                // Kiểm tra số dư trước khi gửi giao dịch
                if let Some(value) = tx.value {
            let balance = match provider.get_balance(address, None).await {
                Ok(balance) => balance,
                Err(e) => {
                    tracing::error!("Không thể lấy số dư cho địa chỉ {}: {}", address, e);
                    return Err(WalletError::ProviderError(format!("Không thể lấy số dư: {}", e)));
                }
            };
                    
                    // Ước tính gas cost
                    let gas_price = match tx.gas_price {
                        Some(price) => price,
                        None => match provider.get_gas_price().await {
                            Ok(price) => price,
                            Err(e) => {
                                tracing::error!("Không thể lấy giá gas: {}", e);
                                return Err(WalletError::ProviderError(format!("Không thể lấy giá gas: {}", e)));
                            }
                        }
                    };
                    
            // Ước tính gas limit nếu chưa được chỉ định
                    let gas_limit = match tx.gas {
                        Some(limit) => limit,
                        None => {
                    // Cố gắng ước tính gas limit
                    match provider.estimate_gas(&tx, None).await {
                        Ok(estimate) => {
                            // Thêm 20% buffer cho an toàn
                            estimate.saturating_mul(12).saturating_div(10)
                        },
                        Err(_) => {
                            // Fallback đến giá trị mặc định dựa trên loại giao dịch
                            if tx.to.is_some() && tx.data.is_none() {
                                // Transfer đơn giản
                            U256::from(21000) 
                            } else {
                                // Contract interaction
                                U256::from(150000)
                            }
                        }
                    }
                        }
                    };
                    
                    let estimated_cost = value + (gas_price * gas_limit);
                    
                    if balance < estimated_cost {
                let error_msg = format!(
                            "Số dư không đủ: có {} < cần {} (giá trị {} + gas {})", 
                            balance, estimated_cost, value, gas_price * gas_limit
                );
                tracing::warn!("{}", error_msg);
                return Err(WalletError::InsufficientFunds(error_msg));
            }
        }
        
        // Triển khai cơ chế retry để gửi giao dịch
        let mut retries = 0;
        const MAX_RETRIES: u8 = 3;
        let mut last_error = None;
        
        while retries < MAX_RETRIES {
            match provider.send_transaction(tx.clone(), Some(&wallet)).await {
                Ok(pending_tx) => {
                    let tx_hash = pending_tx.tx_hash().to_string();
                    tracing::info!("Giao dịch đã gửi thành công, hash: {}", tx_hash);
                    
                    // Kiểm tra nếu mạng chậm, cung cấp thêm thông tin cho người dùng
                    match provider.get_transaction_receipt(pending_tx.tx_hash()).await {
                        Ok(Some(_)) => tracing::debug!("Giao dịch đã được xác nhận"),
                        Ok(None) => tracing::debug!("Giao dịch đang chờ xác nhận"),
                        Err(e) => tracing::warn!("Không thể kiểm tra trạng thái giao dịch: {}", e),
                    }
                    
                    return Ok(tx_hash);
                },
                Err(e) => {
                    retries += 1;
                    last_error = Some(e);
                    
                    // Đọc thông báo lỗi để xử lý đúng
                    let error_str = last_error.as_ref().unwrap().to_string();
                    
                    if error_str.contains("nonce too low") || error_str.contains("already known") {
                        // Lỗi về nonce hoặc giao dịch đã được gửi
                        let updated_tx = tx.clone().nonce(provider.get_transaction_count(address, None).await.unwrap_or_default());
                        tx = updated_tx;
                        tracing::warn!("Điều chỉnh nonce do lỗi: {}", error_str);
                    } else if error_str.contains("gas price too low") || error_str.contains("underpriced") {
                        // Lỗi về gas price
                        let current_gas_price = provider.get_gas_price().await.unwrap_or_default();
                        let new_gas_price = current_gas_price.saturating_mul(12).saturating_div(10); // Tăng 20%
                        let updated_tx = tx.clone().gas_price(new_gas_price);
                        tx = updated_tx;
                        tracing::warn!("Tăng gas price do lỗi: {}", error_str);
                    } else if error_str.contains("gas limit") || error_str.contains("intrinsic gas") {
                        // Lỗi về gas limit
                        let new_gas_limit = tx.gas.unwrap_or(U256::from(21000)).saturating_mul(13).saturating_div(10); // Tăng 30%
                        let updated_tx = tx.clone().gas(new_gas_limit);
                        tx = updated_tx;
                        tracing::warn!("Tăng gas limit do lỗi: {}", error_str);
                    }
                    
                    if retries < MAX_RETRIES {
                        let delay = 1000 * retries as u64; // Tăng thời gian delay giữa các lần thử
                        tracing::warn!("Lỗi khi gửi giao dịch (lần thử {}/{}), thử lại sau {}ms: {}", 
                            retries, MAX_RETRIES, delay, last_error.as_ref().unwrap());
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                    }
                }
            }
        }
        
        // Phân tích lỗi để cung cấp thông tin hữu ích hơn
        let error_str = last_error.as_ref().unwrap().to_string();
        let detailed_error = if error_str.contains("nonce too low") {
            "Lỗi nonce: nonce quá thấp, có thể bạn đã gửi giao dịch khác từ địa chỉ này"
        } else if error_str.contains("gas price too low") || error_str.contains("underpriced") {
            "Lỗi gas price: giá gas quá thấp, mạng có thể đang bận"
        } else if error_str.contains("out of gas") || error_str.contains("intrinsic gas") {
            "Lỗi gas limit: không đủ gas cho giao dịch, hãy tăng gas limit"
        } else if error_str.contains("execution reverted") {
            "Lỗi thực thi: hợp đồng thông minh đã từ chối giao dịch"
        } else {
            "Lỗi không xác định khi gửi giao dịch"
        };
        
        let error_msg = format!("{}: {}", detailed_error, error_str);
        tracing::error!("{}", error_msg);
        Err(WalletError::TransactionFailed(error_msg))
    }

    /// Hàm hỗ trợ để gửi giao dịch Solana
    async fn send_transaction_solana(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<String, WalletError> {
        #[cfg(feature = "solana")]
        {
            let wallets = self.wallets.read().await;
            let info = wallets.get(&address)
                .context(format!("Không tìm thấy ví với địa chỉ {}", address))
                .map_err(|_| WalletError::WalletNotFound(address))?;
            
            let wallet = &info.wallet;
            
            // TODO: Triển khai gửi giao dịch Solana với solana-sdk
            // Solana transaction sending sẽ được triển khai trong Q3 2023
            tracing::info!("Solana transaction sending (roadmap: Q3 2023)");
            
            // Mã giả cho quá trình gửi giao dịch Solana
            // 1. Lấy Solana keypair từ wallet
            // 2. Tạo và ký Solana transaction từ tx parameter
            // 3. Gửi transaction đến mạng
            // 4. Theo dõi xác nhận và trả về signature
        }
        
        let error_msg = "Solana transaction sending not yet implemented (roadmap: Q3 2023)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }
    
    /// Hàm hỗ trợ để gửi giao dịch Tron
    async fn send_transaction_tron(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<String, WalletError> {
        #[cfg(feature = "tron")]
        {
            let wallets = self.wallets.read().await;
            let info = wallets.get(&address)
                .context(format!("Không tìm thấy ví với địa chỉ {}", address))
                .map_err(|_| WalletError::WalletNotFound(address))?;
            
            let wallet = &info.wallet;
            
            // TODO: Triển khai gửi giao dịch Tron với tron-sdk
            // Tron transaction sending sẽ được triển khai trong Q3 2023
            tracing::info!("Tron transaction sending (roadmap: Q3 2023)");
            
            // Mã giả cho quá trình gửi giao dịch Tron
            // 1. Convert EVM wallet sang Tron format
            // 2. Tạo và ký Tron transaction từ tx parameter
            // 3. Gửi transaction đến mạng
            // 4. Theo dõi xác nhận và trả về transaction ID
        }
        
        let error_msg = "Tron transaction sending not yet implemented (roadmap: Q3 2023)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }
    
    /// Hàm hỗ trợ để gửi giao dịch Diamond
    async fn send_transaction_diamond(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<String, WalletError> {
        // Diamond chain sử dụng cơ chế gửi giao dịch tương tự EVM, nhưng cần xử lý đặc thù
        let wallet = match self.get_wallet_internal(address).await {
            Ok(wallet) => wallet,
            Err(e) => {
                tracing::error!("Không thể lấy ví Diamond {}: {}", address, e);
                return Err(e);
            }
        };
            
        let provider = chain_manager.get_provider(chain_id)
            .await
            .context(format!("Không thể lấy provider cho Diamond chain ID {}", chain_id))
            .map_err(|e| {
                tracing::error!("Diamond provider error: {}", e);
                WalletError::ProviderError(e.to_string())
            })?;
            
        // Kiểm tra số dư trước khi gửi giao dịch
        if let Some(value) = tx.value {
            let balance = provider.get_balance(address, None).await
                .context(format!("Không thể lấy số dư Diamond cho địa chỉ {}", address))
                .map_err(|e| WalletError::ProviderError(e.to_string()))?;
                
            // Áp dụng phí giao dịch đặc thù của Diamond blockchain
            let diamond_fee = U256::from(1_000_000); // 0.001 DMD
            let estimated_cost = value + diamond_fee;
            
            if balance < estimated_cost {
                let error_msg = format!(
                    "Số dư Diamond không đủ: có {} < cần {} (giá trị {} + phí {})",
                    balance, estimated_cost, value, diamond_fee
                );
                tracing::warn!("{}", error_msg);
                return Err(WalletError::InsufficientFunds(error_msg));
            }
        }
        
        // Triển khai cơ chế retry để gửi giao dịch
        let mut retries = 0;
        const MAX_RETRIES: u8 = 3;
        let mut last_error = None;
        
        while retries < MAX_RETRIES {
            match provider.send_transaction(tx.clone(), Some(&wallet)).await {
                Ok(pending_tx) => {
                    let tx_hash = pending_tx.tx_hash().to_string();
                    tracing::info!("Giao dịch Diamond đã gửi thành công, hash: {}", tx_hash);
                    return Ok(tx_hash);
                },
                Err(e) => {
                    retries += 1;
                    last_error = Some(e);
                    
                    if retries < MAX_RETRIES {
                        tracing::warn!("Lỗi khi gửi giao dịch Diamond (lần thử {}/{}), thử lại: {}", 
                            retries, MAX_RETRIES, last_error.as_ref().unwrap());
                        tokio::time::sleep(tokio::time::Duration::from_millis(500 * retries as u64)).await;
                    }
                }
            }
        }
        
        let error_msg = format!("Lỗi khi gửi giao dịch Diamond: {}", last_error.unwrap());
        tracing::error!("{}", error_msg);
        Err(WalletError::TransactionFailed(error_msg))
    }
    
    /// Hàm hỗ trợ để gửi giao dịch Bitcoin
    async fn send_transaction_bitcoin(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<String, WalletError> {
        let error_msg = "Bitcoin transaction sending not yet implemented (roadmap: Q1 2024)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }
    
    /// Hàm hỗ trợ để gửi giao dịch TON
    async fn send_transaction_ton(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<String, WalletError> {
        let error_msg = "TON transaction sending not yet implemented (roadmap: Q4 2023)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }

    /// Hàm hỗ trợ để gửi giao dịch NEAR
    async fn send_transaction_near(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<String, WalletError> {
        let error_msg = "NEAR transaction sending not yet implemented (roadmap: Q4 2023)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }

    /// Hàm hỗ trợ để gửi giao dịch Stellar
    async fn send_transaction_stellar(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<String, WalletError> {
        let error_msg = "Stellar transaction sending not yet implemented (roadmap: Q1 2024)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }

    /// Hàm hỗ trợ để gửi giao dịch Sui
    async fn send_transaction_sui(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<String, WalletError> {
        let error_msg = "Sui transaction sending not yet implemented (roadmap: Q1 2024)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }

    /// Hàm hỗ trợ để gửi giao dịch Cosmos
    async fn send_transaction_cosmos(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<String, WalletError> {
        let error_msg = "Cosmos transaction sending not yet implemented (roadmap: Q4 2023)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }

    /// Hàm hỗ trợ để gửi giao dịch Hedera
    async fn send_transaction_hedera(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<String, WalletError> {
        let error_msg = "Hedera transaction sending not yet implemented (roadmap: Q1 2024)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }

    /// Hàm hỗ trợ để gửi giao dịch Aptos
    async fn send_transaction_aptos(&self, address: Address, tx: TransactionRequest, chain_id: u64, chain_manager: &(dyn ChainManager + Send + Sync + 'static)) -> Result<String, WalletError> {
        let error_msg = "Aptos transaction sending not yet implemented (roadmap: Q1 2024)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walletlogic::init::WalletManager;
    use crate::walletmanager::types::{SeedLength, WalletConfig};
    use crate::walletmanager::chain::ChainType;
    use std::sync::Arc;
    use async_trait::async_trait;

    struct MockChainManager;
    #[async_trait]
    impl ChainManager for MockChainManager {
        async fn get_provider(&self, _chain_id: u64) -> Result<Arc<dyn Provider>, WalletError> {
            Ok(Arc::new(MockProvider))
        }
        // ... implement các trait method khác nếu cần ...
    }

    struct MockProvider;
    #[async_trait]
    impl Provider for MockProvider {
        async fn get_block_number(&self) -> Result<u64, WalletError> { Ok(1) }
        async fn get_balance(&self, _address: Address, _block: Option<u64>) -> Result<U256, WalletError> { Ok(U256::from(1000000000000000000u128)) }
        // ... implement các trait method khác nếu cần ...
    }

    #[tokio::test]
    async fn test_sign_transaction_success() {
        let mut manager = WalletManager::new();
        let chain_manager = MockChainManager;
        let result = manager.create_wallet_internal(SeedLength::Twelve, 1, ChainType::EVM, "password").await;
        assert!(result.is_ok(), "Should create wallet successfully");
        let (address, _, _) = result.expect("Valid wallet creation result");
        let tx = TransactionRequest::new().to(Address::zero()).value(100);
        let sign_result = manager.sign_transaction(address, tx, &chain_manager).await;
        assert!(sign_result.is_ok(), "Should sign transaction successfully");
    }

    #[tokio::test]
    async fn test_sign_transaction_wallet_not_found() {
        let manager = WalletManager::new();
        let chain_manager = MockChainManager;
        let tx = TransactionRequest::new().to(Address::zero()).value(100);
        let fake_address = Address::random();
        let sign_result = manager.sign_transaction(fake_address, tx, &chain_manager).await;
        assert!(sign_result.is_err(), "Should fail if wallet not found");
    }

    // ... các test khác giữ nguyên ...
}