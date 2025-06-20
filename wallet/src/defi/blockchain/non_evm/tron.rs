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
use rand;

use crate::defi::blockchain::{
    BlockchainProvider, BlockchainType, BlockchainConfig, ChainId
};
use crate::defi::error::DefiError;
use crate::defi::constants::BLOCKCHAIN_TIMEOUT;

/// Cache cho các providers
static TRON_PROVIDER_CACHE: Lazy<RwLock<HashMap<ChainId, Arc<TronProvider>>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

/// Cache structure for API responses
static API_CACHE: Lazy<RwLock<HashMap<String, (Instant, String)>>> = 
    Lazy::new(|| RwLock::new(HashMap::new()));

const CACHE_TTL: Duration = Duration::from_secs(30); // 30 seconds default TTL

/// Cấu hình cho Tron Provider
#[derive(Debug, Clone)]
pub struct TronConfig {
    /// Chain ID
    pub chain_id: ChainId,
    /// Full node API URL
    pub rpc_url: String,
    /// Timeout (ms)
    pub timeout_ms: u64,
    /// Số lần retry tối đa
    pub max_retries: u8,
}

impl Default for TronConfig {
    fn default() -> Self {
        Self {
            chain_id: ChainId::TronMainnet,
            rpc_url: "https://api.trongrid.io".to_string(),
            timeout_ms: 30000, // 30 seconds default
            max_retries: 3,
        }
    }
}

impl TronConfig {
    /// Tạo cấu hình mới dựa trên chain
    pub fn new(chain_id: ChainId) -> Self {
        match chain_id {
            ChainId::TronMainnet => Self {
                chain_id,
                rpc_url: "https://api.trongrid.io".to_string(),
                timeout_ms: 30000, // 30 giây
                max_retries: 3,
            },
            ChainId::TronNile => Self {
                chain_id,
                rpc_url: "https://api.nileex.io".to_string(),
                timeout_ms: 30000,
                max_retries: 3,
            },
            ChainId::TronShasta => Self {
                chain_id,
                rpc_url: "https://api.shasta.trongrid.io".to_string(),
                timeout_ms: 30000,
                max_retries: 3,
            },
            _ => panic!("Unsupported Tron chain ID"),
        }
    }
    
    /// Cập nhật URL cho Full Node API
    pub fn with_full_node_url(mut self, url: &str) -> Self {
        self.rpc_url = url.to_string();
        self
    }

    /// Cập nhật timeout
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Cập nhật số lần retry tối đa
    pub fn with_max_retries(mut self, max_retries: u8) -> Self {
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
    client: Arc<Client>,
    /// Thời gian cuối cùng được sử dụng
    last_used: Instant,
}

impl TronProvider {
    /// Tạo provider mới
    pub fn new(config: TronConfig) -> Result<Arc<Self>, Box<dyn std::error::Error>> {
        // Check if we already have a provider for this chain_id
        {
            let cache = TRON_PROVIDER_CACHE.read().map_err(|_| "Failed to get read lock on provider cache")?;
            if let Some(provider) = cache.get(&config.chain_id) {
                return Ok(provider.clone());
            }
        }
        
        // Create a new provider
        let client = Arc::new(Client::new());
        let provider = Arc::new(Self {
            config,
            client,
            last_used: Instant::now(),
        });
        
        // Store in cache
        {
            let mut cache = TRON_PROVIDER_CACHE.write().map_err(|_| "Failed to get write lock on provider cache")?;
            cache.insert(provider.config.chain_id, provider.clone());
        }
        
        Ok(provider)
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
        let mut cache = API_CACHE.write().await;
        cache.remove(cache_key);
    }
    
    /// Xóa toàn bộ cache
    pub async fn clear_all_cache() -> Result<(), Box<dyn std::error::Error>> {
        let mut cache = API_CACHE.write().map_err(|_| "Failed to get write lock on cache")?;
        cache.clear();
        Ok(())
    }
    
    /// Dọn dẹp các mục cache hết hạn
    pub async fn cleanup_cache() -> Result<(), Box<dyn std::error::Error>> {
        let mut cache = API_CACHE.write().map_err(|_| "Failed to get write lock on cache")?;
        let now = Instant::now();
        
        // Remove expired entries
        cache.retain(|_, (timestamp, _)| now.duration_since(*timestamp) < CACHE_TTL);
        Ok(())
    }
    
    /// Gọi Full Node API với cơ chế caching
    pub async fn call_full_node_api<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        params: serde_json::Value,
    ) -> Result<T, Box<dyn std::error::Error>> {
        // Create cache key from endpoint and params
        let cache_key = format!("full_node:{}:{}", endpoint, params.to_string());
        
        // Check cache first
        {
            let cache = API_CACHE.read().await;
            if let Some((timestamp, cached_data)) = cache.get(&cache_key) {
                if Instant::now().duration_since(*timestamp) < CACHE_TTL {
                    debug!("Cache hit for {}", cache_key);
                    return serde_json::from_str(cached_data)
                        .map_err(|e| format!("Failed to deserialize cached data: {}", e).into());
                }
            }
        }
        
        // Cache miss or expired, fetch new data
        debug!("Cache miss for {}, fetching from API", cache_key);
        
        let url = format!("{}{}", self.config.rpc_url, endpoint);
        let mut retry_count = 0;
        let max_retries = self.config.max_retries;
        
        loop {
            match self.post_request(&url, params.clone()).await {
                Ok(response) => {
                    // Store in cache
                    {
                        let mut cache = API_CACHE.write().await;
                        cache.insert(cache_key, (Instant::now(), response.clone()));
                    }
                    
                    return serde_json::from_str(&response)
                        .map_err(|e| format!("Failed to deserialize response: {}", e).into());
                }
                Err(e) => {
                    retry_count += 1;
                    if retry_count >= max_retries {
                        return Err(format!("Max retries exceeded: {}", e).into());
                    }
                    
                    let backoff = std::cmp::min(100 * (2_u64.pow(retry_count as u32)), 2000);
                    tokio::time::sleep(Duration::from_millis(backoff)).await;
                }
            }
        }
    }
    
    /// Gọi Solidity Node API với cơ chế caching
    pub async fn call_solidity_node_api<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        params: serde_json::Value,
    ) -> Result<T, Box<dyn std::error::Error>> {
        // Similar implementation with caching
        let cache_key = format!("solidity_node:{}:{}", endpoint, params.to_string());
        
        // Check cache first
        {
            let cache = API_CACHE.read().await;
            if let Some((timestamp, cached_data)) = cache.get(&cache_key) {
                if Instant::now().duration_since(*timestamp) < CACHE_TTL {
                    debug!("Cache hit for {}", cache_key);
                    return serde_json::from_str(cached_data)
                        .map_err(|e| format!("Failed to deserialize cached data: {}", e).into());
                }
            }
        }
        
        // Cache miss or expired, fetch new data
        debug!("Cache miss for {}, fetching from API", cache_key);
        
        let url = format!("{}{}", self.config.rpc_url, endpoint);
        let mut retry_count = 0;
        let max_retries = self.config.max_retries;
        
        loop {
            match self.post_request(&url, params.clone()).await {
                Ok(response) => {
                    // Store in cache
                    {
                        let mut cache = API_CACHE.write().await;
                        cache.insert(cache_key, (Instant::now(), response.clone()));
                    }
                    
                    return serde_json::from_str(&response)
                        .map_err(|e| format!("Failed to deserialize response: {}", e).into());
                }
                Err(e) => {
                    retry_count += 1;
                    if retry_count >= max_retries {
                        return Err(format!("Max retries exceeded: {}", e).into());
                    }
                    
                    let backoff = std::cmp::min(100 * (2_u64.pow(retry_count as u32)), 2000);
                    tokio::time::sleep(Duration::from_millis(backoff)).await;
                }
            }
        }
    }
    
    /// Gọi Event Server API với cơ chế caching
    pub async fn call_event_server_api<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        params: serde_json::Value,
    ) -> Result<T, Box<dyn std::error::Error>> {
        // Similar implementation with caching
        let cache_key = format!("event_server:{}:{}", endpoint, params.to_string());
        
        // Check cache first
        {
            let cache = API_CACHE.read().await;
            if let Some((timestamp, cached_data)) = cache.get(&cache_key) {
                if Instant::now().duration_since(*timestamp) < CACHE_TTL {
                    debug!("Cache hit for {}", cache_key);
                    return serde_json::from_str(cached_data)
                        .map_err(|e| format!("Failed to deserialize cached data: {}", e).into());
                }
            }
        }
        
        // Cache miss or expired, fetch new data
        debug!("Cache miss for {}, fetching from API", cache_key);
        
        let url = format!("{}{}", self.config.rpc_url, endpoint);
        let mut retry_count = 0;
        let max_retries = self.config.max_retries;
        
        loop {
            match self.post_request(&url, params.clone()).await {
                Ok(response) => {
                    // Store in cache
                    {
                        let mut cache = API_CACHE.write().await;
                        cache.insert(cache_key, (Instant::now(), response.clone()));
                    }
                    
                    return serde_json::from_str(&response)
                        .map_err(|e| format!("Failed to deserialize response: {}", e).into());
                }
                Err(e) => {
                    retry_count += 1;
                    if retry_count >= max_retries {
                        return Err(format!("Max retries exceeded: {}", e).into());
                    }
                    
                    let backoff = std::cmp::min(100 * (2_u64.pow(retry_count as u32)), 2000);
                    tokio::time::sleep(Duration::from_millis(backoff)).await;
                }
            }
        }
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
        const MAX_RETRIES: u8 = 3;
        let mut last_error = None;
        
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                // Exponential backoff với jitter
                let backoff_ms = 100 * u64::pow(2, attempt as u32);
                let jitter = rand::random::<u64>() % 100;
                let delay = backoff_ms + jitter;
                
                debug!("Thử lại lấy current block lần {}/{} sau {}ms", attempt, MAX_RETRIES, delay);
                tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
            }
            
            // Thực hiện API call
            let endpoint = "/wallet/getnowblock";
            
            match self.call_full_node_api::<Value>(endpoint, serde_json::Value::Null).await {
                Ok(response) => {
                    // Parse block number từ response
                    match response["block_header"]["raw_data"]["number"].as_u64() {
                        Some(block_number) => {
                            if attempt > 0 {
                                info!("Lấy current block thành công sau {} lần thử: {}", attempt + 1, block_number);
                            } else {
                                debug!("Lấy current block thành công: {}", block_number);
                            }
                            return Ok(block_number);
                        },
                        None => {
                            let error = DefiError::ProviderError("Không thể parse block number từ response".to_string());
                            error!("Lỗi khi parse block number: {:?}", response);
                            last_error = Some(error);
                            
                            if attempt == MAX_RETRIES {
                                error!("Đã hết số lần thử khi parse block number");
                                return Err(DefiError::ProviderError(
                                    "Không thể parse block number từ response sau nhiều lần thử".to_string()
                                ));
                            }
                        }
                    }
                },
                Err(e) => {
                    warn!("Lỗi khi gọi API lần {}/{}: {}", attempt + 1, MAX_RETRIES, e);
                    last_error = Some(DefiError::ProviderError(format!("API error: {}", e)));
                    
                    // Nếu đã hết số lần thử
                    if attempt == MAX_RETRIES {
                        error!("Đã hết số lần thử gọi API để lấy current block");
                        return Err(DefiError::ProviderError(format!(
                            "Không thể lấy current block sau {} lần thử: {}", 
                            MAX_RETRIES + 1, 
                            last_error.as_ref().map_or_else(|| "Unknown error".to_string(), |e| e.to_string())
                        )));
                    }
                }
            }
        }
        
        // Không thể xảy ra do điều kiện return ở trên, nhưng compiler cần điều này
        Err(DefiError::ProviderError("Không thể lấy current block".to_string()))
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

    /// Gửi HTTP POST request
    async fn post_request(&self, url: &str, params: serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();
        let response = client
            .post(url)
            .timeout(Duration::from_millis(self.config.timeout_ms))
            .header("Content-Type", "application/json")
            .json(&params)
            .send()
            .await?
            .text()
            .await?;
            
        Ok(response)
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
        const MAX_RETRIES: u8 = 3;
        let mut last_error = None;
        
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                debug!("Thử lại lấy block number lần {}/{} cho Tron blockchain", attempt, MAX_RETRIES);
                tokio::time::sleep(tokio::time::Duration::from_millis(500 * attempt as u64)).await;
            }
            
            match self.get_current_block().await {
                Ok(block_number) => {
                    if attempt > 0 {
                        info!("Lấy block number thành công sau {} lần thử", attempt + 1);
                    }
                    return Ok(block_number);
                },
                Err(e) => {
                    warn!("Lỗi khi lấy block number lần {}/{}: {}", attempt + 1, MAX_RETRIES, e);
                    last_error = Some(e);
                    
                    // Nếu đã hết số lần thử
                    if attempt == MAX_RETRIES {
                        error!("Đã hết số lần thử lấy block number: attempts={}", MAX_RETRIES + 1);
                        return Err(DefiError::ProviderError(format!(
                            "Không thể lấy block number sau {} lần thử: {}", 
                            MAX_RETRIES + 1, 
                            last_error.as_ref().map_or_else(|| "Unknown error".to_string(), |e| e.to_string())
                        )));
                    }
                }
            }
        }
        
        // Không thể xảy ra do điều kiện return ở trên, nhưng compiler cần điều này
        Err(DefiError::ProviderError("Không thể lấy block number".to_string()))
    }
    
    async fn get_balance(&self, address: &str) -> Result<U256, DefiError> {
        // Validate địa chỉ Tron
        if !self.validate_address(address) {
            error!("Địa chỉ Tron không hợp lệ: {}", address);
            return Err(DefiError::InvalidAddress(format!("Địa chỉ Tron không hợp lệ: {}", address)));
        }
        
        const MAX_RETRIES: u8 = 3;
        let mut last_error = None;
        
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                debug!("Thử lại lấy balance lần {}/{} cho địa chỉ Tron: {}", attempt, MAX_RETRIES, address);
                tokio::time::sleep(tokio::time::Duration::from_millis(500 * attempt as u64)).await;
            }
            
            match self.get_account_balance(address).await {
                Ok(balance) => {
                    if attempt > 0 {
                        info!("Lấy balance thành công sau {} lần thử cho địa chỉ: {}", attempt + 1, address);
                    }
                    return Ok(balance);
                },
                Err(e) => {
                    warn!("Lỗi khi lấy balance lần {}/{} cho địa chỉ {}: {}", attempt + 1, MAX_RETRIES, address, e);
                    last_error = Some(e);
                    
                    // Kiểm tra lỗi không thể retry
                    if let DefiError::InvalidAddress(_) = e {
                        error!("Địa chỉ không hợp lệ, không thể retry: {}", address);
                        return Err(e);
                    }
                    
                    // Nếu đã hết số lần thử
                    if attempt == MAX_RETRIES {
                        error!("Đã hết số lần thử lấy balance cho địa chỉ {}: attempts={}", address, MAX_RETRIES + 1);
                        return Err(DefiError::ProviderError(format!(
                            "Không thể lấy balance cho {} sau {} lần thử: {}", 
                            address,
                            MAX_RETRIES + 1, 
                            last_error.as_ref().map_or_else(|| "Unknown error".to_string(), |e| e.to_string())
                        )));
                    }
                }
            }
        }
        
        // Không thể xảy ra do điều kiện return ở trên, nhưng compiler cần điều này
        Err(DefiError::ProviderError(format!("Không thể lấy balance cho địa chỉ {}", address)))
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
        assert!(config.rpc_url.contains("trongrid.io"));
    }
    
    #[test]
    fn test_tron_config_builder() {
        let config = TronConfig::new(ChainId::TronShasta)
            .with_full_node_url("https://custom-api.shasta.trongrid.io")
            .with_solidity_node_url("https://custom-solidity.shasta.trongrid.io")
            .with_timeout(5000)
            .with_max_retries(5);
            
        assert_eq!(config.chain_id, ChainId::TronShasta);
        assert_eq!(config.rpc_url, "https://custom-api.shasta.trongrid.io");
        assert_eq!(config.timeout_ms, 5000);
        assert_eq!(config.max_retries, 5);
    }
    
    #[test]
    fn test_validate_address() {
        let provider_result = TronProvider::new(TronConfig::new(ChainId::TronMainnet));
        assert!(provider_result.is_ok(), "Khởi tạo TronProvider thất bại: {:?}", provider_result.err());
        let provider = provider_result.unwrap();
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
        let provider_result = TronProvider::new(TronConfig::new(ChainId::TronMainnet));
        assert!(provider_result.is_ok(), "Khởi tạo TronProvider thất bại: {:?}", provider_result.err());
        let provider = provider_result.unwrap();
        // Kiểm tra chuyển đổi địa chỉ <-> hex
        let address = "TTSFjEG3Lu9WkHdp4JrWYhbGP6K1REqnGQ";
        let hex_address = provider.address_to_hex(address).expect("address_to_hex failed");
        let converted_address = provider.hex_to_address(&hex_address).expect("hex_to_address failed");
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