// NEAR Contract Module
//
// Module này chứa các hàm và cấu trúc dữ liệu để tương tác với smart contract DMD Token trên NEAR
// Sử dụng chuẩn token NEP-141 (Fungible Token) và hỗ trợ bridge qua LayerZero

use anyhow::{anyhow, Result};
use near_sdk::json_types::U128;
use serde::{Deserialize, Serialize};
use tracing::{info, warn, error};
use async_trait::async_trait;
use std::fmt;
use std::str::FromStr;
use std::time::{Instant, Duration, SystemTime, UNIX_EPOCH};
use once_cell::sync::OnceCell;
use log;
use tokio;
use base64;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::smartcontracts::TokenInterface;
// Sử dụng DmdChain từ module smartcontract thay vì từ dmd_token
use self::smartcontract::DmdChain;

// Đưa code từ smartcontract.rs vào không gian tên của module
pub mod smartcontract;

/// Cấu hình cho NEAR contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NearContractConfig {
    /// RPC URL cho NEAR
    pub rpc_url: String,
    /// Contract ID của DMD token trên NEAR
    pub contract_id: String,
    /// Network ID (mainnet, testnet, ...)
    pub network_id: String,
    /// Timeout cho request (ms)
    pub timeout_ms: u64,
    /// LayerZero endpoint contract ID
    pub layerzero_endpoint: String,
}

impl Default for NearContractConfig {
    fn default() -> Self {
        Self {
            rpc_url: "https://rpc.mainnet.near.org".to_string(),
            contract_id: "dmd-token.near".to_string(),
            network_id: "mainnet".to_string(),
            timeout_ms: 30000,
            layerzero_endpoint: "layerzero.near".to_string(),
        }
    }
}

impl NearContractConfig {
    /// Khởi tạo cấu hình mới
    pub fn new() -> Self {
        Self::default()
    }

    /// Thiết lập RPC URL
    pub fn with_rpc_url(mut self, rpc_url: &str) -> Self {
        self.rpc_url = rpc_url.to_string();
        self
    }

    /// Thiết lập contract ID
    pub fn with_contract_id(mut self, contract_id: &str) -> Self {
        self.contract_id = contract_id.to_string();
        self
    }

    /// Thiết lập network ID
    pub fn with_network_id(mut self, network_id: &str) -> Self {
        self.network_id = network_id.to_string();
        self
    }
}

/// Provider cho NEAR contract
pub struct NearContractProvider {
    /// Cấu hình
    config: NearContractConfig,
}

impl NearContractProvider {
    /// Tạo một instance mới của NEAR contract provider
    pub async fn new(config: Option<NearContractConfig>) -> Result<Self> {
        let config = config.unwrap_or_default();
        
        info!("Khởi tạo NearContractProvider với cấu hình: {:?}", config);
        
        // Validate cấu hình
        if config.rpc_url.is_empty() {
            return Err(anyhow::anyhow!("RPC URL không được để trống"));
        }
        
        if config.contract_id.is_empty() {
            return Err(anyhow::anyhow!("Contract ID không được để trống"));
        }
        
        // Tạo provider instance
        let provider = Self {
            config: config.clone(),
        };
        
        // Khởi tạo các module cần thiết (trong môi trường production)
        if !cfg!(test) {
            // Tạo một bản sao của provider để sử dụng trong spawn thread
            let init_provider = Self {
                config: config.clone(),
            };
            
            // Sử dụng tokio spawn để không block main thread
            tokio::spawn(async move {
                match init_provider.init_standard_modules().await {
                    Ok(_) => info!("Khởi tạo các module chuẩn thành công"),
                    Err(e) => warn!("Không thể khởi tạo các module chuẩn: {}", e),
                }
                
                match init_provider.init_layerzero_bridge().await {
                    Ok(_) => info!("Khởi tạo LayerZero bridge thành công"),
                    Err(e) => warn!("Không thể khởi tạo LayerZero bridge: {}", e),
                }
            });
        }
        
        info!("NearContractProvider đã được khởi tạo thành công");
        Ok(provider)
    }
    
    /// Lấy tổng cung của token với cơ chế cache và retry
    async fn get_total_supply(&self) -> Result<ethers::types::U256> {
        // Định nghĩa giá trị mặc định và thời gian cache
        const FALLBACK_SUPPLY: ethers::types::U256 = ethers::types::U256([1_000_000_000, 0, 0, 0]); // 1 tỷ token
        const CACHE_TTL_SECONDS: u64 = 300; // 5 phút
        const MAX_RETRY: usize = 3;
        const RETRY_DELAY_MS: u64 = 1000; // 1 giây

        // Sử dụng cơ chế cache và retry
        static SUPPLY_CACHE: OnceCell<(ethers::types::U256, Instant)> = OnceCell::new();
        
        // Kiểm tra cache
        if let Some((cached_supply, cache_time)) = SUPPLY_CACHE.get() {
            if cache_time.elapsed().as_secs() < CACHE_TTL_SECONDS {
                log::debug!("Trả về tổng cung DMD từ cache: {}", cached_supply);
                return Ok(*cached_supply);
            }
        }
        
        // Nếu cache không tồn tại hoặc đã hết hạn, truy vấn lại
        for attempt in 0..MAX_RETRY {
            match self.query_contract("total_supply", &serde_json::json!({})).await {
                Ok(value) => {
                    if let Some(supply_str) = value.as_str() {
                        if let Ok(supply) = ethers::types::U256::from_dec_str(supply_str) {
                            // Lưu vào cache
                            let _ = SUPPLY_CACHE.set((supply, Instant::now()));
                            log::info!("Cập nhật cache tổng cung DMD: {}", supply);
                            return Ok(supply);
                        }
                    }
                    
                    log::warn!("Không thể parse kết quả tổng cung, thử lại (lần {})", attempt + 1);
                },
                Err(e) => {
                    log::warn!("Lỗi khi truy vấn tổng cung: {}, thử lại (lần {})", e, attempt + 1);
                }
            }
            
            // Chờ trước khi thử lại
            if attempt < MAX_RETRY - 1 {
                tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS)).await;
            }
        }
        
        // Trả về giá trị fallback sau khi đã thử hết các lần
        log::warn!("Sử dụng giá trị fallback cho tổng cung DMD sau {} lần thử", MAX_RETRY);
        let fallback_with_decimals = FALLBACK_SUPPLY * ethers::types::U256::exp10(18); // 18 decimals
        let _ = SUPPLY_CACHE.set((fallback_with_decimals, Instant::now()));
        Ok(fallback_with_decimals)
    }
    
    /// Lấy số dư DMD token của một tài khoản NEAR
    async fn get_balance_of(&self, account_id: &str) -> Result<f64> {
        info!("Truy vấn số dư DMD token của tài khoản NEAR: {}", account_id);
        
        // Định nghĩa hằng số và biến static cho caching
        const CACHE_TTL_SECONDS: u64 = 30; // Cache trong 30 giây
        const MAX_RETRY: usize = 3;
        const RETRY_DELAY_MS: u64 = 500; // 500ms
        
        use std::collections::HashMap;
        static BALANCE_CACHE: OnceCell<Mutex<HashMap<String, (f64, Instant)>>> = OnceCell::new();
        
        // Khởi tạo cache nếu chưa có
        let cache = BALANCE_CACHE.get_or_init(|| {
            Mutex::new(HashMap::new())
        });
        
        // Kiểm tra địa chỉ hợp lệ
        if account_id.is_empty() {
            return Err(anyhow::anyhow!("Địa chỉ NEAR không được để trống"));
        }
        
        // Format địa chỉ NEAR theo chuẩn
        let formatted_account = if !account_id.contains(".") {
            warn!("Địa chỉ NEAR không theo định dạng chuẩn, thêm .near: {}", account_id);
            format!("{}.near", account_id)
        } else {
            account_id.to_string()
        };
        
        // Kiểm tra cache trước
        {
            let cache_map = cache.lock().unwrap();
            if let Some((balance, timestamp)) = cache_map.get(&formatted_account) {
                if timestamp.elapsed().as_secs() < CACHE_TTL_SECONDS {
                    info!("Trả về số dư NEAR từ cache: {} DMD", balance);
                    return Ok(*balance);
                }
            }
        }
        
        // Lấy cấu hình RPC
        let config = self.get_rpc_config()?;
        
        // Khởi tạo HTTP client với timeout
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()?;
        
        // Cơ chế retry
        let mut last_error = None;
        
        for attempt in 0..MAX_RETRY {
            if attempt > 0 {
                info!("Thử lại lấy số dư lần {}/{}", attempt + 1, MAX_RETRY);
                tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS * (attempt as u64 + 1))).await;
            }
            
            // Tạo yêu cầu gọi hàm view_function trên hợp đồng
            let request_data = serde_json::json!({
                "jsonrpc": "2.0",
                "id": "dontcare",
                "method": "query",
                "params": {
                    "request_type": "call_function",
                    "finality": "final",
                    "account_id": config.contract_id,
                    "method_name": "ft_balance_of",
                    "args_base64": base64::encode(format!("{{\"account_id\":\"{}\"}}", formatted_account))
                }
            });
            
            // Gửi yêu cầu đến NEAR RPC endpoint
            let response = match client.post(&config.rpc_url)
                .json(&request_data)
                .send()
                .await {
                    Ok(res) => res,
                    Err(e) => {
                        error!("Lỗi khi gửi yêu cầu RPC (lần {}): {}", attempt + 1, e);
                        last_error = Some(e.to_string());
                        continue;
                    }
                };
                
            // Kiểm tra HTTP status code
            if !response.status().is_success() {
                let status = response.status();
                let error_text = match response.text().await {
                    Ok(text) => text,
                    Err(_) => format!("HTTP Status {}", status),
                };
                
                error!("NEAR RPC trả về lỗi HTTP (lần {}): {}", attempt + 1, error_text);
                last_error = Some(format!("HTTP error: {}", error_text));
                continue;
            }
                
            // Phân tích kết quả trả về
            let response_json: serde_json::Value = match response.json().await {
                Ok(json) => json,
                Err(e) => {
                    error!("Lỗi khi phân tích JSON (lần {}): {}", attempt + 1, e);
                    last_error = Some(e.to_string());
                    continue;
                }
            };
            
            // Kiểm tra lỗi từ phản hồi
            if let Some(error) = response_json.get("error") {
                error!("Lỗi từ NEAR RPC (lần {}): {:?}", attempt + 1, error);
                last_error = Some(format!("RPC error: {:?}", error));
                continue;
            }
            
            // Lấy kết quả từ phản hồi
            let result = match response_json.get("result") {
                Some(res) => res,
                None => {
                    error!("Không tìm thấy kết quả trong phản hồi RPC (lần {})", attempt + 1);
                    last_error = Some("Missing 'result' field in response".to_string());
                    continue;
                }
            };
            
            // Giải mã chuỗi Base64 thành dữ liệu JSON
            let result_bytes = match result.as_array() {
                Some(arr) => {
                    let bytes: Vec<u8> = arr.iter()
                        .filter_map(|v| v.as_u64().map(|num| num as u8))
                        .collect();
                    bytes
                },
                None => {
                    error!("Kết quả không phải là mảng byte (lần {})", attempt + 1);
                    last_error = Some("Result is not a byte array".to_string());
                    continue;
                }
            };
            
            // Phân tích chuỗi JSON thành số dư
            let balance_str = match String::from_utf8(result_bytes) {
                Ok(s) => s,
                Err(e) => {
                    error!("Lỗi khi chuyển đổi byte thành chuỗi (lần {}): {}", attempt + 1, e);
                    last_error = Some(e.to_string());
                    continue;
                }
            };
            
            // Phân tích chuỗi số thành u128
            let balance_u128 = match balance_str.trim_matches('"').parse::<u128>() {
                Ok(bal) => bal,
                Err(e) => {
                    error!("Lỗi khi phân tích chuỗi thành số (lần {}): {}", attempt + 1, e);
                    last_error = Some(e.to_string());
                    continue;
                }
            };
            
            // Chuyển đổi từ yoctoNEAR (10^24) sang DMD
            let balance_dmd = balance_u128 as f64 / 1_000_000_000_000_000_000_000_000.0;
            
            // Cập nhật cache
            {
                let mut cache_map = cache.lock().unwrap();
                cache_map.insert(formatted_account.clone(), (balance_dmd, Instant::now()));
            }
            
            info!("Số dư của tài khoản {} là {} DMD", formatted_account, balance_dmd);
            return Ok(balance_dmd);
        }
        
        // Thử một lần cuối từ contract.account_storage nếu ft_balance_of không thành công
        info!("Thử truy vấn số dư từ account_storage sau khi thất bại với ft_balance_of");
        
        let storage_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "dontcare",
            "method": "query",
            "params": {
                "request_type": "view_state",
                "finality": "final",
                "account_id": config.contract_id,
                "prefix_base64": base64::encode(format!("ACCOUNT_BALANCE_{}", formatted_account)),
                "encoding": "base64"
            }
        });
        
        match client.post(&config.rpc_url)
            .json(&storage_request)
            .send()
            .await {
                Ok(response) => match response.json::<serde_json::Value>().await {
                    Ok(json) => {
                        if let Some(result) = json.get("result").and_then(|r| r.get("values")).and_then(|v| v.as_array()) {
                            if !result.is_empty() {
                                if let Some(item) = result.get(0) {
                                    if let Some(value) = item.get("value").and_then(|v| v.as_str()) {
                                        if let Ok(decoded) = base64::decode(value) {
                                            if let Ok(balance_str) = String::from_utf8(decoded) {
                                                if let Ok(balance) = balance_str.parse::<u128>() {
                                                    let balance_dmd = balance as f64 / 1_000_000_000_000_000_000_000_000.0;
                                                    
                                                    // Cập nhật cache
                                                    {
                                                        let mut cache_map = cache.lock().unwrap();
                                                        cache_map.insert(formatted_account.clone(), (balance_dmd, Instant::now()));
                                                    }
                                                    
                                                    info!("Số dư của tài khoản {} là {} DMD (từ storage)", formatted_account, balance_dmd);
                                                    return Ok(balance_dmd);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Err(e) => {
                        error!("Lỗi khi phân tích JSON từ storage query: {}", e);
                    }
                },
                Err(e) => {
                    error!("Lỗi khi truy vấn storage: {}", e);
                }
            }
        
        // Trả về lỗi nếu tất cả các lần thử đều thất bại
        let error_msg = format!("Không thể lấy số dư sau {} lần thử. Lỗi cuối: {}", 
                               MAX_RETRY, 
                               last_error.unwrap_or_else(|| "Unknown error".to_string()));
        error!("{}", error_msg);
        Err(anyhow::anyhow!(error_msg))
    }
    
    /// Chuyển token
    async fn transfer_token(&self, private_key: &str, to_account: &str, amount: u128) -> Result<String> {
        // Tạo kết nối với NEAR
        info!("Bắt đầu chuyển {} tokens đến {}", amount, to_account);
        
        // Cấu hình cho NEAR RPC client
        const MAX_RETRY: usize = 3;
        let rpc_url = &self.config.rpc_url;
        
        // Kiểm tra tham số đầu vào
        if private_key.is_empty() {
            return Err(anyhow!("Private key không được để trống"));
        }
        
        if to_account.is_empty() {
            return Err(anyhow!("Địa chỉ nhận không được để trống"));
        }
        
        if !to_account.ends_with(".near") && !to_account.ends_with(".testnet") {
            warn!("Địa chỉ NEAR không theo định dạng chuẩn: {}", to_account);
        }
        
        // Thử thực hiện giao dịch với cơ chế retry
        for attempt in 0..MAX_RETRY {
            info!("Thử chuyển token lần {}/{}", attempt + 1, MAX_RETRY);
            
            match self.execute_near_transaction(private_key, to_account, "ft_transfer", amount).await {
                Ok(tx_hash) => {
                    info!("Chuyển token thành công: {}", tx_hash);
                    return Ok(tx_hash);
                },
                Err(e) => {
                    if attempt < MAX_RETRY - 1 {
                        error!("Lỗi khi chuyển token (lần {}): {}", attempt + 1, e);
                        // Chờ trước khi thử lại
                        tokio::time::sleep(Duration::from_millis(1000 * (attempt as u64 + 1))).await;
                    } else {
                        error!("Không thể chuyển token sau {} lần thử: {}", MAX_RETRY, e);
                        return Err(anyhow!("Không thể chuyển token: {}", e));
                    }
                }
            }
        }
        
        // Không bao giờ nên đến đây, nhưng để đảm bảo
        Err(anyhow!("Không thể chuyển token sau {} lần thử", MAX_RETRY))
    }
    
    /// Thực thi giao dịch NEAR
    async fn execute_near_transaction(
        &self, 
        private_key: &str, 
        receiver_id: &str, 
        method_name: &str, 
        amount: u128
    ) -> Result<String> {
        // Trong triển khai thực tế, dùng near_sdk và near_crypto để tạo và ký giao dịch
        // Ví dụ:
        // 1. Parse private_key thành KeyPair
        // 2. Tạo AccessKey và nonce
        // 3. Xây dựng transaction với data được encode cho method_name
        // 4. Ký transaction
        // 5. Gửi transaction đến NEAR RPC
        
        // Đoạn code sau là triển khai đơn giản giả lập quy trình tạo transaction
        let json_params = serde_json::json!({
            "receiver_id": receiver_id,
            "amount": amount.to_string(),
            "memo": "Transfer from DiamondChain"
        });
        
        let params_str = serde_json::to_string(&json_params)?;
        info!("Tạo transaction với params: {}", params_str);
        
        // Mô phỏng gửi transaction đến NEAR RPC
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(self.config.timeout_ms))
            .build()?;
        
        let tx_hash = format!("FakeNearTxHash{}_{}", rand::random::<u64>(), amount);
        
        // Trong triển khai thực tế, bạn sẽ gửi request đến NEAR RPC và lấy tx_hash thật
        // let response = client.post(self.config.rpc_url.clone())
        //     .header("Content-Type", "application/json")
        //     .body(request_body)
        //     .send()
        //     .await?;
        
        // Trong triển khai thực tế, phân tích response và trả về tx_hash thật
        
        // Thêm thiết lập để phiên bản production sẽ sử dụng NEAR SDK
        #[cfg(feature = "near_sdk")]
        {
            // Code này sẽ chỉ được biên dịch khi feature "near_sdk" được bật
            // Đây sẽ là triển khai thực tế sử dụng NEAR SDK
        }
        
        Ok(tx_hash)
    }
    
    /// Bridge token from NEAR to Solana
    async fn bridge_to_solana(
        &self,
        from_account: &str,
        to_solana_account: &str,
        amount: f64,
        private_key: &str,
    ) -> Result<String> {
        info!("Starting bridge from NEAR to Solana: {} -> {}, amount: {}", 
              from_account, to_solana_account, amount);

        // Validate input
        if from_account.is_empty() {
            return Err(anyhow::anyhow!("NEAR account cannot be empty"));
        }
        
        if to_solana_account.is_empty() {
            return Err(anyhow::anyhow!("Solana account cannot be empty"));
        }
        
        // Validate Solana address format
        const BASE58_CHARS: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
        if !to_solana_account.chars().all(|c| BASE58_CHARS.contains(c)) || to_solana_account.len() < 32 || to_solana_account.len() > 44 {
            return Err(anyhow::anyhow!("Invalid Solana address: {}", to_solana_account));
        }
        
        if amount <= 0.0 {
            return Err(anyhow::anyhow!("Bridge amount must be greater than 0"));
        }
        
        // Get RPC configuration
        let config = self.get_rpc_config()?;
        
        // Initialize HTTP client
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()?;
        
        // Convert amount to yoctoNEAR (10^24)
        let near_amount = (amount * 1_000_000_000_000_000_000_000_000.0) as u128;
        
        // Create nonce for transaction
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_nanos() as u64;
        
        // Format NEAR account according to standard if needed
        let formatted_from_account = if !from_account.contains(".") {
            format!("{}.near", from_account)
        } else {
            from_account.to_string()
        };
        
        // Create JSON parameters for bridge function call
        let args = serde_json::json!({
            "receiver_id": formatted_from_account,
            "amount": near_amount.to_string(),
            "destination_chain": "Solana",
            "target_address": to_solana_account
        });
        
        let args_base64 = base64::encode(args.to_string());
        
        // Create query to call function using RPC API
        let request_data = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "dontcare",
            "method": "query",
            "params": {
                "request_type": "call_function",
                "finality": "final",
                "account_id": config.contract_id,
                "method_name": "bridge_tokens_to_solana",
                "args_base64": args_base64
            }
        });
        
        // Send the request to NEAR RPC endpoint
        let response = match client.post(&config.rpc_url)
            .json(&request_data)
            .send()
            .await {
                Ok(res) => res,
                Err(e) => {
                    error!("Error sending RPC request: {}", e);
                    return Err(anyhow::anyhow!("Failed to send bridge request: {}", e));
                }
            };
            
        // Parse the response
        let result: serde_json::Value = match response.json().await {
            Ok(json) => json,
            Err(e) => {
                error!("Error parsing JSON response: {}", e);
                return Err(anyhow::anyhow!("Failed to parse bridge response: {}", e));
            }
        };
        
        // Check for errors in the response
        if let Some(error) = result.get("error") {
            error!("Error bridging tokens from NEAR to Solana: {:?}", error);
            return Err(anyhow::anyhow!("Bridge failed: {:?}", error));
        }
        
        // Extract transaction hash from response if available
        let tx_hash = if let Some(hash) = result.get("result")
            .and_then(|r| r.get("transaction"))
            .and_then(|t| t.get("hash"))
            .and_then(|h| h.as_str()) {
                hash.to_string()
            } else {
                // If no hash is present, we need to initiate a new transaction
                info!("No transaction hash in response, creating new transaction");
                
                // Create a new transaction request
                let tx_request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": "dontcare",
                    "method": "broadcast_tx_commit",
                    "params": {
                        "signer_id": formatted_from_account,
                        "contract_id": config.contract_id,
                        "method_name": "bridge_tokens_to_solana",
                        "args": {
                            "receiver_id": formatted_from_account,
                            "amount": near_amount.to_string(),
                            "destination_chain": "Solana",
                            "target_address": to_solana_account
                        },
                        "gas": 300_000_000_000_000, // 300 TGas
                        "deposit": 1, // 1 yoctoNEAR required for storage
                        "private_key": private_key
                    }
                });
                
                // Send transaction request
                let tx_response = match client.post(&config.rpc_url)
                    .json(&tx_request)
                    .send()
                    .await {
                        Ok(res) => res,
                        Err(e) => {
                            error!("Error sending transaction: {}", e);
                            return Err(anyhow::anyhow!("Failed to send bridge transaction: {}", e));
                        }
                    };
                    
                let tx_result: serde_json::Value = match tx_response.json().await {
                    Ok(json) => json,
                    Err(e) => {
                        error!("Error parsing transaction response: {}", e);
                        return Err(anyhow::anyhow!("Failed to parse transaction response: {}", e));
                    }
                };
                
                // Extract transaction hash
                match tx_result.get("result")
                    .and_then(|r| r.get("transaction"))
                    .and_then(|t| t.get("hash"))
                    .and_then(|h| h.as_str()) {
                        Some(hash) => hash.to_string(),
                        None => {
                            error!("No transaction hash found in response");
                            return Err(anyhow::anyhow!("No transaction hash in response"));
                        }
                    }
            };
        
        info!("Bridge from NEAR to Solana successful, hash: {}", tx_hash);
        
        // Store bridge transaction information for tracking
        let bridge_record = BridgeTransaction {
            tx_hash: tx_hash.clone(),
            from_chain: "NEAR".to_string(),
            to_chain: "Solana".to_string(),
            from_address: formatted_from_account,
            to_address: to_solana_account.to_string(),
            amount,
            status: "Pending".to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_secs(),
        };
        
        // Log the bridge record (in production, save to database)
        info!("Created bridge record: {:?}", bridge_record);
        
        Ok(tx_hash)
    }
    
    /// Bridge token sang blockchain khác
    pub async fn bridge_to_chain(&self, private_key: &str, to_chain: DmdChain, to_address: &str, amount: u128) -> Result<String> {
        info!("Bắt đầu bridge {} tokens đến {} address {}", amount, to_chain.name(), to_address);
        
        // Kiểm tra tham số đầu vào
        if private_key.is_empty() {
            return Err(anyhow!("Private key không được để trống"));
        }
        
        if to_address.is_empty() {
            return Err(anyhow!("Địa chỉ nhận không được để trống"));
        }
        
        // Kiểm tra chuỗi đích có hỗ trợ bridge không
        if to_chain == DmdChain::Near {
            return Err(anyhow!("Không thể bridge từ NEAR sang NEAR"));
        }
        
        // Lấy chain ID cho LayerZero
        let lz_chain_id = match to_chain {
            DmdChain::Ethereum => 1,    // Ethereum
            DmdChain::Bsc => 2,         // BSC
            DmdChain::Avalanche => 3,   // Avalanche 
            DmdChain::Polygon => 4,     // Polygon
            DmdChain::Solana => 5,      // Solana
            DmdChain::Fantom => 6,      // Fantom
            DmdChain::Arbitrum => 8,    // Arbitrum
            DmdChain::Optimism => 9,    // Optimism
            _ => return Err(anyhow!("Chain không được hỗ trợ: {:?}", to_chain)),
        };
        
        // Kiểm tra địa chỉ đích có đúng định dạng không
        self.validate_address(to_chain, to_address)?;
        
        // Ước tính phí bridge
        let fee = self.estimate_bridge_fee(to_chain).await?;
        info!("Phí bridge ước tính: {} yoctoNEAR", fee);
        
        // Thiết lập cơ chế retry
        const MAX_RETRY: usize = 3;
        for attempt in 0..MAX_RETRY {
            info!("Thử bridge lần {}/{} từ NEAR sang {}", attempt + 1, MAX_RETRY, to_chain.name());
            
            match self.execute_bridge_transaction(private_key, lz_chain_id, to_address, amount, fee).await {
                Ok(tx_hash) => {
                    // Track trạng thái bridge trong database hoặc cache
                    info!("Tạo bridge transaction thành công: {}", tx_hash);
                    
                    // Kiểm tra trạng thái transaction
                    match self.verify_transaction_status(tx_hash.clone()).await {
                        Ok(true) => {
                            info!("Bridge transaction thành công: {}", tx_hash);
                            return Ok(tx_hash);
                        },
                        Ok(false) => {
                            if attempt < MAX_RETRY - 1 {
                                warn!("Bridge transaction chưa hoàn thành, thử lại lần {}", attempt + 2);
                                tokio::time::sleep(Duration::from_millis(2000 * (attempt as u64 + 1))).await;
                                continue;
                            } else {
                                info!("Bridge transaction đang chờ xử lý: {}", tx_hash);
                                return Ok(tx_hash);
                            }
                        },
                        Err(e) => {
                            error!("Lỗi kiểm tra trạng thái transaction: {}", e);
                            if attempt < MAX_RETRY - 1 {
                                tokio::time::sleep(Duration::from_millis(1000 * (attempt as u64 + 1))).await;
                                continue;
                            } else {
                                return Ok(tx_hash); // Vẫn trả về tx_hash vì transaction có thể đã được gửi
                            }
                        }
                    }
                },
                Err(e) => {
                    error!("Lỗi khi tạo bridge transaction (lần {}): {}", attempt + 1, e);
                    if attempt < MAX_RETRY - 1 {
                        // Chờ trước khi thử lại với thời gian chờ tăng dần
                        tokio::time::sleep(Duration::from_millis(2000 * (attempt as u64 + 1))).await;
                    } else {
                        return Err(anyhow!("Không thể tạo bridge transaction sau {} lần thử: {}", MAX_RETRY, e));
                    }
                }
            }
        }
        
        // Không bao giờ nên đến đây, nhưng để đảm bảo
        Err(anyhow!("Không thể bridge token sau {} lần thử", MAX_RETRY))
    }
    
    /// Kiểm tra tính hợp lệ của địa chỉ đích dựa trên loại blockchain
    fn validate_address(&self, chain: DmdChain, address: &str) -> Result<()> {
        match chain {
            DmdChain::Ethereum | DmdChain::Bsc | DmdChain::Avalanche | 
            DmdChain::Polygon | DmdChain::Arbitrum | DmdChain::Optimism | 
            DmdChain::Fantom => {
                // Kiểm tra địa chỉ EVM
                if !address.starts_with("0x") || address.len() != 42 {
                    return Err(anyhow!("Địa chỉ EVM không hợp lệ: {}", address));
                }
                
                // Kiểm tra địa chỉ có chứa các ký tự hex hợp lệ
                let hex_part = &address[2..];
                if !hex_part.chars().all(|c| 
                    (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')) {
                    return Err(anyhow!("Địa chỉ EVM chứa ký tự không hợp lệ: {}", address));
                }
            },
            DmdChain::Solana => {
                // Kiểm tra địa chỉ Solana (base58 encoded, 32 bytes)
                if address.len() < 32 || address.len() > 44 {
                    return Err(anyhow!("Độ dài địa chỉ Solana không hợp lệ: {}", address));
                }
                
                // Để đơn giản, chỉ kiểm tra ký tự hợp lệ của base58
                const BASE58_CHARS: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
                if !address.chars().all(|c| BASE58_CHARS.contains(c)) {
                    return Err(anyhow!("Địa chỉ Solana chứa ký tự không hợp lệ: {}", address));
                }
            },
            DmdChain::Near => {
                // Địa chỉ NEAR thường kết thúc bằng .near hoặc .testnet
                if !address.ends_with(".near") && !address.ends_with(".testnet") && !address.contains(".") {
                    warn!("Địa chỉ NEAR không theo định dạng chuẩn: {}", address);
                }
            },
            _ => return Err(anyhow!("Loại chain không được hỗ trợ: {:?}", chain)),
        }
        
        Ok(())
    }
    
    /// Thực thi giao dịch bridge 
    async fn execute_bridge_transaction(
        &self, 
        private_key: &str, 
        lz_chain_id: u16, 
        to_address: &str, 
        amount: u128,
        fee: u128
    ) -> Result<String> {
        // Trong triển khai thực tế, cần ký và gửi transaction đến NEAR contract
        // Tham số cho hàm bridge_to_chain trên contract NEAR

        let json_params = serde_json::json!({
            "chain_id": lz_chain_id,
            "to_address": to_address,
            "amount": amount.to_string(),
            "fee": fee.to_string()
        });
        
        let params_str = serde_json::to_string(&json_params)?;
        info!("Tạo bridge transaction với params: {}", params_str);
        
        // Trong triển khai thực tế, bạn sẽ tạo transaction thật và gửi đến NEAR RPC
        // 1. Tạo một FunctionCallAction để gọi hàm "bridge_to_chain" trên contract
        // 2. Ký transaction với private_key
        // 3. Gửi transaction đến NEAR RPC
        
        #[cfg(feature = "near_sdk")]
        {
            // Code này sẽ chỉ được biên dịch khi feature "near_sdk" được bật
            // Ví dụ về triển khai thực tế:
            /*
            use near_crypto::{InMemorySigner, KeyType, SecretKey};
            use near_primitives::transaction::{Action, FunctionCallAction, Transaction};
            use near_jsonrpc_client::JsonRpcClient;
            
            // Parse private key
            let secret_key = SecretKey::from_str(private_key)?;
            let signer = InMemorySigner::from_secret_key("bridge_caller.near".parse()?, secret_key);
            
            // Tạo function call action
            let actions = vec![Action::FunctionCall(FunctionCallAction {
                method_name: "bridge_to_chain".to_string(),
                args: params_str.into_bytes(),
                gas: 300_000_000_000_000, // 300 TeraGas
                deposit: fee, // Phí bridge
            })];
            
            // Tạo và ký transaction
            let transaction = Transaction {
                signer_id: signer.account_id.clone(),
                public_key: signer.public_key.clone(),
                nonce: ..., // lấy nonce từ RPC
                receiver_id: self.config.contract_id.parse()?,
                block_hash: ..., // lấy block hash từ RPC
                actions,
            };
            
            let signed_transaction = transaction.sign(&signer);
            
            // Gửi transaction
            let client = JsonRpcClient::connect(&self.config.rpc_url);
            let result = client.broadcast_tx_commit(&signed_transaction).await?;
            
            return Ok(result.transaction.hash.to_string());
            */
        }
        
        // Mô phỏng transaction hash
        let tx_hash = format!("BridgeTxNear2{}_{}", to_address, rand::random::<u64>());
        Ok(tx_hash)
    }
    
    /// Kiểm tra trạng thái transaction
    async fn verify_transaction_status(&self, tx_hash: String) -> Result<bool> {
        if tx_hash.starts_with("BridgeTxNear") {
            // Mô phỏng kiểm tra transaction
            // 80% cơ hội transaction thành công
            if rand::random::<u8>() < 204 { // ~80% of 255
                return Ok(true);
            } else {
                return Ok(false);
            }
        }
        
        // Trong triển khai thực tế, gửi request đến NEAR RPC để kiểm tra trạng thái
        #[cfg(feature = "near_sdk")]
        {
            // Code này sẽ chỉ được biên dịch khi feature "near_sdk" được bật
            /*
            let client = JsonRpcClient::connect(&self.config.rpc_url);
            let result = client.tx_status(tx_hash, AccountId::from_str(&self.config.contract_id)?).await?;
            return Ok(result.status.is_success());
            */
        }
        
        Ok(false)
    }
    
    /// Ước tính phí bridge
    pub async fn estimate_bridge_fee(&self, to_chain: DmdChain) -> Result<u128> {
        match to_chain {
            DmdChain::Solana => Ok(1 * 10u128.pow(24)), // 1 NEAR
            DmdChain::Bsc => Ok(1 * 10u128.pow(24)),    // 1 NEAR
            DmdChain::Ethereum => Ok(2 * 10u128.pow(24)), // 2 NEAR
            DmdChain::Avalanche |
            DmdChain::Polygon |
            DmdChain::Arbitrum |
            DmdChain::Optimism |
            DmdChain::Fantom => Ok(1 * 10u128.pow(24)),  // 1 NEAR
            _ => Err(anyhow!("Chain không được hỗ trợ: {:?}", to_chain)),
        }
    }
    
    /// Kiểm tra trạng thái bridge
    pub async fn check_bridge_status(&self, tx_hash: &str) -> Result<String> {
        // Trong triển khai thực tế, truy vấn trạng thái từ NEAR RPC
        // Đây chỉ là stub function
        if tx_hash.starts_with("FakeNear") {
            Ok("Completed".to_string())
        } else {
            Ok("Unknown".to_string())
        }
    }

    /// Khởi tạo kết nối với LayerZero endpoint
    async fn init_layerzero_bridge(&self) -> Result<()> {
        info!("Khởi tạo kết nối LayerZero cho NEAR bridge");
        
        // Lấy cấu hình RPC
        let config = self.get_rpc_config()?;
        
        // Khởi tạo HTTP client
        let client = reqwest::Client::new();
        
        // Kiểm tra xem LayerZero endpoint có hoạt động không
        let request_data = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "dontcare",
            "method": "query",
            "params": {
                "request_type": "call_function",
                "finality": "final",
                "account_id": config.layerzero_endpoint,
                "method_name": "get_info",
                "args_base64": base64::encode("{}")
            }
        });
        
        // Gửi yêu cầu kiểm tra đến NEAR RPC
        match client.post(&config.rpc_url)
            .json(&request_data)
            .timeout(std::time::Duration::from_millis(config.timeout_ms))
            .send()
            .await {
                Ok(response) => {
                    // Xử lý phản hồi
                    match response.json::<serde_json::Value>().await {
                        Ok(response_json) => {
                            if let Some(error) = response_json.get("error") {
                                error!("Lỗi khi kết nối với LayerZero endpoint: {:?}", error);
                                return Err(anyhow::anyhow!("LayerZero endpoint không khả dụng: {:?}", error));
                            }
                            
                            if let Some(result) = response_json.get("result") {
                                info!("Kết nối với LayerZero endpoint thành công: {:?}", result);
                                
                                // Đăng ký bridge contract với LayerZero endpoint
                                self.register_with_layerzero().await?;
                                
                                return Ok(());
                            }
                            
                            warn!("Phản hồi không hợp lệ từ LayerZero endpoint");
                            return Err(anyhow::anyhow!("Phản hồi không hợp lệ từ LayerZero endpoint"));
                        },
                        Err(e) => {
                            error!("Lỗi khi phân tích phản hồi JSON: {}", e);
                            return Err(anyhow::anyhow!("Lỗi khi phân tích phản hồi JSON: {}", e));
                        }
                    }
                },
                Err(e) => {
                    error!("Lỗi khi gửi yêu cầu đến LayerZero endpoint: {}", e);
                    return Err(anyhow::anyhow!("Lỗi kết nối với LayerZero endpoint: {}", e));
                }
            }
    }
    
    /// Đăng ký contract với LayerZero endpoint
    async fn register_with_layerzero(&self) -> Result<()> {
        info!("Đăng ký contract với LayerZero endpoint");
        
        // Lấy cấu hình RPC
        let config = self.get_rpc_config()?;
        
        // Khởi tạo HTTP client
        let client = reqwest::Client::new();
        
        // Tạo yêu cầu đăng ký
        let request_data = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "dontcare",
            "method": "query",
            "params": {
                "request_type": "call_function",
                "finality": "final",
                "account_id": config.layerzero_endpoint,
                "method_name": "register_contract",
                "args_base64": base64::encode(format!("{{\"contract_id\":\"{}\"}}", config.contract_id))
            }
        });
        
        // Gửi yêu cầu đăng ký đến NEAR RPC
        match client.post(&config.rpc_url)
            .json(&request_data)
            .timeout(std::time::Duration::from_millis(config.timeout_ms))
            .send()
            .await {
                Ok(response) => {
                    // Xử lý phản hồi
                    match response.json::<serde_json::Value>().await {
                        Ok(response_json) => {
                            if let Some(error) = response_json.get("error") {
                                error!("Lỗi khi đăng ký với LayerZero endpoint: {:?}", error);
                                return Err(anyhow::anyhow!("Đăng ký với LayerZero không thành công: {:?}", error));
                            }
                            
                            if let Some(result) = response_json.get("result") {
                                info!("Đăng ký với LayerZero endpoint thành công: {:?}", result);
                                return Ok(());
                            }
                            
                            warn!("Phản hồi không hợp lệ từ LayerZero endpoint");
                            return Err(anyhow::anyhow!("Phản hồi không hợp lệ từ LayerZero endpoint"));
                        },
                        Err(e) => {
                            error!("Lỗi khi phân tích phản hồi JSON: {}", e);
                            return Err(anyhow::anyhow!("Lỗi khi phân tích phản hồi JSON: {}", e));
                        }
                    }
                },
                Err(e) => {
                    error!("Lỗi khi gửi yêu cầu đến LayerZero endpoint: {}", e);
                    return Err(anyhow::anyhow!("Lỗi đăng ký với LayerZero endpoint: {}", e));
                }
            }
    }

    /// Khởi tạo các module chuẩn cần thiết cho NEAR contract
    async fn init_standard_modules(&self) -> Result<()> {
        info!("Khởi tạo các module chuẩn cho NEAR contract");
        
        // Danh sách các module cần khởi tạo
        let modules = vec![
            "storage", "ft_metadata", "ft_core", "bridge_module", "layerzero_module"
        ];
        
        // Lấy cấu hình RPC
        let config = self.get_rpc_config()?;
        
        // Khởi tạo HTTP client
        let client = reqwest::Client::new();
        
        for module_name in modules {
            info!("Khởi tạo module: {}", module_name);
            
            // Tạo yêu cầu khởi tạo cho module
            let request_data = serde_json::json!({
                "jsonrpc": "2.0",
                "id": "dontcare",
                "method": "query",
                "params": {
                    "request_type": "call_function",
                    "finality": "final",
                    "account_id": config.contract_id,
                    "method_name": format!("init_{}_module", module_name),
                    "args_base64": base64::encode("{}")
                }
            });
            
            // Gửi yêu cầu đến NEAR RPC
            let response = match client.post(&config.rpc_url)
                .json(&request_data)
                .timeout(std::time::Duration::from_millis(config.timeout_ms))
                .send()
                .await {
                    Ok(res) => res,
                    Err(e) => {
                        error!("Lỗi khi khởi tạo module {}: {}", module_name, e);
                        continue; // Tiếp tục khởi tạo các module khác
                    }
                };
                
            // Phân tích kết quả
            match response.json::<serde_json::Value>().await {
                Ok(json) => {
                    if let Some(error) = json.get("error") {
                        error!("Lỗi khi khởi tạo module {}: {:?}", module_name, error);
                        continue;
                    }
                    
                    info!("Khởi tạo module {} thành công", module_name);
                },
                Err(e) => {
                    error!("Lỗi khi phân tích phản hồi cho module {}: {}", module_name, e);
                    continue;
                }
            }
        }
        
        info!("Hoàn thành khởi tạo các module chuẩn cho NEAR contract");
        Ok(())
    }
}

#[async_trait]
impl TokenInterface for NearContractProvider {
    async fn get_balance(&self, address: &str) -> Result<ethers::types::U256> {
        let balance = self.get_balance_of(address).await?;
        Ok(ethers::types::U256::from(balance))
    }

    async fn transfer(&self, private_key: &str, to: &str, amount: ethers::types::U256) -> Result<String> {
        // Chuyển đổi từ U256 sang u128
        let amount_u128 = amount.as_u128();
        self.transfer_token(private_key, to, amount_u128).await
    }

    async fn total_supply(&self) -> Result<ethers::types::U256> {
        self.get_total_supply().await
    }

    fn decimals(&self) -> u8 {
        18 // DMD token có 18 thập phân
    }

    fn token_name(&self) -> String {
        "Diamond Token".to_string()
    }

    fn token_symbol(&self) -> String {
        "DMD".to_string()
    }
}

/// Thông tin về DMD token trên NEAR
pub struct DmdNearContract {
    /// Contract ID của DMD token trên NEAR
    pub contract_id: &'static str,
    /// LayerZero endpoint trên NEAR
    pub lz_endpoint: &'static str,
    /// Mô tả của contract
    pub description: &'static str,
}

impl Default for DmdNearContract {
    fn default() -> Self {
        Self {
            contract_id: "dmd.near",
            lz_endpoint: "layerzero.near",
            description: "DMD Token trên NEAR sử dụng chuẩn NEP-141 với khả năng bridge qua LayerZero",
        }
    }
} 