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
                self.sign_transaction_solana(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::Tron => {
                self.sign_transaction_tron(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::Diamond => {
                // Diamond chain sử dụng cơ chế ký tương tự EVM
                self.sign_transaction_evm(wallet, address, tx, info.chain_id, chain_manager).await
            },
            ChainType::TON => {
                self.sign_transaction_ton(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::NEAR => {
                self.sign_transaction_near(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::Stellar => {
                self.sign_transaction_stellar(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::Sui => {
                self.sign_transaction_sui(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::Cosmos => {
                self.sign_transaction_cosmos(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::Hedera => {
                self.sign_transaction_hedera(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::Aptos => {
                self.sign_transaction_aptos(address, tx, info.chain_id, chain_manager).await
            },
            ChainType::BTC | ChainType::Bitcoin => {
                self.sign_transaction_bitcoin(address, tx, info.chain_id, chain_manager).await
            },
            _ => {
                // Blockchain chưa được hỗ trợ - cung cấp thông tin chi tiết về lộ trình phát triển
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

    /// Lấy số dư của ví
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    /// - `chain_manager`: Quản lý chain.
    ///
    /// # Returns
    /// Số dư của ví dưới dạng chuỗi
    ///
    /// # Errors
    /// Nếu không thể lấy số dư
    async fn get_balance(
        &self,
        address: Address,
        chain_manager: &(dyn ChainManager + Send + Sync + 'static),
    ) -> Result<String, WalletError> {
        // Kiểm tra xem ví có tồn tại không
        let wallets = self.wallets.read().await;
        let wallet_info = wallets
            .get(&address)
            .context(format!("Không tìm thấy ví với địa chỉ {}", address))
            .map_err(|_| WalletError::WalletNotFound(address))?;

        // Lấy thông tin chain
        let chain_type = wallet_info.chain_type;
        let chain_id = wallet_info.chain_id;
        
        tracing::info!("Lấy số dư cho địa chỉ {}, chain_type: {:?}, chain_id: {}", 
            address, chain_type, chain_id);
        
        // Lấy provider từ chain manager
        let provider = chain_manager
            .get_provider(chain_id)
            .await
            .context(format!("Không thể lấy provider cho chain ID {}", chain_id))
            .map_err(|e| {
                tracing::error!("Provider error: {}", e);
                WalletError::ProviderError(e.to_string())
            })?;

        // Xử lý theo từng loại blockchain
        match chain_type {
            ChainType::EVM | ChainType::Polygon | ChainType::BSC | ChainType::Arbitrum |
            ChainType::Optimism | ChainType::Fantom | ChainType::Avalanche => {
                self.get_balance_evm(address, provider).await
            }
            ChainType::Solana => {
                self.get_balance_solana(address, provider).await
            }
            ChainType::Tron => {
                self.get_balance_tron(address, provider).await
            }
            ChainType::Diamond => {
                self.get_balance_diamond(address, provider).await
            }
            ChainType::BTC | ChainType::Bitcoin => {
                self.get_balance_bitcoin(address, provider).await
            }
            ChainType::TON => {
                self.get_balance_ton(address, provider).await
            }
            ChainType::NEAR => {
                self.get_balance_near(address, provider).await
            }
            ChainType::Stellar => {
                self.get_balance_stellar(address, provider).await
            }
            ChainType::Sui => {
                self.get_balance_sui(address, provider).await
            }
            ChainType::Cosmos => {
                self.get_balance_cosmos(address, provider).await
            }
            ChainType::Hedera => {
                self.get_balance_hedera(address, provider).await
            }
            ChainType::Aptos => {
                self.get_balance_aptos(address, provider).await
            }
            _ => {
                let error_msg = format!(
                    "Blockchain {:?} chưa được hỗ trợ cho việc lấy số dư. Lộ trình phát triển: [Q3 2023: Solana, Tron] [Q4 2023: TON, NEAR, Cosmos] [Q1 2024: Các blockchain khác]",
                    chain_type
                );
                tracing::error!("{}", error_msg);
                Err(WalletError::ChainNotSupported(error_msg))
            }
        }
    }

    /// Lấy số dư ví EVM
    async fn get_balance_evm(
        &self,
        address: Address,
        provider: Arc<dyn Provider>,
    ) -> Result<String, WalletError> {
        // Số lần thử lại khi gặp lỗi mạng
        const MAX_RETRIES: u8 = 3;
        let mut retries = 0;
        let mut last_error = None;
        
        while retries < MAX_RETRIES {
            match provider.get_balance(address, None).await {
                Ok(balance) => {
                    // Định dạng số dư với đơn vị ether
                    let balance_eth = format!("{}.{:018}", 
                        balance / U256::exp10(18),
                        balance % U256::exp10(18)
                    );
                    
                    // Loại bỏ số 0 ở cuối
                    let balance_trimmed = balance_eth.trim_end_matches('0').trim_end_matches('.');
                    
                    tracing::debug!("Số dư EVM: {} wei = {} ETH", balance, balance_trimmed);
                    return Ok(balance_trimmed.to_string());
                }
                Err(e) => {
                    retries += 1;
                    last_error = Some(e);
                    
                    if retries < MAX_RETRIES {
                        tracing::warn!("Lỗi khi lấy số dư EVM (lần thử {}/{}), thử lại: {}", 
                            retries, MAX_RETRIES, last_error.as_ref().unwrap());
                        tokio::time::sleep(tokio::time::Duration::from_millis(500 * retries as u64)).await;
                    }
                }
            }
        }
        
        let error_msg = format!("Không thể lấy số dư EVM: {}", last_error.unwrap());
        tracing::error!("{}", error_msg);
        Err(WalletError::ProviderError(error_msg))
    }

    /// Lấy số dư ví Solana
    async fn get_balance_solana(
        &self,
        address: Address,
        provider: Arc<dyn Provider>,
    ) -> Result<String, WalletError> {
        #[cfg(feature = "solana")]
        {
            // TODO: Triển khai lấy số dư Solana với solana-sdk
            tracing::info!("Solana balance checking (roadmap: Q3 2023)");
            
            // Mã giả cho lấy số dư Solana
            // 1. Chuyển đổi địa chỉ sang định dạng Solana
            // 2. Kết nối đến RPC endpoint
            // 3. Gọi phương thức getBalance từ API
            // 4. Định dạng và trả về kết quả
        }
        
        let error_msg = "Solana balance checking not yet implemented (roadmap: Q3 2023)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }

    /// Lấy số dư ví Tron
    async fn get_balance_tron(
        &self,
        address: Address,
        provider: Arc<dyn Provider>,
    ) -> Result<String, WalletError> {
        #[cfg(feature = "tron")]
        {
            // TODO: Triển khai lấy số dư Tron với tron-sdk
            tracing::info!("Tron balance checking (roadmap: Q3 2023)");
            
            // Mã giả cho lấy số dư Tron
            // 1. Chuyển đổi địa chỉ từ format EVM sang format Tron
            // 2. Kết nối đến API Tron
            // 3. Lấy số dư TRX và token
            // 4. Định dạng và trả về kết quả
        }
        
        let error_msg = "Tron balance checking not yet implemented (roadmap: Q3 2023)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }

    /// Lấy số dư ví Diamond
    async fn get_balance_diamond(
        &self,
        address: Address,
        provider: Arc<dyn Provider>,
    ) -> Result<String, WalletError> {
        // Diamond chain sử dụng cơ chế tương tự EVM, với một số điều chỉnh đặc thù
        const MAX_RETRIES: u8 = 3;
        let mut retries = 0;
        let mut last_error = None;
        
        while retries < MAX_RETRIES {
            match provider.get_balance(address, None).await {
                Ok(balance) => {
                    // Định dạng số dư với đơn vị Diamond (DMD)
                    let balance_dmd = format!("{}.{:018}", 
                        balance / U256::exp10(18),
                        balance % U256::exp10(18)
                    );
                    
                    // Loại bỏ số 0 ở cuối
                    let balance_trimmed = balance_dmd.trim_end_matches('0').trim_end_matches('.');
                    
                    tracing::debug!("Số dư Diamond: {} wei = {} DMD", balance, balance_trimmed);
                    return Ok(balance_trimmed.to_string());
                }
                Err(e) => {
                    retries += 1;
                    last_error = Some(e);
                    
                    if retries < MAX_RETRIES {
                        tracing::warn!("Lỗi khi lấy số dư Diamond (lần thử {}/{}), thử lại: {}", 
                            retries, MAX_RETRIES, last_error.as_ref().unwrap());
                        tokio::time::sleep(tokio::time::Duration::from_millis(500 * retries as u64)).await;
                    }
                }
            }
        }
        
        let error_msg = format!("Không thể lấy số dư Diamond: {}", last_error.unwrap());
        tracing::error!("{}", error_msg);
        Err(WalletError::ProviderError(error_msg))
    }

    /// Lấy số dư ví Bitcoin
    async fn get_balance_bitcoin(
        &self,
        address: Address,
        provider: Arc<dyn Provider>,
    ) -> Result<String, WalletError> {
        let error_msg = "Bitcoin balance checking not yet implemented (roadmap: Q1 2024)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }

    /// Lấy số dư ví TON
    async fn get_balance_ton(
        &self,
        address: Address,
        provider: Arc<dyn Provider>,
    ) -> Result<String, WalletError> {
        let error_msg = "TON balance checking not yet implemented (roadmap: Q4 2023)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }

    /// Lấy số dư ví NEAR
    async fn get_balance_near(
        &self,
        address: Address,
        provider: Arc<dyn Provider>,
    ) -> Result<String, WalletError> {
        let error_msg = "NEAR balance checking not yet implemented (roadmap: Q4 2023)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }

    /// Lấy số dư ví Stellar
    async fn get_balance_stellar(
        &self,
        address: Address,
        provider: Arc<dyn Provider>,
    ) -> Result<String, WalletError> {
        let error_msg = "Stellar balance checking not yet implemented (roadmap: Q1 2024)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }

    /// Lấy số dư ví Sui
    async fn get_balance_sui(
        &self,
        address: Address,
        provider: Arc<dyn Provider>,
    ) -> Result<String, WalletError> {
        let error_msg = "Sui balance checking not yet implemented (roadmap: Q1 2024)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }

    /// Lấy số dư ví Cosmos
    async fn get_balance_cosmos(
        &self,
        address: Address,
        provider: Arc<dyn Provider>,
    ) -> Result<String, WalletError> {
        let error_msg = "Cosmos balance checking not yet implemented (roadmap: Q4 2023)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }

    /// Lấy số dư ví Hedera
    async fn get_balance_hedera(
        &self,
        address: Address,
        provider: Arc<dyn Provider>,
    ) -> Result<String, WalletError> {
        let error_msg = "Hedera balance checking not yet implemented (roadmap: Q1 2024)";
        tracing::warn!("{}", error_msg);
        Err(WalletError::NotImplemented(error_msg.to_string()))
    }

    /// Lấy số dư ví Aptos
    async fn get_balance_aptos(
        &self,
        address: Address,
        provider: Arc<dyn Provider>,
    ) -> Result<String, WalletError> {
        let error_msg = "Aptos balance checking not yet implemented (roadmap: Q1 2024)";
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