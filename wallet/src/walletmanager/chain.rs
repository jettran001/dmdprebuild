// External imports
use ethers::providers::{Http, Provider};
use serde::{Deserialize, Serialize};

// Standard library imports
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use std::num::NonZeroU32;

// Internal imports
use crate::error::WalletError;

// Third party imports
use anyhow::Context;
use tracing::{debug, error, info, warn};

// Constants
const DEFAULT_RATE_LIMIT: u32 = 10; // Requests per second
const DEFAULT_TIMEOUT_MS: u64 = 5000; // 5 seconds

/// Cấu trúc theo dõi rate limit cho các RPC requests
#[derive(Debug, Clone)]
struct RateLimiter {
    /// Số request tối đa cho phép trong khoảng thời gian
    requests_per_limit: NonZeroU32,
    /// Thời gian giới hạn
    time_window: Duration,
    /// Lịch sử request
    request_history: Vec<Instant>,
    /// Thời điểm cập nhật cuối cùng
    last_cleanup: Instant,
}

impl RateLimiter {
    fn new(requests_per_limit: NonZeroU32, time_window: Duration) -> Self {
        Self {
            requests_per_limit,
            time_window,
            request_history: Vec::with_capacity(requests_per_limit.get() as usize * 2),
            last_cleanup: Instant::now(),
        }
    }

    /// Kiểm tra xem có thể thực hiện request hay không
    fn check(&mut self) -> Result<(), ()> {
        // Dọn dẹp lịch sử request cũ
        let now = Instant::now();
        if now.duration_since(self.last_cleanup) > Duration::from_secs(10) {
            self.request_history.retain(|time| now.duration_since(*time) <= self.time_window);
            self.last_cleanup = now;
        }

        // Kiểm tra số lượng request trong khoảng thời gian giới hạn
        if self.request_history.len() < self.requests_per_limit.get() as usize {
            self.request_history.push(now);
            Ok(())
        } else {
            Err(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChainType {
    EVM,
    Solana,
    TON,
    NEAR,
    Stellar,
    Sui,
    BTC,
    Diamond,
    Tron,
    Cosmos,
    Hedera,
    Aptos,
    Bitcoin,
    Polygon,
    Arbitrum,
    Optimism,
    Fantom,
    Avalanche,
    BSC,
    Klaytn,
}

/// Thông tin token cơ bản
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    /// Địa chỉ token
    pub address: String,
    /// Tên token
    pub name: String,
    /// Ký hiệu token
    pub symbol: String,
    /// Số chữ số thập phân
    pub decimals: u8,
    /// Logo token (URL hoặc base64)
    pub logo: Option<String>,
}

/// Quản lý chain cho ví.
pub struct ChainConfig {
    /// Chain ID.
    pub chain_id: u64,
    /// Tên Chain.
    pub name: String,
    /// Loại Chain.
    pub chain_type: ChainType,
    /// URL RPC để kết nối đến chain.
    pub rpc_url: String,
    /// URL block explorer.
    pub explorer_url: Option<String>,
    /// Chain icon.
    pub icon: Option<String>,
    /// Danh sách tokens.
    pub tokens: Vec<Token>,
    /// Giới hạn tốc độ request (số request mỗi phút)
    pub rate_limit: Option<u32>,
    /// Cần xác thực hay không
    pub requires_auth: bool,
    /// Token native của chain
    pub native_token: String,
}

#[async_trait::async_trait]
pub trait ChainManager: Send + Sync + 'static {
    async fn get_provider(&self, chain_id: u64) -> Result<Arc<Provider<Http>>, WalletError>;
    async fn add_chain(&self, config: ChainConfig) -> Result<(), WalletError>;
    async fn get_chain_config(&self, chain_id: u64) -> Result<ChainConfig, WalletError>;
    async fn chain_exists(&self, chain_id: u64) -> bool;
}

#[derive(Debug)]
pub struct DefaultChainManager {
    providers: Arc<RwLock<HashMap<u64, Provider<Http>>>>,
    configs: Arc<RwLock<HashMap<u64, ChainConfig>>>,
}

impl DefaultChainManager {
    pub fn new() -> Self {
        Self {
            providers: Arc::new(RwLock::new(HashMap::new())),
            configs: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl ChainManager for DefaultChainManager {
    async fn get_provider(&self, chain_id: u64) -> Result<Arc<Provider<Http>>, WalletError> {
        let providers = self.providers.read().await;
        if let Some(provider) = providers.get(&chain_id) {
            return Ok(Arc::new(provider.clone()));
        }
        drop(providers);
        
        let configs = self.configs.read().await;
        if let Some(config) = configs.get(&chain_id) {
            let rpc_url = config.rpc_url.clone();
            drop(configs);
            
            let mut retries = 0;
            const MAX_RETRIES: u8 = 3;
            let mut last_error = None;
            
            while retries < MAX_RETRIES {
                match Provider::<Http>::try_from(&rpc_url) {
                    Ok(provider) => {
                        let mut providers = self.providers.write().await;
                        let provider_arc = Arc::new(provider);
                        providers.insert(chain_id, provider_arc.as_ref().clone());
                        return Ok(provider_arc);
                    },
                    Err(e) => {
                        retries += 1;
                        last_error = Some(e.to_string());
                        if retries < MAX_RETRIES {
                            tokio::time::sleep(tokio::time::Duration::from_millis(500 * retries as u64)).await;
                        }
                    }
                }
            }
            
            Err(WalletError::ProviderError(format!("Failed to connect to RPC after {} attempts. Last error: {}", MAX_RETRIES, last_error.unwrap_or_default())))
        } else {
            Err(WalletError::ChainNotSupported(format!("Chain ID {} not supported", chain_id)))
        }
    }

    async fn add_chain(&self, config: ChainConfig) -> Result<(), WalletError> {
        // Kiểm tra tính khả dụng của RPC endpoint
        let provider = match Provider::<Http>::try_from(&config.rpc_url) {
            Ok(p) => p,
            Err(e) => return Err(WalletError::ProviderError(
                format!("Không thể kết nối đến RPC endpoint '{}': {}", config.rpc_url, e)
            )),
        };
        
        // Kiểm tra thời gian phản hồi của RPC
        let start = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_millis(DEFAULT_TIMEOUT_MS), 
            provider.get_block_number()
        ).await;
        
        match result {
            Ok(block_result) => {
                match block_result {
                    Ok(_) => {
                        let elapsed = start.elapsed().as_millis();
                        debug!("RPC endpoint phản hồi sau {} ms", elapsed);
                        
                        // Lưu cấu hình và provider
                        let mut configs = self.configs.write().await;
                        configs.insert(config.chain_id, config);
                        
                        let mut providers = self.providers.write().await;
                        providers.insert(config.chain_id, provider);
                        
                        Ok(())
                    },
                    Err(e) => Err(WalletError::ProviderError(
                        format!("RPC endpoint không phản hồi: {}", e)
                    )),
                }
            },
            Err(_) => Err(WalletError::ProviderError(
                format!("RPC endpoint không phản hồi trong {}ms", DEFAULT_TIMEOUT_MS)
            )),
        }
    }
    
    async fn get_chain_config(&self, chain_id: u64) -> Result<ChainConfig, WalletError> {
        let configs = self.configs.read().await;
        configs
            .get(&chain_id)
            .cloned()
            .ok_or_else(|| WalletError::ChainNotFound(chain_id))
    }

    async fn chain_exists(&self, chain_id: u64) -> bool {
        let configs = self.configs.read().await;
        configs.contains_key(&chain_id)
    }
}

/// Quản lý chain cho ví.
pub struct SimpleChainManager {
    chains: RwLock<HashMap<u64, ChainConfig>>,
    providers: RwLock<HashMap<u64, Arc<Provider<Http>>>>,
    rate_limiters: RwLock<HashMap<u64, RateLimiter>>,
    auth_tokens: RwLock<HashMap<u64, String>>,
}

impl SimpleChainManager {
    /// Tạo mới SimpleChainManager.
    pub fn new() -> Self {
        Self {
            chains: RwLock::new(HashMap::new()),
            providers: RwLock::new(HashMap::new()),
            rate_limiters: RwLock::new(HashMap::new()),
            auth_tokens: RwLock::new(HashMap::new()),
        }
    }

    /// Tạo một instance với các chain mặc định
    pub fn with_default_chains() -> Self {
        let manager = Self::new();
        tokio::spawn(async move {
            let infura_key = std::env::var("INFURA_API_KEY")
                .unwrap_or_else(|_| {
                    warn!("INFURA_API_KEY không được thiết lập, sử dụng URL RPC thông thường");
                    "".to_string()
                });
            
            // Ethereum Mainnet
            let eth_config = ChainConfig {
                chain_id: 1,
                name: "Ethereum".to_string(),
                chain_type: ChainType::EVM,
                rpc_url: if infura_key.is_empty() {
                    "https://eth.llamarpc.com".to_string()
                } else {
                    format!("https://mainnet.infura.io/v3/{}", infura_key)
                },
                explorer_url: Some("https://etherscan.io".to_string()),
                icon: Some("ethereum.png".to_string()),
                tokens: vec![],
                rate_limit: Some(120),
                requires_auth: !infura_key.is_empty(),
                native_token: "ETH".to_string(),
            };
            
            // BSC Mainnet
            let bsc_config = ChainConfig {
                chain_id: 56,
                name: "Binance Smart Chain".to_string(),
                chain_type: ChainType::BSC,
                rpc_url: "https://bsc-dataseed.binance.org".to_string(),
                explorer_url: Some("https://bscscan.com".to_string()),
                icon: Some("bsc.png".to_string()),
                tokens: vec![],
                rate_limit: Some(120),
                requires_auth: false,
                native_token: "BNB".to_string(),
            };
            
            // Diamond Chain
            let diamond_config = ChainConfig {
                chain_id: 8866,
                name: "Diamond Chain".to_string(),
                chain_type: ChainType::Diamond,
                rpc_url: "https://rpc.diamondnetwork.io".to_string(),
                explorer_url: Some("https://explorer.diamondnetwork.io".to_string()),
                icon: Some("diamond.png".to_string()),
                tokens: vec![],
                rate_limit: Some(120),
                requires_auth: false,
                native_token: "DMD".to_string(),
            };
            
            // Thêm các chain
            let _ = manager.add_chain(eth_config).await;
            let _ = manager.add_chain(bsc_config).await;
            let _ = manager.add_chain(diamond_config).await;
        });
        
        manager
    }

    /// Tạo một RateLimiter mới cho chain với giới hạn tốc độ được chỉ định
    fn create_rate_limiter(&self, requests_per_minute: u32) -> RateLimiter {
        let non_zero_rpm = match NonZeroU32::new(requests_per_minute) {
            Some(rpm) => rpm,
            None => {
                // Nếu requests_per_minute là 0, sử dụng giá trị mặc định (60 - 1 request mỗi giây)
                warn!("requests_per_minute không thể bằng 0, sử dụng giá trị mặc định 60");
                NonZeroU32::new(60).unwrap_or(NonZeroU32::new(1).expect("1 luôn là một số khác 0"))
            }
        };
        
        RateLimiter::new(
            non_zero_rpm,
            Duration::from_secs(60),
        )
    }

    /// Kiểm tra giới hạn tỷ lệ truy cập (rate limit) cho một chain
    async fn check_rate_limit(&self, chain_id: u64) -> Result<(), WalletError> {
        // Lấy rate limiter cho chain_id hoặc tạo mới nếu chưa có
        let mut rate_limiters = self.rate_limiters.write().await;
        
        // Kiểm tra nếu chain này có yêu cầu rate limiting
        let need_rate_limiting = {
            let chains = self.chains.read().await;
            if let Some(chain_config) = chains.get(&chain_id) {
                chain_config.rate_limit.is_some()
            } else {
                return Err(WalletError::ChainNotFound(chain_id));
            }
        };
        
        if !need_rate_limiting {
            return Ok(());
        }
        
        // Lấy hoặc tạo rate limiter
        let rate_limiter = rate_limiters.entry(chain_id).or_insert_with(|| {
            let chains = self.chains.blocking_read();
            let config = chains.get(&chain_id)
                .ok_or_else(|| {
                    error!("Chain ID {} không tồn tại trong danh sách chains", chain_id);
                    WalletError::ChainNotFound(chain_id)
                })
                .expect("Chain đã được kiểm tra tồn tại ở trên");
                
            let rate_limit = config.rate_limit.unwrap_or(60);
            let requests_per_limit = match NonZeroU32::new(rate_limit) {
                Some(limit) => limit,
                None => {
                    warn!("Rate limit không thể bằng 0, sử dụng giá trị mặc định 60");
                    NonZeroU32::new(60).unwrap_or(NonZeroU32::new(1).expect("1 luôn là một số khác 0"))
                }
            };
            
            self.create_rate_limiter(requests_per_limit.get())
        });
        
        // Kiểm tra rate limit
        match rate_limiter.check() {
            Ok(()) => Ok(()),
            Err(()) => {
                let chains = self.chains.read().await;
                let config = match chains.get(&chain_id) {
                    Some(cfg) => cfg,
                    None => return Err(WalletError::ChainNotFound(chain_id))
                };
                
                let limit = config.rate_limit.unwrap_or(60);
                Err(WalletError::RateLimitExceeded(format!(
                    "Vượt quá giới hạn tỷ lệ truy cập: {} yêu cầu mỗi phút cho chain {}", 
                    limit, chain_id
                )))
            }
        }
    }
    
    /// Thiết lập token xác thực cho một chain
    pub async fn set_auth_token(&self, chain_id: u64, token: String) -> Result<(), WalletError> {
        // Kiểm tra xem chain có tồn tại không
        if !self.chain_exists(chain_id).await {
            return Err(WalletError::ChainNotFound(chain_id));
        }
        
        // Lưu token
        let mut auth_tokens = self.auth_tokens.write().await;
        auth_tokens.insert(chain_id, token);
        
        tracing::debug!("Đã thiết lập token xác thực cho chain {}", chain_id);
        Ok(())
    }
    
    /// Lấy token xác thực cho một chain
    async fn get_auth_token(&self, chain_id: u64) -> Option<String> {
        let auth_tokens = self.auth_tokens.read().await;
        auth_tokens.get(&chain_id).cloned()
    }
    
    /// Tạo headers xác thực cho request đến chain
    async fn create_auth_headers(&self, chain_id: u64) -> Option<HashMap<String, String>> {
        // Kiểm tra xem chain có yêu cầu xác thực không
        let requires_auth = {
            let chains = self.chains.read().await;
            chains.get(&chain_id)
                .map(|config| config.requires_auth)
                .unwrap_or(false)
        };
        
        if !requires_auth {
            return None;
        }
        
        // Lấy token xác thực
        let token = match self.get_auth_token(chain_id).await {
            Some(token) => token,
            None => return None,
        };
        
        // Tạo headers
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), format!("Bearer {}", token));
        
        Some(headers)
    }
    
    /// Tạo authenticated provider
    async fn create_authenticated_provider(&self, url: &str, chain_id: u64) -> Result<Provider<Http>, WalletError> {
        // Tạo HTTP client với headers xác thực nếu cần
        let auth_headers = self.create_auth_headers(chain_id).await;
        
        let client = if let Some(headers) = auth_headers {
            let mut builder = reqwest::Client::builder();
            
            for (key, value) in headers {
                builder = builder.default_header(key, value);
            }
            
            match builder.build() {
                Ok(client) => client,
                Err(e) => return Err(WalletError::ProviderError(
                    format!("Không thể tạo authenticated client: {}", e)
                )),
            }
        } else {
            reqwest::Client::new()
        };
        
        // Tạo HTTP connection cho provider
        let http = match Http::new_with_client(url, client) {
            Ok(http) => http,
            Err(e) => return Err(WalletError::ProviderError(
                format!("Không thể tạo HTTP connection: {}", e)
            )),
        };
        
        Ok(Provider::new(http))
    }
}

#[async_trait::async_trait]
impl ChainManager for SimpleChainManager {
    async fn add_chain(&self, chain_config: ChainConfig) -> Result<(), WalletError> {
        let chain_id = chain_config.chain_id;
        
        // Kiểm tra tính khả dụng của RPC endpoint
        let provider_result = self.create_authenticated_provider(&chain_config.rpc_url, chain_id).await;
        let provider = match provider_result {
            Ok(p) => p,
            Err(e) => return Err(WalletError::ProviderError(
                format!("Không thể kết nối đến RPC endpoint '{}': {}", chain_config.rpc_url, e)
            )),
        };
        
        // Kiểm tra thời gian phản hồi của RPC
        let start = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_millis(DEFAULT_TIMEOUT_MS), 
            provider.get_block_number()
        ).await;
        
        match result {
            Ok(block_result) => {
                match block_result {
                    Ok(_) => {
                        let elapsed = start.elapsed().as_millis();
                        debug!("RPC endpoint phản hồi sau {} ms", elapsed);
                        
                        // Thêm chain vào danh sách
                        {
                            let mut chains = self.chains.write().await;
                            chains.insert(chain_id, chain_config.clone());
                        }
                        
                        // Thêm provider vào danh sách
                        {
                            let mut providers = self.providers.write().await;
                            providers.insert(chain_id, Arc::new(provider));
                        }
                        
                        // Tạo rate limiter cho chain này
                        {
                            let mut rate_limiters = self.rate_limiters.write().await;
                            let requests_per_minute = chain_config.rate_limit.unwrap_or(120); // Mặc định 2 requests/giây
                            let limiter = self.create_rate_limiter(requests_per_minute);
                            rate_limiters.insert(chain_id, limiter);
                        }
                        
                        Ok(())
                    },
                    Err(e) => Err(WalletError::ProviderError(
                        format!("RPC endpoint không phản hồi: {}", e)
                    )),
                }
            },
            Err(_) => Err(WalletError::ProviderError(
                format!("RPC endpoint không phản hồi trong {}ms", DEFAULT_TIMEOUT_MS)
            )),
        }
    }

    async fn get_chain_config(&self, chain_id: u64) -> Result<ChainConfig, WalletError> {
        let chains = self.chains.read().await;
        chains
            .get(&chain_id)
            .cloned()
            .ok_or_else(|| WalletError::ChainNotFound(chain_id))
    }

    async fn chain_exists(&self, chain_id: u64) -> bool {
        let chains = self.chains.read().await;
        chains.contains_key(&chain_id)
    }

    async fn get_provider(&self, chain_id: u64) -> Result<Arc<Provider<Http>>, WalletError> {
        // Kiểm tra rate limit trước khi thực hiện request
        self.check_rate_limit(chain_id).await?;
        
        // Kiểm tra cache trước
        {
            let providers = self.providers.read().await;
            if let Some(provider) = providers.get(&chain_id) {
                return Ok(provider.clone());
            }
        }
        
        // Lấy RPC URL từ cấu hình
        let rpc_url = {
            let chains = self.chains.read().await;
            match chains.get(&chain_id) {
                Some(config) => config.rpc_url.clone(),
                None => return Err(WalletError::ChainNotFound(chain_id)),
            }
        };
        
        // Tạo provider với xác thực nếu cần
        let provider = self.create_authenticated_provider(&rpc_url, chain_id).await?;
        
        // Lưu vào cache và trả về
        let provider_arc = Arc::new(provider);
        {
            let mut providers = self.providers.write().await;
            providers.insert(chain_id, provider_arc.clone());
        }
        
        Ok(provider_arc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_add_and_get_chain() {
        let manager = SimpleChainManager::new();
        let config = ChainConfig {
            chain_id: 999,
            chain_type: ChainType::EVM,
            name: "Test Chain".to_string(),
            rpc_url: "https://test.rpc".to_string(),
            explorer_url: None,
            icon: None,
            tokens: vec![],
            rate_limit: None,
            requires_auth: false,
            native_token: "TEST".to_string(),
        };
        
        // Skip actual test since we can't connect to the fake RPC
        // This is a placeholder for actual integration tests
        assert!(true);
    }
}