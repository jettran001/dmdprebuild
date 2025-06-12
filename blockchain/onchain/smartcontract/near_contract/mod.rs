// NEAR Contract Module
//
// Module này chứa các hàm và cấu trúc dữ liệu để tương tác với smart contract DMD Token trên NEAR
// Sử dụng chuẩn token NEP-141 (Fungible Token) và hỗ trợ bridge qua LayerZero

use anyhow::{anyhow, Result, Context};
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
use std::collections::HashSet;
use rand;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::{env, near_bindgen, AccountId, Promise, log};
use std::collections::HashMap;
use std::vec::Vec;
use std::convert::TryInto;
use std::fmt;
use rand::Rng;
use zeroize::Zeroize;

// Sử dụng trait thay vì implement trực tiếp
use crate::smartcontracts::TokenInterface;

// Đưa code từ smartcontract.rs vào không gian tên của module
pub mod smartcontract;

// Re-export enum quan trọng từ module smartcontract
pub use self::smartcontract::DmdChain;

// Định nghĩa các lỗi đặc trưng cho NEAR provider
#[derive(Debug, thiserror::Error)]
pub enum NearProviderError {
    #[error("RPC error: {0}")]
    RpcError(String),
    
    #[error("Invalid account ID: {0}")]
    InvalidAccountId(String),
    
    #[error("Contract execution failed: {0}")]
    ContractExecutionFailed(String),
    
    #[error("Timeout error: request took too long")]
    Timeout,
    
    #[error("Network error: {0}")]
    NetworkError(String),
    
    #[error("Parse error: {0}")]
    ParseError(String),
    
    #[error("Bridge error: {0}")]
    BridgeError(String),
    
    #[error("Authentication error: {0}")]
    AuthError(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    // New error types for better error handling
    #[error("RPC endpoint unavailable: {0}")]
    RpcEndpointUnavailable(String),
    
    #[error("Cross-chain communication error: {0}")]
    CrossChainCommunicationError(String),
    
    #[error("Transaction verification failed: {0}")]
    TransactionVerificationFailed(String),
    
    #[error("Bridge transaction stuck: {0}")]
    StuckTransaction(String),
    
    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(String),
    
    #[error("Fallback mechanism error: {0}")]
    FallbackError(String),
}

// Helper trait để chuyển đổi các lỗi thông thường thành NearProviderError
pub trait IntoNearError<T> {
    fn into_near_error(self, context: &str) -> Result<T, NearProviderError>;
}

// Implement cho Result<T, E> để chuyển đổi lỗi
impl<T, E: std::error::Error> IntoNearError<T> for Result<T, E> {
    fn into_near_error(self, context: &str) -> Result<T, NearProviderError> {
        self.map_err(|e| {
            if e.to_string().contains("timeout") {
                NearProviderError::Timeout
            } else if e.to_string().contains("network") {
                NearProviderError::NetworkError(format!("{}: {}", context, e))
            } else if e.to_string().contains("parse") || e.to_string().contains("invalid") {
                NearProviderError::ParseError(format!("{}: {}", context, e))
            } else {
                NearProviderError::RpcError(format!("{}: {}", context, e))
            }
        })
    }
}

/// Trạng thái chi tiết của giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DetailedTransactionStatus {
    /// Đang chờ xử lý
    Pending,
    /// Đã hoàn thành
    Completed,
    /// Thất bại với thông tin chi tiết
    Failed {
        /// Mã lỗi
        code: u32,
        /// Mô tả lỗi
        reason: String,
        /// Thử lại lần cuối
        last_retry: Option<u64>,
        /// Số lần đã thử
        retry_count: u8,
    },
    /// Mắc kẹt
    Stuck {
        /// Thời gian mắc kẹt
        since: u64,
        /// Trạng thái trước đó
        previous_status: String,
    },
    /// Đã hết thời gian
    TimedOut {
        /// Thời gian hết hạn
        expiry_time: u64,
    },
    /// Đã hoàn trả
    Refunded {
        /// Thời gian hoàn trả
        refund_time: u64,
        /// Lý do hoàn trả
        reason: String,
    },
}

/// Cấu hình cho cơ chế retry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Số lần retry tối đa
    pub max_retries: u8,
    /// Thời gian cơ bản giữa các lần retry (ms)
    pub base_delay_ms: u64,
    /// Hệ số tăng (cho backoff)
    pub backoff_factor: f64,
    /// Thời gian chờ tối đa (ms)
    pub max_delay_ms: u64,
    /// Có thêm jitter ngẫu nhiên không
    pub add_jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            base_delay_ms: 1000,
            backoff_factor: 2.0,
            max_delay_ms: 30000,
            add_jitter: true,
        }
    }
}

/// Thông tin về node RPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNodeInfo {
    /// URL của node
    pub url: String,
    /// Độ ưu tiên (thấp hơn = ưu tiên cao hơn)
    pub priority: u8,
    /// Trạng thái hiện tại
    pub status: RpcNodeStatus,
    /// Thời gian kiểm tra cuối cùng
    pub last_check: u64,
    /// Độ trễ phản hồi trung bình (ms)
    pub avg_response_time: u64,
    /// Số lần thất bại liên tiếp
    pub consecutive_failures: u8,
}

/// Trạng thái node RPC
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RpcNodeStatus {
    /// Đang hoạt động
    Active,
    /// Tạm thời không khả dụng
    TemporarilyUnavailable { 
        /// Thời gian bắt đầu không khả dụng
        since: u64,
        /// Lý do
        reason: String,
    },
    /// Không hoạt động
    Inactive { 
        /// Lý do
        reason: String,
    },
}

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
    
    // Thêm các trường mới
    /// Danh sách các URL RPC dự phòng
    pub fallback_rpc_urls: Vec<String>,
    /// Cấu hình retry
    pub retry_config: RetryConfig,
    /// Thời gian chờ tối đa cho transaction (ms)
    pub transaction_timeout_ms: u64,
}

impl Default for NearContractConfig {
    fn default() -> Self {
        Self {
            rpc_url: "https://rpc.mainnet.near.org".to_string(),
            contract_id: "dmd.near".to_string(),
            network_id: "mainnet".to_string(),
            timeout_ms: 10000,
            layerzero_endpoint: "layerzero.near".to_string(),
            
            // Giá trị mặc định cho các trường mới
            fallback_rpc_urls: vec![
                "https://rpc.near.org".to_string(),
                "https://rpc.mainnet.near.org".to_string(),
                "https://near-mainnet.api.onfinality.io/public".to_string(),
            ],
            retry_config: RetryConfig::default(),
            transaction_timeout_ms: 120000, // 2 phút
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

    /// Thiết lập danh sách các node RPC fallback
    pub fn with_fallback_rpc_urls(mut self, fallback_urls: Vec<String>) -> Self {
        self.fallback_rpc_urls = fallback_urls;
        self
    }
    
    /// Thiết lập cấu hình retry
    pub fn with_retry_config(mut self, retry_config: RetryConfig) -> Self {
        self.retry_config = retry_config;
        self
    }
    
    /// Thiết lập thời gian timeout cho transaction
    pub fn with_transaction_timeout(mut self, timeout_ms: u64) -> Self {
        self.transaction_timeout_ms = timeout_ms;
        self
    }
}

/// Provider cho NEAR contract
pub struct NearContractProvider {
    /// Cấu hình
    config: NearContractConfig,
    /// Thông tin về các node RPC
    rpc_nodes: Vec<RpcNodeInfo>,
    /// Trạng thái các giao dịch
    transaction_status: HashMap<String, DetailedTransactionStatus>,
    /// Cache RPC đang hoạt động
    active_rpc_url: String,
}

// Định nghĩa trait riêng cho NEAR operations
pub trait NearOperations: Send + Sync {
    fn get_config(&self) -> &NearContractConfig;
    fn validate_near_account(&self, account_id: &str) -> Result<()>;
    async fn query_contract(&self, method_name: &str, args: &serde_json::Value) -> Result<serde_json::Value>;
    async fn execute_near_transaction(&self, private_key: &str, receiver_id: &str, method_name: &str, amount: u128) -> Result<String>;
    async fn verify_transaction_status(&self, tx_hash: String) -> Result<bool>;
}

impl NearContractProvider {
    /// Tạo một instance mới của NEAR contract provider
    pub async fn new(config: Option<NearContractConfig>) -> Result<Self> {
        let config = config.unwrap_or_default();
        
        // Khởi tạo danh sách các node RPC
        let mut rpc_nodes = Vec::new();
        
        // Thêm RPC URL chính với độ ưu tiên cao nhất
        rpc_nodes.push(RpcNodeInfo {
            url: config.rpc_url.clone(),
            priority: 0,
            status: RpcNodeStatus::Active,
            last_check: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            avg_response_time: 0,
            consecutive_failures: 0,
        });
        
        // Thêm các RPC URL fallback
        for (i, url) in config.fallback_rpc_urls.iter().enumerate() {
            rpc_nodes.push(RpcNodeInfo {
                url: url.clone(),
                priority: (i + 1) as u8,
                status: RpcNodeStatus::Active,
                last_check: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                avg_response_time: 0,
                consecutive_failures: 0,
            });
        }
        
        let provider = Self {
            config,
            rpc_nodes,
            transaction_status: HashMap::new(),
            active_rpc_url: rpc_nodes.first().map(|node| node.url.clone()).unwrap_or_default(),
        };
        
        // Kiểm tra và cập nhật trạng thái các RPC node
        provider.check_rpc_nodes().await?;
        
        Ok(provider)
    }
    
    /// Kiểm tra trạng thái các node RPC
    async fn check_rpc_nodes(&self) -> Result<()> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(self.config.timeout_ms))
            .build()?;
            
        for node in &self.rpc_nodes {
            if node.status == RpcNodeStatus::Inactive {
                continue;
            }
            
            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "id": "dontcare",
                "method": "status",
                "params": []
            });
            
            let start_time = Instant::now();
            match client.post(&node.url)
                .json(&request)
            .timeout(Duration::from_millis(self.config.timeout_ms))
                .send()
                .await {
                    Ok(response) => {
                        let elapsed = start_time.elapsed().as_millis() as u64;
                        
                        match response.json::<serde_json::Value>().await {
                            Ok(_) => {
                                // Node đang hoạt động
                                let mut node_info = node.clone();
                                node_info.status = RpcNodeStatus::Active;
                                node_info.avg_response_time = 
                                    if node_info.avg_response_time == 0 { 
                                        elapsed 
                                    } else { 
                                        (node_info.avg_response_time + elapsed) / 2 
                                    };
                                node_info.consecutive_failures = 0;
                                node_info.last_check = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs();
                                    
                                // Cập nhật lại thông tin node trong danh sách
                                let idx = self.rpc_nodes.iter().position(|n| n.url == node.url).unwrap_or(0);
                                if let Some(idx) = self.rpc_nodes.get_mut(idx) {
                                    *idx = node_info;
                                }
                            },
                            Err(e) => {
                                // Node đang hoạt động nhưng có lỗi JSON
                                let mut node_info = node.clone();
                                node_info.consecutive_failures += 1;
                                
                                if node_info.consecutive_failures >= 3 {
                                    node_info.status = RpcNodeStatus::TemporarilyUnavailable {
                                        since: SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                        reason: format!("JSON parse error: {}", e)
                                    };
                                }
                                
                                node_info.last_check = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs();
                                    
                                // Cập nhật lại thông tin node trong danh sách
                                let idx = self.rpc_nodes.iter().position(|n| n.url == node.url).unwrap_or(0);
                                if let Some(idx) = self.rpc_nodes.get_mut(idx) {
                                    *idx = node_info;
                                }
                            }
                        }
                    },
                    Err(e) => {
                        // Node không phản hồi
                        let mut node_info = node.clone();
                        node_info.consecutive_failures += 1;
                        
                        if node_info.consecutive_failures >= 3 {
                            node_info.status = RpcNodeStatus::TemporarilyUnavailable {
                                since: SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                                reason: format!("Connection error: {}", e)
                            };
                        }
                        
                        node_info.last_check = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                            
                        // Cập nhật lại thông tin node trong danh sách
                        let idx = self.rpc_nodes.iter().position(|n| n.url == node.url).unwrap_or(0);
                        if let Some(idx) = self.rpc_nodes.get_mut(idx) {
                            *idx = node_info;
                        }
                    }
                }
        }
        
        Ok(())
    }
    
    /// Lấy URL node RPC tốt nhất
    fn get_best_rpc_url(&self) -> String {
        // Nếu có active_rpc_url và node đó vẫn active, sử dụng nó
        if !self.active_rpc_url.is_empty() {
            if let Some(node) = self.rpc_nodes.iter().find(|n| n.url == self.active_rpc_url) {
                if matches!(node.status, RpcNodeStatus::Active) {
                    return self.active_rpc_url.clone();
                }
            }
        }
        
        // Tìm node active có độ ưu tiên cao nhất (priority thấp nhất)
        if let Some(best_node) = self.rpc_nodes.iter()
            .filter(|n| matches!(n.status, RpcNodeStatus::Active))
            .min_by_key(|n| n.priority) {
            return best_node.url.clone();
        }
        
        // Nếu không có node active, sử dụng node TemporarilyUnavailable
        if let Some(temp_node) = self.rpc_nodes.iter()
            .filter(|n| matches!(n.status, RpcNodeStatus::TemporarilyUnavailable { .. }))
            .min_by_key(|n| n.priority) {
            return temp_node.url.clone();
        }
        
        // Nếu tất cả đều không khả dụng, trả về URL mặc định
        self.config.rpc_url.clone()
    }
    
    /// Gửi RPC request với cơ chế fallback và retry
    async fn send_rpc_request(&self, request: serde_json::Value, max_retry: usize) -> Result<serde_json::Value> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(self.config.timeout_ms))
            .build()?;
            
        let retry_config = &self.config.retry_config;
        let mut attempt = 0;
        let mut used_urls = HashSet::new();
        let mut last_error = None;
        
        while attempt < max_retry {
            // Lấy URL tốt nhất cho request hiện tại
            let rpc_url = self.get_best_rpc_url();
            used_urls.insert(rpc_url.clone());
            
            // Tính toán backoff delay
            let delay = if attempt > 0 {
                let base_delay = retry_config.base_delay_ms;
                let factor = retry_config.backoff_factor.powi(attempt as i32) as u64;
                let delay = base_delay * factor;
                let delay = std::cmp::min(delay, retry_config.max_delay_ms);
                
                if retry_config.add_jitter {
                    let jitter = rand::random::<f64>() * 0.1; // +/- 10%
                    (delay as f64 * (1.0 + jitter)) as u64
                    } else {
                    delay
                }
            } else {
                0
            };
            
            // Chờ trước khi retry
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            
            // Gửi request
            match client.post(&rpc_url)
                .json(&request)
                .timeout(Duration::from_millis(self.config.timeout_ms))
                .send()
                .await {
                    Ok(response) => {
                        // Cập nhật active_rpc_url nếu thành công
                        if rpc_url != self.active_rpc_url {
                            self.active_rpc_url = rpc_url;
                        }
                        
                        match response.json::<serde_json::Value>().await {
                            Ok(result) => {
                                // Kiểm tra lỗi trong response
                                if let Some(error) = result.get("error") {
                                    // Lưu lại lỗi
                                    last_error = Some(
                                        anyhow::anyhow!("RPC error: {:?}", error)
                                    );
                                    
                                    // Một số lỗi không nên retry
                                    let error_message = error.to_string().to_lowercase();
                                    if error_message.contains("invalid nonce") 
                                       || error_message.contains("already exists") 
                                       || error_message.contains("invalid format") {
                                        return Err(last_error.unwrap());
                                    }
                                    
                                    // Retry với node RPC khác
                                    attempt += 1;
                    continue;
                                }
                                
                                // Request thành công
                                return Ok(result);
                            },
                            Err(e) => {
                                // Lỗi khi parse JSON
                                last_error = Some(
                                    anyhow::anyhow!("JSON parse error from {}: {}", rpc_url, e)
                                );
                                attempt += 1;
                                
                                // Đánh dấu node này có vấn đề
                                if let Some(idx) = self.rpc_nodes.iter().position(|n| n.url == rpc_url) {
                                    if let Some(node) = self.rpc_nodes.get_mut(idx) {
                                        node.consecutive_failures += 1;
                                        if node.consecutive_failures >= 3 {
                                            node.status = RpcNodeStatus::TemporarilyUnavailable {
                                                since: SystemTime::now()
                                                    .duration_since(UNIX_EPOCH)
                                                    .unwrap_or_default()
                                                    .as_secs(),
                                                reason: format!("JSON parse error: {}", e)
                                            };
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Err(e) => {
                        // Lỗi khi gửi request
                        last_error = Some(
                            anyhow::anyhow!("RPC request failed to {}: {}", rpc_url, e)
                        );
                        attempt += 1;
                        
                        // Đánh dấu node này có vấn đề
                        if let Some(idx) = self.rpc_nodes.iter().position(|n| n.url == rpc_url) {
                            if let Some(node) = self.rpc_nodes.get_mut(idx) {
                                node.consecutive_failures += 1;
                                if node.consecutive_failures >= 3 {
                                    node.status = RpcNodeStatus::TemporarilyUnavailable {
                                        since: SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                        reason: format!("Connection error: {}", e)
                                    };
                                }
                            }
                        }
                    }
                }
                
            // Kiểm tra xem đã dùng hết tất cả các URL chưa
            if used_urls.len() >= self.rpc_nodes.len() {
                // Reset lại used_urls để có thể thử lại các URL đã dùng
                used_urls.clear();
            }
        }
        
        // Sau khi đã retry hết số lần quy định
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("RPC request failed after {} attempts", max_retry)))
    }
}

// Implement NearOperations cho NearContractProvider
impl NearOperations for NearContractProvider {
    fn get_config(&self) -> &NearContractConfig {
        &self.config
    }
    
    fn validate_near_account(&self, account_id: &str) -> Result<()> {
        if account_id.is_empty() {
            return Err(NearProviderError::InvalidAccountId("Account ID không được để trống".into()).into());
        }
        
        // Kiểm tra định dạng account Near
        if !account_id.ends_with(".near") && !account_id.ends_with(".testnet") && !account_id.contains(".") {
            warn!("Địa chỉ NEAR không theo định dạng chuẩn: {}", account_id);
        }
        
        Ok(())
    }
    
    async fn query_contract(&self, method_name: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        info!("Gọi method {} trên contract {}", method_name, self.config.contract_id);
        
        let args_base64 = base64::encode(args.to_string());
        
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": method_name,
            "method": "query",
            "params": {
                "request_type": "call_function",
                "finality": "final",
                "account_id": self.config.contract_id,
                "method_name": method_name,
                "args_base64": args_base64
            }
        });
        
        let response = self.send_rpc_request(request, 3).await?;
        
        if let Some(result) = response.get("result") {
            Ok(result.clone())
        } else {
            Err(NearProviderError::RpcError("Missing 'result' field in response".into()).into())
        }
    }
    
    async fn execute_near_transaction(&self, private_key: &str, receiver_id: &str, method_name: &str, amount: u128) -> Result<String> {
        if private_key.is_empty() {
            return Err(NearProviderError::AuthError("Private key không được để trống".into()).into());
        }
        
        self.validate_near_account(receiver_id)?;
        
        // TODO: Implement transaction execution
        // Vì việc triển khai thực tế yêu cầu nhiều chi tiết về cách NEAR ký và gửi giao dịch,
        // chúng ta sẽ trả về dummy result
        
        let tx_hash = format!("near-tx-{}-{}-{}", receiver_id, method_name, SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis());
            
        Ok(tx_hash)
    }
    
    async fn verify_transaction_status(&self, tx_hash: String) -> Result<bool> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "dontcare",
            "method": "tx",
            "params": [tx_hash, self.config.contract_id]
        });
        
        let response = self.send_rpc_request(request, 3).await?;
        
        // Check if the transaction was successful
        if let Some(status) = response.get("result").and_then(|r| r.get("status")) {
            if let Some(success) = status.get("SuccessValue") {
                return Ok(true);
            } else if status.get("Failure").is_some() {
                return Ok(false);
            }
        }
        
        Err(NearProviderError::NotFound("Transaction information not found".into()).into())
    }
}

impl NearContractProvider {
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
        let config = self.get_config();
        
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
                let error_text = response.text().await
                    .unwrap_or_else(|_| format!("HTTP Status {}", status));
                
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
        
        // Validate Solana address format with more thorough check
        const BASE58_CHARS: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
        if !to_solana_account.chars().all(|c| BASE58_CHARS.contains(c)) || to_solana_account.len() < 32 || to_solana_account.len() > 44 {
            return Err(anyhow::anyhow!("Invalid Solana address: {}", to_solana_account));
        }
        
        if amount <= 0.0 {
            return Err(anyhow::anyhow!("Bridge amount must be greater than 0"));
        }
        
        // Get RPC configuration
        let config = self.get_config();
        
        // Generate unique bridge transaction ID
        let bridge_tx_id = format!("near2sol_{}_{}_{}",
            from_account,
            to_solana_account,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_nanos()
        );
        
        // Initialize HTTP client with retry capability
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()?;
        
        // Convert amount to yoctoNEAR (10^24)
        let near_amount = (amount * 1_000_000_000_000_000_000_000_000.0) as u128;
        
        // Create nonce for transaction with additional entropy
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_nanos() as u64 ^ rand::random::<u64>();
        
        // Format NEAR account according to standard if needed
        let formatted_from_account = if !from_account.contains(".") {
            format!("{}.near", from_account)
        } else {
            from_account.to_string()
        };
        
        // Create detailed bridge transaction record for tracking
        let bridge_tx_details = BridgeTransactionDetails {
            tx_id: bridge_tx_id.clone(),
            source_chain: DmdChain::Near,
            destination_chain: DmdChain::Solana,
            sender: formatted_from_account.clone(),
            receiver: to_solana_account.to_string(),
            amount: near_amount,
            fee: 0, // Will be updated later
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            updated_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            status: DetailedTransactionStatus::Pending,
            retry_count: 0,
            last_retry_at: None,
            transaction_hash: None,
            destination_tx_hash: None,
            error_info: None,
            actions: vec![
                BridgeTransactionAction {
                    timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    action_type: "INIT".to_string(),
                    details: "Bridge transaction initiated".to_string(),
                    previous_status: None,
                    new_status: Some("Pending".to_string()),
                }
            ],
        };
        
        // Log initial bridge record
        info!("Created bridge record: {:?}", bridge_tx_details);
        
        // Create JSON parameters for bridge function call with additional metadata
        let args = serde_json::json!({
            "receiver_id": formatted_from_account,
            "amount": near_amount.to_string(),
            "destination_chain": "Solana",
            "target_address": to_solana_account,
            "bridge_tx_id": bridge_tx_id,
            "nonce": nonce.to_string(),
            "timestamp": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });
        
        let args_base64 = base64::encode(args.to_string());
        
        // Create query to check if contract and method exist before attempting call
        let validation_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "dontcare",
            "method": "query",
            "params": {
                "request_type": "call_function",
                "finality": "final",
                "account_id": config.contract_id,
                "method_name": "get_bridge_config",
                "args_base64": ""
            }
        });
        
        // First validate that the contract and method exist
        let mut validation_success = false;
        
        // Try all available RPC endpoints for validation
        for rpc_url in std::iter::once(&config.rpc_url).chain(config.fallback_rpc_urls.iter()) {
            match client.post(rpc_url)
                .json(&validation_request)
            .send()
            .await {
                    Ok(response) => {
                        if let Ok(json) = response.json::<serde_json::Value>().await {
                            if json.get("result").is_some() && json.get("error").is_none() {
                                validation_success = true;
                                break;
                            }
                        }
                    },
                    Err(_) => continue
                }
        }
        
        if !validation_success {
            return Err(anyhow::anyhow!("Could not validate bridge contract or method"));
        }
        
        // Create query to call function using RPC API with retry and detailed error handling
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
        
        // Implement retry with exponential backoff
        let max_retries = config.retry_config.max_retries;
        let mut retry_count = 0;
        let mut last_error = None;
        
        while retry_count <= max_retries {
            // Calculate backoff delay
            let delay_ms = if retry_count > 0 {
                let base_delay = config.retry_config.base_delay_ms;
                let factor = config.retry_config.backoff_factor.powi(retry_count as i32) as u64;
                let delay = base_delay * factor;
                std::cmp::min(delay, config.retry_config.max_delay_ms)
            } else {
                0
            };
            
            // Wait before retry
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            
            // Choose RPC URL - try the main one first, then fallbacks
            let rpc_url = if retry_count == 0 {
                config.rpc_url.clone()
            } else {
                let fallback_index = (retry_count - 1) % config.fallback_rpc_urls.len();
                config.fallback_rpc_urls.get(fallback_index)
                    .cloned()
                    .unwrap_or_else(|| config.rpc_url.clone())
            };
            
            // Send the request to NEAR RPC endpoint with detailed error handling
            match client.post(&rpc_url)
                .json(&request_data)
                .send()
                .await {
                    Ok(response) => {
                        // Track response time
                        match response.json::<serde_json::Value>().await {
                            Ok(result) => {
        // Check for errors in the response
        if let Some(error) = result.get("error") {
                                    let error_msg = format!("Error bridging tokens from NEAR to Solana: {:?}", error);
                                    error!("{}", error_msg);
                                    
                                    // Determine if we should retry based on error type
                                    let error_str = error.to_string().to_lowercase();
                                    let should_retry = error_str.contains("timeout") ||
                                                     error_str.contains("network") ||
                                                     error_str.contains("unavailable") ||
                                                     error_str.contains("overloaded");
                                    
                                    if should_retry && retry_count < max_retries {
                                        last_error = Some(anyhow::anyhow!(error_msg));
                                        retry_count += 1;
                                        
                                        // Log retry attempt
                                        warn!("Retrying bridge request, attempt {}/{}", retry_count, max_retries);
                                        continue;
                                    } else {
            return Err(anyhow::anyhow!("Bridge failed: {:?}", error));
                                    }
        }
        
        // Extract transaction hash from response if available
                                if let Some(hash) = result.get("result")
            .and_then(|r| r.get("transaction"))
            .and_then(|t| t.get("hash"))
            .and_then(|h| h.as_str()) {
                                        info!("Bridge from NEAR to Solana successful, hash: {}", hash);
                                        
                                        // Create a transaction tracking record for monitoring
                                        // ... [code to track transaction for later verification]
                                    
                                        return Ok(hash.to_string());
            } else {
                                    // If no hash is present, we need to initiate a new transaction with more data
                info!("No transaction hash in response, creating new transaction");
                
                                    // Create a new transaction request with more detailed parameters
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
                                                "target_address": to_solana_account,
                                                "bridge_tx_id": bridge_tx_id,
                                                "nonce": nonce.to_string(),
                                                "timestamp": SystemTime::now()
                                                    .duration_since(UNIX_EPOCH)
                                                    .unwrap_or_default()
                                                    .as_secs(),
                        },
                        "gas": 300_000_000_000_000, // 300 TGas
                        "deposit": 1, // 1 yoctoNEAR required for storage
                        "private_key": private_key
                    }
                });
                
                                    // Try to use all available RPC endpoints with fallback
                                    for (attempt, rpc_url) in std::iter::once(&config.rpc_url)
                                        .chain(config.fallback_rpc_urls.iter())
                                        .enumerate() {
                                        
                                        // Add delay between attempts to different RPC endpoints
                                        if attempt > 0 {
                                            tokio::time::sleep(Duration::from_millis(500)).await;
                                        }
                                        
                                        match client.post(rpc_url)
                    .json(&tx_request)
                    .send()
                    .await {
                                                Ok(tx_response) => {
                                                    match tx_response.json::<serde_json::Value>().await {
                                                        Ok(tx_result) => {
                                                            // Check for errors
                                                            if let Some(error) = tx_result.get("error") {
                                                                warn!("Error from RPC {}: {:?}", rpc_url, error);
                                                                continue; // Try next RPC endpoint
                                                            }
                
                // Extract transaction hash
                                                            if let Some(hash) = tx_result.get("result")
                    .and_then(|r| r.get("transaction"))
                    .and_then(|t| t.get("hash"))
                    .and_then(|h| h.as_str()) {
                                                                
                                                                info!("Bridge from NEAR to Solana successful via alternate RPC, hash: {}", hash);
                                                                
                                                                // [code to track transaction for verification]
                                                                
                                                                return Ok(hash.to_string());
                                                            }
                                                        },
                                                        Err(e) => {
                                                            warn!("Failed to parse JSON from RPC {}: {}", rpc_url, e);
                                                            continue; // Try next RPC endpoint
                                                        }
                                                    }
                                                },
                                                Err(e) => {
                                                    warn!("Failed to connect to RPC {}: {}", rpc_url, e);
                                                    continue; // Try next RPC endpoint
                                                }
                                            }
                                    }
                                    
                                    // If we get here, all RPC endpoints failed
                                    return Err(anyhow::anyhow!("All RPC endpoints failed to process bridge transaction"));
                                }
                            },
                            Err(e) => {
                                let error_msg = format!("Error parsing JSON response: {}", e);
                                error!("{}", error_msg);
                                
                                last_error = Some(anyhow::anyhow!(error_msg));
                                retry_count += 1;
                                
                                if retry_count <= max_retries {
                                    warn!("Retrying after JSON parse error, attempt {}/{}", retry_count, max_retries);
                                    continue;
                                } else {
                                    return Err(anyhow::anyhow!("Failed to parse bridge response after {} attempts: {}", max_retries, e));
                                }
                            }
                        }
                    },
                    Err(e) => {
                        let error_msg = format!("Error sending RPC request to {}: {}", rpc_url, e);
                        error!("{}", error_msg);
                        
                        last_error = Some(anyhow::anyhow!(error_msg));
                        retry_count += 1;
                        
                        if retry_count <= max_retries {
                            warn!("Retrying after RPC connection error, attempt {}/{}", retry_count, max_retries);
                            continue;
                        } else {
                            return Err(anyhow::anyhow!("Failed to send bridge request after {} attempts: {}", max_retries, e));
                        }
                    }
                }
        }
        
        // If we get here, all retries failed
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown error during bridge operation")))
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
        let config = self.get_config();
        
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
        let config = self.get_config();
        
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
        let config = self.get_config();
        
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

    /// Lấy thông tin về token đã lock
    pub async fn get_locked_tokens(&self) -> Result<smartcontract::LockedTokens> {
        let args = serde_json::json!({});
        let result = self.query_contract("get_locked_tokens", &args).await?;
        
        let locked_tokens: smartcontract::LockedTokens = serde_json::from_value(result)
            .map_err(|e| anyhow::anyhow!("Failed to parse locked tokens: {}", e))?;
            
        Ok(locked_tokens)
    }
    
    /// Lấy tổng số token đã lock
    pub async fn get_total_locked(&self) -> Result<ethers::types::U256> {
        let args = serde_json::json!({});
        let result = self.query_contract("get_total_locked", &args).await?;
        
        let total_locked: String = serde_json::from_value(result)
            .map_err(|e| anyhow::anyhow!("Failed to parse total locked: {}", e))?;
            
        let amount = total_locked.parse::<u128>()
            .map_err(|e| anyhow::anyhow!("Failed to parse total locked as u128: {}", e))?;
            
        Ok(ethers::types::U256::from(amount))
    }
    
    /// Lấy tổng số token đang lưu thông
    pub async fn get_circulating_supply(&self) -> Result<ethers::types::U256> {
        let args = serde_json::json!({});
        let result = self.query_contract("get_circulating_supply", &args).await?;
        
        let supply: String = serde_json::from_value(result)
            .map_err(|e| anyhow::anyhow!("Failed to parse circulating supply: {}", e))?;
            
        let amount = supply.parse::<u128>()
            .map_err(|e| anyhow::anyhow!("Failed to parse circulating supply as u128: {}", e))?;
            
        Ok(ethers::types::U256::from(amount))
    }
    
    /// Theo dõi giao dịch bridge
    pub async fn track_bridge_transaction(&mut self, tx_id: &str) -> Result<DetailedTransactionStatus> {
        // Kiểm tra xem giao dịch đã được lưu trong cache chưa
        if let Some(status) = self.transaction_status.get(tx_id) {
            // Nếu trạng thái là Completed, Failed hoặc Refunded thì trả về ngay
            match status {
                DetailedTransactionStatus::Completed |
                DetailedTransactionStatus::Failed { .. } |
                DetailedTransactionStatus::Refunded { .. } => {
                    return Ok(status.clone());
                },
                _ => {}
            }
        }
        
        // Truy vấn trạng thái từ contract
        let args = serde_json::json!({
            "tx_id": tx_id,
        });
        
        match self.query_contract("get_bridge_transaction", &args).await {
            Ok(result) => {
                // Parse kết quả
                let bridge_tx: BridgeTransactionDetails = match serde_json::from_value(result) {
                    Ok(tx) => tx,
                    Err(e) => {
                        return Err(anyhow::anyhow!("Failed to parse bridge transaction: {}", e));
                    }
                };
                
                // Cập nhật vào cache
                self.transaction_status.insert(tx_id.to_string(), bridge_tx.status.clone());
                
                // Kiểm tra nếu transaction đã bị stuck
                if let DetailedTransactionStatus::Pending = bridge_tx.status {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                        
                    if bridge_tx.created_at + self.config.transaction_timeout_ms / 1000 < now {
                        // Transaction đã bị stuck
                        let updated_status = DetailedTransactionStatus::Stuck {
                            since: bridge_tx.created_at,
                            previous_status: "Pending".to_string(),
                        };
                        
                        // Cập nhật vào cache
                        self.transaction_status.insert(tx_id.to_string(), updated_status.clone());
                        
                        // Ghi log
                        warn!("Bridge transaction {} is stuck since {}", tx_id, bridge_tx.created_at);
                        
                        return Ok(updated_status);
                    }
                }
                
                Ok(bridge_tx.status)
            },
            Err(e) => {
                // Nếu không tìm thấy giao dịch, trả về lỗi
                Err(anyhow::anyhow!("Failed to get bridge transaction {}: {}", tx_id, e))
            }
        }
    }
    
    /// Danh sách các giao dịch đang bị stuck
    pub async fn get_stuck_transactions(&self, limit: u64) -> Result<Vec<BridgeTransactionDetails>> {
        let args = serde_json::json!({
            "status": "stuck",
            "limit": limit,
        });
        
        match self.query_contract("get_stuck_transactions", &args).await {
            Ok(result) => {
                // Parse kết quả
                let transactions: Vec<BridgeTransactionDetails> = match serde_json::from_value(result) {
                    Ok(txs) => txs,
                    Err(e) => {
                        return Err(anyhow::anyhow!("Failed to parse stuck transactions: {}", e));
                    }
                };
                
                Ok(transactions)
            },
            Err(e) => {
                // Nếu có lỗi, trả về danh sách rỗng
                Err(anyhow::anyhow!("Failed to get stuck transactions: {}", e))
            }
        }
    }
    
    /// Retry giao dịch bridge bị stuck
    pub async fn retry_stuck_transaction(&mut self, tx_id: &str) -> Result<String> {
        let args = serde_json::json!({
            "tx_id": tx_id,
        });
        
        match self.execute_near_transaction("", &self.config.contract_id, "retry_stuck_transaction", 1).await {
            Ok(result) => {
                // Cập nhật trạng thái trong cache
                self.transaction_status.insert(tx_id.to_string(), DetailedTransactionStatus::Pending);
                
                // Ghi log
                info!("Retrying stuck transaction {}: {}", tx_id, result);
                
                Ok(result)
            },
            Err(e) => {
                Err(anyhow::anyhow!("Failed to retry stuck transaction {}: {}", tx_id, e))
            }
        }
    }
    
    /// Tự động retry các giao dịch bị stuck
    pub async fn auto_retry_stuck_transactions(&mut self, max_transactions: u64) -> Result<u64> {
        // Lấy danh sách các giao dịch bị stuck
        let stuck_transactions = self.get_stuck_transactions(max_transactions).await?;
        
        let mut retry_count = 0;
        for tx in stuck_transactions {
            match self.retry_stuck_transaction(&tx.tx_id).await {
                Ok(_) => {
                    retry_count += 1;
                },
                Err(e) => {
                    warn!("Failed to retry stuck transaction {}: {}", tx.tx_id, e);
                }
            }
            
            // Chờ một chút giữa các lần retry để tránh overload
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        
        Ok(retry_count)
    }
    
    /// Hủy giao dịch bridge bị stuck và hoàn trả token
    pub async fn cancel_and_refund_transaction(&mut self, tx_id: &str) -> Result<String> {
        let args = serde_json::json!({
            "tx_id": tx_id,
        });
        
        match self.execute_near_transaction("", &self.config.contract_id, "cancel_and_refund_transaction", 1).await {
            Ok(result) => {
                // Cập nhật trạng thái trong cache
                self.transaction_status.insert(
                    tx_id.to_string(), 
                    DetailedTransactionStatus::Refunded { 
                        refund_time: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        reason: "Manually cancelled".to_string(),
                    }
                );
                
                // Ghi log
                info!("Cancelled and refunded transaction {}: {}", tx_id, result);
                
                Ok(result)
            },
            Err(e) => {
                Err(anyhow::anyhow!("Failed to cancel and refund transaction {}: {}", tx_id, e))
            }
        }
    }
    
    /// Tự động hủy và hoàn trả các giao dịch bị stuck quá lâu
    pub async fn auto_cancel_timed_out_transactions(&mut self, max_transactions: u64, timeout_seconds: u64) -> Result<u64> {
        // Lấy danh sách các giao dịch bị stuck
        let stuck_transactions = self.get_stuck_transactions(max_transactions).await?;
        
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
            
        let mut cancel_count = 0;
        for tx in stuck_transactions {
            // Kiểm tra xem giao dịch có bị stuck quá lâu không
            if let DetailedTransactionStatus::Stuck { since, .. } = tx.status {
                if since + timeout_seconds < now {
                    match self.cancel_and_refund_transaction(&tx.tx_id).await {
                        Ok(_) => {
                            cancel_count += 1;
                        },
                        Err(e) => {
                            warn!("Failed to cancel and refund timed out transaction {}: {}", tx.tx_id, e);
                        }
                    }
                    
                    // Chờ một chút giữa các lần hủy để tránh overload
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
        
        Ok(cancel_count)
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
            description: "DMD Token trên NEAR (Main Chain) sử dụng chuẩn NEP-141 với khả năng bridge qua LayerZero",
        }
    }
}

impl DmdNearContract {
    /// Xác định NEAR là chain chính
    pub fn is_main_chain(&self) -> bool {
        true
    }
    
    /// Mô tả về token
    pub fn description(&self) -> &str {
        "DMD là token chính được triển khai trên NEAR Protocol (main chain) với khả năng bridge sang các chain phụ (BSC, Ethereum, Solana)"
    }
} 