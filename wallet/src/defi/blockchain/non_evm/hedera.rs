//! Module cho Hedera blockchain provider
//!
//! Module này cung cấp các chức năng để tương tác với Hedera Hashgraph,
//! bao gồm các chức năng như truy vấn số dư, gửi giao dịch, và quản lý tài khoản.
//!
//! ## Ví dụ:
//! ```rust
//! use wallet::defi::blockchain::non_evm::hedera::{HederaProvider, HederaConfig};
//! use wallet::defi::blockchain::ChainId;
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = HederaConfig::new(ChainId::HederaMainnet);
//!     let provider = HederaProvider::new(config).unwrap();
//!
//!     let balance = provider.get_balance("0.0.12345").await.unwrap();
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
use ethers::types::U256;
use tracing::{debug, info, warn, error};
use reqwest::{Client, ClientBuilder};
use serde_json::{json, Value};
use regex::Regex;

use crate::defi::blockchain::{
    BlockchainProvider, BlockchainType, BlockchainConfig, ChainId
};
use crate::defi::error::DefiError;
use crate::defi::constants::BLOCKCHAIN_TIMEOUT;

/// Cấu hình cho Hedera Provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HederaConfig {
    /// Chain ID
    pub chain_id: ChainId,
    /// Mirror node API URL
    pub mirror_node_url: String,
    /// JSON-RPC API URL (cho EVM tương thích)
    pub json_rpc_url: Option<String>,
    /// Timeout (ms)
    pub timeout_ms: u64,
    /// Số lần retry tối đa
    pub max_retries: u32,
}

impl HederaConfig {
    /// Tạo cấu hình mới với giá trị mặc định cho chain đã cho
    pub fn new(chain_id: ChainId) -> Self {
        match chain_id {
            ChainId::HederaMainnet => Self {
                chain_id,
                mirror_node_url: "https://mainnet-public.mirrornode.hedera.com/api/v1".to_string(),
                json_rpc_url: Some("https://mainnet.hashio.io/api".to_string()),
                timeout_ms: BLOCKCHAIN_TIMEOUT,
                max_retries: crate::defi::constants::MAX_RETRIES,
            },
            ChainId::HederaTestnet => Self {
                chain_id,
                mirror_node_url: "https://testnet.mirrornode.hedera.com/api/v1".to_string(),
                json_rpc_url: Some("https://testnet.hashio.io/api".to_string()),
                timeout_ms: BLOCKCHAIN_TIMEOUT,
                max_retries: crate::defi::constants::MAX_RETRIES,
            },
            _ => panic!("Unsupported Hedera chain ID: {:?}", chain_id),
        }
    }

    /// Thiết lập Mirror Node URL
    pub fn with_mirror_node_url(mut self, url: &str) -> Self {
        self.mirror_node_url = url.to_string();
        self
    }

    /// Thiết lập JSON-RPC URL
    pub fn with_json_rpc_url(mut self, url: Option<&str>) -> Self {
        self.json_rpc_url = url.map(|s| s.to_string());
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
}

/// Cache cho các Hedera provider
static HEDERA_PROVIDER_CACHE: Lazy<RwLock<HashMap<ChainId, Arc<HederaProvider>>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

/// Hedera Provider
pub struct HederaProvider {
    /// Cấu hình
    pub config: HederaConfig,
    /// HTTP Client cho Mirror Node API
    mirror_client: Client,
    /// HTTP Client cho JSON-RPC (nếu có)
    rpc_client: Option<Client>,
}

impl HederaProvider {
    /// Tạo provider mới
    pub fn new(config: HederaConfig) -> Result<Self, DefiError> {
        let mirror_client = ClientBuilder::new()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|e| DefiError::ProviderError(format!("Failed to create Mirror Node client: {}", e)))?;

        let rpc_client = match &config.json_rpc_url {
            Some(_) => {
                Some(ClientBuilder::new()
                    .timeout(Duration::from_millis(config.timeout_ms))
                    .build()
                    .map_err(|e| DefiError::ProviderError(format!("Failed to create JSON-RPC client: {}", e)))?)
            },
            None => None,
        };

        Ok(Self {
            config,
            mirror_client,
            rpc_client,
        })
    }

    /// Lấy Hedera provider từ cache hoặc tạo mới
    pub async fn get_provider(chain_id: ChainId) -> Result<Arc<Self>, DefiError> {
        // Kiểm tra chain ID có phải là Hedera chain không
        match chain_id {
            ChainId::HederaMainnet | ChainId::HederaTestnet => {},
            _ => return Err(DefiError::ChainNotSupported(chain_id.to_string())),
        }

        // Thử lấy từ cache trước
        {
            let cache = HEDERA_PROVIDER_CACHE.read().await;
            if let Some(provider) = cache.get(&chain_id) {
                return Ok(provider.clone());
            }
        }

        // Nếu không có trong cache, tạo mới
        let config = HederaConfig::new(chain_id);
        let provider = Arc::new(
            Self::new(config)
                .map_err(|e| DefiError::ProviderError(format!("Failed to create Hedera provider: {}", e)))?
        );

        // Lưu vào cache
        {
            let mut cache = HEDERA_PROVIDER_CACHE.write().await;
            cache.insert(chain_id, provider.clone());
        }

        Ok(provider)
    }

    /// Gọi Mirror Node API
    async fn call_mirror_api<T: for<'de> Deserialize<'de>>(
        &self,
        endpoint: &str,
    ) -> Result<T, DefiError> {
        let url = format!("{}{}", self.config.mirror_node_url, endpoint);

        let response = self.mirror_client
            .get(&url)
            .send()
            .await
            .map_err(|e| DefiError::ProviderError(format!("Mirror Node request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            return Err(DefiError::ProviderError(format!("Mirror Node request failed with status: {}", status)));
        }

        response
            .json::<T>()
            .await
            .map_err(|e| DefiError::ProviderError(format!("Failed to parse Mirror Node response: {}", e)))
    }

    /// Gọi JSON-RPC API
    async fn call_json_rpc<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: Vec<Value>,
    ) -> Result<T, DefiError> {
        let rpc_url = self.config.json_rpc_url.as_ref()
            .ok_or_else(|| DefiError::ProviderError("JSON-RPC URL not configured".to_string()))?;
        
        let rpc_client = self.rpc_client.as_ref()
            .ok_or_else(|| DefiError::ProviderError("JSON-RPC client not initialized".to_string()))?;

        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params
        });

        let response = rpc_client
            .post(rpc_url)
            .json(&request)
            .send()
            .await
            .map_err(|e| DefiError::ProviderError(format!("JSON-RPC request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            return Err(DefiError::ProviderError(format!("JSON-RPC request failed with status: {}", status)));
        }

        let resp: Value = response
            .json()
            .await
            .map_err(|e| DefiError::ProviderError(format!("Failed to parse JSON-RPC response: {}", e)))?;

        if let Some(error) = resp.get("error") {
            return Err(DefiError::ProviderError(format!("JSON-RPC error: {}", error)));
        }

        let result = resp.get("result")
            .ok_or_else(|| DefiError::ProviderError("Missing 'result' in JSON-RPC response".to_string()))?;

        serde_json::from_value(result.clone())
            .map_err(|e| DefiError::ProviderError(format!("Failed to parse JSON-RPC result: {}", e)))
    }

    /// Kiểm tra địa chỉ Hedera có hợp lệ không
    pub fn validate_account_id(&self, account_id: &str) -> bool {
        let re = Regex::new(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$").unwrap();
        re.is_match(account_id)
    }

    /// Kiểm tra EVM address có hợp lệ không
    pub fn validate_evm_address(&self, address: &str) -> bool {
        let re = Regex::new(r"^0x[a-fA-F0-9]{40}$").unwrap();
        re.is_match(address)
    }

    /// Chuyển đổi từ Hedera account ID sang EVM address
    pub async fn account_id_to_evm_address(&self, account_id: &str) -> Result<String, DefiError> {
        if !self.validate_account_id(account_id) {
            return Err(DefiError::ProviderError(format!("Invalid Hedera account ID: {}", account_id)));
        }

        let endpoint = format!("/accounts/{}", account_id);
        let response: Value = self.call_mirror_api(&endpoint).await?;

        let evm_address = response["evm_address"]
            .as_str()
            .ok_or_else(|| DefiError::ProviderError(format!("EVM address not found for account ID: {}", account_id)))?;

        Ok(evm_address.to_string())
    }

    /// Chuyển đổi từ EVM address sang Hedera account ID
    pub async fn evm_address_to_account_id(&self, evm_address: &str) -> Result<String, DefiError> {
        if !self.validate_evm_address(evm_address) {
            return Err(DefiError::ProviderError(format!("Invalid EVM address: {}", evm_address)));
        }

        let endpoint = format!("/accounts?evm_address={}", evm_address);
        let response: Value = self.call_mirror_api(&endpoint).await?;

        if let Some(accounts) = response["accounts"].as_array() {
            if let Some(account) = accounts.first() {
                if let Some(account_id) = account["account"].as_str() {
                    return Ok(account_id.to_string());
                }
            }
        }

        Err(DefiError::ProviderError(format!("Account ID not found for EVM address: {}", evm_address)))
    }

    /// Lấy số dư tài khoản (tính bằng hbar)
    pub async fn get_account_balance(&self, account_id: &str) -> Result<U256, DefiError> {
        if !self.validate_account_id(account_id) {
            return Err(DefiError::ProviderError(format!("Invalid Hedera account ID: {}", account_id)));
        }

        let endpoint = format!("/accounts/{}", account_id);
        let response: Value = self.call_mirror_api(&endpoint).await?;

        let balance = response["balance"]["balance"]
            .as_i64()
            .ok_or_else(|| DefiError::ProviderError(format!("Balance not found for account ID: {}", account_id)))?;

        // Hedera balance là tính bằng tinybar (10^-8 hbar)
        Ok(U256::from(balance))
    }

    /// Lấy thông tin token balance
    pub async fn get_token_balance(&self, account_id: &str, token_id: &str) -> Result<U256, DefiError> {
        if !self.validate_account_id(account_id) {
            return Err(DefiError::ProviderError(format!("Invalid Hedera account ID: {}", account_id)));
        }

        if !self.validate_account_id(token_id) {
            return Err(DefiError::ProviderError(format!("Invalid Hedera token ID: {}", token_id)));
        }

        let endpoint = format!("/accounts/{}/tokens?token.id={}", account_id, token_id);
        let response: Value = self.call_mirror_api(&endpoint).await?;

        if let Some(tokens) = response["tokens"].as_array() {
            if let Some(token) = tokens.first() {
                if let Some(balance) = token["balance"].as_i64() {
                    return Ok(U256::from(balance));
                }
            }
        }

        // Token không tìm thấy hoặc số dư bằng 0
        Ok(U256::zero())
    }

    /// Lấy tất cả token balances cho tài khoản
    pub async fn get_all_token_balances(&self, account_id: &str) -> Result<HashMap<String, U256>, DefiError> {
        if !self.validate_account_id(account_id) {
            return Err(DefiError::ProviderError(format!("Invalid Hedera account ID: {}", account_id)));
        }

        let endpoint = format!("/accounts/{}/tokens", account_id);
        let response: Value = self.call_mirror_api(&endpoint).await?;

        let mut balances = HashMap::new();
        
        if let Some(tokens) = response["tokens"].as_array() {
            for token in tokens {
                if let (Some(token_id), Some(balance)) = (
                    token["token_id"].as_str(),
                    token["balance"].as_i64()
                ) {
                    balances.insert(token_id.to_string(), U256::from(balance));
                }
            }
        }

        Ok(balances)
    }

    /// Lấy thông tin về token
    pub async fn get_token_info(&self, token_id: &str) -> Result<Value, DefiError> {
        if !self.validate_account_id(token_id) {
            return Err(DefiError::ProviderError(format!("Invalid Hedera token ID: {}", token_id)));
        }

        let endpoint = format!("/tokens/{}", token_id);
        self.call_mirror_api(&endpoint).await
    }

    /// Lấy số block hiện tại (chỉ áp dụng cho JSON-RPC)
    pub async fn get_block_number(&self) -> Result<u64, DefiError> {
        if self.rpc_client.is_none() {
            return Err(DefiError::ProviderError("JSON-RPC client not initialized".to_string()));
        }

        let block_number: String = self.call_json_rpc("eth_blockNumber", vec![]).await?;
        let block_number = u64::from_str_radix(&block_number[2..], 16)
            .map_err(|e| DefiError::ProviderError(format!("Failed to parse block number: {}", e)))?;

        Ok(block_number)
    }

    /// Lấy thông tin giao dịch từ transaction ID
    pub async fn get_transaction(&self, transaction_id: &str) -> Result<Value, DefiError> {
        let endpoint = format!("/transactions/{}", transaction_id);
        self.call_mirror_api(&endpoint).await
    }

    /// Lấy thông tin về NFT
    pub async fn get_nft_info(&self, token_id: &str, serial_number: u64) -> Result<Value, DefiError> {
        if !self.validate_account_id(token_id) {
            return Err(DefiError::ProviderError(format!("Invalid Hedera token ID: {}", token_id)));
        }

        let endpoint = format!("/tokens/{}/nfts/{}", token_id, serial_number);
        self.call_mirror_api(&endpoint).await
    }

    /// Lấy danh sách NFT của tài khoản
    pub async fn get_account_nfts(&self, account_id: &str) -> Result<Value, DefiError> {
        if !self.validate_account_id(account_id) {
            return Err(DefiError::ProviderError(format!("Invalid Hedera account ID: {}", account_id)));
        }

        let endpoint = format!("/accounts/{}/nfts", account_id);
        self.call_mirror_api(&endpoint).await
    }

    /// Lấy thông tin smart contract từ contract ID
    pub async fn get_contract_info(&self, contract_id: &str) -> Result<Value, DefiError> {
        if !self.validate_account_id(contract_id) {
            return Err(DefiError::ProviderError(format!("Invalid Hedera contract ID: {}", contract_id)));
        }

        let endpoint = format!("/contracts/{}", contract_id);
        self.call_mirror_api(&endpoint).await
    }
}

#[async_trait]
impl BlockchainProvider for HederaProvider {
    fn chain_id(&self) -> ChainId {
        self.config.chain_id
    }
    
    fn blockchain_type(&self) -> BlockchainType {
        BlockchainType::Hedera
    }
    
    async fn is_connected(&self) -> Result<bool, DefiError> {
        // Kiểm tra kết nối với Mirror Node
        let endpoint = "/network/nodes";
        match self.call_mirror_api::<Value>(endpoint).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
    
    async fn get_block_number(&self) -> Result<u64, DefiError> {
        if self.rpc_client.is_some() {
            self.get_block_number().await
        } else {
            // Hedera không có khái niệm block number truyền thống nếu không dùng JSON-RPC
            // Trả về timestamp hiện tại làm "block number" thay thế
            Ok(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs())
        }
    }
    
    async fn get_balance(&self, address: &str) -> Result<U256, DefiError> {
        // Kiểm tra xem address là account ID hay EVM address
        if self.validate_account_id(address) {
            self.get_account_balance(address).await
        } else if self.validate_evm_address(address) {
            let account_id = self.evm_address_to_account_id(address).await?;
            self.get_account_balance(&account_id).await
        } else {
            Err(DefiError::ProviderError(format!("Invalid Hedera address format: {}", address)))
        }
    }
}

/// Tạo Hedera provider
pub async fn create_hedera_provider(config: BlockchainConfig) -> Result<Box<dyn BlockchainProvider>, DefiError> {
    // Kiểm tra chain ID có phải là Hedera chain không
    match config.chain_id {
        ChainId::HederaMainnet | ChainId::HederaTestnet => {},
        _ => return Err(DefiError::ChainNotSupported(format!(
            "Expected Hedera chains, got {:?}", 
            config.chain_id
        ))),
    }
    
    // Tạo cấu hình mặc định cho chain
    let mut hedera_config = HederaConfig::new(config.chain_id);
    
    // Cập nhật Mirror Node URL nếu được cung cấp
    if !config.rpc_url.is_empty() {
        hedera_config = hedera_config.with_mirror_node_url(&config.rpc_url);
    }
    
    // Cập nhật các tham số khác
    hedera_config = hedera_config
        .with_timeout(config.timeout_ms)
        .with_max_retries(config.max_retries);
    
    let provider = HederaProvider::new(hedera_config)?;
    Ok(Box::new(provider))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_hedera_config() {
        let config = HederaConfig::new(ChainId::HederaMainnet);
        assert_eq!(config.chain_id, ChainId::HederaMainnet);
        assert!(config.mirror_node_url.contains("mainnet"));
        assert!(config.json_rpc_url.is_some());
    }
    
    #[test]
    fn test_hedera_config_builder() {
        let config = HederaConfig::new(ChainId::HederaTestnet)
            .with_mirror_node_url("https://custom-testnet.mirrornode.hedera.com/api/v1")
            .with_json_rpc_url(Some("https://custom-testnet.hashio.io/api"))
            .with_timeout(5000)
            .with_max_retries(5);
            
        assert_eq!(config.chain_id, ChainId::HederaTestnet);
        assert_eq!(config.mirror_node_url, "https://custom-testnet.mirrornode.hedera.com/api/v1");
        assert_eq!(config.json_rpc_url, Some("https://custom-testnet.hashio.io/api".to_string()));
        assert_eq!(config.timeout_ms, 5000);
        assert_eq!(config.max_retries, 5);
    }
    
    #[test]
    fn test_validate_account_id() {
        let provider_result = HederaProvider::new(HederaConfig::new(ChainId::HederaMainnet));
        assert!(provider_result.is_ok(), "Khởi tạo HederaProvider thất bại: {:?}", provider_result.err());
        let provider = provider_result.unwrap();
        // Các account ID hợp lệ
        assert!(provider.validate_account_id("0.0.12345"));
        assert!(provider.validate_account_id("1.2.3"));
        // Các account ID không hợp lệ
        assert!(!provider.validate_account_id("0.0."));
        assert!(!provider.validate_account_id("00.12345"));
        assert!(!provider.validate_account_id("hedera://0.0.12345"));
        assert!(!provider.validate_account_id("invalid_id"));
    }
    
    #[test]
    fn test_validate_evm_address() {
        let provider_result = HederaProvider::new(HederaConfig::new(ChainId::HederaMainnet));
        assert!(provider_result.is_ok(), "Khởi tạo HederaProvider thất bại: {:?}", provider_result.err());
        let provider = provider_result.unwrap();
        // Các EVM address hợp lệ
        assert!(provider.validate_evm_address("0x1234567890123456789012345678901234567890"));
        assert!(provider.validate_evm_address("0xabcdef1234567890abcdef1234567890abcdef12"));
        // Các EVM address không hợp lệ
        assert!(!provider.validate_evm_address("1234567890123456789012345678901234567890"));
        assert!(!provider.validate_evm_address("0x123456789012345678901234567890123456789")); // Thiếu 1 ký tự
        assert!(!provider.validate_evm_address("0x123456789012345678901234567890123456789G")); // Chứa ký tự không hợp lệ
    }
    
    // Integration tests would require actual Mirror Node connection
    // #[tokio::test]
    // async fn test_hedera_provider_integration() {
    //     let config = HederaConfig::new(ChainId::HederaTestnet);
    //     let provider = HederaProvider::new(config).unwrap();
    //     
    //     let is_connected = provider.is_connected().await.unwrap();
    //     assert!(is_connected);
    //     
    //     let balance = provider.get_account_balance("0.0.3").await.unwrap();
    //     println!("Balance: {}", balance);
    // }
} 