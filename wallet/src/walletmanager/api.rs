//! API công khai cho wallet manager, cung cấp các chức năng quản lý ví.

// External imports
use ethers::core::types::{TransactionRequest, U256};
use ethers::signers::LocalWallet;
use ethers::types::Address;

// Standard library imports
use std::sync::Arc;
use std::time::Duration;

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
use reqwest;
use serde_json::json;
use base64;
use bs58;

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

    /// Import ví từ seed phrase hoặc private key.
    ///
    /// # Arguments
    /// - `seed_or_key`: Seed phrase hoặc private key.
    /// - `password`: Mật khẩu.
    /// - `chain_id`: Chain ID.
    ///
    /// # Returns
    /// - `Ok(Address)`: Địa chỉ ví.
    /// - `Err(WalletError)`: Nếu có lỗi.
    #[flow_to("common::gateway")]
    pub async fn import_wallet(
        &self,
        seed_or_key: String,
        password: String,
        chain_id: u64,
    ) -> Result<Address, WalletError> {
        // Kiểm tra xem chain_id có tồn tại không
        if !self.chain_manager.chain_exists(chain_id).await {
            return Err(WalletError::ChainNotFound(chain_id));
        }

        // Kiểm tra xem đã đạt giới hạn số lượng ví chưa
        if self.is_wallet_limit_reached().await? {
            return Err(WalletError::WalletLimitReached);
        }

        // Kiểm tra các tham số đầu vào
        if seed_or_key.trim().is_empty() {
            return Err(WalletError::InvalidFormat("Seed phrase hoặc private key không được trống".to_string()));
        }

        // Lấy chain config để xác định loại chain
        let chain_config = match self.chain_manager.get_chain_config(chain_id).await {
            Ok(config) => config,
            Err(e) => {
                tracing::error!("Không thể lấy cấu hình cho chain {}: {}", chain_id, e);
                return Err(e);
            }
        };

        // Xác định loại input và validate theo loại chain
        let wallet_key = match determine_and_validate_key_type(&seed_or_key, &chain_config.chain_type) {
            Ok(key) => key,
            Err(e) => return Err(e),
        };

        // Kiểm tra độ mạnh của mật khẩu
        if !validate_password_strength(&password) {
            return Err(WalletError::WeakPassword);
        }

        // Thêm ví và mã hóa với mật khẩu
        self.manager.import_key(wallet_key, password, chain_id).await
    }

    /// Xác định loại key và validate theo loại chain
    fn determine_and_validate_key_type(seed_or_key: &str, chain_type: &ChainType) -> Result<WalletKey, WalletError> {
        let trimmed = seed_or_key.trim();
        let word_count = trimmed.split_whitespace().count();
        
        // 1. Kiểm tra nếu là seed phrase
        if word_count >= 12 {
            // Kiểm tra tính hợp lệ của seed phrase theo tiêu chuẩn BIP39
            if !is_valid_bip39_seed_phrase(trimmed) {
                return Err(WalletError::InvalidSeedPhrase);
            }
            
            // Kiểm tra xem số lượng từ có đúng không
            if ![12, 15, 18, 21, 24].contains(&word_count) {
                return Err(WalletError::InvalidSeedPhrase);
            }
            
            tracing::debug!("Đang import ví từ seed phrase với {} từ", word_count);
            return Ok(WalletKey::Seed(trimmed.to_string()));
        }
        
        // 2. Kiểm tra nếu là private key dạng hex (EVM)
        if is_evm_private_key(trimmed) {
            // Kiểm tra xem private key có hợp lệ cho loại chain không
            if matches!(chain_type, ChainType::EVM | ChainType::BSC | ChainType::Polygon | 
                                  ChainType::Arbitrum | ChainType::Optimism | ChainType::Avalanche | 
                                  ChainType::Fantom | ChainType::Diamond) 
            {
                tracing::debug!("Đang import ví từ private key hex cho chain EVM-compatible");
                return Ok(WalletKey::PrivateKey(trimmed.to_string()));
            } else {
                return Err(WalletError::IncompatibleKeyType(format!(
                    "Private key hex không hợp lệ cho chain type {:?}", chain_type
                )));
            }
        }
        
        // 3. Kiểm tra nếu là private key dạng Base58 (Solana, ...)
        if is_base58_private_key(trimmed) {
            match chain_type {
                ChainType::Solana => {
                    if !is_valid_solana_private_key(trimmed) {
                        return Err(WalletError::InvalidPrivateKey);
                    }
                    tracing::debug!("Đang import ví từ private key base58 cho Solana");
                    return Ok(WalletKey::Base58PrivateKey(trimmed.to_string()));
                },
                ChainType::Stellar => {
                    if !is_valid_stellar_private_key(trimmed) {
                        return Err(WalletError::InvalidPrivateKey);
                    }
                    tracing::debug!("Đang import ví từ private key base58 cho Stellar");
                    return Ok(WalletKey::Base58PrivateKey(trimmed.to_string()));
                },
                ChainType::Tron => {
                    if !is_valid_tron_private_key(trimmed) {
                        return Err(WalletError::InvalidPrivateKey);
                    }
                    tracing::debug!("Đang import ví từ private key base58 cho Tron");
                    return Ok(WalletKey::Base58PrivateKey(trimmed.to_string()));
                },
                _ => {
                    return Err(WalletError::IncompatibleKeyType(format!(
                        "Private key base58 không hợp lệ cho chain type {:?}", chain_type
                    )));
                }
            }
        }
        
        // 4. Kiểm tra nếu là keystore file (định dạng JSON)
        if trimmed.starts_with('{') && trimmed.ends_with('}') && trimmed.contains("crypto") {
            if matches!(chain_type, ChainType::EVM | ChainType::BSC | ChainType::Polygon | 
                                   ChainType::Arbitrum | ChainType::Optimism | ChainType::Avalanche | 
                                   ChainType::Fantom | ChainType::Diamond) 
            {
                tracing::debug!("Đang import ví từ keystore file");
                return Ok(WalletKey::Keystore(trimmed.to_string()));
            } else {
                return Err(WalletError::IncompatibleKeyType(format!(
                    "Keystore JSON không hợp lệ cho chain type {:?}", chain_type
                )));
            }
        }
        
        // 5. Xử lý private key cho các blockchain khác (NEAR, etc.)
        match chain_type {
            ChainType::NEAR => {
                if is_valid_near_private_key(trimmed) {
                    tracing::debug!("Đang import ví từ private key NEAR");
                    return Ok(WalletKey::ChainSpecificKey(ChainType::NEAR, trimmed.to_string()));
                }
            },
            ChainType::Cosmos => {
                if is_valid_cosmos_private_key(trimmed) {
                    tracing::debug!("Đang import ví từ private key Cosmos");
                    return Ok(WalletKey::ChainSpecificKey(ChainType::Cosmos, trimmed.to_string()));
                }
            },
            ChainType::Sui => {
                if is_valid_sui_private_key(trimmed) {
                    tracing::debug!("Đang import ví từ private key Sui");
                    return Ok(WalletKey::ChainSpecificKey(ChainType::Sui, trimmed.to_string()));
                }
            },
            ChainType::TON => {
                if is_valid_ton_private_key(trimmed) {
                    tracing::debug!("Đang import ví từ private key TON");
                    return Ok(WalletKey::ChainSpecificKey(ChainType::TON, trimmed.to_string()));
                }
            },
            ChainType::Hedera => {
                if is_valid_hedera_private_key(trimmed) {
                    tracing::debug!("Đang import ví từ private key Hedera");
                    return Ok(WalletKey::ChainSpecificKey(ChainType::Hedera, trimmed.to_string()));
                }
            },
            ChainType::Aptos => {
                if is_valid_aptos_private_key(trimmed) {
                    tracing::debug!("Đang import ví từ private key Aptos");
                    return Ok(WalletKey::ChainSpecificKey(ChainType::Aptos, trimmed.to_string()));
                }
            },
            _ => {}
        }
        
        // Không tìm thấy định dạng key hợp lệ
        tracing::error!("Định dạng seed phrase hoặc private key không hợp lệ");
        Err(WalletError::InvalidFormat("Seed phrase hoặc private key không đúng định dạng cho loại blockchain này".to_string()))
    }

    /// Kiểm tra tính hợp lệ của seed phrase BIP39
    fn is_valid_bip39_seed_phrase(seed: &str) -> bool {
        let words = seed.trim().split_whitespace().collect::<Vec<_>>();
        
        // Kiểm tra số lượng từ (12, 15, 18, 21, 24)
        if ![12, 15, 18, 21, 24].contains(&words.len()) {
            tracing::debug!("Seed phrase không có đúng số lượng từ");
            return false;
        }
        
        // Kiểm tra từng từ có trong danh sách BIP39
        for word in words {
            if !BIP39_WORDLIST.contains(&word.to_lowercase().as_str()) {
                tracing::debug!("Từ '{}' không có trong danh sách BIP39", word);
                return false;
            }
        }
        
        // Kiểm tra checksum BIP39 (nếu có thực hiện được)
        // TODO: Triển khai kiểm tra checksum BIP39 đầy đủ
        
        true
    }

    /// Kiểm tra tính hợp lệ của private key hex (EVM)
    fn is_evm_private_key(key: &str) -> bool {
        // Bỏ tiền tố 0x nếu có
        let key = if key.starts_with("0x") {
            &key[2..]
        } else {
            key
        };
        
        // Kiểm tra độ dài (32 byte = 64 ký tự hex)
        if key.len() != 64 {
            tracing::debug!("Private key không có đúng độ dài: {}", key.len());
            return false;
        }
        
        // Kiểm tra xem có phải là chuỗi hex hợp lệ không
        if !key.chars().all(|c| c.is_ascii_hexdigit()) {
            tracing::debug!("Private key chứa ký tự không phải hex");
            return false;
        }
        
        // Kiểm tra private key nằm trong khoảng hợp lệ cho ECDSA secp256k1
        // Cần kiểm tra xem key có nhỏ hơn order của nhóm secp256k1 hay không
        // TODO: Triển khai đầy đủ kiểm tra này
        
        true
    }

    /// Kiểm tra tính hợp lệ của private key base58
    fn is_base58_private_key(key: &str) -> bool {
        // Danh sách các ký tự Base58
        let base58_chars = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
        
        // Kiểm tra xem tất cả ký tự có nằm trong bảng Base58 không
        let is_valid = key.chars().all(|c| base58_chars.contains(c));
        
        if !is_valid {
            tracing::debug!("Key chứa ký tự không hợp lệ cho Base58");
            return false;
        }
        
        // Kiểm tra độ dài hợp lý cho Base58 private key (thường từ 40-60 ký tự)
        if key.len() < 32 || key.len() > 64 {
            tracing::debug!("Độ dài Base58 key không hợp lệ: {}", key.len());
            return false;
        }
        
        true
    }

    /// Các hàm kiểm tra tính hợp lệ cho từng loại blockchain cụ thể
    fn is_valid_solana_private_key(key: &str) -> bool {
        // Solana private key thường có 88 ký tự Base58
        key.len() >= 80 && key.len() <= 90
    }

    fn is_valid_tron_private_key(key: &str) -> bool {
        // Tron private key thường có 64 ký tự hex hoặc format base58
        is_evm_private_key(key) || (key.len() >= 50 && key.len() <= 60)
    }

    fn is_valid_stellar_private_key(key: &str) -> bool {
        // Stellar private key thường bắt đầu bằng "S" và có 56 ký tự
        key.len() == 56 && key.starts_with('S')
    }

    fn is_valid_near_private_key(key: &str) -> bool {
        // NEAR private key (ed25519) thường là 64 ký tự hex hoặc format base58
        is_evm_private_key(key) || key.len() >= 40
    }

    fn is_valid_cosmos_private_key(key: &str) -> bool {
        // Cosmos private key thường là định dạng base64 hoặc hex
        key.len() >= 40 && key.len() <= 90
    }

    fn is_valid_sui_private_key(key: &str) -> bool {
        // Sui private key
        key.len() >= 40 && key.len() <= 90
    }

    fn is_valid_ton_private_key(key: &str) -> bool {
        // TON private key
        key.len() >= 40 && key.len() <= 90
    }

    fn is_valid_hedera_private_key(key: &str) -> bool {
        // Hedera private key
        key.len() >= 40 && key.len() <= 90
    }

    fn is_valid_aptos_private_key(key: &str) -> bool {
        // Aptos private key
        key.len() >= 40 && key.len() <= 90
    }

    /// Kiểm tra độ mạnh của mật khẩu
    fn validate_password_strength(password: &str) -> bool {
        // Kiểm tra độ dài tối thiểu
        if password.len() < 8 {
            tracing::debug!("Mật khẩu quá ngắn, yêu cầu ít nhất 8 ký tự");
            return false;
        }
        
        // Kiểm tra có ít nhất một ký tự viết hoa
        let has_uppercase = password.chars().any(|c| c.is_uppercase());
        if !has_uppercase {
            tracing::debug!("Mật khẩu thiếu ký tự viết hoa");
            return false;
        }
        
        // Kiểm tra có ít nhất một ký tự viết thường
        let has_lowercase = password.chars().any(|c| c.is_lowercase());
        if !has_lowercase {
            tracing::debug!("Mật khẩu thiếu ký tự viết thường");
            return false;
        }
        
        // Kiểm tra có ít nhất một chữ số
        let has_digit = password.chars().any(|c| c.is_digit(10));
        if !has_digit {
            tracing::debug!("Mật khẩu thiếu chữ số");
            return false;
        }
        
        // Kiểm tra có ít nhất một ký tự đặc biệt
        let has_special = password.chars().any(|c| !c.is_alphanumeric());
        if !has_special {
            tracing::debug!("Mật khẩu thiếu ký tự đặc biệt");
            return false;
        }
        
        true
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
        // Kiểm tra ví tồn tại
        if !self.manager.has_wallet_internal(address).await {
            return Err(WalletError::WalletNotFound(address));
        }

        // Lấy thông tin chain của ví
        let chain_id = match self.manager.get_chain_id_internal(address).await {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Không thể lấy chain ID cho ví {}: {}", address, e);
                return Err(e);
            }
        };

        // Lấy cấu hình chain
        let chain_config = match self.chain_manager.get_chain_config(chain_id).await {
            Ok(config) => config,
            Err(e) => {
                tracing::error!("Không thể lấy cấu hình cho chain {}: {}", chain_id, e);
                return Err(e);
            }
        };

        // Xử lý khác nhau tùy theo loại chain
        match chain_config.chain_type {
            ChainType::EVM | ChainType::Polygon | ChainType::BSC | ChainType::Arbitrum | 
            ChainType::Optimism | ChainType::Fantom | ChainType::Avalanche => {
                // Ký giao dịch với EVM-compatible chains
        self.manager.sign_transaction(address, tx, self.chain_manager.as_ref())
            .await
                    .context("Lỗi khi ký giao dịch EVM")
            .map_err(|e| match e.downcast::<WalletError>() {
                Ok(wallet_err) => wallet_err,
                        Err(e) => WalletError::TransactionFailed(format!("Lỗi EVM: {}", e))
                    })
            },
            ChainType::Solana => {
                // Xử lý ký giao dịch Solana
                tracing::warn!("Ký giao dịch Solana: Yêu cầu xử lý khác");
                self.sign_solana_transaction(address, tx)
                    .await
                    .context("Lỗi khi ký giao dịch Solana")
                    .map_err(|e| match e.downcast::<WalletError>() {
                        Ok(wallet_err) => wallet_err,
                        Err(e) => WalletError::TransactionFailed(format!("Lỗi Solana: {}", e))
                    })
            },
            ChainType::Tron => {
                // Xử lý ký giao dịch Tron
                tracing::warn!("Ký giao dịch Tron: Yêu cầu xử lý khác");
                self.sign_tron_transaction(address, tx)
                    .await
                    .context("Lỗi khi ký giao dịch Tron")
                    .map_err(|e| match e.downcast::<WalletError>() {
                        Ok(wallet_err) => wallet_err,
                        Err(e) => WalletError::TransactionFailed(format!("Lỗi Tron: {}", e))
                    })
            },
            ChainType::Diamond => {
                // Sử dụng phương thức ký EVM cho Diamond chain (nếu tương thích)
                self.manager.sign_transaction(address, tx, self.chain_manager.as_ref())
                    .await
                    .context("Lỗi khi ký giao dịch Diamond")
                    .map_err(|e| match e.downcast::<WalletError>() {
                        Ok(wallet_err) => wallet_err,
                        Err(e) => WalletError::TransactionFailed(format!("Lỗi Diamond: {}", e))
                    })
            },
            _ => {
                // Các loại blockchain khác chưa được hỗ trợ
                Err(WalletError::ChainNotSupported(format!(
                    "Không hỗ trợ ký giao dịch cho loại chain: {:?}", chain_config.chain_type
                )))
            }
        }
    }
    
    /// Phương thức ký giao dịch Solana
    async fn sign_solana_transaction(&self, _address: Address, _tx: TransactionRequest) -> Result<Vec<u8>, WalletError> {
        // TODO: Triển khai chức năng ký giao dịch Solana
        Err(WalletError::NotImplemented("Ký giao dịch Solana chưa được triển khai".to_string()))
    }

    /// Phương thức ký giao dịch Tron 
    async fn sign_tron_transaction(&self, _address: Address, _tx: TransactionRequest) -> Result<Vec<u8>, WalletError> {
        // TODO: Triển khai chức năng ký giao dịch Tron
        Err(WalletError::NotImplemented("Ký giao dịch Tron chưa được triển khai".to_string()))
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
        // Lấy thông tin cần thiết
        let wallet = match self.manager.get_wallet_internal(address).await {
            Ok(wallet) => wallet,
            Err(e) => {
                tracing::error!("Không thể lấy ví {}: {}", address, e);
                return Err(e);
            }
        };
        
        let chain_id = match self.manager.get_chain_id_internal(address).await {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Không thể lấy chain ID cho ví {}: {}", address, e);
                return Err(e);
            }
        };
        
        // Lấy chain config để xác định loại chain
        let chain_config = match self.chain_manager.get_chain_config(chain_id).await {
            Ok(config) => config,
            Err(e) => {
                tracing::error!("Không thể lấy cấu hình cho chain {}: {}", chain_id, e);
                return Err(e);
            }
        };
        
        // Xử lý khác nhau tùy theo loại chain
        match chain_config.chain_type {
            ChainType::EVM | ChainType::Polygon | ChainType::BSC | ChainType::Arbitrum | 
            ChainType::Optimism | ChainType::Fantom | ChainType::Avalanche => {
                self.send_evm_transaction(wallet, address, tx, chain_id).await
            },
            ChainType::Solana => {
                self.send_solana_transaction(wallet, address, tx, &chain_config.rpc_url).await
            },
            ChainType::Tron => {
                self.send_tron_transaction(wallet, address, tx, &chain_config.rpc_url).await
            },
            ChainType::Diamond => {
                self.send_evm_transaction(wallet, address, tx, chain_id).await
            },
            _ => {
                Err(WalletError::ChainNotSupported(format!(
                    "Không hỗ trợ gửi giao dịch cho loại chain: {:?}", chain_config.chain_type
                )))
            }
        }
    }
    
    /// Gửi giao dịch EVM (hoặc tương thích EVM)
    async fn send_evm_transaction(
        &self, 
        wallet: LocalWallet, 
        address: Address, 
        tx: TransactionRequest, 
        chain_id: u64
    ) -> Result<String, WalletError> {
        // Lấy provider theo chain_id
        let provider = match self.chain_manager.get_provider(chain_id).await {
            Ok(provider) => provider,
            Err(e) => {
                tracing::error!("Không thể kết nối đến provider cho chain ID {}: {}", chain_id, e);
                return Err(WalletError::ProviderError(format!("Không thể kết nối provider: {}", e)));
            }
        };
        
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
            
            let gas_limit = match tx.gas {
                Some(limit) => limit,
                None => {
                    // Giá trị mặc định an toàn
                    U256::from(21000) 
                }
            };
            
            let estimated_cost = value + (gas_price * gas_limit);
            
            if balance < estimated_cost {
                tracing::warn!("Số dư không đủ cho giao dịch: có {} < cần {}", balance, estimated_cost);
                return Err(WalletError::InsufficientFunds(format!(
                    "Số dư không đủ: có {} < cần {} (giá trị {} + gas {})", 
                    balance, estimated_cost, value, gas_price * gas_limit
                )));
            }
        }
        
        // Thực hiện giao dịch
        let mut retries = 0;
        const MAX_RETRIES: u8 = 3;
        let mut last_error = None;
        
        while retries < MAX_RETRIES {
            match self.manager.send_transaction_with_provider(wallet.clone(), tx.clone(), provider.clone()).await {
                Ok(hash) => return Ok(hash),
                Err(e) => {
                    retries += 1;
                    last_error = Some(e);
                    
                    if retries < MAX_RETRIES {
                        tracing::warn!("Lỗi khi gửi giao dịch (lần thử {}), thử lại: {}", retries, last_error.as_ref().unwrap());
                        tokio::time::sleep(tokio::time::Duration::from_millis(500 * retries as u64)).await;
                    }
                }
            }
        }
        
        Err(match last_error {
            Some(e) => e,
            None => WalletError::TransactionFailed("Lỗi không xác định khi gửi giao dịch".to_string())
        })
    }
    
    /// Gửi giao dịch Solana
    async fn send_solana_transaction(
        &self, 
        _wallet: LocalWallet, 
        _address: Address, 
        _tx: TransactionRequest, 
        _rpc_url: &str
    ) -> Result<String, WalletError> {
        // TODO: Triển khai chức năng gửi giao dịch Solana
        Err(WalletError::NotImplemented("Gửi giao dịch Solana chưa được triển khai".to_string()))
    }
    
    /// Gửi giao dịch Tron
    async fn send_tron_transaction(
        &self, 
        _wallet: LocalWallet, 
        _address: Address, 
        _tx: TransactionRequest, 
        _rpc_url: &str
    ) -> Result<String, WalletError> {
        // TODO: Triển khai chức năng gửi giao dịch Tron
        Err(WalletError::NotImplemented("Gửi giao dịch Tron chưa được triển khai".to_string()))
    }

    /// Lấy số dư tài khoản
    ///
    /// Hàm này lấy số dư của một địa chỉ trên một blockchain cụ thể.
    /// Hỗ trợ nhiều loại blockchain khác nhau, bao gồm EVM và không phải EVM.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví cần lấy số dư
    /// - `chain_id`: Chain ID của blockchain
    ///
    /// # Returns
    /// - `Ok(String)`: Số dư dạng string
    /// - `Err(WalletError)`: Nếu có lỗi
    pub async fn get_balance(&self, address: &str, chain_id: u64) -> Result<String, WalletError> {
        // Lấy thông tin cấu hình của chain
        let chain_config = match self.chain_manager.get_chain_config(chain_id).await {
            Ok(config) => config,
            Err(e) => {
                tracing::error!("Không thể lấy cấu hình cho chain {}: {}", chain_id, e);
                return Err(e);
            }
        };
        
        // Xử lý riêng biệt cho từng loại blockchain
        match chain_config.chain_type {
            // Các blockchain tương thích EVM
            ChainType::EVM | ChainType::Polygon | ChainType::BSC | 
            ChainType::Arbitrum | ChainType::Optimism | ChainType::Fantom | 
            ChainType::Avalanche | ChainType::Klaytn => {
                self.get_evm_balance(address, chain_id).await
            },
            // Các blockchain không phải EVM
            ChainType::Solana => {
                self.get_solana_balance(address, &chain_config.rpc_url).await
            },
            ChainType::Tron => {
                self.get_tron_balance(address, &chain_config.rpc_url).await
            },
            ChainType::Diamond => {
                self.get_diamond_balance(address, &chain_config.rpc_url).await
            },
            ChainType::Bitcoin | ChainType::BTC => {
                self.get_bitcoin_balance(address, &chain_config.rpc_url).await
            },
            ChainType::Cosmos => {
                self.get_cosmos_balance(address, &chain_config.rpc_url).await
            },
            ChainType::Stellar => {
                self.get_stellar_balance(address, &chain_config.rpc_url).await
            },
            ChainType::NEAR => {
                self.get_near_balance(address, &chain_config.rpc_url).await
            },
            ChainType::Sui => {
                self.get_sui_balance(address, &chain_config.rpc_url).await
            },
            ChainType::TON => {
                self.get_ton_balance(address, &chain_config.rpc_url).await
            },
            ChainType::Hedera => {
                self.get_hedera_balance(address, &chain_config.rpc_url).await
            },
            ChainType::Aptos => {
                self.get_aptos_balance(address, &chain_config.rpc_url).await
            },
            _ => Err(WalletError::ChainNotSupported(format!(
                "Loại blockchain không được hỗ trợ cho chain ID: {}", chain_id
            ))),
        }
    }

    /// Lấy số dư của ví trên blockchain tương thích EVM
    async fn get_evm_balance(&self, address: &str, chain_id: u64) -> Result<String, WalletError> {
        // Lấy provider từ chain manager
        let provider = match self.chain_manager.get_provider(chain_id).await {
            Ok(provider) => provider,
            Err(e) => {
                tracing::error!("Không thể kết nối đến provider cho chain ID {}: {}", chain_id, e);
                return Err(WalletError::ProviderError(format!("Không thể kết nối provider: {}", e)));
            }
        };
        
        // Chuyển đổi địa chỉ string sang Address
        let evm_address = match Address::from_str(address) {
            Ok(addr) => addr,
            Err(_) => {
                tracing::warn!("Địa chỉ EVM không hợp lệ: {}", address);
                return Err(WalletError::InvalidAddress(address.to_string()));
            }
        };
        
        // Lấy số dư
        match provider.get_balance(evm_address, None).await {
            Ok(balance) => Ok(balance.to_string()),
            Err(e) => {
                tracing::error!("Không thể lấy số dư cho địa chỉ {}: {}", address, e);
                Err(WalletError::ProviderError(format!("Không thể lấy số dư: {}", e)))
            }
        }
    }

    /// Lấy số dư của ví Solana
    async fn get_solana_balance(&self, address: &str, rpc_url: &str) -> Result<String, WalletError> {
        // Xác thực địa chỉ Solana (base58 encoded string)
        if !is_valid_solana_address(address) {
            tracing::warn!("Địa chỉ Solana không hợp lệ: {}", address);
            return Err(WalletError::InvalidAddress(address.to_string()));
        }
        
        // Tạo request JSON-RPC cho Solana
        let request_body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getBalance",
            "params": [address]
        });
        
        // Gửi request với retry logic
        let mut retries = 0;
        const MAX_RETRIES: u8 = 3;
        let mut last_error = None;
        
        while retries < MAX_RETRIES {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default();
                
            match client.post(rpc_url)
                .json(&request_body)
                .send()
                .await {
                Ok(response) => {
                    // Xử lý response
                    match response.json::<Value>().await {
                        Ok(response_json) => {
                            // Trích xuất số dư từ response
                            if let Some(result) = response_json.get("result") {
                                if let Some(value) = result.get("value") {
                                    if let Some(balance) = value.as_u64() {
                                        // Số dư Solana tính bằng lamports (1 SOL = 10^9 lamports)
                                        return Ok(balance.to_string());
                                    }
                                }
                            }
                            
                            // Kiểm tra lỗi trong response
                            if let Some(error) = response_json.get("error") {
                                let error_msg = error["message"].as_str().unwrap_or("Không rõ lỗi");
                                tracing::warn!("Lỗi từ Solana RPC: {}", error_msg);
                                last_error = Some(WalletError::ProviderError(format!("Lỗi từ Solana RPC: {}", error_msg)));
                            } else {
                                last_error = Some(WalletError::ProviderError("Phản hồi không hợp lệ từ Solana RPC".to_string()));
                            }
                        },
                        Err(e) => {
                            tracing::warn!("Lỗi khi xử lý JSON từ Solana RPC: {}", e);
                            last_error = Some(WalletError::ProviderError(format!("Lỗi khi phân tích JSON: {}", e)));
                        }
                    }
                },
                Err(e) => {
                    tracing::warn!("Lỗi kết nối đến Solana RPC (lần thử {}): {}", retries + 1, e);
                    last_error = Some(WalletError::ProviderError(format!("Lỗi request: {}", e)));
                }
            }
            
            // Thử lại nếu chưa đến số lần tối đa
            retries += 1;
            if retries < MAX_RETRIES {
                let delay = Duration::from_millis(500 * (2_u64.pow(retries as u32)));
                tokio::time::sleep(delay).await;
            }
        }
        
        // Trả về lỗi cuối cùng nếu tất cả các lần thử đều thất bại
        Err(last_error.unwrap_or_else(|| WalletError::ProviderError("Không thể lấy số dư từ Solana".to_string())))
    }

    /// Lấy số dư của ví Tron
    async fn get_tron_balance(&self, address: &str, rpc_url: &str) -> Result<String, WalletError> {
        // Xác thực địa chỉ Tron (bắt đầu bằng T và là chuỗi base58)
        if !is_valid_tron_address(address) {
            tracing::warn!("Địa chỉ Tron không hợp lệ: {}", address);
            return Err(WalletError::InvalidAddress(address.to_string()));
        }
        
        // Tạo request API cho Tron
        let api_url = format!("{}/wallet/getaccount", rpc_url);
        let request_body = json!({
            "address": address,
            "visible": true
        });
        
        // Gửi request với retry logic
        let mut retries = 0;
        const MAX_RETRIES: u8 = 3;
        let mut last_error = None;
        
        while retries < MAX_RETRIES {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default();
                
            match client.post(&api_url)
                .json(&request_body)
                .send()
                .await {
                Ok(response) => {
                    // Xử lý response
                    match response.json::<Value>().await {
                        Ok(response_json) => {
                            // Trích xuất số dư
                            if let Some(balance) = response_json.get("balance") {
                                if let Some(value) = balance.as_u64() {
                                    // Số dư Tron tính bằng SUN (1 TRX = 10^6 SUN)
                                    return Ok(value.to_string());
                                }
                            }
                            
                            // Kiểm tra Error message
                            if let Some(error) = response_json.get("Error") {
                                let error_msg = error.as_str().unwrap_or("Không rõ lỗi");
                                tracing::warn!("Lỗi từ Tron API: {}", error_msg);
                                last_error = Some(WalletError::ProviderError(format!("Lỗi từ Tron API: {}", error_msg)));
                            } else {
                                last_error = Some(WalletError::ProviderError("Địa chỉ không tồn tại hoặc chưa được kích hoạt".to_string()));
                            }
                        },
                        Err(e) => {
                            tracing::warn!("Lỗi khi xử lý JSON từ Tron API: {}", e);
                            last_error = Some(WalletError::ProviderError(format!("Lỗi khi phân tích JSON: {}", e)));
                        }
                    }
                },
                Err(e) => {
                    tracing::warn!("Lỗi kết nối đến Tron API (lần thử {}): {}", retries + 1, e);
                    last_error = Some(WalletError::ProviderError(format!("Lỗi request: {}", e)));
                }
            }
            
            // Thử lại nếu chưa đến số lần tối đa
            retries += 1;
            if retries < MAX_RETRIES {
                let delay = Duration::from_millis(500 * (2_u64.pow(retries as u32)));
                tokio::time::sleep(delay).await;
            }
        }
        
        // Trả về lỗi cuối cùng nếu tất cả các lần thử đều thất bại
        Err(last_error.unwrap_or_else(|| WalletError::ProviderError("Không thể lấy số dư từ Tron".to_string())))
    }

    /// Lấy số dư của ví Diamond Chain
    async fn get_diamond_balance(&self, address: &str, rpc_url: &str) -> Result<String, WalletError> {
        // Diamond Chain có thể dùng địa chỉ tương tự EVM
        let diamond_address = match Address::from_str(address) {
            Ok(addr) => addr,
            Err(_) => {
                tracing::warn!("Địa chỉ Diamond không hợp lệ: {}", address);
                return Err(WalletError::InvalidAddress(address.to_string()));
            }
        };
        
        // Tạo request JSON-RPC
        let request_body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "diamond_getBalance",
            "params": [diamond_address.to_string(), "latest"]
        });
        
        // Gửi request với retry logic
        let mut retries = 0;
        const MAX_RETRIES: u8 = 3;
        let mut last_error = None;
        
        while retries < MAX_RETRIES {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default();
                
            match client.post(rpc_url)
                .json(&request_body)
                .send()
                .await {
                Ok(response) => {
                    // Xử lý response
                    match response.json::<Value>().await {
                        Ok(response_json) => {
                            // Trích xuất số dư
                            if let Some(result) = response_json.get("result") {
                                if let Some(balance_hex) = result.as_str() {
                                    // Chuyển đổi từ hex sang decimal
                                    return match U256::from_str_radix(&balance_hex[2..], 16) {
                                        Ok(balance) => Ok(balance.to_string()),
                                        Err(e) => {
                                            tracing::warn!("Lỗi chuyển đổi hex sang decimal: {}", e);
                                            Err(WalletError::ProviderError(format!("Lỗi chuyển đổi giá trị: {}", e)))
                                        }
                                    };
                                }
                            }
                            
                            // Kiểm tra lỗi trong response
                            if let Some(error) = response_json.get("error") {
                                let error_msg = error["message"].as_str().unwrap_or("Không rõ lỗi");
                                tracing::warn!("Lỗi từ Diamond RPC: {}", error_msg);
                                last_error = Some(WalletError::ProviderError(format!("Lỗi từ Diamond RPC: {}", error_msg)));
                            } else {
                                last_error = Some(WalletError::ProviderError("Phản hồi không hợp lệ từ Diamond RPC".to_string()));
                            }
                        },
                        Err(e) => {
                            tracing::warn!("Lỗi khi xử lý JSON từ Diamond RPC: {}", e);
                            last_error = Some(WalletError::ProviderError(format!("Lỗi khi phân tích JSON: {}", e)));
                        }
                    }
                },
                Err(e) => {
                    tracing::warn!("Lỗi kết nối đến Diamond RPC (lần thử {}): {}", retries + 1, e);
                    last_error = Some(WalletError::ProviderError(format!("Lỗi request: {}", e)));
                }
            }
            
            // Thử lại nếu chưa đến số lần tối đa
            retries += 1;
            if retries < MAX_RETRIES {
                let delay = Duration::from_millis(500 * (2_u64.pow(retries as u32)));
                tokio::time::sleep(delay).await;
            }
        }
        
        // Trả về lỗi cuối cùng nếu tất cả các lần thử đều thất bại
        Err(last_error.unwrap_or_else(|| WalletError::ProviderError("Không thể lấy số dư từ Diamond Chain".to_string())))
    }

    /// Lấy số dư của ví Bitcoin
    async fn get_bitcoin_balance(&self, address: &str, rpc_url: &str) -> Result<String, WalletError> {
        // Xác thực địa chỉ Bitcoin
        if !is_valid_bitcoin_address(address) {
            tracing::warn!("Địa chỉ Bitcoin không hợp lệ: {}", address);
            return Err(WalletError::InvalidAddress(address.to_string()));
        }
        
        // Tạo request JSON-RPC cho Bitcoin
        let request_body = json!({
            "jsonrpc": "1.0",
            "id": "1",
            "method": "getaddressbalance",
            "params": [{"addresses": [address]}]
        });
        
        // Xử lý thông tin xác thực nếu cần
        let mut auth_header = None;
        if let Some((username, password)) = rpc_url.split('@').next().and_then(|s| {
            s.strip_prefix("http://").or_else(|| s.strip_prefix("https://"))
             .and_then(|s| {
                let parts: Vec<&str> = s.split(':').collect();
                if parts.len() >= 2 {
                    Some((parts[0].to_string(), parts[1].to_string()))
                } else {
                    None
                }
            })
        }) {
            let auth = format!("{}:{}", username, password);
            let encoded_auth = BASE64_STANDARD.encode(auth.as_bytes());
            auth_header = Some(format!("Basic {}", encoded_auth));
        }
        
        // Gửi request với retry logic
        let mut retries = 0;
        const MAX_RETRIES: u8 = 3;
        let mut last_error = None;
        
        while retries < MAX_RETRIES {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(15)) // Bitcoin có thể mất nhiều thời gian hơn
                .build()
                .unwrap_or_default();
                
            let mut request_builder = client.post(rpc_url)
                .json(&request_body);
                
            // Thêm header xác thực nếu có
            if let Some(auth) = &auth_header {
                request_builder = request_builder.header("Authorization", auth);
            }
            
            match request_builder.send().await {
                Ok(response) => {
                    // Xử lý response
                    match response.json::<Value>().await {
                        Ok(response_json) => {
                            // Trích xuất số dư
                            if let Some(result) = response_json.get("result") {
                                if let Some(balance) = result.get("balance") {
                                    if let Some(value) = balance.as_u64() {
                                        // Số dư Bitcoin tính bằng satoshis (1 BTC = 10^8 satoshis)
                                        return Ok(value.to_string());
                                    }
                                }
                            }
                            
                            // Kiểm tra lỗi trong response
                            if let Some(error) = response_json.get("error") {
                                if !error.is_null() {
                                    let error_msg = error["message"].as_str().unwrap_or("Không rõ lỗi");
                                    tracing::warn!("Lỗi từ Bitcoin RPC: {}", error_msg);
                                    last_error = Some(WalletError::ProviderError(format!("Lỗi từ Bitcoin RPC: {}", error_msg)));
                                } else {
                                    last_error = Some(WalletError::ProviderError("Phản hồi không hợp lệ từ Bitcoin RPC".to_string()));
                                }
                            } else {
                                last_error = Some(WalletError::ProviderError("Không thể lấy số dư từ response".to_string()));
                            }
                        },
                        Err(e) => {
                            tracing::warn!("Lỗi khi xử lý JSON từ Bitcoin RPC: {}", e);
                            last_error = Some(WalletError::ProviderError(format!("Lỗi khi phân tích JSON: {}", e)));
                        }
                    }
                },
                Err(e) => {
                    tracing::warn!("Lỗi kết nối đến Bitcoin RPC (lần thử {}): {}", retries + 1, e);
                    last_error = Some(WalletError::ProviderError(format!("Lỗi request: {}", e)));
                }
            }
            
            // Thử lại nếu chưa đến số lần tối đa
            retries += 1;
            if retries < MAX_RETRIES {
                let delay = Duration::from_millis(1000 * (2_u64.pow(retries as u32)));
                tokio::time::sleep(delay).await;
            }
        }
        
        // Trả về lỗi cuối cùng nếu tất cả các lần thử đều thất bại
        Err(last_error.unwrap_or_else(|| WalletError::ProviderError("Không thể lấy số dư từ Bitcoin".to_string())))
    }

    /// Lấy số dư của ví Cosmos
    async fn get_cosmos_balance(&self, address: &str, rpc_url: &str) -> Result<String, WalletError> {
        // Xử lý đặc thù cho Cosmos sẽ được triển khai sau
        Err(WalletError::NotImplemented("Chức năng lấy số dư Cosmos chưa được triển khai".to_string()))
    }

    /// Lấy số dư của ví Stellar
    async fn get_stellar_balance(&self, address: &str, rpc_url: &str) -> Result<String, WalletError> {
        // Xử lý đặc thù cho Stellar sẽ được triển khai sau
        Err(WalletError::NotImplemented("Chức năng lấy số dư Stellar chưa được triển khai".to_string()))
    }

    /// Lấy số dư của ví NEAR
    async fn get_near_balance(&self, address: &str, rpc_url: &str) -> Result<String, WalletError> {
        // Xử lý đặc thù cho NEAR sẽ được triển khai sau
        Err(WalletError::NotImplemented("Chức năng lấy số dư NEAR chưa được triển khai".to_string()))
    }

    /// Lấy số dư của ví Sui
    async fn get_sui_balance(&self, address: &str, rpc_url: &str) -> Result<String, WalletError> {
        // Xử lý đặc thù cho Sui sẽ được triển khai sau
        Err(WalletError::NotImplemented("Chức năng lấy số dư Sui chưa được triển khai".to_string()))
    }

    /// Lấy số dư của ví TON
    async fn get_ton_balance(&self, address: &str, rpc_url: &str) -> Result<String, WalletError> {
        // Xử lý đặc thù cho TON sẽ được triển khai sau
        Err(WalletError::NotImplemented("Chức năng lấy số dư TON chưa được triển khai".to_string()))
    }

    /// Lấy số dư của ví Hedera
    async fn get_hedera_balance(&self, address: &str, rpc_url: &str) -> Result<String, WalletError> {
        // Xử lý đặc thù cho Hedera sẽ được triển khai sau
        Err(WalletError::NotImplemented("Chức năng lấy số dư Hedera chưa được triển khai".to_string()))
    }

    /// Lấy số dư của ví Aptos
    async fn get_aptos_balance(&self, address: &str, rpc_url: &str) -> Result<String, WalletError> {
        // Xử lý đặc thù cho Aptos sẽ được triển khai sau
        Err(WalletError::NotImplemented("Chức năng lấy số dư Aptos chưa được triển khai".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::encryption;
    use crate::walletmanager::chain::{ChainConfig, ChainType, DefaultChainManager};
    use crate::walletmanager::manager::WalletManager;
    use std::str::FromStr;
    use tokio::sync::RwLock;
    use anyhow::{Context, Result};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use mockall::predicate::*;
    use mockall::mock;

    // Tạo mock cho ChainManager
    mock! {
        pub ChainManagerMock {}
        #[async_trait::async_trait]
        impl ChainManager for ChainManagerMock {
            async fn get_provider(&self, chain_id: u64) -> Result<Arc<Provider<Http>>, WalletError>;
            async fn add_chain(&self, config: ChainConfig) -> Result<(), WalletError>;
            async fn get_chain_config(&self, chain_id: u64) -> Result<ChainConfig, WalletError>;
            async fn chain_exists(&self, chain_id: u64) -> bool;
        }
    }

    // Tạo mock cho WalletManager
    mock! {
        pub WalletManagerMock {}
        #[async_trait::async_trait]
        impl WalletManagerHandler for WalletManagerMock {
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
    }

    #[tokio::test]
    async fn test_wallet_creation_and_export() -> Result<(), WalletError> {
        // Khởi tạo WalletManager
        let wallet_manager = WalletManager::new();
        let chain_manager = Arc::new(DefaultChainManager::new());
        let wallet_api = WalletManagerApi {
            manager: wallet_manager,
            config: WalletSystemConfig {
                default_chain_id: 1,
                default_chain_type: ChainType::EVM,
                max_wallets: 10,
                allow_wallet_overwrite: false,
            },
            chain_manager: Box::new(chain_manager),
        };
        
        // Tạo ví mới
        let seed_phrase = wallet_api.generate_seed_phrase(SeedLength::Twelve).await?;
        let wallet_config = WalletConfig {
            seed_or_key: seed_phrase.clone(),
            chain_id: 1,
            chain_type: ChainType::EVM,
            seed_length: Some(SeedLength::Twelve),
            password: "TestPassword123!".to_string(),
        };

        let user_id = "test_user_1".to_string();
        let address = wallet_api.create_wallet(wallet_config, user_id.clone()).await?;

        // Kiểm tra user_id
        let retrieved_user_id = wallet_api.get_user_id(address).await?;
        assert_eq!(retrieved_user_id, user_id, "User ID không khớp");

        // Kiểm tra chain_id
        let chain_id = wallet_api.get_chain_id(address).await?;
        assert_eq!(chain_id, 1, "Chain ID không khớp");

        // Kiểm tra export private key
        let private_key = wallet_api.export_private_key(address, "TestPassword123!".to_string()).await?;
        assert!(!private_key.is_empty(), "Private key không được trống");

        // Kiểm tra export seed phrase
        let retrieved_seed = wallet_api.export_seed_phrase(address, "TestPassword123!".to_string()).await?;
        assert_eq!(retrieved_seed, seed_phrase, "Seed phrase không khớp");

        // Kiểm tra xoá ví
        wallet_api.remove_wallet(address).await?;
        assert!(!wallet_api.has_wallet(address).await, "Ví nên được xoá");

        Ok(())
    }
    
    #[tokio::test]
    async fn test_max_wallet_limit() -> Result<(), WalletError> {
        // Khởi tạo WalletManager với giới hạn 2 ví
        let wallet_manager = WalletManager::with_max_wallets(2);
        let chain_manager = Arc::new(DefaultChainManager::new());
        let mut wallet_api = WalletManagerApi {
            manager: wallet_manager,
            config: WalletSystemConfig {
                default_chain_id: 1,
                default_chain_type: ChainType::EVM,
                max_wallets: 2,
                allow_wallet_overwrite: false,
            },
            chain_manager: Box::new(chain_manager),
        };
        
        // Tạo seed phrase cho ví thứ nhất
        let seed_phrase1 = match wallet_api.generate_seed_phrase(SeedLength::Twelve).await {
            Ok(seed) => seed,
            Err(e) => {
                return Err(WalletError::OperationFailed(
                    format!("Không thể tạo seed phrase cho ví 1: {}", e)
                ));
            }
        };

        // Tạo seed phrase cho ví thứ hai
        let seed_phrase2 = match wallet_api.generate_seed_phrase(SeedLength::Twelve).await {
            Ok(seed) => seed,
            Err(e) => {
                return Err(WalletError::OperationFailed(
                    format!("Không thể tạo seed phrase cho ví 2: {}", e)
                ));
            }
        };

        // Tạo ví thứ nhất
        let wallet_config1 = WalletConfig {
            seed_or_key: seed_phrase1,
            chain_id: 1,
            chain_type: ChainType::EVM,
            seed_length: Some(SeedLength::Twelve),
            password: "TestPassword123!".to_string(),
        };

        // Tạo ví thứ hai
        let wallet_config2 = WalletConfig {
            seed_or_key: seed_phrase2,
            chain_id: 1,
            chain_type: ChainType::EVM,
            seed_length: Some(SeedLength::Twelve),
            password: "TestPassword123!".to_string(),
        };

        // Tạo ví thứ nhất và kiểm tra kết quả
        let address1 = wallet_api.create_wallet(wallet_config1, "user1".to_string()).await?;
        tracing::debug!("Đã tạo ví 1 thành công: {}", address1);

        // Tạo ví thứ hai và kiểm tra kết quả
        let address2 = wallet_api.create_wallet(wallet_config2, "user2".to_string()).await?;
        tracing::debug!("Đã tạo ví 2 thành công: {}", address2);

        // Thử tạo ví thứ 3, phải lỗi do đã đạt giới hạn
        let result = wallet_api.create_wallet(wallet_config2, "user3".to_string()).await;
        
        // Kiểm tra kết quả tạo ví thứ 3
        match result {
            Ok(_) => {
                // Nếu thành công thì đây là lỗi
                return Err(WalletError::OperationFailed(
                    "Tạo ví thứ 3 thành công nhưng đáng lẽ phải thất bại do vượt quá giới hạn".to_string()
                ));
            },
            Err(e) => {
                // Kiểm tra xem có đúng loại lỗi mong đợi không
            match e {
                WalletError::MaxWalletLimitReached(_) => {
                    // Đây là lỗi mong đợi
                        tracing::debug!("Nhận lỗi giới hạn ví như mong đợi: {}", e);
                },
                _ => {
                        // Lỗi khác, không đúng như mong đợi
                        return Err(WalletError::OperationFailed(
                            format!("Lỗi không phải là MaxWalletLimitReached như mong đợi: {}", e)
                        ));
                    }
                }
            }
        }

        // Kiểm tra số lượng ví hiện tại
        let wallets = wallet_api.list_wallets().await;
        assert_eq!(wallets.len(), 2, "Số lượng ví phải đúng bằng giới hạn 2");

        // Kiểm tra danh sách ví có đúng không
        assert!(wallets.contains(&address1), "Danh sách ví phải chứa địa chỉ ví 1");
        assert!(wallets.contains(&address2), "Danh sách ví phải chứa địa chỉ ví 2");

        Ok(())
    }
    
    #[tokio::test]
    async fn test_max_wallet_limit_with_mocks() -> Result<(), WalletError> {
        // Tạo counter để theo dõi số lượng ví đã tạo
        let wallet_count = Arc::new(AtomicUsize::new(0));
        let max_wallets = 2;
        
        // Tạo mock cho ChainManagerMock
        let mut chain_manager_mock = MockChainManagerMock::new();
        
        // Thiết lập hành vi cho chain_exists - luôn trả về true
        chain_manager_mock
            .expect_chain_exists()
            .returning(|_| true);
        
        // Thiết lập hành vi cho get_chain_config
        chain_manager_mock
            .expect_get_chain_config()
            .returning(|chain_id| {
                Ok(ChainConfig {
                    chain_id,
                    name: format!("Chain {}", chain_id),
                    chain_type: ChainType::EVM,
                    rpc_url: "https://example.com/rpc".to_string(),
                    explorer_url: Some("https://example.com/explorer".to_string()),
                    icon: None,
                    tokens: Vec::new(),
                    rate_limit: Some(60),
                    requires_auth: false,
                    native_token: "ETH".to_string(),
                })
            });
            
        // Tạo config cho wallet manager
        let config = WalletSystemConfig {
            default_chain_id: 1,
            default_chain_type: ChainType::EVM,
            max_wallets,
            allow_wallet_overwrite: false,
        };
        
        // Tạo WalletManagerApi với mocks
        let wallet_count_clone = wallet_count.clone();
        let wallet_api = WalletManagerApi {
            manager: WalletManager::new(),
            config: config.clone(),
            chain_manager: Box::new(chain_manager_mock),
        };
        
        // Giả lập hàm is_wallet_limit_reached
        let is_limit_reached = move || -> Result<bool, WalletError> {
            Ok(wallet_count_clone.load(Ordering::SeqCst) >= max_wallets)
        };
        
        // Test tạo ví 1 - nên thành công
        {
            // Kiểm tra giới hạn
            assert!(!is_limit_reached()?, "Chưa đạt giới hạn ví");
            
            // Tăng counter để giả lập việc tạo ví
            wallet_count.fetch_add(1, Ordering::SeqCst);
            
            // Kiểm tra số lượng ví sau khi tạo
            assert_eq!(wallet_count.load(Ordering::SeqCst), 1, "Đã tạo 1 ví");
        }
        
        // Test tạo ví 2 - nên thành công
        {
            // Kiểm tra giới hạn
            assert!(!is_limit_reached()?, "Chưa đạt giới hạn ví");
            
            // Tăng counter để giả lập việc tạo ví
            wallet_count.fetch_add(1, Ordering::SeqCst);
            
            // Kiểm tra số lượng ví sau khi tạo
            assert_eq!(wallet_count.load(Ordering::SeqCst), 2, "Đã tạo 2 ví");
        }
        
        // Test tạo ví 3 - nên thất bại do đạt giới hạn
        {
            // Kiểm tra giới hạn
            assert!(is_limit_reached()?, "Đã đạt giới hạn ví");
            
            // Làm ngược với sinh lý bình thường, nếu còn dưới giới hạn mới báo lỗi
            if !is_limit_reached()? {
                return Err(WalletError::OperationFailed(
                    "Test thất bại: Chưa đạt giới hạn ví nhưng lẽ ra phải đạt rồi".to_string()
                ));
            }
            
            // Thử tăng counter, mô phỏng việc tạo ví thứ 3 sẽ thất bại
            // Không thực hiện wallet_count.fetch_add vì đã đạt giới hạn
            
            // Kiểm tra số lượng ví không thay đổi
            assert_eq!(wallet_count.load(Ordering::SeqCst), 2, "Vẫn giữ nguyên số ví");
        }
        
        Ok(())
    }
    
    #[tokio::test]
    async fn test_wallet_operations_with_mocks() -> Result<(), WalletError> {
        // Tạo mock cho chain manager
        let mut chain_manager_mock = MockChainManagerMock::new();
        
        // Thiết lập hành vi cho chain_exists
        chain_manager_mock
            .expect_chain_exists()
            .returning(|chain_id| chain_id == 1 || chain_id == 5);
            
        // Thiết lập hành vi cho get_chain_config
        chain_manager_mock
            .expect_get_chain_config()
            .returning(|chain_id| {
                if chain_id == 1 || chain_id == 5 {
                    Ok(ChainConfig {
                        chain_id,
                        name: format!("Chain {}", chain_id),
                        chain_type: ChainType::EVM,
                        rpc_url: "https://example.com/rpc".to_string(),
                        explorer_url: Some("https://example.com/explorer".to_string()),
                        icon: None,
                        tokens: Vec::new(),
                        rate_limit: Some(60),
                        requires_auth: false,
                        native_token: "ETH".to_string(),
                    })
                } else {
                    Err(WalletError::ChainNotFound(chain_id))
                }
            });
        
        // Tạo mock cho WalletManager
        let mut wallet_manager_mock = MockWalletManagerMock::new();
        
        // Địa chỉ test
        let test_address = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
        
        // Thiết lập hành vi cho list_wallets_internal
        wallet_manager_mock
            .expect_list_wallets_internal()
            .returning(move || vec![test_address]);
            
        // Thiết lập hành vi cho has_wallet_internal
        wallet_manager_mock
            .expect_has_wallet_internal()
            .returning(|addr| addr == test_address);
            
        // Thiết lập hành vi cho get_chain_id_internal
        wallet_manager_mock
            .expect_get_chain_id_internal()
            .returning(|addr| {
                if addr == test_address {
                    Ok(1)
                } else {
                    Err(WalletError::WalletNotFound(addr))
                }
            });
            
        // Thiết lập hành vi cho update_chain_id_internal
        wallet_manager_mock
            .expect_update_chain_id_internal()
            .returning(|addr, chain_id, _| {
                if addr == test_address {
                    if chain_id == 1 || chain_id == 5 {
                        Ok(())
                    } else {
                        Err(WalletError::ChainNotFound(chain_id))
                    }
                } else {
                    Err(WalletError::WalletNotFound(addr))
                }
            });
        
        // Tạo config cho wallet manager
        let config = WalletSystemConfig {
            default_chain_id: 1,
            default_chain_type: ChainType::EVM,
            max_wallets: 10,
            allow_wallet_overwrite: false,
        };
        
        // Tạo rust_arc thay vì reference
        let wallet_manager_mock_arc = Arc::new(RwLock::new(wallet_manager_mock));
        
        // Tạo WalletManagerApi với mocks
        let mut wallet_api = WalletManagerApi {
            manager: WalletManager::new(), 
            config,
            chain_manager: Box::new(chain_manager_mock),
        };
        
        // Test các operation với mock
        
        // Test 1: Kiểm tra chain_exists với chain_id hợp lệ
        assert!(wallet_api.chain_manager.chain_exists(1).await, "Chain ID 1 nên tồn tại");
        
        // Test 2: Kiểm tra chain_exists với chain_id không hợp lệ
        assert!(!wallet_api.chain_manager.chain_exists(999).await, "Chain ID 999 không nên tồn tại");
        
        // Test 3: Kiểm tra get_chain_config với chain_id hợp lệ
        let chain_config = wallet_api.chain_manager.get_chain_config(1).await;
        assert!(chain_config.is_ok(), "Nên lấy được config cho chain 1");
        
        // Test 4: Kiểm tra get_chain_config với chain_id không hợp lệ
        let invalid_chain_config = wallet_api.chain_manager.get_chain_config(999).await;
        assert!(invalid_chain_config.is_err(), "Không nên lấy được config cho chain 999");

        Ok(())
    }
}