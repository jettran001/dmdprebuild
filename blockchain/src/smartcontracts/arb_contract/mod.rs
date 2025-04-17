use anyhow::{anyhow, Result};
use ethers::{
    prelude::*,
    providers::{Http, Provider},
    types::{Address, H256, U256},
    utils::hex,
};
use log::{debug, error, info, warn};
use once_cell::sync::OnceCell;
use std::{
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::blockchain::DmdChain;
use crate::smartcontracts::TokenInterface;

pub const SOLIDITY_FILE_PATH: &str = "blockchain/src/smartcontracts/arb_contract/DiamondTokenARB.sol";

// Cấu trúc Tier Token cho DMD trên Arbitrum
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DmdTokenTier {
    /// Token fungible tiêu chuẩn (balance < 1000)
    Regular,
    /// VIP badge tier bronze (balance >= 1000)
    Bronze,
    /// VIP badge tier silver (balance >= 5000)
    Silver,
    /// VIP badge tier gold (balance >= 10000)
    Gold,
    /// VIP badge tier diamond (balance >= 30000)
    Diamond,
    /// Tier tùy chỉnh
    Custom(u8),
}

impl DmdTokenTier {
    // Chuyển đổi từ index sang enum
    pub fn from(index: u8) -> Self {
        match index {
            0 => Self::Regular,
            1 => Self::Bronze,
            2 => Self::Silver,
            3 => Self::Gold,
            4 => Self::Diamond,
            n => Self::Custom(n),
        }
    }

    // Chuyển đổi từ enum sang index
    pub fn to_index(&self) -> u8 {
        match self {
            Self::Regular => 0,
            Self::Bronze => 1,
            Self::Silver => 2,
            Self::Gold => 3,
            Self::Diamond => 4,
            Self::Custom(n) => *n,
        }
    }
}

// Cấu hình ngưỡng cho mỗi tier
#[derive(Debug, Clone)]
pub struct DmdTokenTierConfig {
    /// Số dư tối thiểu cho tier Bronze
    pub bronze_threshold: U256,
    /// Số dư tối thiểu cho tier Silver
    pub silver_threshold: U256,
    /// Số dư tối thiểu cho tier Gold
    pub gold_threshold: U256,
    /// Số dư tối thiểu cho tier Diamond
    pub diamond_threshold: U256,
}

impl Default for DmdTokenTierConfig {
    fn default() -> Self {
        Self {
            bronze_threshold: U256::from(1_000) * U256::exp10(18),
            silver_threshold: U256::from(5_000) * U256::exp10(18),
            gold_threshold: U256::from(10_000) * U256::exp10(18),
            diamond_threshold: U256::from(30_000) * U256::exp10(18),
        }
    }
}

pub struct GlobalTierConfig {
    config: Mutex<DmdTokenTierConfig>,
}

impl GlobalTierConfig {
    pub fn instance() -> &'static GlobalTierConfig {
        static INSTANCE: OnceCell<GlobalTierConfig> = OnceCell::new();
        INSTANCE.get_or_init(|| GlobalTierConfig {
            config: Mutex::new(DmdTokenTierConfig::default()),
        })
    }

    pub fn get_config(&self) -> DmdTokenTierConfig {
        let config = self.config.blocking_lock();
        config.clone()
    }

    pub fn update_config(&self, new_config: DmdTokenTierConfig) {
        let mut config = self.config.blocking_lock();
        *config = new_config;
    }

    pub fn update_threshold(&self, tier: DmdTokenTier, new_threshold: U256) {
        let mut config = self.config.blocking_lock();
        match tier {
            DmdTokenTier::Bronze => config.bronze_threshold = new_threshold,
            DmdTokenTier::Silver => config.silver_threshold = new_threshold,
            DmdTokenTier::Gold => config.gold_threshold = new_threshold,
            DmdTokenTier::Diamond => config.diamond_threshold = new_threshold,
            _ => {} // Custom tiers có ngưỡng riêng
        }
    }
}

// Thông tin DMD Contract trên Arbitrum
pub struct DmdArbContract {
    /// Địa chỉ của contract ERC-1155 trên Arbitrum
    pub contract_address: &'static str,
    /// LayerZero endpoint trên Arbitrum
    pub lz_endpoint: &'static str,
    /// Mô tả của contract
    pub description: &'static str,
}

impl Default for DmdArbContract {
    fn default() -> Self {
        Self {
            contract_address: "0x0000000000000000000000000000000000000000", // Thay thế với địa chỉ thật khi deploy
            lz_endpoint: "0x3c2269811836af69497E5F486A85D7316753cf62", // LayerZero endpoint trên Arbitrum
            description: "DMD Token (ERC-1155) trên Arbitrum, hỗ trợ bridge qua LayerZero",
        }
    }
}

// Cấu hình provider cho Arbitrum
pub struct ArbContractConfig {
    /// Địa chỉ RPC của Arbitrum
    pub rpc_url: String,
    /// Các URL RPC dự phòng khi URL chính không hoạt động
    pub fallback_rpc_urls: Vec<String>,
    /// Địa chỉ contract DMD trên Arbitrum
    pub contract_address: String,
    /// Gas limit mặc định
    pub default_gas_limit: U256,
    /// Gas price mặc định (nếu không lấy từ mạng)
    pub default_gas_price: Option<U256>,
    /// Timeout cho request (ms)
    pub timeout_ms: u64,
    /// Số lần retry khi RPC không phản hồi
    pub max_retries: usize,
}

impl Default for ArbContractConfig {
    fn default() -> Self {
        Self {
            rpc_url: "https://arb1.arbitrum.io/rpc".to_string(),
            fallback_rpc_urls: vec![
                "https://arbitrum-one.public.blastapi.io".to_string(),
                "https://arb-mainnet.g.alchemy.com/v2/demo".to_string(),
                "https://endpoints.omniatech.io/v1/arbitrum/one/public".to_string(),
                "https://arbitrum.blockpi.network/v1/rpc/public".to_string(),
            ],
            contract_address: DmdArbContract::default().contract_address.to_string(),
            default_gas_limit: U256::from(3_000_000),
            default_gas_price: None,
            timeout_ms: 30000,
            max_retries: 3,
        }
    }
}

impl ArbContractConfig {
    // Khởi tạo cấu hình mới
    pub fn new() -> Self {
        Self::default()
    }

    // Thiết lập RPC URL
    pub fn with_rpc_url(mut self, rpc_url: &str) -> Self {
        self.rpc_url = rpc_url.to_string();
        self
    }

    // Thiết lập địa chỉ contract
    pub fn with_contract_address(mut self, contract_address: &str) -> Self {
        self.contract_address = contract_address.to_string();
        self
    }
}

// Provider cho tương tác với Arbitrum smart contract
pub struct ArbContractProvider {
    /// Cấu hình
    config: ArbContractConfig,
    /// Provider HTTP cho Arbitrum
    provider: Arc<Provider<Http>>,
    /// Index của RPC URL hiện tại
    current_rpc_index: Arc<RwLock<usize>>,
}

impl ArbContractProvider {
    // Khởi tạo provider mới
    pub async fn new(config: Option<ArbContractConfig>) -> Result<Self> {
        let config = config.unwrap_or_default();
        
        // Khởi tạo provider với URL RPC chính
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()?;
        
        let provider = Provider::new_client(&config.rpc_url, client)?;
        let provider = Arc::new(provider);
        
        // Kiểm tra kết nối với RPC chính
        match timeout(Duration::from_secs(5), provider.get_block_number()).await {
            Ok(Ok(_)) => {
                // RPC chính hoạt động tốt
                Ok(Self {
                    config,
                    provider,
                    current_rpc_index: Arc::new(RwLock::new(0)),
                })
            },
            _ => {
                // RPC chính không hoạt động, thử các URL dự phòng
                let mut arb_provider = Self {
                    config,
                    provider,
                    current_rpc_index: Arc::new(RwLock::new(0)),
                };
                
                // Thử chuyển sang provider dự phòng
                arb_provider.switch_to_next_provider().await?;
                
                Ok(arb_provider)
            }
        }
    }
    
    /// Chuyển sang provider tiếp theo
    pub async fn switch_to_next_provider(&self) -> Result<()> {
        // Sử dụng tokio::sync::Semaphore để đảm bảo chỉ một luồng có thể thay đổi provider
        // Semaphore được khởi tạo với số permits là 1
        static SWITCH_SEMAPHORE: tokio::sync::OnceCell<tokio::sync::Semaphore> = tokio::sync::OnceCell::const_new();
        let semaphore = SWITCH_SEMAPHORE.get_or_init(|| async {
            tokio::sync::Semaphore::new(1)
        }).await;
        
        // Lấy permit từ semaphore, sẽ đợi nếu đã có luồng khác đang giữ permit
        let permit = semaphore.acquire().await.expect("Semaphore đã bị đóng");
        
        // Bọc permit trong SemaphorePermit để đảm bảo permit được trả lại khi hàm kết thúc
        let _permit_guard = tokio::sync::SemaphorePermit::new(permit);

        // Giờ mới thực hiện thay đổi provider dưới sự bảo vệ của semaphore
        let mut rpc_lock = self.current_rpc_index.write().await;
        let total_providers = 1 + self.config.fallback_rpc_urls.len();
        
        // Lưu lại index hiện tại để log
        let current_idx = *rpc_lock;
        *rpc_lock = (*rpc_lock + 1) % total_providers;
        
        // Log kết quả tùy theo index mới
        if *rpc_lock == 0 {
            info!("Đã thử tất cả các provider Arbitrum (0-{}), quay lại provider chính", total_providers - 1);
        } else {
            info!("Đã chuyển từ Arbitrum provider {} sang provider {}", current_idx, *rpc_lock);
        }
        
        // Khi SemaphorePermit được drop, permit sẽ tự động được trả lại
        Ok(())
    }
    
    /// Lấy provider hiện tại hoặc chuyển sang provider dự phòng nếu cần
    pub async fn get_provider(&self) -> Result<Arc<Provider<Http>>> {
        // Đọc index provider hiện tại một cách atomic
        let rpc_lock = self.current_rpc_index.read().await;
        let current_index = *rpc_lock;
        drop(rpc_lock); // Giải phóng lock trước khi sử dụng
        
        let rpc_url = if current_index == 0 {
            &self.config.rpc_url
        } else {
            &self.config.fallback_rpc_urls[current_index - 1]
        };
        
        // Các provider tạo tạm thời sẽ được cache lại để tránh tạo quá nhiều kết nối
        use std::collections::HashMap;
        static PROVIDER_CACHE: tokio::sync::OnceCell<tokio::sync::Mutex<HashMap<String, (Arc<Provider<Http>>, Instant)>>> = 
            tokio::sync::OnceCell::const_new();
        
        let provider_cache = PROVIDER_CACHE.get_or_init(|| async {
            tokio::sync::Mutex::new(HashMap::new())
        }).await;
        
        // Kiểm tra cache để tái sử dụng provider
        let mut cache = provider_cache.lock().await;
        if let Some((provider, timestamp)) = cache.get(rpc_url) {
            // Chỉ sử dụng provider từ cache nếu chưa quá 5 phút
            if timestamp.elapsed() < Duration::from_secs(300) {
                return Ok(provider.clone());
            }
            // Nếu provider đã quá cũ, xóa khỏi cache
            cache.remove(rpc_url);
        }
        
        // Tạo HTTP client với timeout
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(self.config.timeout_ms))
            .build()?;
            
        // Tạo provider mới từ client
        let provider = Provider::new_client(rpc_url, client)?;
        
        // Kiểm tra xem provider có hoạt động không bằng timeout
        let provider_check = match tokio::time::timeout(
            Duration::from_secs(5), 
            provider.get_block_number()
        ).await {
            Ok(Ok(_)) => {
                // Provider đang hoạt động tốt
                let provider_arc = Arc::new(provider);
                
                // Cập nhật cache
                cache.insert(rpc_url.clone(), (provider_arc.clone(), Instant::now()));
                
                info!("Provider {} hoạt động tốt", rpc_url);
                Ok(provider_arc)
            },
            _ => {
                // Provider không phản hồi, thử chuyển sang provider khác
                drop(cache); // Giải phóng lock trước khi chuyển provider
                
                info!("Provider {} không phản hồi, chuyển sang URL dự phòng", rpc_url);
                self.switch_to_next_provider().await?;
                
                // Gọi đệ quy để thử với provider mới
                // Sử dụng atomic counter để ngăn đệ quy vô hạn
                static RECURSION_COUNTER: AtomicUsize = AtomicUsize::new(0);
                
                let counter = RECURSION_COUNTER.fetch_add(1, Ordering::SeqCst);
                if counter > self.config.fallback_rpc_urls.len() + 1 {
                    // Đã thử tất cả các provider mà không thành công
                    RECURSION_COUNTER.store(0, Ordering::SeqCst);
                    return Err(anyhow!("Tất cả các provider Arbitrum đều không phản hồi"));
                }
                
                let result = self.get_provider().await;
                // Reset counter nếu tìm được provider hoạt động
                if result.is_ok() {
                    RECURSION_COUNTER.store(0, Ordering::SeqCst);
                }
                
                result
            }
        };
        
        provider_check
    }
    
    // Tạo wallet từ private key
    fn create_wallet(private_key: &str) -> Result<LocalWallet> {
        let private_key = private_key.trim_start_matches("0x");
        let private_key_bytes = hex::decode(private_key)?;
        LocalWallet::from_bytes(&private_key_bytes)
            .map_err(|e| anyhow!("Lỗi khi tạo wallet: {}", e))
    }
    
    /// Thực hiện gọi contract với retry khi cần thiết
    pub async fn call_with_retry<T, F>(&self, operation: F) -> Result<T> 
    where
        F: Fn(Arc<Provider<Http>>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send>> + Send + Sync,
    {
        let mut retries = 0;
        let max_retries = self.config.max_retries;
        
        loop {
            let provider = match self.get_provider().await {
                Ok(provider) => provider,
                Err(e) => {
                    retries += 1;
                    if retries >= max_retries {
                        return Err(anyhow!("Không thể kết nối với Arbitrum RPC sau {} lần thử: {}", max_retries, e));
                    }
                    warn!("Lỗi khi lấy provider (thử lần {}): {}", retries, e);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
            };
            
            match operation(provider.clone()).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    retries += 1;
                    if retries >= max_retries {
                        return Err(anyhow!("Thao tác thất bại sau {} lần thử: {}", max_retries, e));
                    }
                    warn!("Thao tác thất bại (thử lần {}): {}", retries, e);
                    
                    // Thử chuyển sang provider khác
                    if let Err(e) = self.switch_to_next_provider().await {
                        warn!("Không thể chuyển provider: {}", e);
                    }
                    
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }
    
    // Gửi giao dịch chuyển token
    async fn send_transfer_transaction(
        &self,
        private_key: &str,
        to_address: &str,
        amount: U256,
    ) -> Result<String> {
        // Sử dụng call_with_retry thay vì truy cập trực tiếp self.provider
        self.call_with_retry(|provider| {
            let private_key = private_key.to_string();
            let to_address = to_address.to_string();
            let contract_address = self.config.contract_address.clone();
            let default_gas_limit = self.config.default_gas_limit;
            let default_gas_price = self.config.default_gas_price;
            
            Box::pin(async move {
                // Tạo wallet từ private key
                let wallet = Self::create_wallet(&private_key)?;
                let wallet = wallet.with_chain_id(42161u64); // Chain ID của Arbitrum One
                
                // Lấy địa chỉ gửi từ wallet
                let from_address = wallet.address();
                
                // Parse địa chỉ người nhận
                let to_address = Address::from_str(&to_address)?;
                
                // Tạo provider có ký giao dịch
                let provider = SignerMiddleware::new(provider.clone(), wallet);
                
                // Lấy địa chỉ contract
                let contract_address = Address::from_str(&contract_address)?;
                
                // Tạo dữ liệu cho giao dịch
                // Function selector cho safeTransferFrom(address,address,uint256,uint256,bytes)
                // keccak256("safeTransferFrom(address,address,uint256,uint256,bytes)")[0:4]
                let selector = hex::decode("f242432a")?;
                
                // Mã hóa tham số
                let mut data = selector;
                
                // 1. from_address (32 bytes - padded)
                let mut from_bytes = [0u8; 32];
                from_bytes[12..].copy_from_slice(&from_address.as_bytes());
                data.extend_from_slice(&from_bytes);
                
                // 2. to_address (32 bytes - padded)
                let mut to_bytes = [0u8; 32];
                to_bytes[12..].copy_from_slice(&to_address.as_bytes());
                data.extend_from_slice(&to_bytes);
                
                // 3. id (32 bytes) - DMD Token ID = 0
                let mut id_bytes = [0u8; 32];
                // Không cần thay đổi vì giá trị 0 là mặc định
                data.extend_from_slice(&id_bytes);
                
                // 4. amount (32 bytes)
                let mut amount_bytes = [0u8; 32];
                amount.to_big_endian(&mut amount_bytes);
                data.extend_from_slice(&amount_bytes);
                
                // 5. data offset (32 bytes) - 0xa0 = 160 (5 * 32)
                let mut data_offset = [0u8; 32];
                data_offset[31] = 0xa0;
                data.extend_from_slice(&data_offset);
                
                // 6. data length (32 bytes) - 0 vì không có data bổ sung
                let data_length = [0u8; 32];
                data.extend_from_slice(&data_length);
                
                // Tạo transaction request
                let tx = TransactionRequest::new()
                    .to(contract_address)
                    .data(data)
                    .gas(default_gas_limit);
                
                // Nếu có gas price mặc định, sử dụng nó
                let tx = if let Some(gas_price) = default_gas_price {
                    tx.gas_price(gas_price)
                } else {
                    tx
                };
                
                // Gửi giao dịch và chờ kết quả
                let pending_tx = provider.send_transaction(tx, None).await?;
                
                // Lấy hash của giao dịch
                let tx_hash = pending_tx.tx_hash();
                
                info!("Đã gửi giao dịch chuyển token: {}", tx_hash);
                
                Ok(format!("{:x}", tx_hash))
            })
        }).await
    }
    
    // Hàm bridge token từ Arbitrum sang chain khác
    async fn send_bridge_transaction(
        &self,
        private_key: &str,
        to_address: &str,
        chain_id: u16,
        amount: U256,
    ) -> Result<String> {
        // Tạo wallet từ private key
        let wallet = Self::create_wallet(private_key)?;
        let wallet = wallet.with_chain_id(42161u64); // Chain ID của Arbitrum One
        
        // Tạo provider có ký giao dịch
        let provider = SignerMiddleware::new(self.provider.clone(), wallet);
        
        // Lấy địa chỉ contract
        let contract_address = Address::from_str(&self.config.contract_address)?;
        
        // Ước tính phí bridge - cần thiết để đính kèm đủ ETH
        let (native_fee, _) = self.estimate_bridge_fee(chain_id, to_address).await?;
        
        // Tạo dữ liệu cho giao dịch
        // Function selector cho bridgeToOtherChain(uint16,bytes,uint256,uint256)
        let selector = hex::decode("e26b7433")?;
        
        // Mã hóa tham số
        let mut data = selector;
        
        // 1. chainId (uint16 padded to 32 bytes)
        let mut chain_id_bytes = [0u8; 32];
        chain_id_bytes[30..].copy_from_slice(&chain_id.to_be_bytes());
        data.extend_from_slice(&chain_id_bytes);
        
        // 2. to_address (bytes - complex type, first store offset)
        let offset = 32 * 4; // 4 words: chainId, to_address_offset, id, amount
        let mut offset_bytes = [0u8; 32];
        U256::from(offset).to_big_endian(&mut offset_bytes);
        data.extend_from_slice(&offset_bytes);
        
        // 3. id (uint256) - DMD Token ID = 0
        let id_bytes = [0u8; 32];
        data.extend_from_slice(&id_bytes);
        
        // 4. amount (uint256)
        let mut amount_bytes = [0u8; 32];
        amount.to_big_endian(&mut amount_bytes);
        data.extend_from_slice(&amount_bytes);
        
        // 5. to_address data (bytes)
        // 5.1. Length of to_address (32 bytes)
        let to_address_bytes = to_address.as_bytes();
        let mut length_bytes = [0u8; 32];
        U256::from(to_address_bytes.len()).to_big_endian(&mut length_bytes);
        data.extend_from_slice(&length_bytes);
        
        // 5.2. to_address content
        // Pad to multiple of 32 bytes
        let to_address_padded_len = ((to_address_bytes.len() + 31) / 32) * 32;
        let mut to_address_padded = vec![0; to_address_padded_len];
        to_address_padded[..to_address_bytes.len()].copy_from_slice(to_address_bytes);
        data.extend_from_slice(&to_address_padded);
        
        // Tạo transaction request với value = native_fee
        let tx = TransactionRequest::new()
            .to(contract_address)
            .data(data)
            .value(native_fee)
            .gas(self.config.default_gas_limit);
        
        // Nếu có gas price mặc định, sử dụng nó
        let tx = if let Some(gas_price) = self.config.default_gas_price {
            tx.gas_price(gas_price)
        } else {
            tx
        };
        
        // Gửi giao dịch và chờ kết quả
        let pending_tx = provider.send_transaction(tx, None).await?;
        
        // Lấy hash của giao dịch
        let tx_hash = pending_tx.tx_hash();
        
        info!("Đã gửi giao dịch bridge token: {}", tx_hash);
        
        Ok(format!("{:x}", tx_hash))
    }
    
    // Lấy tổng cung của token - cố gắng lấy từ contract trước, nếu không có thì dùng giá trị mặc định
    async fn get_total_supply(&self) -> Result<U256> {
        // Khai báo giá trị fallback cho trường hợp không lấy được từ contract
        const FALLBACK_SUPPLY: U256 = U256([1_000_000_000, 0, 0, 0]); // 1 tỷ token (không bao gồm decimals)
        const MAX_RETRY: usize = 3;
        const RETRY_DELAY_MS: u64 = 500;
        const CACHE_TTL_SECONDS: u64 = 300; // 5 phút
        
        // Sử dụng cơ chế cache và retry
        static SUPPLY_CACHE: OnceCell<(U256, Instant)> = OnceCell::new();
        
        // Kiểm tra cache
        if let Some((cached_supply, cache_time)) = SUPPLY_CACHE.get() {
            if cache_time.elapsed().as_secs() < CACHE_TTL_SECONDS {
                info!("Trả về tổng cung DMD từ cache: {}", cached_supply);
                return Ok(*cached_supply);
            }
        }
        
        // Cố gắng gọi contract với số lần thử lại
        for attempt in 0..MAX_RETRY {
            // Function selector cho totalSupply()
            let selector = match hex::decode("18160ddd") {
                Ok(s) => s,
                Err(e) => {
                    error!("Lỗi decode function selector: {}", e);
                    if attempt == MAX_RETRY - 1 {
                        warn!("Đã thử hết số lần, trả về giá trị mặc định");
                        let fallback_with_decimals = FALLBACK_SUPPLY * U256::exp10(18);
                        let _ = SUPPLY_CACHE.set((fallback_with_decimals, Instant::now()));
                        return Ok(fallback_with_decimals);
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                    continue;
                }
            };
            
            // Tạo call request
            let call_data = Bytes::from(selector);
            
            // Địa chỉ contract
            let contract_address = match Address::from_str(&self.config.contract_address) {
                Ok(address) => address,
                Err(e) => {
                    error!("Lỗi parse địa chỉ contract: {}", e);
                    if attempt == MAX_RETRY - 1 {
                        warn!("Đã thử hết số lần, trả về giá trị mặc định");
                        let fallback_with_decimals = FALLBACK_SUPPLY * U256::exp10(18);
                        let _ = SUPPLY_CACHE.set((fallback_with_decimals, Instant::now()));
                        return Ok(fallback_with_decimals);
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                    continue;
                }
            };
            
            // Tạo request
            let tx_request = ethers::types::TransactionRequest::new()
                .to(contract_address)
                .data(call_data);
            
            // Gọi function totalSupply
            match self.provider.call(&tx_request, None).await {
                Ok(result) => {
                    // Parse kết quả (uint256)
                    if result.len() >= 32 {
                        let total_supply = U256::from_big_endian(&result[0..32]);
                        info!("Lấy tổng cung thành công: {}", total_supply);
                        
                        // Lưu vào cache
                        let _ = SUPPLY_CACHE.set((total_supply, Instant::now()));
                        
                        return Ok(total_supply);
                    } else {
                        warn!("Kết quả không hợp lệ, độ dài: {}", result.len());
                        if attempt == MAX_RETRY - 1 {
                            warn!("Đã thử hết số lần, trả về giá trị mặc định");
                            let fallback_with_decimals = FALLBACK_SUPPLY * U256::exp10(18);
                            let _ = SUPPLY_CACHE.set((fallback_with_decimals, Instant::now()));
                            return Ok(fallback_with_decimals);
                        }
                    }
                },
                Err(e) => {
                    error!("Lỗi gọi totalSupply: {}", e);
                    if attempt == MAX_RETRY - 1 {
                        warn!("Đã thử hết số lần, trả về giá trị mặc định");
                        let fallback_with_decimals = FALLBACK_SUPPLY * U256::exp10(18);
                        let _ = SUPPLY_CACHE.set((fallback_with_decimals, Instant::now()));
                        return Ok(fallback_with_decimals);
                    }
                }
            }
            
            // Chờ một chút trước khi thử lại
            tokio::time::sleep(tokio::time::Duration::from_millis(RETRY_DELAY_MS)).await;
        }
        
        // Không thể gọi thành công sau nhiều lần thử
        warn!("Không thể lấy tổng cung sau {} lần thử, trả về giá trị mặc định", MAX_RETRY);
        let fallback_with_decimals = FALLBACK_SUPPLY * U256::exp10(18);
        let _ = SUPPLY_CACHE.set((fallback_with_decimals, Instant::now()));
        Ok(fallback_with_decimals)
    }
}

// Implement TokenInterface cho ArbContractProvider
#[async_trait::async_trait]
impl TokenInterface for ArbContractProvider {
    async fn get_balance(&self, address: &str) -> Result<U256> {
        // Cần nhận dữ liệu từ contract ERC-1155
        // Tạo payload cho balanceOf(address,uint256)
        // keccak256("balanceOf(address,uint256)")[0:4]
        let selector = hex::decode("00fdd58e")?;
        
        // Địa chỉ cần truy vấn
        let address = Address::from_str(address)?;
        
        // Token ID cho DMD (fungible token trong ERC-1155)
        let token_id = 0u32;
        
        // Mã hóa tham số
        let mut data = selector;
        
        // 1. address (32 bytes - padded)
        let mut address_bytes = [0u8; 32];
        address_bytes[12..].copy_from_slice(&address.as_bytes());
        data.extend_from_slice(&address_bytes);
        
        // 2. token_id (32 bytes)
        let mut id_bytes = [0u8; 32];
        id_bytes[31] = token_id as u8;
        data.extend_from_slice(&id_bytes);
        
        // Tạo call request
        let call_data = Bytes::from(data);
        
        // Địa chỉ contract
        let contract_address = Address::from_str(&self.config.contract_address)?;
        
        // Gọi function balanceOf
        let result = self.provider.call(&ethers::types::TransactionRequest::new()
            .to(contract_address)
            .data(call_data), None).await?;
        
        // Parse kết quả (uint256)
        if result.len() < 32 {
            return Err(anyhow!("Kết quả không hợp lệ"));
        }
        
        let balance = U256::from_big_endian(&result[0..32]);
        
        Ok(balance)
    }

    async fn transfer(&self, private_key: &str, to: &str, amount: U256) -> Result<String> {
        self.send_transfer_transaction(private_key, to, amount).await
    }

    async fn total_supply(&self) -> Result<U256> {
        self.get_total_supply().await
    }

    fn decimals(&self) -> u8 {
        // DMD token có 18 decimals trên Arbitrum
        18
    }

    fn token_name(&self) -> String {
        "Diamond Token".to_string()
    }

    fn token_symbol(&self) -> String {
        "DMD".to_string()
    }
}

// Các hàm bridge cho Arbitrum contract
impl ArbContractProvider {
    // Bridge token từ Arbitrum sang Ethereum
    pub async fn bridge_to_eth(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
        self.send_bridge_transaction(private_key, to_address, 101, amount).await
    }
    
    // Bridge token từ Arbitrum sang BSC
    pub async fn bridge_to_bsc(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
        self.send_bridge_transaction(private_key, to_address, 102, amount).await
    }
    
    // Bridge token từ Arbitrum sang NEAR
    pub async fn bridge_to_near(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
        self.send_bridge_transaction(private_key, to_address, 115, amount).await
    }
    
    // Ước tính phí bridge
    pub async fn estimate_bridge_fee(&self, chain_id: u16, to_address: &str) -> Result<(U256, U256)> {
        // Function selector cho estimateBridgeFee(uint16,bytes,uint256,uint256)
        let selector = hex::decode("8f1cfce3")?;
        
        // Mã hóa tham số
        let mut data = selector;
        
        // 1. chainId (uint16 padded to 32 bytes)
        let mut chain_id_bytes = [0u8; 32];
        chain_id_bytes[30..].copy_from_slice(&chain_id.to_be_bytes());
        data.extend_from_slice(&chain_id_bytes);
        
        // 2. to_address (bytes - complex type, first store offset)
        let offset = 32 * 4; // 4 words: chainId, to_address_offset, id, amount
        let mut offset_bytes = [0u8; 32];
        U256::from(offset).to_big_endian(&mut offset_bytes);
        data.extend_from_slice(&offset_bytes);
        
        // 3. id (uint256) - DMD Token ID = 0
        let id_bytes = [0u8; 32];
        data.extend_from_slice(&id_bytes);
        
        // 4. amount (uint256) - use a placeholder value like 1 token
        let mut amount_bytes = [0u8; 32];
        U256::from(10).to_big_endian(&mut amount_bytes); // Dùng 10 tokens làm mẫu
        data.extend_from_slice(&amount_bytes);
        
        // 5. to_address data (bytes)
        // 5.1. Length of to_address (32 bytes)
        let to_address_bytes = to_address.as_bytes();
        let mut length_bytes = [0u8; 32];
        U256::from(to_address_bytes.len()).to_big_endian(&mut length_bytes);
        data.extend_from_slice(&length_bytes);
        
        // 5.2. to_address content
        // Pad to multiple of 32 bytes
        let to_address_padded_len = ((to_address_bytes.len() + 31) / 32) * 32;
        let mut to_address_padded = vec![0; to_address_padded_len];
        to_address_padded[..to_address_bytes.len()].copy_from_slice(to_address_bytes);
        data.extend_from_slice(&to_address_padded);
        
        // Địa chỉ contract
        let contract_address = Address::from_str(&self.config.contract_address)?;
        
        // Tạo call request
        let call_data = Bytes::from(data);
        
        // Gọi function estimateBridgeFee
        let result = self.provider.call(&ethers::types::TransactionRequest::new()
            .to(contract_address)
            .data(call_data), None).await?;
        
        // Parse kết quả (nativeFee, zroFee - 2 giá trị uint256)
        if result.len() < 64 {
            return Err(anyhow!("Kết quả không hợp lệ"));
        }
        
        let native_fee = U256::from_big_endian(&result[0..32]);
        let zro_fee = U256::from_big_endian(&result[32..64]);
        
        Ok((native_fee, zro_fee))
    }
    
    // Kiểm tra trạng thái của giao dịch bridge
    pub async fn check_bridge_status(&self, tx_hash: &str) -> Result<String> {
        // Parse transaction hash
        let tx_hash = H256::from_str(tx_hash)?;
        
        // Lấy transaction receipt
        let receipt = self.provider.get_transaction_receipt(tx_hash).await?;
        
        // Kiểm tra receipt đã có chưa
        match receipt {
            Some(receipt) => {
                // Nếu giao dịch thành công
                if receipt.status == Some(1.into()) {
                    // Tìm event TokenBridged trong logs
                    for log in receipt.logs {
                        // Kiểm tra nếu log từ contract của chúng ta
                        if log.address == Address::from_str(&self.config.contract_address)? {
                            // Kiểm tra topic[0] là event TokenBridged
                            // keccak256("TokenBridged(address,uint256,uint256,uint16,bytes)")
                            let event_signature = "0x7eb7c5bed9324adbb5d0e9e63868e9c978b1818fb52d3c4ce84d34b7e769b272";
                            let event_topic = H256::from_str(event_signature)?;
                            
                            if log.topics.get(0) == Some(&event_topic) {
                                return Ok("Confirmed".to_string());
                            }
                        }
                    }
                    
                    // Không tìm thấy event cụ thể nhưng giao dịch thành công
                    Ok("Success".to_string())
                } else {
                    // Giao dịch thất bại
                    Ok("Failed".to_string())
                }
            },
            None => {
                // Giao dịch chưa được xác nhận
                Ok("Pending".to_string())
            }
        }
    }
    
    // Lấy tier hiện tại của người dùng
    pub async fn get_token_tier(&self, address: &str) -> Result<DmdTokenTier> {
        let balance = self.get_balance(address).await?;
        Ok(self.determine_tier_from_balance(balance))
    }
    
    // Xác định tier từ balance
    pub fn determine_tier_from_balance(&self, balance: U256) -> DmdTokenTier {
        let config = GlobalTierConfig::instance().get_config();
        
        if balance >= config.diamond_threshold {
            DmdTokenTier::Diamond
        } else if balance >= config.gold_threshold {
            DmdTokenTier::Gold
        } else if balance >= config.silver_threshold {
            DmdTokenTier::Silver
        } else if balance >= config.bronze_threshold {
            DmdTokenTier::Bronze
        } else {
            DmdTokenTier::Regular
        }
    }
} 