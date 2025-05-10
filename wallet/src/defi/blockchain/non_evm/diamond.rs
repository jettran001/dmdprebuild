//! Module cho Diamond blockchain provider
//!
//! Module này cung cấp các chức năng để tương tác với mạng Diamond,
//! bao gồm các chức năng như truy vấn số dư, gửi giao dịch, và quản lý tài khoản.
//!
//! ## Ví dụ:
//! ```rust
//! use wallet::defi::blockchain::non_evm::diamond::{DiamondProvider, DiamondConfig};
//! use wallet::defi::blockchain::ChainId;
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = DiamondConfig::new(ChainId::DiamondMainnet);
//!     let provider = DiamondProvider::new(config).unwrap();
//!
//!     let balance = provider.get_balance("DdkNKDGXR7FkH3mZAwxMrhxJAnoEzQ9qWL").await.unwrap();
//!     println!("Balance: {}", balance);
//! }
//! ```

use std::sync::Arc;
use std::time::Duration;
use std::collections::HashMap;
use once_cell::sync::Lazy;
use tokio::sync::RwLock;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use ethers::types::{U256, H256};
use tracing::{debug, info, warn, error};
use reqwest::{Client, ClientBuilder};
use serde_json::{json, Value};
use regex::Regex;
use bs58;
use sha2::{Sha256};
use std::time::Instant;

use crate::defi::blockchain::{
    BlockchainProvider, BlockchainType, BlockchainConfig
};
use crate::defi::chain::ChainId;
use crate::defi::error::DefiError;
use crate::defi::constants::BLOCKCHAIN_TIMEOUT;

/// Cấu hình cho Diamond Provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiamondConfig {
    /// Chain ID
    pub chain_id: ChainId,
    /// RPC URL
    pub rpc_url: String,
    /// Explorer URL
    pub explorer_url: String,
    /// Timeout (ms)
    pub timeout_ms: u64,
    /// Số lần retry tối đa
    pub max_retries: u32,
    /// Thời gian chờ giữa các lần retry (ms)
    pub retry_delay_ms: u64,
}

impl DiamondConfig {
    /// Tạo cấu hình mới với các giá trị mặc định
    pub fn new(chain_id: ChainId) -> Self {
        let (rpc_url, explorer_url) = match chain_id {
            ChainId::DiamondMainnet => (
                "https://mainnet-rpc.diamondprotocol.co",
                "https://explorer.diamondprotocol.co"
            ),
            ChainId::DiamondTestnet => (
                "https://testnet-rpc.diamondprotocol.co",
                "https://testnet-explorer.diamondprotocol.co"
            ),
            _ => panic!("Invalid Diamond chain ID")
        };

        Self {
            chain_id,
            rpc_url: rpc_url.to_string(),
            explorer_url: explorer_url.to_string(),
            timeout_ms: BLOCKCHAIN_TIMEOUT,
            max_retries: 3,
            retry_delay_ms: 1000, // 1 giây
        }
    }

    /// Thiết lập RPC URL
    pub fn with_rpc_url(mut self, rpc_url: &str) -> Self {
        self.rpc_url = rpc_url.to_string();
        self
    }

    /// Thiết lập Explorer URL
    pub fn with_explorer_url(mut self, explorer_url: &str) -> Self {
        self.explorer_url = explorer_url.to_string();
        self
    }

    /// Thiết lập timeout
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Thiết lập số lần retry tối đa
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Thiết lập thời gian chờ giữa các lần retry
    pub fn with_retry_delay(mut self, retry_delay_ms: u64) -> Self {
        self.retry_delay_ms = retry_delay_ms;
        self
    }
}

/// Định nghĩa Diamond Provider
static DIAMOND_PROVIDER_CACHE: Lazy<RwLock<HashMap<ChainId, Arc<DiamondProvider>>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

/// Loại token của Diamond blockchain
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiamondTokenType {
    /// Native token của Diamond blockchain
    Native,
    /// Token chuẩn DRC-20 (tương tự ERC-20)
    DRC20,
    /// Token chuẩn DRC-721 (tương tự ERC-721, NFT)
    DRC721,
    /// Token chuẩn DRC-1155 (tương tự ERC-1155, Multi-token)
    DRC1155,
}

/// Thông tin token Diamond
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiamondTokenInfo {
    /// ID của token
    pub id: String,
    /// Tên token
    pub name: String,
    /// Ký hiệu token
    pub symbol: String,
    /// Số thập phân (decimals)
    pub decimals: u8,
    /// Tổng cung
    pub total_supply: U256,
    /// Loại token
    pub token_type: DiamondTokenType,
    /// Địa chỉ của contract token 
    pub contract_address: String,
}

/// Diamond blockchain provider
pub struct DiamondProvider {
    /// Cấu hình
    pub config: DiamondConfig,
    /// HTTP client
    client: Client,
    /// Lock cho RPC calls
    rpc_lock: tokio::sync::Mutex<()>,
    /// Cache cho các kết quả RPC
    rpc_cache: RwLock<HashMap<String, (Value, Instant)>>,
}

impl DiamondProvider {
    /// Tạo provider mới
    pub fn new(config: DiamondConfig) -> Result<Self, DefiError> {
        let client = ClientBuilder::new()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|e| DefiError::ProviderError(format!("Failed to create Diamond client: {}", e)))?;

        Ok(Self {
            config,
            client,
            rpc_lock: tokio::sync::Mutex::new(()),
            rpc_cache: RwLock::new(HashMap::new()),
        })
    }

    /// Lấy Diamond provider từ cache hoặc tạo mới
    pub async fn get_provider(chain_id: ChainId) -> Result<Arc<DiamondProvider>, DefiError> {
        // Kiểm tra chain ID có phải là Diamond chain không
        match chain_id {
            ChainId::DiamondMainnet | ChainId::DiamondTestnet => {},
            _ => return Err(DefiError::ChainNotSupported(format!(
                "Expected Diamond chains, got {:?}", 
                chain_id
            ))),
        }

        // Thử lấy từ cache trước
        {
            let cache = DIAMOND_PROVIDER_CACHE.read().await;
            if let Some(provider) = cache.get(&chain_id) {
                return Ok(provider.clone());
            }
        }

        // Nếu không có trong cache, tạo mới
        let config = DiamondConfig::new(chain_id);
        let provider = DiamondProvider::new(config)?;
        let provider = Arc::new(provider);

        // Lưu vào cache
        {
            let mut cache = DIAMOND_PROVIDER_CACHE.write().await;
            cache.insert(chain_id, provider.clone());
        }

        Ok(provider)
    }

    /// Kiểm tra địa chỉ Diamond có hợp lệ không
    pub fn validate_address(&self, address: &str) -> bool {
        // Kiểm tra độ dài
        if address.len() != 34 {
            return false;
        }

        // Kiểm tra prefix
        if !address.starts_with('D') {
            return false;
        }

        // Kiểm tra ký tự hợp lệ
        let valid_chars = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
        if !address.chars().all(|c| valid_chars.contains(c)) {
            return false;
        }

        // Kiểm tra checksum
        let base58 = bs58::decode(address)
            .into_vec()
            .map_err(|_| false)
            .unwrap_or_default();
            
        if base58.len() != 25 {
            return false;
        }

        // Tách version và payload
        let version = base58[0];
        let payload = &base58[1..21];
        let checksum = &base58[21..];

        // Tính checksum
        let mut hasher = Sha256::new();
        hasher.update(&[version]);
        hasher.update(payload);
        let hash1 = hasher.finalize();

        let mut hasher = Sha256::new();
        hasher.update(hash1);
        let hash2 = hasher.finalize();

        // So sánh checksum
        &hash2[..4] == checksum
    }

    /// Thêm cơ chế retry cho RPC calls
    async fn call_rpc_with_retry<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: Value,
    ) -> Result<T, DefiError> {
        let mut retries = 0;
        let mut last_error = None;

        while retries < self.config.max_retries {
            match self.call_rpc(method, params.clone()).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = Some(e);
                    retries += 1;
                    if retries < self.config.max_retries {
                        tokio::time::sleep(Duration::from_millis(self.config.retry_delay_ms)).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            DefiError::ProviderError("Max retries exceeded".to_string())
        }))
    }

    /// Gọi RPC với caching và đồng bộ hóa
    async fn call_rpc<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: Value,
    ) -> Result<T, DefiError> {
        // Tạo cache key
        let cache_key = format!("{}_{}", method, params.to_string());
        
        // Kiểm tra cache
        {
            let cache = self.rpc_cache.read().await;
            if let Some((cached_value, timestamp)) = cache.get(&cache_key) {
                if timestamp.elapsed() < Duration::from_secs(30) {
                    return serde_json::from_value(cached_value.clone())
                        .map_err(|e| DefiError::ProviderError(format!(
                            "Failed to deserialize cached value: {}", e
                        )));
                }
            }
        }

        // Lấy lock trước khi gọi RPC
        let _guard = self.rpc_lock.lock().await;

        // Gọi RPC
        let response = self.client
            .post(&self.config.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": method,
                "params": params
            }))
            .send()
            .await
            .map_err(|e| DefiError::ProviderError(format!("RPC call failed: {}", e)))?;

        // Parse response
        let result: Value = response.json().await
            .map_err(|e| DefiError::ProviderError(format!("Failed to parse RPC response: {}", e)))?;

        // Kiểm tra lỗi
        if let Some(error) = result.get("error") {
            return Err(DefiError::ProviderError(format!(
                "RPC error: {}", error
            )));
        }

        // Lấy kết quả
        let result_value = result.get("result")
            .ok_or_else(|| DefiError::ProviderError("No result in RPC response".to_string()))?
            .clone();

        // Cập nhật cache
        {
            let mut cache = self.rpc_cache.write().await;
            cache.insert(cache_key, (result_value.clone(), Instant::now()));
        }

        // Parse kết quả
        serde_json::from_value(result_value)
            .map_err(|e| DefiError::ProviderError(format!(
                "Failed to deserialize RPC result: {}", e
            )))
    }

    /// Xóa cache cũ
    async fn cleanup_cache(&self) {
        let mut cache = self.rpc_cache.write().await;
        cache.retain(|_, (_, timestamp)| timestamp.elapsed() < Duration::from_secs(30));
    }

    /// Lấy số dư an toàn
    pub async fn get_balance(&self, address: &str) -> Result<U256, DefiError> {
        // Kiểm tra địa chỉ hợp lệ
        if !self.validate_address(address) {
            return Err(DefiError::InvalidAddress(format!(
                "Invalid Diamond address: {}",
                address
            )));
        }

        // Log thông tin
        debug!("Getting balance for address: {}", address);

        // Gọi RPC với retry
        let balance = self.call_rpc_with_retry(
            "getbalance",
            json!([address])
        ).await?;

        // Parse balance an toàn
        let balance_str = balance.to_string();
        match U256::from_dec_str(&balance_str) {
            Ok(balance) => {
                debug!("Successfully parsed balance: {} for address: {}", balance, address);
                Ok(balance)
            },
            Err(e) => {
                error!("Failed to parse balance: {} for address: {}", e, address);
                Err(DefiError::ProviderError(format!(
                    "Failed to parse balance: {}",
                    e
                )))
            }
        }
    }

    /// Lấy chiều cao block hiện tại
    pub async fn get_block_height(&self) -> Result<u64, DefiError> {
        #[derive(Deserialize)]
        struct BlockHeightResult {
            height: u64,
        }

        let result: BlockHeightResult = self.call_rpc("blockchain.getBlockHeight", json!([])).await?;
        Ok(result.height)
    }

    /// Lấy thông tin block theo số hoặc hash
    pub async fn get_block(&self, block_id: &str) -> Result<Value, DefiError> {
        // block_id có thể là số block hoặc hash
        self.call_rpc("blockchain.getBlock", json!([block_id])).await
    }

    /// Lấy thông tin giao dịch từ hash
    pub async fn get_transaction(&self, tx_hash: &str) -> Result<Value, DefiError> {
        self.call_rpc("blockchain.getTransaction", json!([tx_hash])).await
    }

    /// Lấy số dư tài khoản với xử lý tối ưu
    pub async fn get_account_balance(&self, address: &str) -> Result<U256, DefiError> {
        // Kiểm tra địa chỉ hợp lệ
        if !self.validate_address(address) {
            return Err(DefiError::InvalidAddress(format!(
                "Invalid Diamond address: {}", 
                address
            )));
        }

        // Log thông tin
        debug!("Getting account balance for address: {}", address);

        // Kiểm tra địa chỉ có tồn tại không
        let is_valid = self.is_address_valid(address).await?;
        if !is_valid {
            debug!("Address {} does not exist, returning zero balance", address);
            return Ok(U256::zero());
        }

        #[derive(Deserialize)]
        struct AccountBalanceResult {
            balance: String,
        }

        let params = json!([address]);
        let result: AccountBalanceResult = self.call_rpc("diamond_getBalance", params).await?;

        // Parse balance an toàn
        match result.balance.parse::<U256>() {
            Ok(balance) => {
                debug!("Successfully parsed account balance: {} for address: {}", balance, address);
                Ok(balance)
            },
            Err(e) => {
                error!("Failed to parse account balance: {} for address: {}", e, address);
                Err(DefiError::ProviderError(format!(
                    "Failed to parse balance: {} for address: {}", 
                    e, address
                )))
            }
        }
    }

    /// Kiểm tra địa chỉ có tồn tại không
    async fn is_address_valid(&self, address: &str) -> Result<bool, DefiError> {
        let params = json!([address]);
        let result: bool = self.call_rpc("diamond_isAddressValid", params).await?;
        Ok(result)
    }

    /// Lấy số dư token DRC-20 (tương tự ERC-20)
    pub async fn get_drc20_balance(&self, address: &str, token_id: &str) -> Result<U256, DefiError> {
        // Kiểm tra địa chỉ hợp lệ
        if !self.validate_address(address) {
            return Err(DefiError::ProviderError(format!(
                "Invalid Diamond address: {}", 
                address
            )));
        }

        // Log thông tin
        debug!("Getting DRC-20 balance for address: {}, token: {}", address, token_id);

        #[derive(Deserialize)]
        struct TokenBalanceResult {
            balance: String,
        }

        // Gọi RPC với retry và caching
        let result: TokenBalanceResult = self.call_rpc_with_retry(
            "tokens.getDRC20Balance", 
            json!([address, token_id])
        ).await?;
        
        // Parse balance an toàn
        match U256::from_dec_str(&result.balance) {
            Ok(balance) => {
                debug!("Successfully parsed DRC-20 balance: {} for address: {}, token: {}", 
                    balance, address, token_id);
                Ok(balance)
            },
            Err(e) => {
                error!("Failed to parse DRC-20 balance: {} for address: {}, token: {}", 
                    e, address, token_id);
                Err(DefiError::ProviderError(format!(
                    "Failed to parse DRC-20 token balance: {}", 
                    e
                )))
            }
        }
    }

    /// Gửi token DRC-20
    pub async fn send_drc20_token(
        &self, 
        private_key: &str, 
        to_address: &str, 
        token_id: &str, 
        amount: U256
    ) -> Result<String, DefiError> {
        // Kiểm tra địa chỉ người nhận
        if !self.validate_address(to_address) {
            return Err(DefiError::ProviderError(format!(
                "Invalid recipient address: {}", 
                to_address
            )));
        }

        // Log thông tin
        debug!("Sending DRC-20 token: {} amount: {} to: {}", 
            token_id, amount, to_address);

        #[derive(Deserialize)]
        struct TransactionResult {
            tx_hash: String,
        }

        let amount_str = amount.to_string();

        // Gọi RPC với retry
        let result: TransactionResult = self.call_rpc_with_retry(
            "tokens.transferDRC20", 
            json!([private_key, to_address, token_id, amount_str])
        ).await?;
        
        // Log kết quả
        info!("Successfully sent DRC-20 token: {} amount: {} to: {}, tx: {}", 
            token_id, amount, to_address, result.tx_hash);
        
        Ok(result.tx_hash)
    }

    /// Lấy thông tin về token
    pub async fn get_token_info(&self, token_id: &str) -> Result<DiamondTokenInfo, DefiError> {
        // Log thông tin
        debug!("Getting token info for token: {}", token_id);

        #[derive(Deserialize)]
        struct TokenInfoResult {
            id: String,
            name: String,
            symbol: String,
            decimals: u8,
            total_supply: String,
            contract_address: String,
            token_type: String,
        }

        // Gọi RPC với retry và caching
        let result: TokenInfoResult = self.call_rpc_with_retry(
            "tokens.getTokenInfo", 
            json!([token_id])
        ).await?;
        
        // Parse token type
        let token_type = match result.token_type.as_str() {
            "native" => DiamondTokenType::Native,
            "drc20" => DiamondTokenType::DRC20,
            "drc721" => DiamondTokenType::DRC721,
            "drc1155" => DiamondTokenType::DRC1155,
            _ => {
                error!("Unknown token type: {} for token: {}", result.token_type, token_id);
                return Err(DefiError::ProviderError(format!(
                    "Unknown token type: {}", 
                    result.token_type
                )));
            }
        };

        // Parse total supply an toàn
        let total_supply = match U256::from_dec_str(&result.total_supply) {
            Ok(supply) => supply,
            Err(e) => {
                error!("Failed to parse total supply: {} for token: {}", e, token_id);
                return Err(DefiError::ProviderError(format!(
                    "Failed to parse token total supply: {}", 
                    e
                )));
            }
        };

        // Log kết quả
        debug!("Successfully retrieved token info for token: {}", token_id);

        Ok(DiamondTokenInfo {
            id: result.id,
            name: result.name,
            symbol: result.symbol,
            decimals: result.decimals,
            total_supply,
            token_type,
            contract_address: result.contract_address,
        })
    }

    /// Lấy số dư token DRC-721 (NFT)
    pub async fn get_drc721_balance(&self, address: &str, token_id: &str) -> Result<U256, DefiError> {
        // Kiểm tra địa chỉ hợp lệ
        if !self.validate_address(address) {
            return Err(DefiError::ProviderError(format!(
                "Invalid Diamond address: {}", 
                address
            )));
        }

        // Log thông tin
        debug!("Getting DRC-721 balance for address: {}, token: {}", address, token_id);

        #[derive(Deserialize)]
        struct TokenBalanceResult {
            balance: String,
        }

        // Gọi RPC với retry và caching
        let result: TokenBalanceResult = self.call_rpc_with_retry(
            "tokens.getDRC721Balance", 
            json!([address, token_id])
        ).await?;
        
        // Parse balance an toàn
        match U256::from_dec_str(&result.balance) {
            Ok(balance) => {
                debug!("Successfully parsed DRC-721 balance: {} for address: {}, token: {}", 
                    balance, address, token_id);
                Ok(balance)
            },
            Err(e) => {
                error!("Failed to parse DRC-721 balance: {} for address: {}, token: {}", 
                    e, address, token_id);
                Err(DefiError::ProviderError(format!(
                    "Failed to parse DRC-721 token balance: {}", 
                    e
                )))
            }
        }
    }

    /// Lấy danh sách token ID của NFT cho một địa chỉ
    pub async fn get_drc721_tokens(&self, address: &str, token_id: &str) -> Result<Vec<String>, DefiError> {
        // Kiểm tra địa chỉ hợp lệ
        if !self.validate_address(address) {
            return Err(DefiError::ProviderError(format!(
                "Invalid Diamond address: {}", 
                address
            )));
        }

        // Log thông tin
        debug!("Getting DRC-721 tokens for address: {}, token: {}", address, token_id);

        #[derive(Deserialize)]
        struct TokensResult {
            token_ids: Vec<String>,
        }

        // Gọi RPC với retry và caching
        let result: TokensResult = self.call_rpc_with_retry(
            "tokens.getDRC721TokensOwned", 
            json!([address, token_id])
        ).await?;
        
        // Log kết quả
        debug!("Successfully retrieved {} DRC-721 tokens for address: {}, token: {}", 
            result.token_ids.len(), address, token_id);
        
        Ok(result.token_ids)
    }

    /// Lấy số dư token DRC-1155 (Multi-token)
    pub async fn get_drc1155_balance(
        &self, 
        address: &str, 
        token_id: &str, 
        item_id: &str
    ) -> Result<U256, DefiError> {
        // Kiểm tra địa chỉ hợp lệ
        if !self.validate_address(address) {
            return Err(DefiError::ProviderError(format!(
                "Invalid Diamond address: {}", 
                address
            )));
        }

        // Log thông tin
        debug!("Getting DRC-1155 balance for address: {}, token: {}, item: {}", 
            address, token_id, item_id);

        #[derive(Deserialize)]
        struct TokenBalanceResult {
            balance: String,
        }

        // Gọi RPC với retry và caching
        let result: TokenBalanceResult = self.call_rpc_with_retry(
            "tokens.getDRC1155Balance", 
            json!([address, token_id, item_id])
        ).await?;
        
        // Parse balance an toàn
        match U256::from_dec_str(&result.balance) {
            Ok(balance) => {
                debug!("Successfully parsed DRC-1155 balance: {} for address: {}, token: {}, item: {}", 
                    balance, address, token_id, item_id);
                Ok(balance)
            },
            Err(e) => {
                error!("Failed to parse DRC-1155 balance: {} for address: {}, token: {}, item: {}", 
                    e, address, token_id, item_id);
                Err(DefiError::ProviderError(format!(
                    "Failed to parse DRC-1155 token balance: {}", 
                    e
                )))
            }
        }
    }

    /// Gửi token DRC-1155
    pub async fn send_drc1155_token(
        &self,
        private_key: &str,
        to_address: &str,
        token_id: &str,
        item_id: &str,
        amount: U256
    ) -> Result<String, DefiError> {
        // Kiểm tra địa chỉ người nhận
        if !self.validate_address(to_address) {
            return Err(DefiError::ProviderError(format!(
                "Invalid recipient address: {}", 
                to_address
            )));
        }

        // Log thông tin
        debug!("Sending DRC-1155 token: {} item: {} amount: {} to: {}", 
            token_id, item_id, amount, to_address);

        #[derive(Deserialize)]
        struct TransactionResult {
            tx_hash: String,
        }

        let amount_str = amount.to_string();

        // Gọi RPC với retry
        let result: TransactionResult = self.call_rpc_with_retry(
            "tokens.transferDRC1155", 
            json!([private_key, to_address, token_id, item_id, amount_str])
        ).await?;
        
        // Log kết quả
        info!("Successfully sent DRC-1155 token: {} item: {} amount: {} to: {}, tx: {}", 
            token_id, item_id, amount, to_address, result.tx_hash);
        
        Ok(result.tx_hash)
    }
}

#[async_trait]
impl BlockchainProvider for DiamondProvider {
    fn chain_id(&self) -> ChainId {
        self.config.chain_id
    }
    
    fn blockchain_type(&self) -> BlockchainType {
        BlockchainType::Diamond
    }
    
    async fn is_connected(&self) -> Result<bool, DefiError> {
        // Kiểm tra kết nối bằng cách lấy block hiện tại
        match self.get_block_height().await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
    
    async fn get_block_number(&self) -> Result<u64, DefiError> {
        self.get_block_height().await
    }
    
    async fn get_balance(&self, address: &str) -> Result<U256, DefiError> {
        self.get_account_balance(address).await
    }
}

/// Tạo Diamond provider
pub async fn create_diamond_provider(config: BlockchainConfig) -> Result<Box<dyn BlockchainProvider>, DefiError> {
    // Kiểm tra chain ID có phải là Diamond chain không
    match config.chain_id {
        ChainId::DiamondMainnet | ChainId::DiamondTestnet => {},
        _ => return Err(DefiError::ChainNotSupported(format!(
            "Expected Diamond chains, got {:?}", 
            config.chain_id
        ))),
    }
    
    // Tạo cấu hình mặc định cho chain
    let mut diamond_config = DiamondConfig::new(config.chain_id);
    
    // Cập nhật RPC URL nếu được cung cấp
    if !config.rpc_url.is_empty() {
        diamond_config = diamond_config.with_rpc_url(&config.rpc_url);
    }
    
    // Cập nhật các tham số khác
    diamond_config = diamond_config
        .with_timeout(config.timeout_ms)
        .with_max_retries(config.max_retries);
    
    let provider = DiamondProvider::new(diamond_config)?;
    Ok(Box::new(provider))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diamond_config() {
        let config = DiamondConfig::new(ChainId::DiamondMainnet);
        assert_eq!(config.chain_id, ChainId::DiamondMainnet);
        assert_eq!(config.rpc_url, "https://mainnet-rpc.diamondprotocol.co");
        
        let config = config.with_timeout(5000);
        assert_eq!(config.timeout_ms, 5000);
        
        let config = config.with_max_retries(5);
        assert_eq!(config.max_retries, 5);
    }

    #[test]
    fn test_validate_address() {
        let config = DiamondConfig::new(ChainId::DiamondMainnet);
        let provider = DiamondProvider::new(config).unwrap();
        
        // Địa chỉ hợp lệ (giả định)
        assert!(provider.validate_address("DdkNKDGXR7FkH3mZAwxMrhxJAnoEzQ9qWL"));
        
        // Địa chỉ không hợp lệ
        assert!(!provider.validate_address("invalid_address"));
        assert!(!provider.validate_address("1dkNKDGXR7FkH3mZAwxMrhxJAnoEzQ9qWL"));
    }
} 