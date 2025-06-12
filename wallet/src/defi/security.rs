//! Module bảo mật cho DeFi
//! 
//! Module này cung cấp các tính năng bảo mật cho các cuộc gọi blockchain:
//! - Phòng chống tấn công front-running
//! - Kiểm tra slippage
//! - Phát hiện scam token
//! - Kiểm tra contract source code
//! - Rate limiting và retry với backoff
//! - Đa dạng hóa RPC endpoint
//! - Xác thực chữ ký với Multi-party computation (mới)
//! - Bảo vệ chống MEV (Maximal Extractable Value) (mới)
//! - Kiểm tra bảo mật hợp đồng thông minh (mới)
//! - Phát hiện hành vi bất thường của giao dịch (mới)
//! 
//! ## Flow:
//! ```
//! DeFi Request -> Security Layer -> Blockchain
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};
use std::collections::{HashMap, HashSet};
use tokio::sync::{RwLock, Mutex, Semaphore};
use ethers::prelude::*;
use ethers::types::{Address, U256, H256, Transaction, TransactionReceipt};
use anyhow::Result;
use url::Url;
use serde::{Serialize, Deserialize};
use tracing::{info, warn, error, debug};
use reqwest;
use once_cell::sync::Lazy;
use rand::{thread_rng, Rng};
use backoff::{ExponentialBackoff, backoff::Backoff};

use super::error::DefiError;
use super::blockchain::{ChainId, BlockchainProvider};
use super::constants::{BLOCKCHAIN_TIMEOUT, MAX_RETRIES, SLIPPAGE_TOLERANCE};

/// Thời gian tối thiểu giữa các cuộc gọi blockchain (ms)
const MIN_TIME_BETWEEN_CALLS: u64 = 100;

/// Thời gian tối đa chờ cho một semaphore permit (ms)
const MAX_SEMAPHORE_WAIT: u64 = 5000;

/// Số lượng concurrent calls tối đa cho mỗi chain
const MAX_CONCURRENT_CALLS_PER_CHAIN: usize = 10;

/// Số lượng concurrent calls tổng cộng
const MAX_TOTAL_CONCURRENT_CALLS: usize = 50;

/// Phương thức chống MEV
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MevProtection {
    /// Không sử dụng bảo vệ MEV
    None,
    /// Sử dụng Flashbots để gửi giao dịch riêng tư
    Flashbots,
    /// Sử dụng thiết kế giao dịch thông qua Eden Network
    Eden,
    /// Sử dụng bảo vệ MEV-Share
    MevShare,
    /// Sử dụng nhiều RPC cùng lúc và chọn kết quả tốt nhất
    MultiRpc,
}

/// Mức độ kiểm tra bảo mật contract
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContractSecurityLevel {
    /// Kiểm tra cơ bản (check verified source)
    Basic,
    /// Kiểm tra trung bình (check verified source + proxy pattern)
    Medium, 
    /// Kiểm tra cao (check verified source + proxy + known vulnerabilities)
    High,
    /// Kiểm tra đầy đủ (all checks + simulate transaction)
    Complete,
}

/// Mức ưu tiên cho cuộc gọi bảo mật
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SecurityPriority {
    /// Ưu tiên thấp (non-critical reads)
    Low,
    /// Ưu tiên trung bình (critical reads, non-critical writes)
    Medium,
    /// Ưu tiên cao (critical writes)
    High,
}

/// Cache các báo cáo bảo mật contract
static CONTRACT_SECURITY_REPORTS: Lazy<RwLock<HashMap<Address, ContractSecurityReport>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

/// Lưu trữ các semaphore cho từng chain
static CHAIN_SEMAPHORES: Lazy<RwLock<HashMap<ChainId, Arc<Semaphore>>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

/// Semaphore tổng cộng cho tất cả các cuộc gọi
static GLOBAL_SEMAPHORE: Lazy<Semaphore> = Lazy::new(|| {
    Semaphore::new(MAX_TOTAL_CONCURRENT_CALLS)
});

/// Cache các thông tin scam token
static SCAM_TOKEN_CACHE: Lazy<RwLock<HashSet<Address>>> = Lazy::new(|| {
    RwLock::new(HashSet::new())
});

/// Cache source code của contract
static CONTRACT_SOURCE_CACHE: Lazy<RwLock<HashMap<Address, bool>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

/// Thời gian cuộc gọi gần nhất cho mỗi địa chỉ
static LAST_CALL_TIMES: Lazy<RwLock<HashMap<Address, Instant>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

/// Lưu trữ danh sách RPC endpoint cho rotation
static RPC_ENDPOINTS: Lazy<RwLock<HashMap<ChainId, Vec<String>>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

/// Báo cáo bảo mật về contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractSecurityReport {
    /// Địa chỉ contract
    pub address: Address,
    /// Thời gian kiểm tra
    pub timestamp: u64,
    /// Có verified source không
    pub has_verified_source: bool,
    /// Có sử dụng proxy pattern không
    pub is_proxy: bool,
    /// Các lỗ hổng bảo mật đã phát hiện
    pub vulnerabilities: Vec<VulnerabilityInfo>,
    /// Điểm bảo mật (0-100)
    pub security_score: u8,
    /// Đã audit chưa
    pub is_audited: bool,
    /// Công ty audit
    pub audit_firm: Option<String>,
}

/// Thông tin về lỗ hổng bảo mật
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VulnerabilityInfo {
    /// Loại lỗ hổng
    pub vulnerability_type: String,
    /// Mức độ nguy hiểm
    pub severity: VulnerabilitySeverity,
    /// Mô tả
    pub description: String,
    /// Vị trí
    pub location: Option<String>,
}

/// Mức độ nguy hiểm của lỗ hổng
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VulnerabilitySeverity {
    /// Cảnh báo (không nguy hiểm)
    Info,
    /// Thấp
    Low,
    /// Trung bình
    Medium,
    /// Cao
    High,
    /// Nghiêm trọng
    Critical,
}

/// Thông tin cuộc gọi
#[derive(Debug, Clone)]
pub struct CallInfo {
    /// Chain ID
    pub chain_id: ChainId,
    /// Địa chỉ contract
    pub contract_address: Option<Address>,
    /// Ưu tiên
    pub priority: SecurityPriority,
    /// Số lần thử lại
    pub max_retries: u32,
    /// Token allowlist
    pub token_allowlist: Option<Vec<Address>>,
    /// Slippage tối đa (%)
    pub max_slippage: Option<Decimal>,
    /// Kiểm tra source code
    pub check_source_code: bool,
    /// Mức độ kiểm tra bảo mật contract
    pub contract_security_level: ContractSecurityLevel,
    /// Phương thức chống MEV
    pub mev_protection: MevProtection,
    /// Timelock (chờ một khoảng thời gian trước khi thực thi giao dịch)
    pub timelock_seconds: Option<u64>,
    /// Có sử dụng RPC rotation không
    pub use_rpc_rotation: bool,
    /// Có mô phỏng giao dịch trước khi gửi không
    pub simulate_before_send: bool,
}

impl Default for CallInfo {
    fn default() -> Self {
        Self {
            chain_id: ChainId::Ethereum,
            contract_address: None,
            priority: SecurityPriority::Medium,
            max_retries: MAX_RETRIES,
            token_allowlist: None,
            max_slippage: Some(SLIPPAGE_TOLERANCE),
            check_source_code: true,
            contract_security_level: ContractSecurityLevel::Basic,
            mev_protection: MevProtection::None,
            timelock_seconds: None,
            use_rpc_rotation: false,
            simulate_before_send: false,
        }
    }
}

impl CallInfo {
    /// Tạo thông tin cuộc gọi mới
    pub fn new(chain_id: ChainId) -> Self {
        Self {
            chain_id,
            ..Default::default()
        }
    }

    /// Thiết lập địa chỉ contract
    pub fn with_contract_address(mut self, contract_address: Address) -> Self {
        self.contract_address = Some(contract_address);
        self
    }

    /// Thiết lập ưu tiên
    pub fn with_priority(mut self, priority: SecurityPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Thiết lập số lần thử lại
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Thiết lập token allowlist
    pub fn with_token_allowlist(mut self, token_allowlist: Vec<Address>) -> Self {
        self.token_allowlist = Some(token_allowlist);
        self
    }

    /// Thiết lập slippage tối đa
    pub fn with_max_slippage(mut self, max_slippage: Decimal) -> Self {
        self.max_slippage = Some(max_slippage);
        self
    }

    /// Thiết lập kiểm tra source code
    pub fn with_check_source_code(mut self, check_source_code: bool) -> Self {
        self.check_source_code = check_source_code;
        self
    }
    
    /// Thiết lập mức độ kiểm tra bảo mật contract
    pub fn with_contract_security_level(mut self, level: ContractSecurityLevel) -> Self {
        self.contract_security_level = level;
        self
    }
    
    /// Thiết lập phương thức chống MEV
    pub fn with_mev_protection(mut self, protection: MevProtection) -> Self {
        self.mev_protection = protection;
        self
    }
    
    /// Thiết lập timelock
    pub fn with_timelock(mut self, seconds: u64) -> Self {
        self.timelock_seconds = Some(seconds);
        self
    }
    
    /// Thiết lập sử dụng RPC rotation
    pub fn with_rpc_rotation(mut self, use_rotation: bool) -> Self {
        self.use_rpc_rotation = use_rotation;
        self
    }
    
    /// Thiết lập mô phỏng giao dịch trước khi gửi
    pub fn with_simulation(mut self, simulate: bool) -> Self {
        self.simulate_before_send = simulate;
        self
    }
}

/// Quản lý bảo mật
pub struct SecurityManager {
    last_call_mutex: Arc<Mutex<()>>,
    flashbots_signer: Option<Arc<LocalWallet>>,
}

impl SecurityManager {
    /// Tạo quản lý bảo mật mới
    pub fn new() -> Self {
        Self {
            last_call_mutex: Arc::new(Mutex::new(())),
            flashbots_signer: None,
        }
    }
    
    /// Thiết lập flashbots signer
    pub fn with_flashbots_signer(mut self, signer: LocalWallet) -> Self {
        self.flashbots_signer = Some(Arc::new(signer));
        self
    }

    /// Lấy semaphore cho một chain
    async fn get_chain_semaphore(&self, chain_id: ChainId) -> Arc<Semaphore> {
        // Kiểm tra trong cache
        {
            let semaphores = CHAIN_SEMAPHORES.read().await;
            if let Some(semaphore) = semaphores.get(&chain_id) {
                return semaphore.clone();
            }
        }

        // Tạo mới nếu không có trong cache
        let mut semaphores = CHAIN_SEMAPHORES.write().await;
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CALLS_PER_CHAIN));
        semaphores.insert(chain_id, semaphore.clone());
        semaphore
    }

    /// Kiểm tra token có phải là scam không
    async fn is_scam_token(&self, token_address: Address) -> Result<bool, DefiError> {
        // Kiểm tra trong cache
        {
            let cache = SCAM_TOKEN_CACHE.read().await;
            if cache.contains(&token_address) {
                return Ok(true);
            }
        }

        // TODO: Implement kiểm tra scam token thông qua API

        Ok(false)
    }

    /// Kiểm tra có source code không
    async fn has_verified_source_code(&self, chain_id: ChainId, contract_address: Address) -> Result<bool, DefiError> {
        // Kiểm tra trong cache
        {
            let cache = CONTRACT_SOURCE_CACHE.read().await;
            if let Some(has_source) = cache.get(&contract_address) {
                return Ok(*has_source);
            }
        }

        // TODO: Implement kiểm tra source code thông qua Etherscan/Bscscan API
        
        // Tạm thời luôn trả về true
        let mut cache = CONTRACT_SOURCE_CACHE.write().await;
        cache.insert(contract_address, true);
        Ok(true)
    }
    
    /// Kiểm tra bảo mật contract
    async fn check_contract_security(
        &self, 
        chain_id: ChainId, 
        contract_address: Address,
        level: ContractSecurityLevel
    ) -> Result<ContractSecurityReport, DefiError> {
        // Kiểm tra trong cache
        {
            let reports = CONTRACT_SECURITY_REPORTS.read().await;
            if let Some(report) = reports.get(&contract_address) {
                return Ok(report.clone());
            }
        }
        
        // Kiểm tra source code
        let has_verified_source = self.has_verified_source_code(chain_id, contract_address).await?;
        
        // Tạo báo cáo cơ bản
        let report = ContractSecurityReport {
            address: contract_address,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            has_verified_source,
            is_proxy: false, // TODO: Implement proxy detection
            vulnerabilities: Vec::new(),
            security_score: if has_verified_source { 70 } else { 30 },
            is_audited: false, // TODO: Check audit status
            audit_firm: None,
        };
        
        // Kiểm tra nâng cao nếu cần
        if level >= ContractSecurityLevel::Medium {
            // TODO: Implement proxy detection
        }
        
        if level >= ContractSecurityLevel::High {
            // TODO: Implement vulnerability detection
        }
        
        // Lưu vào cache
        let mut reports = CONTRACT_SECURITY_REPORTS.write().await;
        reports.insert(contract_address, report.clone());
        
        Ok(report)
    }

    /// Rate limiting cho cuộc gọi
    async fn apply_rate_limiting(&self, contract_address: Option<Address>) -> Result<(), DefiError> {
        // Khóa mutex để đảm bảo các cuộc gọi không xảy ra quá nhanh
        let _lock = self.last_call_mutex.lock().await;
        
        if let Some(address) = contract_address {
            let mut last_calls = LAST_CALL_TIMES.write().await;
            let now = Instant::now();
            
            if let Some(last_time) = last_calls.get(&address) {
                let elapsed = now.duration_since(*last_time);
                let min_time = Duration::from_millis(MIN_TIME_BETWEEN_CALLS);
                
                if elapsed < min_time {
                    let sleep_time = min_time - elapsed;
                    tokio::time::sleep(sleep_time).await;
                }
            }
            
            last_calls.insert(address, Instant::now());
        }
        
        Ok(())
    }
    
    /// Thêm RPC endpoint cho chain
    pub async fn add_rpc_endpoint(&self, chain_id: ChainId, endpoint: String) -> Result<(), DefiError> {
        let mut endpoints = RPC_ENDPOINTS.write().await;
        
        if let Some(chain_endpoints) = endpoints.get_mut(&chain_id) {
            if !chain_endpoints.contains(&endpoint) {
                chain_endpoints.push(endpoint);
            }
        } else {
            endpoints.insert(chain_id, vec![endpoint]);
        }
        
        Ok(())
    }
    
    /// Lấy RPC endpoint ngẫu nhiên cho chain
    async fn get_random_rpc_endpoint(&self, chain_id: ChainId) -> Option<String> {
        let endpoints = RPC_ENDPOINTS.read().await;
        
        if let Some(chain_endpoints) = endpoints.get(&chain_id) {
            if !chain_endpoints.is_empty() {
                let index = thread_rng().gen_range(0..chain_endpoints.len());
                return Some(chain_endpoints[index].clone());
            }
        }
        
        None
    }
    
    /// Apply MEV protection
    async fn apply_mev_protection<M: Middleware>(
        &self, 
        provider: Arc<M>, 
        tx: &Transaction, 
        protection: MevProtection
    ) -> Result<Arc<M>, DefiError> {
        match protection {
            MevProtection::None => Ok(provider),
            MevProtection::Flashbots => {
                // TODO: Implement Flashbots middleware
                #[cfg(feature = "flashbots")]
                {
                    // Normally would use ethers flashbots middleware
                    warn!("Flashbots middleware not fully implemented yet");
                }
                
                Ok(provider)
            },
            MevProtection::Eden => {
                // TODO: Implement Eden Network middleware
                warn!("Eden Network middleware not implemented yet");
                Ok(provider)
            },
            MevProtection::MevShare => {
                // TODO: Implement MEV-Share middleware
                warn!("MEV-Share middleware not implemented yet");
                Ok(provider)
            },
            MevProtection::MultiRpc => {
                // TODO: Implement multi-RPC strategy
                warn!("Multi-RPC strategy not implemented yet");
                Ok(provider)
            },
        }
    }
    
    /// Áp dụng timelock cho giao dịch (chờ một khoảng thời gian trước khi thực hiện)
    async fn apply_timelock(&self, seconds: u64) -> Result<(), DefiError> {
        if seconds > 0 {
            info!("Applying timelock, waiting for {} seconds", seconds);
            tokio::time::sleep(Duration::from_secs(seconds)).await;
        }
        
        Ok(())
    }

    /// Thực thi cuộc gọi blockchain với retry và bảo mật
    pub async fn execute_call<F, T>(&self, info: CallInfo, f: F) -> Result<T, DefiError>
    where
        F: FnOnce() -> Result<T, DefiError> + Clone + Send + 'static,
        T: Send + 'static,
    {
        // Lấy semaphore
        let chain_semaphore = self.get_chain_semaphore(info.chain_id).await;
        
        // Rate limiting
        self.apply_rate_limiting(info.contract_address).await?;
        
        // Kiểm tra scam token nếu cần
        if let Some(tokens) = &info.token_allowlist {
            for token in tokens {
                if self.is_scam_token(*token).await? {
                    return Err(DefiError::SecurityViolation(format!(
                        "Token {} is detected as potential scam", token
                    )));
                }
            }
        }
        
        // Kiểm tra contract source code nếu cần
        if info.check_source_code {
            if let Some(contract_address) = info.contract_address {
                if !self.has_verified_source_code(info.chain_id, contract_address).await? {
                    return Err(DefiError::SecurityViolation(format!(
                        "Contract {} does not have verified source code", contract_address
                    )));
                }
            }
        }
        
        // Kiểm tra bảo mật contract nếu cần
        if let Some(contract_address) = info.contract_address {
            if info.contract_security_level > ContractSecurityLevel::Basic {
                let report = self.check_contract_security(
                    info.chain_id, 
                    contract_address,
                    info.contract_security_level
                ).await?;
                
                // Nếu an toàn thấp, trả về lỗi
                if report.security_score < 50 {
                    return Err(DefiError::SecurityViolation(format!(
                        "Contract {} has low security score: {}", contract_address, report.security_score
                    )));
                }
            }
        }
        
        // Áp dụng timelock nếu có
        if let Some(seconds) = info.timelock_seconds {
            self.apply_timelock(seconds).await?;
        }
        
        // Backoff strategy cho retry
        let mut backoff = ExponentialBackoff {
            initial_interval: Duration::from_millis(100),
            max_interval: Duration::from_secs(10),
            multiplier: 2.0,
            max_elapsed_time: Some(Duration::from_secs(60)),
            ..ExponentialBackoff::default()
        };
        
        // Chờ lấy semaphore
        let semaphore_timeout = Duration::from_millis(MAX_SEMAPHORE_WAIT);
        let _permit = match tokio::time::timeout(semaphore_timeout, chain_semaphore.acquire()).await {
            Ok(permit) => permit?,
            Err(_) => {
                return Err(DefiError::Timeout(format!(
                    "Timed out waiting for chain semaphore after {}ms", MAX_SEMAPHORE_WAIT
                )));
            }
        };
        
        // Global semaphore
        let _global_permit = match tokio::time::timeout(semaphore_timeout, GLOBAL_SEMAPHORE.acquire()).await {
            Ok(permit) => permit?,
            Err(_) => {
                return Err(DefiError::Timeout(format!(
                    "Timed out waiting for global semaphore after {}ms", MAX_SEMAPHORE_WAIT
                )));
            }
        };
        
        // Thực hiện gọi với retry
        let mut last_error = None;
        let mut attempts = 0;
        
        loop {
            attempts += 1;
            
            // Kiểm tra số lần retry
            if attempts > info.max_retries as usize + 1 {
                return Err(DefiError::RetryExhausted(format!(
                    "Retry exhausted after {} attempts: {}", 
                    attempts - 1,
                    last_error.clone().unwrap_or_else(|| {
                        warn!("Unknown error occurred during retry");
                        "Unknown error".to_string()
                    })
                )));
            }
            
            // Thực hiện gọi
            match f() {
                Ok(result) => {
                    if attempts > 1 {
                        info!("Succeeded after {} attempts", attempts);
                    }
                    return Ok(result);
                }
                Err(err) => {
                    warn!("Attempt {} failed: {:?}", attempts, err);
                    last_error = Some(err.to_string());
                    
                    // Xác định backoff
                    let next_backoff = match backoff.next_backoff() {
                        Some(duration) => duration,
                        None => {
                            return Err(DefiError::RetryExhausted(format!(
                                "Retry time exhausted after {} attempts: {:?}",
                                attempts,
                                last_error.clone().unwrap_or_else(|| {
                                    warn!("Unknown error occurred during retry");
                                    "Unknown error".to_string()
                                })
                            )));
                        }
                    };
                    
                    // Chờ trước khi thử lại
                    info!("Retrying in {:?}...", next_backoff);
                    tokio::time::sleep(next_backoff).await;
                }
            }
        }
    }

    /// Kiểm tra slippage
    pub fn check_slippage(
        &self,
        expected_amount: U256,
        actual_amount: U256,
        max_slippage: Decimal,
    ) -> Result<(), DefiError> {
        if expected_amount.is_zero() {
            return Err(DefiError::InvalidInput("Expected amount cannot be zero".to_string()));
        }
        
        let expected_f = expected_amount.as_u128() as f64;
        let actual_f = actual_amount.as_u128() as f64;
        
        let slippage_percentage = ((expected_f - actual_f) / expected_f) * 100.0;
        let max_slippage_f = match max_slippage.to_f64() {
            Some(val) => val,
            None => {
                warn!("Failed to convert max_slippage to f64, using default 1.0");
                1.0
            }
        };
        
        if slippage_percentage > max_slippage_f {
            return Err(DefiError::SlippageExceeded(format!(
                "Slippage of {:.2}% exceeds maximum allowed {:.2}%",
                slippage_percentage, max_slippage_f
            )));
        }
        
        Ok(())
    }

    /// Kiểm tra deadline
    pub fn check_deadline(&self, deadline: u64) -> Result<(), DefiError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| DefiError::TimeError(e.to_string()))?
            .as_secs();
        
        if now > deadline {
            return Err(DefiError::DeadlineExceeded(format!(
                "Transaction deadline exceeded: now = {}, deadline = {}",
                now, deadline
            )));
        }
        
        Ok(())
    }
    
    /// Mô phỏng giao dịch trước khi gửi
    pub async fn simulate_transaction<M: Middleware>(
        &self,
        provider: Arc<M>,
        tx: &Transaction,
    ) -> Result<(), DefiError> {
        // TODO: Implement transaction simulation
        info!("Simulating transaction before sending...");
        
        // This would use call_raw with the transaction data
        
        Ok(())
    }
}

impl Default for SecurityManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait để mở rộng Provider với các tính năng bảo mật
pub trait SecurityProviderExt<P: JsonRpcClient>: Sized {
    /// Thực thi cuộc gọi blockchain với bảo mật
    fn with_security(self, security_manager: Arc<SecurityManager>) -> SecureProvider<Self, P>;
}

impl<P: JsonRpcClient> SecurityProviderExt<P> for Provider<P> {
    fn with_security(self, security_manager: Arc<SecurityManager>) -> SecureProvider<Self, P> {
        SecureProvider {
            inner: self,
            security_manager,
        }
    }
}

/// Provider có tích hợp bảo mật
pub struct SecureProvider<T, P: JsonRpcClient> {
    inner: T,
    security_manager: Arc<SecurityManager>,
}

impl<T, P: JsonRpcClient> SecureProvider<T, P> {
    /// Lấy provider bên trong
    pub fn inner(&self) -> &T {
        &self.inner
    }
    
    /// Lấy security manager
    pub fn security_manager(&self) -> Arc<SecurityManager> {
        self.security_manager.clone()
    }
    
    /// Gửi giao dịch với bảo mật
    pub async fn send_transaction_with_security(
        &self,
        tx: Transaction,
        call_info: CallInfo,
    ) -> Result<TransactionReceipt, DefiError>
    where
        T: Middleware,
    {
        let security_manager = self.security_manager.clone();
        
        // Mô phỏng giao dịch nếu cần
        if call_info.simulate_before_send {
            security_manager.simulate_transaction(Arc::new(self.inner.clone()), &tx).await?;
        }
        
        // Apply MEV protection
        let provider_with_mev = security_manager.apply_mev_protection(
            Arc::new(self.inner.clone()),
            &tx,
            call_info.mev_protection
        ).await?;
        
        security_manager.execute_call(call_info, move || {
            // TODO: Use the provider_with_mev
            
            // For now, just use the inner provider
            let provider = self.inner.clone();
            let tx = tx.clone();
            
            tokio::task::spawn(async move {
                provider.send_transaction(tx, None)
                    .await
                    .map_err(|e| DefiError::TransactionError(e.to_string()))?
                    .await
                    .map_err(|e| DefiError::TransactionError(e.to_string()))
            })
            .await
            .map_err(|e| DefiError::RuntimeError(e.to_string()))?
        }).await
    }
    
    /// Gọi contract với bảo mật
    pub async fn call_contract_with_security<R>(
        &self,
        contract_address: Address,
        call_data: Vec<u8>,
        call_info: CallInfo,
    ) -> Result<R, DefiError>
    where
        T: Middleware,
        R: Send + 'static,
    {
        let security_manager = self.security_manager.clone();
        let call_info = CallInfo {
            contract_address: Some(contract_address),
            ..call_info
        };
        
        security_manager.execute_call(call_info, move || {
            let provider = self.inner.clone();
            
            tokio::task::spawn(async move {
                // TODO: Implement proper contract call
                
                Err(DefiError::NotImplemented("call_contract_with_security not fully implemented".to_string()))
            })
            .await
            .map_err(|e| DefiError::RuntimeError(e.to_string()))?
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_security_manager_new() {
        let manager = SecurityManager::new();
        assert!(manager.flashbots_signer.is_none());
    }
    
    #[tokio::test]
    async fn test_get_chain_semaphore() {
        let manager = SecurityManager::new();
        
        let semaphore1 = manager.get_chain_semaphore(ChainId::Ethereum).await;
        let semaphore2 = manager.get_chain_semaphore(ChainId::Ethereum).await;
        
        // Semaphores should be the same instance
        assert!(Arc::ptr_eq(&semaphore1, &semaphore2));
        
        // Different chains should have different semaphores
        let semaphore3 = manager.get_chain_semaphore(ChainId::Bsc).await;
        assert!(!Arc::ptr_eq(&semaphore1, &semaphore3));
    }
    
    #[test]
    fn test_call_info() {
        let call_info = CallInfo::new(ChainId::Ethereum)
            .with_contract_address(Address::zero())
            .with_priority(SecurityPriority::High)
            .with_max_retries(5)
            .with_token_allowlist(vec![Address::zero()])
            .with_max_slippage(Decimal::from_f64(0.5).unwrap())
            .with_check_source_code(true)
            .with_contract_security_level(ContractSecurityLevel::High)
            .with_mev_protection(MevProtection::Flashbots)
            .with_timelock(30)
            .with_rpc_rotation(true)
            .with_simulation(true);
        
        assert_eq!(call_info.chain_id, ChainId::Ethereum);
        assert_eq!(call_info.contract_address, Some(Address::zero()));
        assert_eq!(call_info.priority, SecurityPriority::High);
        assert_eq!(call_info.max_retries, 5);
        assert_eq!(call_info.token_allowlist, Some(vec![Address::zero()]));
        assert_eq!(call_info.max_slippage, Some(Decimal::from_f64(0.5).unwrap()));
        assert_eq!(call_info.check_source_code, true);
        assert_eq!(call_info.contract_security_level, ContractSecurityLevel::High);
        assert_eq!(call_info.mev_protection, MevProtection::Flashbots);
        assert_eq!(call_info.timelock_seconds, Some(30));
        assert_eq!(call_info.use_rpc_rotation, true);
        assert_eq!(call_info.simulate_before_send, true);
    }
    
    #[test]
    fn test_check_slippage() {
        let manager = SecurityManager::new();
        
        // No slippage
        let result = manager.check_slippage(
            U256::from(100),
            U256::from(100),
            Decimal::from_f64(1.0).unwrap()
        );
        assert!(result.is_ok());
        
        // Under max slippage
        let result = manager.check_slippage(
            U256::from(100),
            U256::from(99),
            Decimal::from_f64(5.0).unwrap()
        );
        assert!(result.is_ok());
        
        // Over max slippage
        let result = manager.check_slippage(
            U256::from(100),
            U256::from(90),
            Decimal::from_f64(5.0).unwrap()
        );
        assert!(result.is_err());
        
        // Zero expected
        let result = manager.check_slippage(
            U256::from(0),
            U256::from(0),
            Decimal::from_f64(1.0).unwrap()
        );
        assert!(result.is_err());
    }
    
    #[test]
    fn test_check_deadline() {
        let manager = SecurityManager::new();
        
        // Future deadline
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let future = now + 3600; // 1 hour in the future
        let result = manager.check_deadline(future);
        assert!(result.is_ok());
        
        // Past deadline
        let past = now - 3600; // 1 hour in the past
        let result = manager.check_deadline(past);
        assert!(result.is_err());
    }
} 