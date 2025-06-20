//! Module cho Cosmos blockchain provider
//!
//! Module này cung cấp các chức năng để tương tác với hệ sinh thái Cosmos,
//! bao gồm các chức năng như truy vấn số dư, gửi giao dịch, và quản lý tài khoản.
//!
//! ## Ví dụ:
//! ```rust
//! use wallet::defi::blockchain::non_evm::cosmos::{CosmosProvider, CosmosConfig};
//! use wallet::defi::blockchain::ChainId;
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = CosmosConfig::new(ChainId::CosmosHub);
//!     let provider = CosmosProvider::new(config).unwrap();
//!
//!     let balance = provider.get_balance("cosmos1hsk6jryyqjfhp5dhc55tc9jtckygx0eph6dd02").await.unwrap();
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
use bech32::{ToBase32, Variant};

use crate::defi::blockchain::{
    BlockchainProvider, BlockchainType, BlockchainConfig, ChainId
};
use crate::defi::error::DefiError;
use crate::defi::constants::BLOCKCHAIN_TIMEOUT;

/// Đơn vị tiền tệ của Cosmos
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Denom {
    /// Tên đơn vị
    pub name: String,
    /// Số thập phân
    pub decimals: u8,
}

impl Denom {
    /// Tạo đơn vị mới
    pub fn new(name: &str, decimals: u8) -> Self {
        Self {
            name: name.to_string(),
            decimals,
        }
    }
}

/// Cấu hình cho Cosmos Provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CosmosConfig {
    /// Chain ID
    pub chain_id: ChainId,
    /// RPC URL cho Cosmos API (Tendermint RPC)
    pub rpc_url: String,
    /// REST API URL cho Cosmos SDK
    pub rest_url: String,
    /// Prefix địa chỉ (ví dụ: "cosmos", "osmo", "juno")
    pub bech32_prefix: String,
    /// Đơn vị tiền tệ chính
    pub primary_denom: Denom,
    /// Danh sách đơn vị tiền tệ được hỗ trợ
    pub supported_denoms: Vec<Denom>,
    /// Timeout (ms)
    pub timeout_ms: u64,
    /// Số lần retry tối đa
    pub max_retries: u32,
}

impl CosmosConfig {
    /// Tạo cấu hình mới với giá trị mặc định cho chain đã cho
    pub fn new(chain_id: ChainId) -> Self {
        match chain_id {
            ChainId::CosmosHub => Self {
                chain_id,
                rpc_url: "https://rpc.cosmos.network".to_string(),
                rest_url: "https://rest.cosmos.network".to_string(),
                bech32_prefix: "cosmos".to_string(),
                primary_denom: Denom::new("uatom", 6),
                supported_denoms: vec![Denom::new("uatom", 6)],
                timeout_ms: BLOCKCHAIN_TIMEOUT,
                max_retries: crate::defi::constants::MAX_RETRIES,
            },
            ChainId::Osmosis => Self {
                chain_id,
                rpc_url: "https://rpc.osmosis.zone".to_string(),
                rest_url: "https://rest.osmosis.zone".to_string(),
                bech32_prefix: "osmo".to_string(),
                primary_denom: Denom::new("uosmo", 6),
                supported_denoms: vec![Denom::new("uosmo", 6)],
                timeout_ms: BLOCKCHAIN_TIMEOUT,
                max_retries: crate::defi::constants::MAX_RETRIES,
            },
            ChainId::Juno => Self {
                chain_id,
                rpc_url: "https://rpc.juno.omniflix.co".to_string(),
                rest_url: "https://rest.juno.omniflix.co".to_string(),
                bech32_prefix: "juno".to_string(),
                primary_denom: Denom::new("ujuno", 6),
                supported_denoms: vec![Denom::new("ujuno", 6)],
                timeout_ms: BLOCKCHAIN_TIMEOUT,
                max_retries: crate::defi::constants::MAX_RETRIES,
            },
            _ => panic!("Unsupported Cosmos chain ID: {:?}", chain_id),
        }
    }

    /// Thiết lập RPC URL
    pub fn with_rpc_url(mut self, rpc_url: &str) -> Self {
        self.rpc_url = rpc_url.to_string();
        self
    }

    /// Thiết lập REST URL
    pub fn with_rest_url(mut self, rest_url: &str) -> Self {
        self.rest_url = rest_url.to_string();
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

    /// Thêm đơn vị tiền tệ mới
    pub fn add_denom(mut self, denom: Denom) -> Self {
        self.supported_denoms.push(denom);
        self
    }
}

/// Cache cho các Cosmos provider
static COSMOS_PROVIDER_CACHE: Lazy<RwLock<HashMap<ChainId, Arc<CosmosProvider>>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

/// Cosmos Provider
pub struct CosmosProvider {
    /// Cấu hình
    pub config: CosmosConfig,
    /// HTTP Client cho RPC
    rpc_client: Client,
    /// HTTP Client cho REST API
    rest_client: Client,
}

impl CosmosProvider {
    /// Tạo provider mới
    pub fn new(config: CosmosConfig) -> Result<Self, DefiError> {
        let rpc_client = ClientBuilder::new()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|e| DefiError::ProviderError(format!("Failed to create RPC client: {}", e)))?;

        let rest_client = ClientBuilder::new()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|e| DefiError::ProviderError(format!("Failed to create REST client: {}", e)))?;

        Ok(Self {
            config,
            rpc_client,
            rest_client,
        })
    }

    /// Lấy Cosmos provider từ cache hoặc tạo mới
    pub async fn get_provider(chain_id: ChainId) -> Result<Arc<Self>, DefiError> {
        // Kiểm tra chain ID có phải là Cosmos chain không
        match chain_id {
            ChainId::CosmosHub | ChainId::Osmosis | ChainId::Juno => {},
            _ => return Err(DefiError::ChainNotSupported(chain_id.to_string())),
        }

        // Thử lấy từ cache trước
        {
            let cache = COSMOS_PROVIDER_CACHE.read().await;
            if let Some(provider) = cache.get(&chain_id) {
                return Ok(provider.clone());
            }
        }

        // Nếu không có trong cache, tạo mới
        let config = CosmosConfig::new(chain_id);
        let provider = Arc::new(
            Self::new(config)
                .map_err(|e| DefiError::ProviderError(format!("Failed to create Cosmos provider: {}", e)))?
        );

        // Lưu vào cache
        {
            let mut cache = COSMOS_PROVIDER_CACHE.write().await;
            cache.insert(chain_id, provider.clone());
        }

        Ok(provider)
    }

    /// Gọi Tendermint RPC
    async fn call_rpc<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<T, DefiError> {
        let request = match params {
            Some(p) => json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": method,
                "params": p
            }),
            None => json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": method,
                "params": []
            }),
        };

        let response = self.rpc_client
            .post(&self.config.rpc_url)
            .json(&request)
            .send()
            .await
            .map_err(|e| DefiError::ProviderError(format!("RPC request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            return Err(DefiError::ProviderError(format!("RPC request failed with status: {}", status)));
        }

        let resp: Value = response
            .json()
            .await
            .map_err(|e| DefiError::ProviderError(format!("Failed to parse RPC response: {}", e)))?;

        if let Some(error) = resp.get("error") {
            return Err(DefiError::ProviderError(format!("RPC error: {}", error)));
        }

        let result = resp.get("result")
            .ok_or_else(|| DefiError::ProviderError("Missing 'result' in RPC response".to_string()))?;

        serde_json::from_value(result.clone())
            .map_err(|e| DefiError::ProviderError(format!("Failed to parse RPC result: {}", e)))
    }

    /// Gọi REST API
    async fn call_rest<T: for<'de> Deserialize<'de>>(
        &self,
        endpoint: &str,
    ) -> Result<T, DefiError> {
        let url = format!("{}{}", self.config.rest_url, endpoint);

        let response = self.rest_client
            .get(&url)
            .send()
            .await
            .map_err(|e| DefiError::ProviderError(format!("REST request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            return Err(DefiError::ProviderError(format!("REST request failed with status: {}", status)));
        }

        response
            .json::<T>()
            .await
            .map_err(|e| DefiError::ProviderError(format!("Failed to parse REST response: {}", e)))
    }

    /// Kiểm tra địa chỉ có hợp lệ không
    pub fn validate_address(&self, address: &str) -> bool {
        if let Ok((hrp, _)) = bech32::decode(address) {
            hrp == self.config.bech32_prefix
        } else {
            false
        }
    }

    /// Tạo địa chỉ mới từ public key
    pub fn address_from_pubkey(&self, pubkey: &[u8]) -> Result<String, DefiError> {
        let data = pubkey.to_base32();
        bech32::encode(&self.config.bech32_prefix, data, Variant::Bech32)
            .map_err(|e| DefiError::ProviderError(format!("Failed to encode address: {}", e)))
    }

    /// Lấy số dư tài khoản cho đơn vị tiền tệ cụ thể
    pub async fn get_denom_balance(&self, address: &str, denom: &str) -> Result<U256, DefiError> {
        // Kiểm tra địa chỉ
        if !self.validate_address(address) {
            return Err(DefiError::ProviderError(format!("Invalid address format for {}", self.config.bech32_prefix)));
        }

        let endpoint = format!("/bank/balances/{}", address);
        let response: Value = self.call_rest(&endpoint).await?;

        if let Some(balances) = response.get("balances").and_then(|b| b.as_array()) {
            for balance in balances {
                if let (Some(amount), Some(d)) = (balance.get("amount").and_then(|a| a.as_str()), 
                                               balance.get("denom").and_then(|d| d.as_str())) {
                    if d == denom {
                        if let Ok(amount_u256) = amount.parse::<u128>() {
                            return Ok(U256::from(amount_u256));
                        }
                    }
                }
            }
        }

        // Nếu không tìm thấy đơn vị tiền tệ, trả về 0
        Ok(U256::zero())
    }

    /// Lấy tất cả số dư tài khoản
    pub async fn get_all_balances(&self, address: &str) -> Result<HashMap<String, U256>, DefiError> {
        // Kiểm tra địa chỉ
        if !self.validate_address(address) {
            return Err(DefiError::ProviderError(format!("Invalid address format for {}", self.config.bech32_prefix)));
        }

        let endpoint = format!("/bank/balances/{}", address);
        let response: Value = self.call_rest(&endpoint).await?;

        let mut balances = HashMap::new();
        
        if let Some(balance_array) = response.get("balances").and_then(|b| b.as_array()) {
            for balance in balance_array {
                if let (Some(amount), Some(denom)) = (balance.get("amount").and_then(|a| a.as_str()), 
                                                  balance.get("denom").and_then(|d| d.as_str())) {
                    if let Ok(amount_u256) = amount.parse::<u128>() {
                        balances.insert(denom.to_string(), U256::from(amount_u256));
                    }
                }
            }
        }

        Ok(balances)
    }

    /// Lấy thông tin tài khoản
    pub async fn get_account_info(&self, address: &str) -> Result<Value, DefiError> {
        // Kiểm tra địa chỉ
        if !self.validate_address(address) {
            return Err(DefiError::ProviderError(format!("Invalid address format for {}", self.config.bech32_prefix)));
        }

        let endpoint = format!("/auth/accounts/{}", address);
        self.call_rest(&endpoint).await
    }

    /// Lấy số block hiện tại
    pub async fn get_latest_block(&self) -> Result<Value, DefiError> {
        self.call_rpc::<Value>("block", None).await
    }

    /// Lấy số block hiện tại
    pub async fn get_block_number(&self) -> Result<u64, DefiError> {
        let response: Value = self.call_rpc("block", None).await?;
        
        let height = response["block"]["header"]["height"]
            .as_str()
            .ok_or_else(|| DefiError::ProviderError("Invalid block height format".to_string()))?
            .parse::<u64>()
            .map_err(|e| DefiError::ProviderError(format!("Failed to parse block height: {}", e)))?;
            
        Ok(height)
    }

    /// Lấy thông tin giao dịch từ hash
    pub async fn get_transaction(&self, tx_hash: &str) -> Result<Value, DefiError> {
        // Tendermint RPC yêu cầu hash ở dạng base64 hoặc hex nhưng với prefix 0x
        let query_hash = if tx_hash.starts_with("0x") {
            tx_hash.to_string()
        } else {
            format!("0x{}", tx_hash)
        };
        
        let params = json!({
            "hash": query_hash,
            "prove": false
        });
        
        self.call_rpc("tx", Some(params)).await
    }

    /// Lấy thông tin validator
    pub async fn get_validators(&self) -> Result<Value, DefiError> {
        let endpoint = "/staking/validators";
        self.call_rest(endpoint).await
    }

    /// Lấy thông tin phần thưởng từ staking
    pub async fn get_delegation_rewards(&self, delegator_addr: &str, validator_addr: &str) -> Result<Value, DefiError> {
        // Kiểm tra địa chỉ
        if !self.validate_address(delegator_addr) {
            return Err(DefiError::ProviderError(format!("Invalid delegator address format for {}", self.config.bech32_prefix)));
        }

        let endpoint = format!("/distribution/delegators/{}/rewards/{}", delegator_addr, validator_addr);
        self.call_rest(&endpoint).await
    }

    /// Lấy tất cả các phần thưởng từ staking
    pub async fn get_all_delegation_rewards(&self, delegator_addr: &str) -> Result<Value, DefiError> {
        // Kiểm tra địa chỉ
        if !self.validate_address(delegator_addr) {
            return Err(DefiError::ProviderError(format!("Invalid delegator address format for {}", self.config.bech32_prefix)));
        }

        let endpoint = format!("/distribution/delegators/{}/rewards", delegator_addr);
        self.call_rest(&endpoint).await
    }
}

#[async_trait]
impl BlockchainProvider for CosmosProvider {
    fn chain_id(&self) -> ChainId {
        self.config.chain_id
    }
    
    fn blockchain_type(&self) -> BlockchainType {
        BlockchainType::Cosmos
    }
    
    async fn is_connected(&self) -> Result<bool, DefiError> {
        match self.get_block_number().await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
    
    async fn get_block_number(&self) -> Result<u64, DefiError> {
        self.get_block_number().await
    }
    
    async fn get_balance(&self, address: &str) -> Result<U256, DefiError> {
        let primary_denom = &self.config.primary_denom.name;
        self.get_denom_balance(address, primary_denom).await
    }
}

/// Tạo Cosmos provider
pub async fn create_cosmos_provider(config: BlockchainConfig) -> Result<Box<dyn BlockchainProvider>, DefiError> {
    // Kiểm tra chain ID có phải là Cosmos chain không
    match config.chain_id {
        ChainId::CosmosHub | ChainId::Osmosis | ChainId::Juno => {},
        _ => return Err(DefiError::ChainNotSupported(format!(
            "Expected Cosmos chains, got {:?}", 
            config.chain_id
        ))),
    }
    
    // Tạo cấu hình mặc định cho chain
    let mut cosmos_config = CosmosConfig::new(config.chain_id);
    
    // Cập nhật RPC URL nếu được cung cấp
    if !config.rpc_url.is_empty() {
        cosmos_config = cosmos_config.with_rpc_url(&config.rpc_url);
    }
    
    // Cập nhật các tham số khác
    cosmos_config = cosmos_config
        .with_timeout(config.timeout_ms)
        .with_max_retries(config.max_retries);
    
    let provider = CosmosProvider::new(cosmos_config)?;
    Ok(Box::new(provider))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cosmos_config() {
        let config = CosmosConfig::new(ChainId::CosmosHub);
        assert_eq!(config.chain_id, ChainId::CosmosHub);
        assert_eq!(config.bech32_prefix, "cosmos");
        assert_eq!(config.primary_denom.name, "uatom");
        assert_eq!(config.primary_denom.decimals, 6);
    }
    
    #[test]
    fn test_cosmos_config_builder() {
        let config = CosmosConfig::new(ChainId::Osmosis)
            .with_rpc_url("https://custom-rpc.osmosis.zone")
            .with_rest_url("https://custom-rest.osmosis.zone")
            .with_timeout(5000)
            .with_max_retries(5)
            .add_denom(Denom::new("ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2", 6));
            
        assert_eq!(config.chain_id, ChainId::Osmosis);
        assert_eq!(config.rpc_url, "https://custom-rpc.osmosis.zone");
        assert_eq!(config.rest_url, "https://custom-rest.osmosis.zone");
        assert_eq!(config.timeout_ms, 5000);
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.supported_denoms.len(), 2); // Mặc định + thêm mới
    }
    
    #[test]
    fn test_address_validation() {
        let provider = CosmosProvider::new(CosmosConfig::new(ChainId::CosmosHub)).unwrap();
        
        // Địa chỉ Cosmos Hub hợp lệ
        assert!(provider.validate_address("cosmos1hsk6jryyqjfhp5dhc55tc9jtckygx0eph6dd02"));
        
        // Địa chỉ Osmosis không hợp lệ cho Cosmos Hub provider
        assert!(!provider.validate_address("osmo1ualhu3fjgg77g485gmyswkq3w0dp7gys8mf85k"));
        
        // Địa chỉ không hợp lệ
        assert!(!provider.validate_address("invalid_address"));
    }
    
    // Integration tests would require actual RPC connection
    // #[tokio::test]
    // async fn test_cosmos_provider_integration() {
    //     let config = CosmosConfig::new(ChainId::CosmosHub);
    //     let provider = CosmosProvider::new(config).unwrap();
    //     
    //     let is_connected = provider.is_connected().await.unwrap();
    //     assert!(is_connected);
    //     
    //     let block_number = provider.get_block_number().await.unwrap();
    //     assert!(block_number > 0);
    //     
    //     let balance = provider.get_balance("cosmos1hsk6jryyqjfhp5dhc55tc9jtckygx0eph6dd02").await.unwrap();
    //     println!("Balance: {}", balance);
    // }
} 