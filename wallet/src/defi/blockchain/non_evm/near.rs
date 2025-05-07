//! Module cho NEAR blockchain provider
//!
//! Module này cung cấp các chức năng để tương tác với NEAR blockchain,
//! bao gồm các chức năng như truy vấn số dư, gửi giao dịch, và quản lý tài khoản.
//!
//! ## Ví dụ:
//! ```rust
//! use wallet::defi::blockchain::non_evm::near::{NearProvider, NearConfig};
//! use wallet::defi::blockchain::ChainId;
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = NearConfig::new(ChainId::Near);
//!     let provider = NearProvider::new(config).unwrap();
//!
//!     let balance = provider.get_balance("example.near").await.unwrap();
//!     println!("Balance: {}", balance);
//! }
//! ```

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use std::collections::HashMap;
use once_cell::sync::Lazy;
use tokio::sync::RwLock;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use ethers::types::{U256, H256};
use tracing::{info, warn, error, debug};
use reqwest::{Client, ClientBuilder};
use serde_json::{json, Value};

use crate::defi::blockchain::{
    BlockchainProvider, BlockchainType, BlockchainConfig, ChainId
};
use crate::defi::error::DefiError;
use crate::defi::constants::BLOCKCHAIN_TIMEOUT;

/// Cấu hình cho NEAR Provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NearConfig {
    /// Chain ID
    pub chain_id: ChainId,
    /// RPC URL
    pub rpc_url: String,
    /// Timeout (ms)
    pub timeout_ms: u64,
    /// Số lần retry tối đa
    pub max_retries: u32,
    /// Network ID
    pub network_id: String,
}

impl NearConfig {
    /// Tạo cấu hình mới với giá trị mặc định
    pub fn new(chain_id: ChainId) -> Self {
        if chain_id != ChainId::Near && chain_id != ChainId::NearTestnet {
            panic!("Invalid chain ID for NEAR config");
        }

        let network_id = match chain_id {
            ChainId::Near => "mainnet".to_string(),
            ChainId::NearTestnet => "testnet".to_string(),
            _ => unreachable!()
        };

        Self {
            chain_id,
            rpc_url: chain_id.default_rpc_url().to_string(),
            timeout_ms: BLOCKCHAIN_TIMEOUT,
            max_retries: crate::defi::constants::MAX_RETRIES,
            network_id,
        }
    }

    /// Thiết lập RPC URL
    pub fn with_rpc_url(mut self, rpc_url: &str) -> Self {
        self.rpc_url = rpc_url.to_string();
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

    /// Thiết lập network ID
    pub fn with_network_id(mut self, network_id: &str) -> Self {
        self.network_id = network_id.to_string();
        self
    }
}

/// Cache cho các NEAR provider
static NEAR_PROVIDER_CACHE: Lazy<RwLock<HashMap<ChainId, Arc<NearProvider>>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

/// Kết quả RPC từ NEAR
#[derive(Debug, Serialize, Deserialize)]
struct NearRpcResponse<T> {
    id: String,
    jsonrpc: String,
    result: Option<T>,
    error: Option<NearRpcError>,
}

/// Lỗi RPC từ NEAR
#[derive(Debug, Serialize, Deserialize)]
struct NearRpcError {
    code: i32,
    message: String,
    data: Option<Value>,
}

/// NEAR Provider
pub struct NearProvider {
    /// Cấu hình
    pub config: NearConfig,
    /// HTTP Client
    client: Client,
}

impl NearProvider {
    /// Tạo provider mới
    pub fn new(config: NearConfig) -> Result<Self, DefiError> {
        if config.chain_id != ChainId::Near && config.chain_id != ChainId::NearTestnet {
            return Err(DefiError::ChainNotSupported(format!(
                "Expected NEAR chains, got {:?}", 
                config.chain_id
            )));
        }

        let client = ClientBuilder::new()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|e| DefiError::ProviderError(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            config,
            client,
        })
    }

    /// Gửi yêu cầu RPC tới NEAR
    async fn rpc_call<T: for<'de> Deserialize<'de>>(&self, method: &str, params: Value) -> Result<T, DefiError> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": "dontcare",
            "method": method,
            "params": params
        });

        let response = self.client
            .post(self.config.rpc_url.clone())
            .json(&request)
            .send()
            .await
            .map_err(|e| DefiError::ProviderError(format!("RPC request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            return Err(DefiError::ProviderError(format!("RPC request failed with status: {}", status)));
        }

        let rpc_response: NearRpcResponse<T> = response
            .json()
            .await
            .map_err(|e| DefiError::ProviderError(format!("Failed to parse RPC response: {}", e)))?;

        if let Some(error) = rpc_response.error {
            return Err(DefiError::ProviderError(format!("RPC error: {} (code: {})", error.message, error.code)));
        }

        rpc_response.result.ok_or_else(|| DefiError::ProviderError("RPC response missing result".to_string()))
    }

    /// Lấy NEAR provider từ cache hoặc tạo mới
    pub async fn get_provider(chain_id: ChainId) -> Result<Arc<Self>, DefiError> {
        if chain_id != ChainId::Near && chain_id != ChainId::NearTestnet {
            return Err(DefiError::ChainNotSupported(chain_id.to_string()));
        }

        // Thử lấy từ cache trước
        {
            let cache = NEAR_PROVIDER_CACHE.read().await;
            if let Some(provider) = cache.get(&chain_id) {
                return Ok(provider.clone());
            }
        }

        // Nếu không có trong cache, tạo mới
        let config = NearConfig::new(chain_id);
        let provider = Arc::new(
            Self::new(config)
                .map_err(|e| DefiError::ProviderError(format!("Failed to create NEAR provider: {}", e)))?
        );

        // Lưu vào cache
        {
            let mut cache = NEAR_PROVIDER_CACHE.write().await;
            cache.insert(chain_id, provider.clone());
        }

        Ok(provider)
    }

    /// Lấy thông tin tài khoản NEAR
    pub async fn get_account(&self, account_id: &str) -> Result<Value, DefiError> {
        self.rpc_call(
            "query",
            json!({
                "request_type": "view_account",
                "finality": "final",
                "account_id": account_id
            })
        ).await
    }

    /// Lấy số dư tài khoản (trả về yoctoNEAR - 10^-24 NEAR)
    pub async fn get_balance_yocto(&self, account_id: &str) -> Result<U256, DefiError> {
        let account_info: Value = self.get_account(account_id).await?;
        
        let amount = account_info["amount"]
            .as_str()
            .ok_or_else(|| DefiError::ProviderError("Invalid account balance format".to_string()))?;
            
        U256::from_dec_str(amount)
            .map_err(|e| DefiError::ProviderError(format!("Failed to parse balance: {}", e)))
    }

    /// Lấy số dư tài khoản chuyển đổi sang NEAR (từ yoctoNEAR)
    pub async fn get_balance_near(&self, account_id: &str) -> Result<f64, DefiError> {
        let yocto_balance = self.get_balance_yocto(account_id).await?;
        
        // Chuyển từ yoctoNEAR (10^-24) sang NEAR
        // Vì U256 không hỗ trợ trực tiếp phép chia dấu phẩy động, nên chuyển đổi sang String
        let balance_str = yocto_balance.to_string();
        let balance_f64 = balance_str.parse::<f64>().unwrap_or(0.0);
        
        Ok(balance_f64 / 1_000_000_000_000_000_000_000_000.0) // 1 NEAR = 10^24 yoctoNEAR
    }

    /// Lấy số block hiện tại
    pub async fn get_block_height(&self) -> Result<u64, DefiError> {
        let block_info: Value = self.rpc_call(
            "block",
            json!({
                "finality": "final"
            })
        ).await?;
        
        let height = block_info["header"]["height"]
            .as_u64()
            .ok_or_else(|| DefiError::ProviderError("Invalid block height format".to_string()))?;
            
        Ok(height)
    }

    /// Lấy thông tin một block theo height
    pub async fn get_block_by_height(&self, height: u64) -> Result<Value, DefiError> {
        self.rpc_call(
            "block",
            json!({
                "block_id": height
            })
        ).await
    }

    /// Gọi view function trên một smart contract
    pub async fn view_function(&self, contract_id: &str, method_name: &str, args: Value) -> Result<Value, DefiError> {
        let args_base64 = if args.is_null() {
            base64::encode([])
        } else {
            base64::encode(args.to_string())
        };
        
        self.rpc_call(
            "query",
            json!({
                "request_type": "call_function",
                "finality": "final",
                "account_id": contract_id,
                "method_name": method_name,
                "args_base64": args_base64
            })
        ).await
    }

    /// Kiểm tra xem giao dịch đã được xác nhận chưa
    pub async fn check_transaction(&self, tx_hash: &str, account_id: &str) -> Result<Value, DefiError> {
        self.rpc_call(
            "tx",
            json!([
                tx_hash,
                account_id
            ])
        ).await
    }
}

#[async_trait]
impl BlockchainProvider for NearProvider {
    fn chain_id(&self) -> ChainId {
        self.config.chain_id
    }
    
    fn blockchain_type(&self) -> BlockchainType {
        BlockchainType::Near
    }
    
    async fn is_connected(&self) -> Result<bool, DefiError> {
        match self.get_block_height().await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
    
    async fn get_block_number(&self) -> Result<u64, DefiError> {
        self.get_block_height().await
    }
    
    async fn get_balance(&self, address: &str) -> Result<U256, DefiError> {
        self.get_balance_yocto(address).await
    }
}

/// Tạo NEAR provider
pub async fn create_near_provider(config: BlockchainConfig) -> Result<Box<dyn BlockchainProvider>, DefiError> {
    if !matches!(config.chain_id, ChainId::Near | ChainId::NearTestnet) {
        return Err(DefiError::ChainNotSupported(format!(
            "Expected NEAR chains, got {:?}", 
            config.chain_id
        )));
    }
    
    let network_id = match config.chain_id {
        ChainId::Near => "mainnet",
        ChainId::NearTestnet => "testnet",
        _ => unreachable!()
    };
    
    let near_config = NearConfig {
        chain_id: config.chain_id,
        rpc_url: config.rpc_url,
        timeout_ms: config.timeout_ms,
        max_retries: config.max_retries,
        network_id: network_id.to_string(),
    };
    
    let provider = NearProvider::new(near_config)?;
    Ok(Box::new(provider))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_near_config() {
        let config = NearConfig::new(ChainId::Near);
        assert_eq!(config.chain_id, ChainId::Near);
        assert_eq!(config.rpc_url, ChainId::Near.default_rpc_url());
        assert_eq!(config.network_id, "mainnet");
    }
    
    #[test]
    fn test_near_config_builder() {
        let config = NearConfig::new(ChainId::NearTestnet)
            .with_rpc_url("https://custom-rpc.near.org")
            .with_timeout(5000)
            .with_max_retries(5)
            .with_network_id("custom-testnet");
            
        assert_eq!(config.chain_id, ChainId::NearTestnet);
        assert_eq!(config.rpc_url, "https://custom-rpc.near.org");
        assert_eq!(config.timeout_ms, 5000);
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.network_id, "custom-testnet");
    }
    
    // Integration tests would require actual RPC connection
    // #[tokio::test]
    // async fn test_near_provider_integration() {
    //     let config = NearConfig::new(ChainId::NearTestnet);
    //     let provider = NearProvider::new(config).unwrap();
    //     
    //     let is_connected = provider.is_connected().await.unwrap();
    //     assert!(is_connected);
    //     
    //     let block_number = provider.get_block_number().await.unwrap();
    //     assert!(block_number > 0);
    //     
    //     let balance = provider.get_balance("example.testnet").await.unwrap();
    //     println!("Balance: {}", balance);
    // }
} 