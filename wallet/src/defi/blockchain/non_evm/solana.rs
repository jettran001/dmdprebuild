//! Module cho Solana blockchain provider
//!
//! Module này cung cấp các chức năng để tương tác với mạng Solana,
//! bao gồm các chức năng như truy vấn số dư, gửi giao dịch, và quản lý tài khoản.
//!
//! ## Ví dụ:
//! ```rust
//! use wallet::defi::blockchain::non_evm::solana::{SolanaProvider, SolanaConfig};
//! use wallet::defi::blockchain::ChainId;
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = SolanaConfig::new(ChainId::SolanaMainnet);
//!     let provider = SolanaProvider::new(config).unwrap();
//!
//!     let balance = provider.get_balance("5U3bH5b6XtG99aVmwNDTyaqbpDnHrZEuGwMxhKnj5SJY").await.unwrap();
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
use bs58;
use borsh::{BorshDeserialize, BorshSerialize};

use crate::defi::blockchain::{
    BlockchainProvider, BlockchainType, BlockchainConfig, ChainId
};
use crate::defi::error::DefiError;
use crate::defi::constants::BLOCKCHAIN_TIMEOUT;

/// Cấu hình cho Solana Provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaConfig {
    /// Chain ID
    pub chain_id: ChainId,
    /// RPC URL
    pub rpc_url: String,
    /// Websocket URL (tùy chọn)
    pub ws_url: Option<String>,
    /// Commitment level
    pub commitment: String,
    /// Timeout (ms)
    pub timeout_ms: u64,
    /// Số lần retry tối đa
    pub max_retries: u32,
}

impl SolanaConfig {
    /// Tạo cấu hình mới với giá trị mặc định cho chain đã cho
    pub fn new(chain_id: ChainId) -> Self {
        match chain_id {
            ChainId::SolanaMainnet => Self {
                chain_id,
                rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
                ws_url: Some("wss://api.mainnet-beta.solana.com".to_string()),
                commitment: "confirmed".to_string(),
                timeout_ms: BLOCKCHAIN_TIMEOUT,
                max_retries: crate::defi::constants::MAX_RETRIES,
            },
            ChainId::SolanaDevnet => Self {
                chain_id,
                rpc_url: "https://api.devnet.solana.com".to_string(),
                ws_url: Some("wss://api.devnet.solana.com".to_string()),
                commitment: "confirmed".to_string(),
                timeout_ms: BLOCKCHAIN_TIMEOUT,
                max_retries: crate::defi::constants::MAX_RETRIES,
            },
            ChainId::SolanaTestnet => Self {
                chain_id,
                rpc_url: "https://api.testnet.solana.com".to_string(),
                ws_url: Some("wss://api.testnet.solana.com".to_string()),
                commitment: "confirmed".to_string(),
                timeout_ms: BLOCKCHAIN_TIMEOUT,
                max_retries: crate::defi::constants::MAX_RETRIES,
            },
            _ => panic!("Unsupported Solana chain ID: {:?}", chain_id),
        }
    }

    /// Thiết lập RPC URL
    pub fn with_rpc_url(mut self, url: &str) -> Self {
        self.rpc_url = url.to_string();
        self
    }

    /// Thiết lập Websocket URL
    pub fn with_ws_url(mut self, url: Option<&str>) -> Self {
        self.ws_url = url.map(|s| s.to_string());
        self
    }

    /// Thiết lập commitment level
    pub fn with_commitment(mut self, commitment: &str) -> Self {
        self.commitment = commitment.to_string();
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

/// Cache cho các Solana provider
static SOLANA_PROVIDER_CACHE: Lazy<RwLock<HashMap<ChainId, Arc<SolanaProvider>>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

/// Solana Provider
pub struct SolanaProvider {
    /// Cấu hình
    pub config: SolanaConfig,
    /// HTTP Client
    client: Client,
    /// ID yêu cầu hiện tại
    request_id: std::sync::atomic::AtomicU64,
}

impl SolanaProvider {
    /// Tạo provider mới
    pub fn new(config: SolanaConfig) -> Result<Self, DefiError> {
        let client = ClientBuilder::new()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|e| DefiError::ProviderError(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            config,
            client,
            request_id: std::sync::atomic::AtomicU64::new(1),
        })
    }

    /// Lấy Solana provider từ cache hoặc tạo mới
    pub async fn get_provider(chain_id: ChainId) -> Result<Arc<Self>, DefiError> {
        // Kiểm tra chain ID có phải là Solana chain không
        match chain_id {
            ChainId::SolanaMainnet | ChainId::SolanaDevnet | ChainId::SolanaTestnet => {},
            _ => return Err(DefiError::ChainNotSupported(chain_id.to_string())),
        }

        // Thử lấy từ cache trước
        {
            let cache = SOLANA_PROVIDER_CACHE.read().await;
            if let Some(provider) = cache.get(&chain_id) {
                return Ok(provider.clone());
            }
        }

        // Nếu không có trong cache, tạo mới
        let config = SolanaConfig::new(chain_id);
        let provider = Arc::new(
            Self::new(config)
                .map_err(|e| DefiError::ProviderError(format!("Failed to create Solana provider: {}", e)))?
        );

        // Lưu vào cache
        {
            let mut cache = SOLANA_PROVIDER_CACHE.write().await;
            cache.insert(chain_id, provider.clone());
        }

        Ok(provider)
    }

    /// Tạo ID cho mỗi yêu cầu JSON-RPC
    fn get_next_request_id(&self) -> u64 {
        self.request_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    /// Gọi JSON-RPC API của Solana
    async fn call_rpc<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<T, DefiError> {
        let id = self.get_next_request_id();
        
        let mut payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });
        
        if let Some(params_value) = params {
            payload["params"] = params_value;
        }

        let response = self.client
            .post(&self.config.rpc_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| DefiError::ProviderError(format!("RPC request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            return Err(DefiError::ProviderError(format!("RPC request failed with status: {}", status)));
        }

        let response_json: Value = response
            .json()
            .await
            .map_err(|e| DefiError::ProviderError(format!("Failed to parse RPC response: {}", e)))?;

        // Kiểm tra lỗi JSON-RPC
        if let Some(error) = response_json.get("error") {
            let error_message = error["message"].as_str().unwrap_or("Unknown error");
            let error_code = error["code"].as_i64().unwrap_or(0);
            return Err(DefiError::ProviderError(format!("RPC error: {} (code: {})", error_message, error_code)));
        }

        // Lấy kết quả
        let result = response_json.get("result")
            .ok_or_else(|| DefiError::ProviderError("No result field in RPC response".to_string()))?;

        serde_json::from_value(result.clone())
            .map_err(|e| DefiError::ProviderError(format!("Failed to parse RPC result: {}", e)))
    }

    /// Kiểm tra địa chỉ Solana có hợp lệ không
    pub fn validate_address(&self, address: &str) -> bool {
        // Địa chỉ Solana là Base58 encoded và có 32 bytes (đã mã hóa)
        match bs58::decode(address).into_vec() {
            Ok(bytes) => bytes.len() == 32,
            Err(_) => false,
        }
    }

    /// Lấy số dư tài khoản (đơn vị lamports, 1 SOL = 10^9 lamports)
    pub async fn get_account_balance(&self, address: &str) -> Result<U256, DefiError> {
        if !self.validate_address(address) {
            return Err(DefiError::ProviderError(format!("Invalid Solana address: {}", address)));
        }

        let params = json!([
            address,
            {
                "commitment": self.config.commitment
            }
        ]);

        let response: Value = self.call_rpc("getBalance", Some(params)).await?;
        
        // Lấy số dư từ kết quả
        let value = response.get("value")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| DefiError::ProviderError(format!("Failed to parse balance for address: {}", address)))?;

        Ok(U256::from(value))
    }

    /// Lấy thông tin tài khoản token SPL
    pub async fn get_token_account_balance(&self, token_account: &str) -> Result<(U256, u8), DefiError> {
        if !self.validate_address(token_account) {
            return Err(DefiError::ProviderError(format!("Invalid Solana token account address: {}", token_account)));
        }

        let params = json!([
            token_account,
            {
                "commitment": self.config.commitment
            }
        ]);

        let response: Value = self.call_rpc("getTokenAccountBalance", Some(params)).await?;
        
        // Lấy thông tin từ kết quả
        let value = response.get("value")
            .ok_or_else(|| DefiError::ProviderError(format!("Failed to parse token balance for account: {}", token_account)))?;

        let amount = value["amount"]
            .as_str()
            .ok_or_else(|| DefiError::ProviderError("Token amount not available".to_string()))?
            .parse::<u64>()
            .map_err(|e| DefiError::ProviderError(format!("Failed to parse token amount: {}", e)))?;

        let decimals = value["decimals"]
            .as_u64()
            .ok_or_else(|| DefiError::ProviderError("Token decimals not available".to_string()))? as u8;

        Ok((U256::from(amount), decimals))
    }

    /// Tìm tài khoản token SPL cho một địa chỉ cụ thể và mint
    pub async fn find_token_account_by_owner(
        &self,
        owner: &str,
        mint: &str,
    ) -> Result<Vec<String>, DefiError> {
        if !self.validate_address(owner) || !self.validate_address(mint) {
            return Err(DefiError::ProviderError("Invalid Solana address".to_string()));
        }

        // Token Program ID trên Solana
        let token_program_id = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

        let params = json!([
            {
                "mint": mint,
                "programId": token_program_id
            },
            {
                "encoding": "jsonParsed",
                "commitment": self.config.commitment
            }
        ]);

        let response: Value = self.call_rpc("getTokenAccountsByOwner", Some(params)).await?;
        
        let mut token_accounts = Vec::new();
        
        if let Some(accounts) = response.get("value").and_then(|v| v.as_array()) {
            for account in accounts {
                if let Some(pubkey) = account.get("pubkey").and_then(|p| p.as_str()) {
                    token_accounts.push(pubkey.to_string());
                }
            }
        }

        Ok(token_accounts)
    }

    /// Lấy số block hiện tại (slot)
    pub async fn get_slot(&self) -> Result<u64, DefiError> {
        let params = json!([{
            "commitment": self.config.commitment
        }]);

        let response: Value = self.call_rpc("getSlot", Some(params)).await?;
        
        response.as_u64()
            .ok_or_else(|| DefiError::ProviderError("Failed to parse slot number".to_string()))
    }

    /// Lấy thông tin block
    pub async fn get_block(&self, slot: u64) -> Result<Value, DefiError> {
        let params = json!([
            slot,
            {
                "encoding": "json",
                "transactionDetails": "full",
                "commitment": self.config.commitment
            }
        ]);

        self.call_rpc("getBlock", Some(params)).await
    }

    /// Lấy thông tin giao dịch
    pub async fn get_transaction(&self, signature: &str) -> Result<Value, DefiError> {
        let params = json!([
            signature,
            {
                "encoding": "json",
                "commitment": self.config.commitment
            }
        ]);

        self.call_rpc("getTransaction", Some(params)).await
    }

    /// Lấy thông tin tài khoản
    pub async fn get_account_info(&self, address: &str) -> Result<Value, DefiError> {
        if !self.validate_address(address) {
            return Err(DefiError::ProviderError(format!("Invalid Solana address: {}", address)));
        }

        let params = json!([
            address,
            {
                "encoding": "jsonParsed",
                "commitment": self.config.commitment
            }
        ]);

        self.call_rpc("getAccountInfo", Some(params)).await
    }

    /// Lấy tất cả tài khoản token SPL của một địa chỉ
    pub async fn get_all_token_accounts(&self, owner: &str) -> Result<HashMap<String, (U256, u8)>, DefiError> {
        if !self.validate_address(owner) {
            return Err(DefiError::ProviderError(format!("Invalid Solana address: {}", owner)));
        }

        // Token Program ID trên Solana
        let token_program_id = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

        let params = json!([
            owner,
            {
                "programId": token_program_id
            },
            {
                "encoding": "jsonParsed",
                "commitment": self.config.commitment
            }
        ]);

        let response: Value = self.call_rpc("getTokenAccountsByOwner", Some(params)).await?;
        
        let mut token_accounts = HashMap::new();
        
        if let Some(accounts) = response.get("value").and_then(|v| v.as_array()) {
            for account in accounts {
                if let (Some(pubkey), Some(account_data)) = (
                    account.get("pubkey").and_then(|p| p.as_str()),
                    account.get("account").and_then(|a| a.get("data")).and_then(|d| d.get("parsed")).and_then(|p| p.get("info"))
                ) {
                    if let (Some(mint), Some(amount), Some(decimals)) = (
                        account_data.get("mint").and_then(|m| m.as_str()),
                        account_data.get("tokenAmount").and_then(|t| t.get("amount")).and_then(|a| a.as_str()).and_then(|a| a.parse::<u64>().ok()),
                        account_data.get("tokenAmount").and_then(|t| t.get("decimals")).and_then(|d| d.as_u64())
                    ) {
                        token_accounts.insert(
                            mint.to_string(), 
                            (U256::from(amount), decimals as u8)
                        );
                    }
                }
            }
        }

        Ok(token_accounts)
    }

    /// Tính phí giao dịch hiện tại (đơn vị: lamports)
    pub async fn get_recent_blockhash_fee(&self) -> Result<u64, DefiError> {
        let params = json!([{
            "commitment": self.config.commitment
        }]);

        let response: Value = self.call_rpc("getRecentBlockhash", Some(params)).await?;
        
        response
            .get("value")
            .and_then(|v| v.get("feeCalculator"))
            .and_then(|f| f.get("lamportsPerSignature"))
            .and_then(|l| l.as_u64())
            .ok_or_else(|| DefiError::ProviderError("Failed to get recent blockhash fee".to_string()))
    }

    /// Kiểm tra trạng thái của một chữ ký giao dịch
    pub async fn confirm_transaction(&self, signature: &str) -> Result<bool, DefiError> {
        let params = json!([
            signature,
            {
                "commitment": self.config.commitment
            }
        ]);

        let response: Value = self.call_rpc("confirmTransaction", Some(params)).await?;
        
        response
            .get("value")
            .and_then(|v| v.get("confirmationStatus"))
            .and_then(|c| c.as_str())
            .map(|status| status == "confirmed" || status == "finalized")
            .ok_or_else(|| DefiError::ProviderError("Failed to confirm transaction".to_string()))
    }
}

#[async_trait]
impl BlockchainProvider for SolanaProvider {
    fn chain_id(&self) -> ChainId {
        self.config.chain_id
    }
    
    fn blockchain_type(&self) -> BlockchainType {
        BlockchainType::Solana
    }
    
    async fn is_connected(&self) -> Result<bool, DefiError> {
        // Kiểm tra kết nối bằng cách lấy slot hiện tại
        match self.get_slot().await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
    
    async fn get_block_number(&self) -> Result<u64, DefiError> {
        self.get_slot().await
    }
    
    async fn get_balance(&self, address: &str) -> Result<U256, DefiError> {
        self.get_account_balance(address).await
    }
}

/// Tạo Solana provider
pub async fn create_solana_provider(config: BlockchainConfig) -> Result<Box<dyn BlockchainProvider>, DefiError> {
    // Kiểm tra chain ID có phải là Solana chain không
    match config.chain_id {
        ChainId::SolanaMainnet | ChainId::SolanaDevnet | ChainId::SolanaTestnet => {},
        _ => return Err(DefiError::ChainNotSupported(format!(
            "Expected Solana chains, got {:?}", 
            config.chain_id
        ))),
    }
    
    // Tạo cấu hình mặc định cho chain
    let mut solana_config = SolanaConfig::new(config.chain_id);
    
    // Cập nhật RPC URL nếu được cung cấp
    if !config.rpc_url.is_empty() {
        solana_config = solana_config.with_rpc_url(&config.rpc_url);
    }
    
    // Cập nhật các tham số khác
    solana_config = solana_config
        .with_timeout(config.timeout_ms)
        .with_max_retries(config.max_retries);
    
    let provider = SolanaProvider::new(solana_config)?;
    Ok(Box::new(provider))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_solana_config() {
        let config = SolanaConfig::new(ChainId::SolanaMainnet);
        assert_eq!(config.chain_id, ChainId::SolanaMainnet);
        assert_eq!(config.rpc_url, "https://api.mainnet-beta.solana.com");
        assert_eq!(config.commitment, "confirmed");
    }
    
    #[test]
    fn test_solana_config_builder() {
        let config = SolanaConfig::new(ChainId::SolanaDevnet)
            .with_rpc_url("https://custom-api.solana.com")
            .with_ws_url(Some("wss://custom-ws.solana.com"))
            .with_commitment("finalized")
            .with_timeout(5000)
            .with_max_retries(5);
            
        assert_eq!(config.chain_id, ChainId::SolanaDevnet);
        assert_eq!(config.rpc_url, "https://custom-api.solana.com");
        assert_eq!(config.ws_url, Some("wss://custom-ws.solana.com".to_string()));
        assert_eq!(config.commitment, "finalized");
        assert_eq!(config.timeout_ms, 5000);
        assert_eq!(config.max_retries, 5);
    }
    
    #[test]
    fn test_validate_address() {
        let provider = SolanaProvider::new(SolanaConfig::new(ChainId::SolanaMainnet)).unwrap();
        
        // Các địa chỉ hợp lệ
        assert!(provider.validate_address("5U3bH5b6XtG99aVmwNDTyaqbpDnHrZEuGwMxhKnj5SJY"));
        assert!(provider.validate_address("So11111111111111111111111111111111111111112"));
        
        // Các địa chỉ không hợp lệ
        assert!(!provider.validate_address("invalid_address"));
        assert!(!provider.validate_address("0x71C7656EC7ab88b098defB751B7401B5f6d8976F")); // Địa chỉ Ethereum
    }
    
    // Integration tests would require actual Solana network connection
    // #[tokio::test]
    // async fn test_solana_provider_integration() {
    //     let config = SolanaConfig::new(ChainId::SolanaDevnet);
    //     let provider = SolanaProvider::new(config).unwrap();
    //     
    //     let is_connected = provider.is_connected().await.unwrap();
    //     assert!(is_connected);
    //     
    //     let slot = provider.get_slot().await.unwrap();
    //     assert!(slot > 0);
    //     
    //     // Sử dụng địa chỉ Solana devnet
    //     let balance = provider.get_account_balance("5U3bH5b6XtG99aVmwNDTyaqbpDnHrZEuGwMxhKnj5SJY").await.unwrap();
    //     println!("Balance: {}", balance);
    // }
} 