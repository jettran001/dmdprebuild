//! Module cho Tron blockchain provider
//!
//! Module này cung cấp các chức năng để tương tác với mạng Tron,
//! bao gồm các chức năng như truy vấn số dư, gửi giao dịch, và quản lý tài khoản.
//!
//! ## Ví dụ:
//! ```rust
//! use wallet::defi::blockchain::non_evm::tron::{TronProvider, TronConfig};
//! use wallet::defi::blockchain::ChainId;
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = TronConfig::new(ChainId::TronMainnet);
//!     let provider = TronProvider::new(config).unwrap();
//!
//!     let balance = provider.get_balance("TTSFjEG3Lu9WkHdp4JrWYhbGP6K1REqnGQ").await.unwrap();
//!     println!("Balance: {}", balance);
//! }
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};
use std::collections::HashMap;
use once_cell::sync::Lazy;
use tokio::sync::{Mutex, RwLock};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use ethers::types::U256;
use tracing::{debug, info, warn, error};
use reqwest::{Client, ClientBuilder};
use serde_json::{json, Value};
use regex::Regex;
use base58::{FromBase58, ToBase58};
use sha2::{Sha256, Digest};
use hex;

use crate::defi::blockchain::{
    BlockchainProvider, BlockchainType, BlockchainConfig, ChainId
};
use crate::defi::error::DefiError;
use crate::defi::constants::BLOCKCHAIN_TIMEOUT;

/// Cache cho các providers
static TRON_PROVIDER_CACHE: Lazy<RwLock<HashMap<ChainId, Arc<TronProvider>>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

/// Cấu hình cho Tron Provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TronConfig {
    /// Chain ID
    pub chain_id: ChainId,
    /// Full node API URL
    pub full_node_url: String,
    /// Solidity node API URL (cho hợp đồng thông minh)
    pub solidity_node_url: String,
    /// Event server URL
    pub event_server_url: Option<String>,
    /// Timeout (ms)
    pub timeout_ms: u64,
    /// Số lần retry tối đa
    pub max_retries: u32,
}

impl TronConfig {
    /// Tạo cấu hình mới dựa trên chain
    pub fn new(chain_id: ChainId) -> Self {
        match chain_id {
            ChainId::TronMainnet => Self {
                chain_id,
                full_node_url: "https://api.trongrid.io".to_string(),
                solidity_node_url: "https://api.trongrid.io".to_string(),
                event_server_url: Some("https://api.trongrid.io".to_string()),
                timeout_ms: 30000, // 30 giây
                max_retries: 3,
            },
            ChainId::TronNile => Self {
                chain_id,
                full_node_url: "https://api.nileex.io".to_string(),
                solidity_node_url: "https://api.nileex.io".to_string(),
                event_server_url: Some("https://api.nileex.io".to_string()),
                timeout_ms: 30000,
                max_retries: 3,
            },
            ChainId::TronShasta => Self {
                chain_id,
                full_node_url: "https://api.shasta.trongrid.io".to_string(),
                solidity_node_url: "https://api.shasta.trongrid.io".to_string(),
                event_server_url: Some("https://api.shasta.trongrid.io".to_string()),
                timeout_ms: 30000,
                max_retries: 3,
            },
            _ => panic!("Unsupported Tron chain ID"),
        }
    }
    
    /// Cập nhật URL cho Full Node API
    pub fn with_full_node_url(mut self, url: &str) -> Self {
        self.full_node_url = url.to_string();
        self
    }
    
    /// Cập nhật URL cho Solidity Node API
    pub fn with_solidity_node_url(mut self, url: &str) -> Self {
        self.solidity_node_url = url.to_string();
        self
    }
    
    /// Cập nhật URL cho Event Server API
    pub fn with_event_server_url(mut self, url: Option<&str>) -> Self {
        self.event_server_url = url.map(|s| s.to_string());
        self
    }
    
    /// Cập nhật timeout
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }
    
    /// Cập nhật số lần retry tối đa
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }
}

/// Kết quả cache
#[derive(Clone, Debug)]
struct CacheEntry<T> {
    value: T,
    timestamp: Instant,
    ttl_seconds: u64,
}

/// Tron blockchain provider
pub struct TronProvider {
    /// Cấu hình
    pub config: TronConfig,
    /// HTTP Client cho Full Node API
    full_node_client: Client,
    /// HTTP Client cho Solidity Node API
    solidity_node_client: Client,
    /// HTTP Client cho Event Server API (nếu có)
    event_server_client: Option<Client>,
    /// Cache cho các kết quả API 
    cache: RwLock<HashMap<String, CacheEntry<Value>>>,
    /// Lock cho các hoạt động quan trọng
    operation_lock: Mutex<()>,
    /// Thời gian TTL mặc định cho cache (giây)
    default_cache_ttl: u64,
}

impl TronProvider {
    /// Tạo provider mới
    pub fn new(config: TronConfig) -> Result<Self, DefiError> {
        let full_node_client = ClientBuilder::new()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|e| DefiError::ProviderError(format!("Failed to create Tron Full Node client: {}", e)))?;
            
        let solidity_node_client = ClientBuilder::new()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|e| DefiError::ProviderError(format!("Failed to create Tron Solidity Node client: {}", e)))?;
            
        let event_server_client = if config.event_server_url.is_some() {
            Some(ClientBuilder::new()
                .timeout(Duration::from_millis(config.timeout_ms))
                .build()
                .map_err(|e| DefiError::ProviderError(format!("Failed to create Tron Event Server client: {}", e)))?)
        } else {
            None
        };
        
        Ok(Self {
            config,
            full_node_client,
            solidity_node_client,
            event_server_client,
            cache: RwLock::new(HashMap::new()),
            operation_lock: Mutex::new(()),
            default_cache_ttl: 30, // 30 giây mặc định
        })
    }
    
    /// Lấy provider cho chain cụ thể
    pub async fn get_provider(chain_id: ChainId) -> Result<Arc<Self>, DefiError> {
        match chain_id {
            ChainId::TronMainnet | ChainId::TronNile | ChainId::TronShasta => {},
            _ => return Err(DefiError::ChainNotSupported(format!("Expected Tron chains, got {:?}", chain_id))),
        }
        
        // Thử lấy từ cache trước
        {
            let cache = TRON_PROVIDER_CACHE.read().await;
            if let Some(provider) = cache.get(&chain_id) {
                return Ok(provider.clone());
            }
        }
        
        // Nếu không có trong cache, tạo mới
        let config = TronConfig::new(chain_id);
        let provider = Self::new(config)?;
        let provider = Arc::new(provider);
        
        // Lưu vào cache
        {
            let mut cache = TRON_PROVIDER_CACHE.write().await;
            cache.insert(chain_id, provider.clone());
        }
        
        Ok(provider)
    }
    
    /// Thiết lập TTL mặc định cho cache
    pub fn set_default_cache_ttl(&mut self, ttl_seconds: u64) {
        self.default_cache_ttl = ttl_seconds;
    }
    
    /// Xóa cache theo key
    pub async fn clear_cache_item(&self, cache_key: &str) {
        let mut cache = self.cache.write().await;
        cache.remove(cache_key);
    }
    
    /// Xóa toàn bộ cache
    pub async fn clear_all_cache(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
    
    /// Dọn dẹp các mục cache hết hạn
    pub async fn cleanup_cache(&self) {
        let mut cache = self.cache.write().await;
        cache.retain(|_, entry| {
            entry.timestamp.elapsed().as_secs() < entry.ttl_seconds
        });
    }
    
    /// Gọi Full Node API với cơ chế caching
    async fn call_full_node_api<T: for<'de> Deserialize<'de>>(
        &self,
        endpoint: &str,
        payload: Option<Value>,
    ) -> Result<T, DefiError> {
        // Tạo cache key
        let cache_key = match &payload {
            Some(p) => format!("full_node_{}{}", endpoint, p.to_string()),
            None => format!("full_node_{}", endpoint),
        };
        
        // Kiểm tra cache
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(&cache_key) {
                if entry.timestamp.elapsed().as_secs() < entry.ttl_seconds {
                    return serde_json::from_value(entry.value.clone())
                        .map_err(|e| DefiError::ProviderError(format!("Failed to deserialize cached value: {}", e)));
                }
            }
        }
        
        // Không có trong cache hoặc đã hết hạn, gọi API
        let url = format!("{}{}", self.config.full_node_url, endpoint);
        let response = match payload {
            Some(json_data) => {
                self.full_node_client
                    .post(&url)
                    .json(&json_data)
                    .send()
                    .await
            },
            None => {
                self.full_node_client
                    .get(&url)
                    .send()
                    .await
            }
        };
        
        let response = response.map_err(|e| {
            DefiError::ProviderError(format!("Failed to call Tron Full Node API: {}", e))
        })?;
        
        let status = response.status();
        if !status.is_success() {
            return Err(DefiError::ProviderError(
                format!("Tron Full Node API returned error status: {}", status)
            ));
        }
        
        let result: Value = response.json().await.map_err(|e| {
            DefiError::ProviderError(format!("Failed to parse Tron Full Node API response: {}", e))
        })?;
        
        // Lưu vào cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(cache_key, CacheEntry {
                value: result.clone(),
                timestamp: Instant::now(),
                ttl_seconds: self.default_cache_ttl,
            });
        }
        
        // Parse kết quả
        serde_json::from_value(result).map_err(|e| {
            DefiError::ProviderError(format!("Failed to deserialize Tron Full Node API result: {}", e))
        })
    }
    
    /// Gọi Solidity Node API với cơ chế caching
    async fn call_solidity_node_api<T: for<'de> Deserialize<'de>>(
        &self,
        endpoint: &str,
        payload: Option<Value>,
    ) -> Result<T, DefiError> {
        // Tạo cache key
        let cache_key = match &payload {
            Some(p) => format!("solidity_node_{}{}", endpoint, p.to_string()),
            None => format!("solidity_node_{}", endpoint),
        };
        
        // Kiểm tra cache
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(&cache_key) {
                if entry.timestamp.elapsed().as_secs() < entry.ttl_seconds {
                    return serde_json::from_value(entry.value.clone())
                        .map_err(|e| DefiError::ProviderError(format!("Failed to deserialize cached value: {}", e)));
                }
            }
        }
        
        // Cơ chế lock để tránh quá nhiều request đồng thời
        let _guard = self.operation_lock.lock().await;
        
        // Không có trong cache hoặc đã hết hạn, gọi API
        let url = format!("{}{}", self.config.solidity_node_url, endpoint);
        let response = match payload {
            Some(json_data) => {
                self.solidity_node_client
                    .post(&url)
                    .json(&json_data)
                    .send()
                    .await
            },
            None => {
                self.solidity_node_client
                    .get(&url)
                    .send()
                    .await
            }
        };
        
        let response = response.map_err(|e| {
            DefiError::ProviderError(format!("Failed to call Tron Solidity Node API: {}", e))
        })?;
        
        let status = response.status();
        if !status.is_success() {
            return Err(DefiError::ProviderError(
                format!("Tron Solidity Node API returned error status: {}", status)
            ));
        }
        
        let result: Value = response.json().await.map_err(|e| {
            DefiError::ProviderError(format!("Failed to parse Tron Solidity Node API response: {}", e))
        })?;
        
        // Lưu vào cache với TTL dài hơn cho dữ liệu Solidity Node
        // (thường là dữ liệu như ABI, bytecode ít thay đổi)
        {
            let mut cache = self.cache.write().await;
            cache.insert(cache_key, CacheEntry {
                value: result.clone(),
                timestamp: Instant::now(),
                ttl_seconds: self.default_cache_ttl * 2, // TTL dài hơn
            });
        }
        
        // Parse kết quả
        serde_json::from_value(result).map_err(|e| {
            DefiError::ProviderError(format!("Failed to deserialize Tron Solidity Node API result: {}", e))
        })
    }
    
    /// Gọi Event Server API với cơ chế caching
    async fn call_event_server_api<T: for<'de> Deserialize<'de>>(
        &self,
        endpoint: &str,
        payload: Option<Value>,
    ) -> Result<T, DefiError> {
        // Không có event server URL
        let event_server_url = match &self.config.event_server_url {
            Some(url) => url,
            None => return Err(DefiError::ProviderError(
                "Event server URL not configured".to_string()
            )),
        };
        
        // Không có event server client
        let event_server_client = match &self.event_server_client {
            Some(client) => client,
            None => return Err(DefiError::ProviderError(
                "Event server client not initialized".to_string()
            )),
        };
        
        // Tạo cache key
        let cache_key = match &payload {
            Some(p) => format!("event_server_{}{}", endpoint, p.to_string()),
            None => format!("event_server_{}", endpoint),
        };
        
        // Kiểm tra cache
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(&cache_key) {
                if entry.timestamp.elapsed().as_secs() < entry.ttl_seconds {
                    return serde_json::from_value(entry.value.clone())
                        .map_err(|e| DefiError::ProviderError(format!("Failed to deserialize cached value: {}", e)));
                }
            }
        }
        
        // Không có trong cache hoặc đã hết hạn, gọi API
        let url = format!("{}{}", event_server_url, endpoint);
        let response = match payload {
            Some(json_data) => {
                event_server_client
                    .post(&url)
                    .json(&json_data)
                    .send()
                    .await
            },
            None => {
                event_server_client
                    .get(&url)
                    .send()
                    .await
            }
        };
        
        let response = response.map_err(|e| {
            DefiError::ProviderError(format!("Failed to call Tron Event Server API: {}", e))
        })?;
        
        let status = response.status();
        if !status.is_success() {
            return Err(DefiError::ProviderError(
                format!("Tron Event Server API returned error status: {}", status)
            ));
        }
        
        let result: Value = response.json().await.map_err(|e| {
            DefiError::ProviderError(format!("Failed to parse Tron Event Server API response: {}", e))
        })?;
        
        // Lưu vào cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(cache_key, CacheEntry {
                value: result.clone(),
                timestamp: Instant::now(),
                // Events thường thay đổi nhanh hơn, TTL ngắn hơn
                ttl_seconds: self.default_cache_ttl / 2,
            });
        }
        
        // Parse kết quả
        serde_json::from_value(result).map_err(|e| {
            DefiError::ProviderError(format!("Failed to deserialize Tron Event Server API result: {}", e))
        })
    }

    /// Kiểm tra địa chỉ Tron có hợp lệ không
    pub fn validate_address(&self, address: &str) -> bool {
        // Kiểm tra địa chỉ base58
        if !address.starts_with('T') || address.len() != 34 {
            return false;
        }

        // Giải mã địa chỉ base58
        let decoded = match address.from_base58() {
            Ok(bytes) => bytes,
            Err(_) => return false,
        };

        // Địa chỉ Tron sau khi giải mã phải có đúng 25 bytes (1 byte version + 20 bytes address + 4 bytes checksum)
        if decoded.len() != 25 {
            return false;
        }

        // Địa chỉ Tron mainnet phải bắt đầu bằng 0x41
        if decoded[0] != 0x41 {
            return false;
        }

        // Kiểm tra checksum
        let mut hasher = Sha256::new();
        hasher.update(&decoded[0..21]);
        let hash1 = hasher.finalize();
        
        let mut hasher = Sha256::new();
        hasher.update(&hash1);
        let hash2 = hasher.finalize();
        
        for i in 0..4 {
            if decoded[21 + i] != hash2[i] {
                return false;
            }
        }

        true
    }

    /// Chuyển đổi địa chỉ Tron sang dạng hex
    pub fn address_to_hex(&self, address: &str) -> Result<String, DefiError> {
        if !self.validate_address(address) {
            return Err(DefiError::ProviderError(format!("Invalid Tron address: {}", address)));
        }

        let decoded = address.from_base58()
            .map_err(|e| DefiError::ProviderError(format!("Failed to decode Base58 address: {}", e)))?;

        // Lấy 21 bytes đầu tiên (1 byte version + 20 bytes address)
        let hex_address = hex::encode(&decoded[0..21]);
        Ok(format!("0x{}", hex_address))
    }

    /// Chuyển đổi địa chỉ hex sang địa chỉ Tron
    pub fn hex_to_address(&self, hex_str: &str) -> Result<String, DefiError> {
        let hex_str = hex_str.trim_start_matches("0x");
        
        // Kiểm tra độ dài (41 cho địa chỉ Tron hex gồm: 0x + version + address)
        if hex_str.len() != 42 && hex_str.len() != 40 {
            return Err(DefiError::ProviderError(format!("Invalid Tron hex address length: {}", hex_str.len())));
        }
        
        let bytes = hex::decode(hex_str)
            .map_err(|e| DefiError::ProviderError(format!("Failed to decode hex address: {}", e)))?;
        
        // Kiểm tra version byte (0x41 cho mainnet)
        if bytes[0] != 0x41 {
            return Err(DefiError::ProviderError(format!("Invalid Tron address version: {:x}", bytes[0])));
        }
        
        // Tạo checksum
        let mut hasher = Sha256::new();
        hasher.update(&bytes[0..21]);
        let hash1 = hasher.finalize();
        
        let mut hasher = Sha256::new();
        hasher.update(&hash1);
        let hash2 = hasher.finalize();
        
        // Tạo địa chỉ gồm 25 bytes (21 bytes ban đầu + 4 bytes checksum)
        let mut address_bytes = bytes[0..21].to_vec();
        address_bytes.extend_from_slice(&hash2[0..4]);
        
        // Mã hóa base58
        let address = address_bytes.to_base58();
        Ok(address)
    }

    /// Lấy số dư tài khoản
    pub async fn get_account_balance(&self, address: &str) -> Result<U256, DefiError> {
        if !self.validate_address(address) {
            return Err(DefiError::ProviderError(format!("Invalid Tron address: {}", address)));
        }

        let endpoint = "/wallet/getaccount";
        let payload = json!({
            "address": address,
            "visible": true
        });

        let response: Value = self.call_full_node_api(endpoint, Some(payload)).await?;
        
        // Kiểm tra kết quả
        if response["balance"].is_null() {
            // Tài khoản không tồn tại hoặc số dư bằng 0
            return Ok(U256::zero());
        }

        // Lấy số dư TRX (đơn vị: sun, 1 TRX = 1,000,000 sun)
        let balance = response["balance"]
            .as_u64()
            .ok_or_else(|| DefiError::ProviderError(format!("Failed to parse balance for address: {}", address)))?;

        Ok(U256::from(balance))
    }

    /// Lấy thông tin về token TRC20
    pub async fn get_trc20_balance(&self, address: &str, contract_address: &str) -> Result<U256, DefiError> {
        if !self.validate_address(address) || !self.validate_address(contract_address) {
            return Err(DefiError::ProviderError(format!("Invalid Tron address")));
        }

        // Chuyển đổi địa chỉ sang hex
        let address_hex = self.address_to_hex(address)?;
        let address_hex = address_hex.trim_start_matches("0x");
        
        // Đệm địa chỉ đến 64 ký tự
        let address_param = format!("{:0>64}", address_hex.trim_start_matches("41"));

        // Gọi hàm balanceOf trên smart contract
        let endpoint = "/wallet/triggerconstantcontract";
        let payload = json!({
            "owner_address": address,
            "contract_address": contract_address,
            "function_selector": "balanceOf(address)",
            "parameter": address_param,
            "visible": true
        });

        let response: Value = self.call_full_node_api(endpoint, Some(payload)).await?;
        
        // Kiểm tra kết quả
        if let Some(constant_result) = response["constant_result"].as_array() {
            if let Some(result) = constant_result.first() {
                if let Some(hex_result) = result.as_str() {
                    // Chuyển đổi kết quả hex thành số
                    if hex_result.len() >= 2 {
                        if let Ok(bytes) = hex::decode(hex_result) {
                            let mut amount = U256::zero();
                            for byte in bytes {
                                amount = amount << 8;
                                amount = amount + U256::from(byte);
                            }
                            return Ok(amount);
                        }
                    }
                }
            }
        }

        // Mặc định trả về 0 nếu không lấy được
        Ok(U256::zero())
    }

    /// Lấy thông tin token TRC10
    pub async fn get_trc10_balance(&self, address: &str, token_id: &str) -> Result<U256, DefiError> {
        if !self.validate_address(address) {
            return Err(DefiError::ProviderError(format!("Invalid Tron address: {}", address)));
        }

        let endpoint = "/wallet/getaccount";
        let payload = json!({
            "address": address,
            "visible": true
        });

        let response: Value = self.call_full_node_api(endpoint, Some(payload)).await?;
        
        // Nếu không có danh sách tài sản
        if response["assetV2"].is_null() {
            return Ok(U256::zero());
        }

        // Tìm token trong danh sách
        if let Some(assets) = response["assetV2"].as_array() {
            for asset in assets {
                if let (Some(key), Some(value)) = (asset["key"].as_str(), asset["value"].as_u64()) {
                    if key == token_id {
                        return Ok(U256::from(value));
                    }
                }
            }
        }

        // Mặc định trả về 0 nếu không tìm thấy token
        Ok(U256::zero())
    }

    /// Lấy danh sách token TRC10 của tài khoản
    pub async fn get_all_trc10_balances(&self, address: &str) -> Result<HashMap<String, U256>, DefiError> {
        if !self.validate_address(address) {
            return Err(DefiError::ProviderError(format!("Invalid Tron address: {}", address)));
        }

        let endpoint = "/wallet/getaccount";
        let payload = json!({
            "address": address,
            "visible": true
        });

        let response: Value = self.call_full_node_api(endpoint, Some(payload)).await?;
        
        let mut balances = HashMap::new();
        
        // Thêm TRX balance nếu có
        if let Some(trx_balance) = response["balance"].as_u64() {
            balances.insert("TRX".to_string(), U256::from(trx_balance));
        }
        
        // Thêm các TRC10 token balances
        if let Some(assets) = response["assetV2"].as_array() {
            for asset in assets {
                if let (Some(key), Some(value)) = (asset["key"].as_str(), asset["value"].as_u64()) {
                    balances.insert(key.to_string(), U256::from(value));
                }
            }
        }

        Ok(balances)
    }

    /// Lấy số block hiện tại
    pub async fn get_current_block(&self) -> Result<u64, DefiError> {
        let endpoint = "/wallet/getnowblock";
        let response: Value = self.call_full_node_api(endpoint, None).await?;
        
        let block_number = response["block_header"]["raw_data"]["number"]
            .as_u64()
            .ok_or_else(|| DefiError::ProviderError("Failed to parse block number".to_string()))?;

        Ok(block_number)
    }

    /// Lấy thông tin giao dịch từ transaction ID
    pub async fn get_transaction(&self, tx_id: &str) -> Result<Value, DefiError> {
        let endpoint = "/wallet/gettransactionbyid";
        let payload = json!({
            "value": tx_id,
            "visible": true
        });

        self.call_full_node_api(endpoint, Some(payload)).await
    }

    /// Lấy thông tin về smart contract
    pub async fn get_contract(&self, contract_address: &str) -> Result<Value, DefiError> {
        if !self.validate_address(contract_address) {
            return Err(DefiError::ProviderError(format!("Invalid Tron contract address: {}", contract_address)));
        }

        let endpoint = "/wallet/getcontract";
        let payload = json!({
            "value": contract_address,
            "visible": true
        });

        self.call_full_node_api(endpoint, Some(payload)).await
    }

    /// Lấy thông tin tài nguyên (băng thông, năng lượng) của tài khoản
    pub async fn get_account_resources(&self, address: &str) -> Result<Value, DefiError> {
        if !self.validate_address(address) {
            return Err(DefiError::ProviderError(format!("Invalid Tron address: {}", address)));
        }

        let endpoint = "/wallet/getaccountresource";
        let payload = json!({
            "address": address,
            "visible": true
        });

        self.call_full_node_api(endpoint, Some(payload)).await
    }
}

#[async_trait]
impl BlockchainProvider for TronProvider {
    fn chain_id(&self) -> ChainId {
        self.config.chain_id
    }
    
    fn blockchain_type(&self) -> BlockchainType {
        BlockchainType::Tron
    }
    
    async fn is_connected(&self) -> Result<bool, DefiError> {
        // Kiểm tra kết nối bằng cách lấy block hiện tại
        match self.get_current_block().await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
    
    async fn get_block_number(&self) -> Result<u64, DefiError> {
        self.get_current_block().await
    }
    
    async fn get_balance(&self, address: &str) -> Result<U256, DefiError> {
        self.get_account_balance(address).await
    }
}

/// Tạo Tron provider
pub async fn create_tron_provider(config: BlockchainConfig) -> Result<Box<dyn BlockchainProvider>, DefiError> {
    // Kiểm tra chain ID có phải là Tron chain không
    match config.chain_id {
        ChainId::TronMainnet | ChainId::TronShasta | ChainId::TronNile => {},
        _ => return Err(DefiError::ChainNotSupported(format!(
            "Expected Tron chains, got {:?}", 
            config.chain_id
        ))),
    }
    
    // Tạo cấu hình mặc định cho chain
    let mut tron_config = TronConfig::new(config.chain_id);
    
    // Cập nhật Full Node URL nếu được cung cấp
    if !config.rpc_url.is_empty() {
        tron_config = tron_config.with_full_node_url(&config.rpc_url);
        // Nếu chỉ cung cấp một URL, thì dùng URL đó cho cả solidity node
        tron_config = tron_config.with_solidity_node_url(&config.rpc_url);
    }
    
    // Cập nhật các tham số khác
    tron_config = tron_config
        .with_timeout(config.timeout_ms)
        .with_max_retries(config.max_retries);
    
    let provider = TronProvider::new(tron_config)?;
    Ok(Box::new(provider))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_tron_config() {
        let config = TronConfig::new(ChainId::TronMainnet);
        assert_eq!(config.chain_id, ChainId::TronMainnet);
        assert!(config.full_node_url.contains("trongrid.io"));
        assert!(config.solidity_node_url.contains("trongrid.io"));
    }
    
    #[test]
    fn test_tron_config_builder() {
        let config = TronConfig::new(ChainId::TronShasta)
            .with_full_node_url("https://custom-api.shasta.trongrid.io")
            .with_solidity_node_url("https://custom-solidity.shasta.trongrid.io")
            .with_event_server_url(Some("https://custom-event.shasta.trongrid.io"))
            .with_timeout(5000)
            .with_max_retries(5);
            
        assert_eq!(config.chain_id, ChainId::TronShasta);
        assert_eq!(config.full_node_url, "https://custom-api.shasta.trongrid.io");
        assert_eq!(config.solidity_node_url, "https://custom-solidity.shasta.trongrid.io");
        assert_eq!(config.event_server_url, Some("https://custom-event.shasta.trongrid.io".to_string()));
        assert_eq!(config.timeout_ms, 5000);
        assert_eq!(config.max_retries, 5);
    }
    
    #[test]
    fn test_validate_address() {
        let provider = TronProvider::new(TronConfig::new(ChainId::TronMainnet)).unwrap();
        
        // Các địa chỉ hợp lệ
        assert!(provider.validate_address("TTSFjEG3Lu9WkHdp4JrWYhbGP6K1REqnGQ"));
        assert!(provider.validate_address("TZ7A4upCEjn7c5SHTepnxHM4VPDYYxDaGS"));
        
        // Các địa chỉ không hợp lệ
        assert!(!provider.validate_address("TTSFjEG3Lu9WkHdp4JrWYhbGP6K1REqn")); // Thiếu ký tự
        assert!(!provider.validate_address("BTSFjEG3Lu9WkHdp4JrWYhbGP6K1REqnGQ")); // Không bắt đầu bằng T
        assert!(!provider.validate_address("0x71C7656EC7ab88b098defB751B7401B5f6d8976F")); // Địa chỉ Ethereum
    }
    
    #[test]
    fn test_address_conversion() {
        let provider = TronProvider::new(TronConfig::new(ChainId::TronMainnet)).unwrap();
        
        // Kiểm tra chuyển đổi địa chỉ <-> hex
        let address = "TTSFjEG3Lu9WkHdp4JrWYhbGP6K1REqnGQ";
        let hex_address = provider.address_to_hex(address).unwrap();
        let converted_address = provider.hex_to_address(&hex_address).unwrap();
        
        assert_eq!(address, converted_address);
    }
    
    // Integration tests would require actual Tron network connection
    // #[tokio::test]
    // async fn test_tron_provider_integration() {
    //     let config = TronConfig::new(ChainId::TronShasta);
    //     let provider = TronProvider::new(config).unwrap();
    //     
    //     let is_connected = provider.is_connected().await.unwrap();
    //     assert!(is_connected);
    //     
    //     // Sử dụng địa chỉ Tron testnet
    //     let balance = provider.get_account_balance("TJRabPrwbZy45sbavfcjinPJC18kjpRs").await.unwrap();
    //     println!("Balance: {}", balance);
    // }
} 