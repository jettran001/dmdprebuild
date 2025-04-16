// BSC Contract Module
//
// Module này chứa các hàm và cấu trúc dữ liệu để tương tác với smart contract DMD Token trên BSC
// Sử dụng chuẩn token ERC-1155 và hỗ trợ bridge qua LayerZero

use ethers::core::types::TransactionReceipt;
use ethers::providers::{Http, Provider};
use ethers::signers::{LocalWallet, Signer};
use ethers::types::{Address, Bytes, TransactionRequest, U256};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tracing::{info, warn, error};
use tokio;
use std::time::Instant;
use once_cell::sync::OnceCell;
use std::sync::Mutex;
use ethers::abi::Abi;
use ethers::contract::Contract;
use tokio::sync::RwLock;
use std::time::{Duration, SystemTime};

use crate::smartcontracts::TokenInterface;
use crate::smartcontracts::DmdChain;

// Đường dẫn đến file Solidity
pub const SOLIDITY_FILE_PATH: &str = "blockchain/src/smartcontracts/bsc_contract/dmd_bsc_contract.sol";

/// Các tier của badge VIP dựa trên số dư DMD token
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Chuyển đổi từ u8 sang DmdTokenTier
    pub fn from(index: u8) -> Self {
        match index {
            0 => Self::Regular,
            1 => Self::Bronze,
            2 => Self::Silver,
            3 => Self::Gold,
            4 => Self::Diamond,
            _ => Self::Custom(index),
        }
    }
    
    /// Chuyển đổi từ DmdTokenTier sang u8
    pub fn to_index(&self) -> u8 {
        match self {
            Self::Regular => 0,
            Self::Bronze => 1,
            Self::Silver => 2,
            Self::Gold => 3,
            Self::Diamond => 4,
            Self::Custom(index) => *index,
        }
    }
}

/// Cấu hình cho các tier của DMD token
#[derive(Debug, Clone, Serialize, Deserialize)]
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
            bronze_threshold: U256::from(1000) * U256::exp10(18), // 1000 DMD với 18 decimals
            silver_threshold: U256::from(5000) * U256::exp10(18), // 5000 DMD
            gold_threshold: U256::from(10000) * U256::exp10(18),  // 10000 DMD
            diamond_threshold: U256::from(30000) * U256::exp10(18), // 30000 DMD
        }
    }
}

/// Quản lý cấu hình tier toàn cục
pub struct GlobalTierConfig {
    config: Mutex<DmdTokenTierConfig>,
}

impl GlobalTierConfig {
    pub fn instance() -> &'static GlobalTierConfig {
        static INSTANCE: OnceCell<GlobalTierConfig> = OnceCell::new();
        INSTANCE.get_or_init(|| {
            GlobalTierConfig {
                config: Mutex::new(DmdTokenTierConfig::default()),
            }
        })
    }

    /// Lấy cấu hình tier hiện tại
    pub fn get_config(&self) -> DmdTokenTierConfig {
        self.config.lock().unwrap().clone()
    }

    /// Cập nhật cấu hình tier
    pub fn update_config(&self, new_config: DmdTokenTierConfig) {
        let mut config = self.config.lock().unwrap();
        *config = new_config;
    }

    /// Cập nhật một ngưỡng cụ thể
    pub fn update_threshold(&self, tier: DmdTokenTier, new_threshold: U256) {
        let mut config = self.config.lock().unwrap();
        match tier {
            DmdTokenTier::Bronze => config.bronze_threshold = new_threshold,
            DmdTokenTier::Silver => config.silver_threshold = new_threshold,
            DmdTokenTier::Gold => config.gold_threshold = new_threshold,
            DmdTokenTier::Diamond => config.diamond_threshold = new_threshold,
            DmdTokenTier::Regular => {}, // Không có ngưỡng cho Regular
        }
    }
}

/// Thông tin về DMD token trên BSC
pub struct DmdBscContract {
    /// Địa chỉ của contract ERC-1155 trên BSC
    pub contract_address: &'static str,
    /// LayerZero endpoint trên BSC
    pub lz_endpoint: &'static str,
    /// Mô tả của contract
    pub description: &'static str,
}

impl Default for DmdBscContract {
    fn default() -> Self {
        Self {
            contract_address: "0x16a7FA783c2FF584B234e13825960F95244Fc7bC",
            lz_endpoint: "0x3c2269811836af69497E5F486A85D7316753cf62",
            description: "DMD Token trên BSC sử dụng chuẩn ERC-1155 với khả năng bridge qua LayerZero",
        }
    }
}

/// Cấu hình cho BSC contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BscConfig {
    /// Địa chỉ contract của DMD token trên BSC
    pub contract_address: String,
    
    /// URL RPC của BSC mainnet
    pub rpc_url: String,
    
    /// URL RPC dự phòng của BSC mainnet
    pub fallback_rpc_urls: Vec<String>,
    
    /// Thời gian cache (giây)
    pub cache_duration: u64,
}

impl Default for BscConfig {
    fn default() -> Self {
        Self {
            contract_address: "0x90fE084F877C65e1b577c7b2eA64B8D8dd1AB278".to_string(),
            rpc_url: "https://bsc-dataseed.binance.org/".to_string(),
            fallback_rpc_urls: vec![
                "https://bsc-dataseed1.defibit.io/".to_string(),
                "https://bsc-dataseed1.ninicoin.io/".to_string(),
                "https://bsc-dataseed2.binance.org/".to_string(),
                "https://bsc-dataseed3.binance.org/".to_string(),
                "https://bsc-dataseed4.binance.org/".to_string(),
            ],
            cache_duration: 300, // 5 phút
        }
    }
}

/// Cấu trúc cache cho tổng cung
#[derive(Clone, Debug)]
struct TotalSupplyCache {
    /// Giá trị tổng cung
    value: f64,
    /// Thời gian lưu cache
    timestamp: u64,
}

/// Provider cho BSC contract
pub struct BscContractProvider {
    /// Cấu hình
    config: BscConfig,
    /// ABI của contract
    abi: Abi,
    /// Cache cho tổng cung
    total_supply_cache: Arc<RwLock<Option<TotalSupplyCache>>>,
    /// Index của RPC URL hiện tại
    current_rpc_index: Arc<RwLock<usize>>,
}

impl BscContractProvider {
    /// Tạo mới một provider
    pub fn new(config: Option<BscConfig>) -> Self {
        let config = config.unwrap_or_default();
        
        // TODO: Load ABI từ file
        let abi = serde_json::from_str(include_str!("abi.json")).expect("Invalid ABI");
        
        Self {
            config,
            abi,
            total_supply_cache: Arc::new(RwLock::new(None)),
            current_rpc_index: Arc::new(RwLock::new(0)),
        }
    }
    
    /// Lấy provider hiện tại
    async fn get_provider(&self) -> Result<Arc<Provider<Http>>> {
        let rpc_lock = self.current_rpc_index.read().await;
        let current_index = *rpc_lock;
        drop(rpc_lock); // Giải phóng lock trước khi sử dụng
        
        let provider = Provider::<Http>::try_from(
            if current_index == 0 {
                &self.config.rpc_url
            } else {
                &self.config.fallback_rpc_urls[current_index - 1]
            }
        ).map_err(|e| anyhow!("Failed to create provider: {}", e))?;
        
        // Kiểm tra xem provider có hoạt động không
        match timeout(Duration::from_secs(5), provider.get_block_number()).await {
            Ok(Ok(_)) => {
                // Provider đang hoạt động tốt
                Ok(Arc::new(provider))
            },
            _ => {
                // Provider không hoạt động, chuyển sang provider tiếp theo
                info!("BSC RPC không phản hồi, chuyển sang fallback URL");
                self.switch_to_next_provider().await?;
                self.get_provider().await
            }
        }
    }
    
    /// Chuyển sang provider tiếp theo
    async fn switch_to_next_provider(&self) -> Result<()> {
        let mut rpc_lock = self.current_rpc_index.write().await;
        let total_providers = 1 + self.config.fallback_rpc_urls.len();
        *rpc_lock = (*rpc_lock + 1) % total_providers;
        info!("Đã chuyển sang provider BSC thứ {}", *rpc_lock);
        Ok(())
    }
    
    /// Lấy Contract
    async fn get_contract(&self) -> Result<Contract<Provider<Http>>> {
        let provider = self.get_provider().await?;
        
        let address = Address::from_str(&self.config.contract_address)
            .map_err(|e| anyhow!("Invalid contract address: {}", e))?;
            
        Ok(Contract::new(address, self.abi.clone(), provider.as_ref().clone()))
    }
    
    /// Lấy tổng cung token với retry
    pub async fn get_total_supply_with_retry(&self, max_retries: usize) -> Result<f64> {
        let mut retries = 0;
        
        while retries < max_retries {
            match self.get_total_supply_internal().await {
                Ok(supply) => return Ok(supply),
                Err(e) => {
                    retries += 1;
                    warn!("Lỗi khi lấy tổng cung BSC (thử lần {}): {}", retries, e);
                    
                    if retries < max_retries {
                        // Chuyển sang provider tiếp theo và thử lại
                        if let Err(e) = self.switch_to_next_provider().await {
                            warn!("Không thể chuyển provider: {}", e);
                        }
                        
                        // Đợi một chút trước khi thử lại
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                }
            }
        }
        
        Err(anyhow!("Không thể lấy tổng cung sau {} lần thử", max_retries))
    }
    
    /// Lấy tổng cung token (internal)
    async fn get_total_supply_internal(&self) -> Result<f64> {
        // Kiểm tra cache
        if let Some(cache) = &*self.total_supply_cache.read().await {
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs();
                
            if now - cache.timestamp < self.config.cache_duration {
                debug!("Sử dụng tổng cung từ cache: {}", cache.value);
                return Ok(cache.value);
            }
        }
        
        let contract = self.get_contract().await?;
        
        // Gọi hàm totalSupply
        let total_supply: U256 = contract
            .method("totalSupply", ())
            .map_err(|e| anyhow!("Không thể tạo phương thức totalSupply: {}", e))?
            .call()
            .await
            .map_err(|e| anyhow!("Lỗi khi gọi totalSupply: {}", e))?;
            
        // Chuyển đổi từ wei sang token (giả sử 18 decimals)
        let decimals = self.decimals();
        let divisor = U256::from(10).pow(U256::from(decimals));
        let total_supply_f64 = total_supply.as_u128() as f64 / divisor.as_u128() as f64;
        
        // Cập nhật cache
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        let cache = TotalSupplyCache {
            value: total_supply_f64,
            timestamp: now,
        };
        
        *self.total_supply_cache.write().await = Some(cache);
        
        debug!("Đã lấy tổng cung mới: {}", total_supply_f64);
        Ok(total_supply_f64)
    }
}

#[async_trait]
impl TokenInterface for BscContractProvider {
    async fn get_balance(&self, address: &str) -> Result<U256> {
        // Parse địa chỉ người dùng
        let user_address = Address::from_str(address)
            .map_err(|e| anyhow!("Địa chỉ không hợp lệ: {}", e))?;
        
        // Parse địa chỉ contract
        let contract_address = Address::from_str(&self.config.contract_address)
            .map_err(|e| anyhow!("Địa chỉ contract không hợp lệ: {}", e))?;
        
        // Tạo dữ liệu cho transaction call
        // Function selector cho balanceOf(address,uint256)
        // keccak256("balanceOf(address,uint256)")[0:4]
        let selector = hex::decode("00fdd58e")
            .map_err(|e| anyhow!("Lỗi khi tạo selector: {}", e))?;
        
        let mut data = selector;
        
        // Địa chỉ người dùng (32 bytes padded)
        let mut address_bytes = [0u8; 32];
        address_bytes[12..].copy_from_slice(&user_address.as_bytes());
        data.extend_from_slice(&address_bytes);
        
        // ID token (32 bytes) - DMD Token ID = 0
        let mut id_bytes = [0u8; 32];
        // Mặc định là 0
        data.extend_from_slice(&id_bytes);
        
        // Tạo call để đọc dữ liệu từ blockchain
        let call_data = Bytes::from(data);
        
        // Thực hiện call và nhận lại kết quả
        let result = self.get_provider().await?
            .call(&ethers::types::TransactionRequest::new()
                .to(contract_address)
                .data(call_data), None)
            .await
            .map_err(|e| anyhow!("Lỗi khi gọi RPC: {}", e))?;
        
        // Parse kết quả là U256
        if result.len() < 32 {
            return Err(anyhow!("Kết quả không hợp lệ khi kiểm tra số dư: {:?}", result));
        }
        
        let balance = U256::from_big_endian(&result[0..32]);
        
        info!("Số dư DMD của địa chỉ {}: {}", address, balance);
        
        Ok(balance)
    }
    
    async fn transfer(&self, private_key: &str, to: &str, amount: U256) -> Result<String> {
        // TODO: Implement transfer
        Err(anyhow!("Not implemented yet"))
    }
    
    async fn total_supply(&self) -> Result<U256> {
        let contract = self.get_contract().await?;
        
        let total_supply: U256 = contract
            .method("totalSupply", ())
            .map_err(|e| anyhow!("Failed to create totalSupply method: {}", e))?
            .call()
            .await
            .map_err(|e| anyhow!("Failed to call totalSupply: {}", e))?;
            
        Ok(total_supply)
    }
    
    fn decimals(&self) -> u8 {
        // BSC DMD token có 18 decimals
        18
    }
    
    fn token_name(&self) -> String {
        "Diamond".to_string()
    }
    
    fn token_symbol(&self) -> String {
        "DMD".to_string()
    }
    
    async fn get_total_supply(&self) -> Result<f64> {
        self.get_total_supply_with_retry(3).await.context("Không thể lấy tổng cung DMD token trên BSC")
    }
    
    async fn bridge_to(&self, to_chain: &str, from_account: &str, to_account: &str, amount: f64) -> Result<String> {
        // TODO: Implement bridge
        Err(anyhow!("Not implemented yet"))
    }
}

/// Các hàm bridge cho BSC contract
impl BscContractProvider {
    /// Bridge token từ BSC sang NEAR
    pub async fn bridge_to_near(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
        // Kiểm tra địa chỉ đích không phải là chuỗi rỗng
        if to_address.trim().is_empty() {
            return Err(anyhow!("Địa chỉ đích không thể là chuỗi rỗng"));
        }
        
        // Tạo ví từ private key
        let wallet = Self::create_wallet(private_key)?;
        let from_address = wallet.address();
        
        // Địa chỉ contract
        let contract_address = Address::from_str(&self.config.contract_address)?;
        
        // Tạo provider với signer
        let client = ethers::middleware::SignerMiddleware::new(
            self.get_provider().await?,
            wallet,
        );
        
        // Token ID cho DMD (fungible token trong ERC-1155)
        let token_id = 0u32;
        
        // Function selector cho bridgeToNEAR(bytes,uint256,uint256)
        // keccak256("bridgeToNEAR(bytes,uint256,uint256)")[0:4]
        let selector = hex::decode("3a7dc5e6")?;
        
        // Mã hóa tham số
        let mut data = selector;
        
        // 1. Offset cho bytes toAddress (32 bytes)
        let mut offset_bytes = [0u8; 32];
        offset_bytes[31] = 96; // 3 * 32 = 96
        data.extend_from_slice(&offset_bytes);
        
        // 2. token_id (32 bytes)
        let mut id_bytes = [0u8; 32];
        id_bytes[31] = token_id as u8;
        data.extend_from_slice(&id_bytes);
        
        // 3. amount (32 bytes)
        let amount_bytes = amount.to_be_bytes();
        data.extend_from_slice(&amount_bytes);
        
        // 4. length of to_address bytes (32 bytes)
        let to_bytes = to_address.as_bytes();
        let mut length_bytes = [0u8; 32];
        length_bytes[31] = to_bytes.len() as u8;
        data.extend_from_slice(&length_bytes);
        
        // 5. to_address (padded to multiple of 32 bytes)
        let padding_size = (32 - (to_bytes.len() % 32)) % 32;
        data.extend_from_slice(to_bytes);
        data.extend_from_slice(&vec![0u8; padding_size]);
        
        // Ước tính phí LayerZero
        // Trong thực tế, nên gọi estimateFees trên contract
        // Giá trị này là giả định
        let lz_fee = U256::from(0.015 * 1e18 as u64); // ~0.015 BNB
        
        // Tạo transaction request với value = lz_fee
        let tx = TransactionRequest::new()
            .to(contract_address)
            .data(data)
            .value(lz_fee)
            .gas(self.config.default_gas_limit)
            .from(from_address);
        
        // Gửi transaction
        let pending_tx = client.send_transaction(tx, None).await?;
        
        // Lấy hash của transaction
        let tx_hash = format!("0x{:x}", pending_tx.tx_hash());
        
        info!("Bridge transaction sent: {}", tx_hash);
        
        Ok(tx_hash)
    }
    
    /// Ước tính phí bridge
    pub async fn estimate_bridge_fee(&self, to_chain: DmdChain, to_address: &str) -> Result<U256> {
        // Trong thực tế, cần gọi hàm estimateFees trên contract LayerZero
        // Đây là giá trị ước tính
        
        match to_chain {
            DmdChain::Near => Ok(U256::from(0.015 * 1e18 as u64)), // ~0.015 BNB
            DmdChain::Ethereum => Ok(U256::from(0.01 * 1e18 as u64)), // ~0.01 BNB
            DmdChain::Avalanche | 
            DmdChain::Polygon | 
            DmdChain::Arbitrum | 
            DmdChain::Optimism | 
            DmdChain::Fantom => Ok(U256::from(0.005 * 1e18 as u64)), // ~0.005 BNB
            _ => Err(anyhow!("Chain không được hỗ trợ: {:?}", to_chain)),
        }
    }
    
    /// Kiểm tra trạng thái bridge
    pub async fn check_bridge_status(&self, tx_hash: &str) -> Result<String> {
        // Kiểm tra receipt của transaction
        let tx_hash = tx_hash.trim_start_matches("0x");
        let tx_hash = ethers::types::H256::from_str(tx_hash)?;
        
        let receipt = self.get_provider().await?.get_transaction_receipt(tx_hash).await?;
        
        match receipt {
            Some(receipt) => {
                if receipt.status == Some(1.into()) {
                    // Transaction thành công
                    Ok("Completed".to_string())
                } else {
                    // Transaction thất bại
                    Ok("Failed".to_string())
                }
            },
            None => {
                // Transaction chưa được xác nhận
                Ok("Pending".to_string())
            }
        }
    }

    /// Xác định tier của một địa chỉ dựa trên số dư
    pub async fn get_token_tier(&self, address: &str) -> Result<DmdTokenTier> {
        let balance = self.get_balance(address).await?;
        Ok(self.determine_tier_from_balance(balance))
    }

    /// Xác định tier từ số dư
    pub fn determine_tier_from_balance(&self, balance: U256) -> DmdTokenTier {
        // Lấy các ngưỡng tier từ contract
        // Lưu ý: ở đây balance đã bao gồm decimals (18)
        let bronze_threshold = U256::from(1000) * U256::exp10(18); // 1000 DMD
        let silver_threshold = U256::from(5000) * U256::exp10(18); // 5000 DMD
        let gold_threshold = U256::from(10000) * U256::exp10(18);  // 10000 DMD
        let diamond_threshold = U256::from(30000) * U256::exp10(18); // 30000 DMD
        
        if balance >= diamond_threshold {
            DmdTokenTier::Diamond
        } else if balance >= gold_threshold {
            DmdTokenTier::Gold
        } else if balance >= silver_threshold {
            DmdTokenTier::Silver
        } else if balance >= bronze_threshold {
            DmdTokenTier::Bronze
        } else {
            DmdTokenTier::Regular
        }
    }

    /// Cập nhật cấu hình tier
    pub async fn update_tier_threshold(&self, private_key: &str, tier: DmdTokenTier, new_threshold: U256) -> Result<String> {
        // Chuẩn bị tham số
        let wallet = Self::create_wallet(private_key)?;
        let from_address = wallet.address();
        
        // Địa chỉ contract
        let contract_address = Address::from_str(&self.config.contract_address)?;
        
        // Tạo provider với signer
        let client = ethers::middleware::SignerMiddleware::new(
            self.get_provider().await?,
            wallet,
        );
        
        // Tier enum trong Solidity
        let tier_index = match tier {
            DmdTokenTier::Regular => 0, // Regular tier không có ngưỡng, không nên gọi
            DmdTokenTier::Bronze => 1,
            DmdTokenTier::Silver => 2,
            DmdTokenTier::Gold => 3,
            DmdTokenTier::Diamond => 4,
        };
        
        // Function selector cho updateTierThreshold(uint8,uint256)
        // keccak256("updateTierThreshold(uint8,uint256)")[0:4]
        let selector = hex::decode("8b818aaa")?;
        
        // Mã hóa tham số
        let mut data = selector;
        
        // 1. tier (32 bytes - padded)
        let mut tier_bytes = [0u8; 32];
        tier_bytes[31] = tier_index as u8;
        data.extend_from_slice(&tier_bytes);
        
        // 2. new_threshold (32 bytes)
        let threshold_bytes = new_threshold.to_be_bytes();
        data.extend_from_slice(&threshold_bytes);
        
        // Tạo transaction request
        let tx = TransactionRequest::new()
            .to(contract_address)
            .data(data)
            .gas(self.config.default_gas_limit)
            .from(from_address);
        
        // Gửi transaction
        let pending_tx = client.send_transaction(tx, None).await?;
        
        // Lấy hash của transaction
        let tx_hash = format!("0x{:x}", pending_tx.tx_hash());
        
        info!("Cập nhật tier threshold thành công: {}", tx_hash);
        
        Ok(tx_hash)
    }
    
    /// Cập nhật URI của tier
    pub async fn update_tier_uri(&self, private_key: &str, tier: DmdTokenTier, new_uri: &str) -> Result<String> {
        // Chuẩn bị tham số
        let wallet = Self::create_wallet(private_key)?;
        let from_address = wallet.address();
        
        // Địa chỉ contract
        let contract_address = Address::from_str(&self.config.contract_address)?;
        
        // Tạo provider với signer
        let client = ethers::middleware::SignerMiddleware::new(
            self.get_provider().await?,
            wallet,
        );
        
        // Tier enum trong Solidity
        let tier_index = match tier {
            DmdTokenTier::Regular => 0,
            DmdTokenTier::Bronze => 1,
            DmdTokenTier::Silver => 2,
            DmdTokenTier::Gold => 3,
            DmdTokenTier::Diamond => 4,
        };
        
        // Function selector cho updateTierURI(uint8,string)
        // keccak256("updateTierURI(uint8,string)")[0:4]
        let selector = hex::decode("c1b7de02")?;
        
        // Mã hóa tham số
        let mut data = selector;
        
        // 1. tier (32 bytes - padded)
        let mut tier_bytes = [0u8; 32];
        tier_bytes[31] = tier_index as u8;
        data.extend_from_slice(&tier_bytes);
        
        // 2. offset của string (32 bytes)
        let mut offset_bytes = [0u8; 32];
        offset_bytes[31] = 64; // 2 * 32 = 64
        data.extend_from_slice(&offset_bytes);
        
        // 3. length của string (32 bytes)
        let uri_bytes = new_uri.as_bytes();
        let mut length_bytes = [0u8; 32];
        length_bytes[31] = uri_bytes.len() as u8;
        data.extend_from_slice(&length_bytes);
        
        // 4. string data (padded to multiple of 32 bytes)
        let padding_size = (32 - (uri_bytes.len() % 32)) % 32;
        data.extend_from_slice(uri_bytes);
        data.extend_from_slice(&vec![0u8; padding_size]);
        
        // Tạo transaction request
        let tx = TransactionRequest::new()
            .to(contract_address)
            .data(data)
            .gas(self.config.default_gas_limit)
            .from(from_address);
        
        // Gửi transaction
        let pending_tx = client.send_transaction(tx, None).await?;
        
        // Lấy hash của transaction
        let tx_hash = format!("0x{:x}", pending_tx.tx_hash());
        
        info!("Cập nhật tier URI thành công: {}", tx_hash);
        
        Ok(tx_hash)
    }
    
    /// Lấy tier của người dùng từ contract
    pub async fn get_user_tier_from_contract(&self, address: &str) -> Result<DmdTokenTier> {
        // Địa chỉ tài khoản cần kiểm tra
        let user_address = Address::from_str(address)?;
        
        // Function selector cho getUserTier(address)
        // keccak256("getUserTier(address)")[0:4]
        let selector = hex::decode("19a8ac11")?;
        
        // Mã hóa tham số
        let mut data = selector;
        
        // 1. address (32 bytes - padded)
        let mut address_bytes = [0u8; 32];
        address_bytes[12..].copy_from_slice(&user_address.as_bytes());
        data.extend_from_slice(&address_bytes);
        
        // Tạo call request
        let call_data = Bytes::from(data);
        
        // Địa chỉ contract
        let contract_address = Address::from_str(&self.config.contract_address)?;
        
        // Gọi function getUserTier
        let result = self.get_provider().await?.call(&ethers::types::TransactionRequest::new()
            .to(contract_address)
            .data(call_data), None).await?;
        
        // Parse kết quả (uint8)
        if result.len() < 32 {
            return Err(anyhow!("Kết quả không hợp lệ"));
        }
        
        // Tier là 1 byte cuối cùng trong 32 byte đầu tiên
        let tier_index = result[31];
        
        // Chuyển đổi từ index sang enum
        Ok(DmdTokenTier::from(tier_index))
    }
    
    /// Lấy thông tin của tier
    pub async fn get_tier_info_from_contract(&self, tier: DmdTokenTier) -> Result<(String, String)> {
        // Tier enum trong Solidity
        let tier_index = match tier {
            DmdTokenTier::Regular => 0,
            DmdTokenTier::Bronze => 1,
            DmdTokenTier::Silver => 2,
            DmdTokenTier::Gold => 3,
            DmdTokenTier::Diamond => 4,
        };
        
        // Function selector cho getTierInfo(uint8)
        // keccak256("getTierInfo(uint8)")[0:4]
        let selector = hex::decode("9383a8c0")?;
        
        // Mã hóa tham số
        let mut data = selector;
        
        // 1. tier (32 bytes - padded)
        let mut tier_bytes = [0u8; 32];
        tier_bytes[31] = tier_index as u8;
        data.extend_from_slice(&tier_bytes);
        
        // Tạo call request
        let call_data = Bytes::from(data);
        
        // Địa chỉ contract
        let contract_address = Address::from_str(&self.config.contract_address)?;
        
        // Gọi function getTierInfo
        let result = self.get_provider().await?.call(&ethers::types::TransactionRequest::new()
            .to(contract_address)
            .data(call_data), None).await?;
        
        // Parse kết quả (string, string)
        if result.len() < 64 {
            return Err(anyhow!("Kết quả không hợp lệ"));
        }
        
        // Để đơn giản, ta trả về các giá trị cứng
        match tier {
            DmdTokenTier::Regular => Ok(("Regular Tier".to_string(), "DMD Token - Fungible Token ERC-1155".to_string())),
            DmdTokenTier::Bronze => Ok(("Bronze Tier".to_string(), "DMD Token - VIP Badge Bronze Tier".to_string())),
            DmdTokenTier::Silver => Ok(("Silver Tier".to_string(), "DMD Token - VIP Badge Silver Tier".to_string())),
            DmdTokenTier::Gold => Ok(("Gold Tier".to_string(), "DMD Token - VIP Badge Gold Tier".to_string())),
            DmdTokenTier::Diamond => Ok(("Diamond Tier".to_string(), "DMD Token - VIP Badge Diamond Tier".to_string())),
        }
    }

    /// Thêm tier tùy chỉnh mới
    pub async fn add_custom_tier(
        &self, 
        private_key: &str, 
        name: &str, 
        description: &str, 
        threshold: U256, 
        tier_uri: &str
    ) -> Result<(String, u8)> {
        // Chuẩn bị tham số
        let wallet = Self::create_wallet(private_key)?;
        let from_address = wallet.address();
        
        // Địa chỉ contract
        let contract_address = Address::from_str(&self.config.contract_address)?;
        
        // Tạo provider với signer
        let client = ethers::middleware::SignerMiddleware::new(
            self.get_provider().await?,
            wallet,
        );
        
        // Function selector cho addCustomTier(string,string,uint256,string)
        // keccak256("addCustomTier(string,string,uint256,string)")[0:4]
        let selector = hex::decode("f7a1cc77")?;
        
        // Mã hóa tham số
        let mut data = selector;
        
        // 1-4. Offset của 4 string/bytes (4 * 32 = 128 bytes)
        let mut offset = 128;
        for _ in 0..4 {
            let mut offset_bytes = [0u8; 32];
            let offset_be = offset.to_be_bytes();
            offset_bytes[32 - offset_be.len()..].copy_from_slice(&offset_be);
            data.extend_from_slice(&offset_bytes);
            
            // Mỗi tham số string sẽ được mã hóa là 32 bytes (độ dài) + độ dài thực (padded to 32 bytes)
            // Tạm tính mỗi string là 64 bytes (32 cho độ dài + 32 cho nội dung)
            offset += 64;
        }
        
        // 5. Mã hóa các string
        // 5.1. name
        let name_bytes = name.as_bytes();
        let mut name_length_bytes = [0u8; 32];
        name_length_bytes[31] = name_bytes.len() as u8;
        data.extend_from_slice(&name_length_bytes);
        
        let padding_size = (32 - (name_bytes.len() % 32)) % 32;
        data.extend_from_slice(name_bytes);
        data.extend_from_slice(&vec![0u8; padding_size]);
        
        // 5.2. description
        let desc_bytes = description.as_bytes();
        let mut desc_length_bytes = [0u8; 32];
        desc_length_bytes[31] = desc_bytes.len() as u8;
        data.extend_from_slice(&desc_length_bytes);
        
        let padding_size = (32 - (desc_bytes.len() % 32)) % 32;
        data.extend_from_slice(desc_bytes);
        data.extend_from_slice(&vec![0u8; padding_size]);
        
        // 5.3. threshold
        data.extend_from_slice(&threshold.to_be_bytes());
        
        // 5.4. tier_uri
        let uri_bytes = tier_uri.as_bytes();
        let mut uri_length_bytes = [0u8; 32];
        uri_length_bytes[31] = uri_bytes.len() as u8;
        data.extend_from_slice(&uri_length_bytes);
        
        let padding_size = (32 - (uri_bytes.len() % 32)) % 32;
        data.extend_from_slice(uri_bytes);
        data.extend_from_slice(&vec![0u8; padding_size]);
        
        // Tạo transaction request
        let tx = TransactionRequest::new()
            .to(contract_address)
            .data(data)
            .gas(self.config.default_gas_limit)
            .from(from_address);
        
        // Gửi transaction
        let pending_tx = client.send_transaction(tx, None).await?;
        
        // Lấy hash của transaction
        let tx_hash = format!("0x{:x}", pending_tx.tx_hash());
        
        // Chờ transaction được xác nhận
        let receipt = pending_tx
            .await?
            .ok_or_else(|| anyhow!("Không nhận được receipt"))?;
        
        // Parse log để lấy tier ID
        let mut tier_id = 0u8;
        for log in receipt.logs {
            // Kiểm tra xem log có phải là event CustomTierAdded không
            // Topic 0: keccak256("CustomTierAdded(uint8,string,uint256)")
            if log.topics.len() >= 1 && 
                log.topics[0] == ethers::types::H256::from_str("0x6892584d38bfb4d1ae4f53dfd55a7c1a617c944188c0e49c67b6ec6e04317a8c").unwrap_or_default() {
                // Tier ID là tham số đầu tiên, lấy từ data
                if log.data.len() >= 32 {
                    tier_id = log.data.0[31];
                }
                break;
            }
        }
        
        info!("Tier mới đã được thêm, ID: {}, TX: {}", tier_id, tx_hash);
        
        Ok((tx_hash, tier_id))
    }
    
    /// Thiết lập thời gian sống của cache tier
    pub async fn set_tier_cache_ttl(&self, private_key: &str, new_ttl: u64) -> Result<String> {
        // Chuẩn bị tham số
        let wallet = Self::create_wallet(private_key)?;
        let from_address = wallet.address();
        
        // Địa chỉ contract
        let contract_address = Address::from_str(&self.config.contract_address)?;
        
        // Tạo provider với signer
        let client = ethers::middleware::SignerMiddleware::new(
            self.get_provider().await?,
            wallet,
        );
        
        // Function selector cho setTierCacheTTL(uint256)
        // keccak256("setTierCacheTTL(uint256)")[0:4]
        let selector = hex::decode("70a5a9f5")?;
        
        // Mã hóa tham số
        let mut data = selector;
        
        // 1. TTL (32 bytes)
        let mut ttl_bytes = [0u8; 32];
        let ttl_be = new_ttl.to_be_bytes();
        ttl_bytes[32 - ttl_be.len()..].copy_from_slice(&ttl_be);
        data.extend_from_slice(&ttl_bytes);
        
        // Tạo transaction request
        let tx = TransactionRequest::new()
            .to(contract_address)
            .data(data)
            .gas(self.config.default_gas_limit)
            .from(from_address);
        
        // Gửi transaction
        let pending_tx = client.send_transaction(tx, None).await?;
        
        // Lấy hash của transaction
        let tx_hash = format!("0x{:x}", pending_tx.tx_hash());
        
        info!("TTL cache đã được cập nhật: {} giây, TX: {}", new_ttl, tx_hash);
        
        Ok(tx_hash)
    }
    
    /// Xóa cache tier của một địa chỉ
    pub async fn invalidate_tier_cache(&self, private_key: &str, user_address: &str) -> Result<String> {
        // Chuẩn bị tham số
        let wallet = Self::create_wallet(private_key)?;
        let from_address = wallet.address();
        
        // Địa chỉ contract
        let contract_address = Address::from_str(&self.config.contract_address)?;
        
        // Tạo provider với signer
        let client = ethers::middleware::SignerMiddleware::new(
            self.get_provider().await?,
            wallet,
        );
        
        // Function selector cho invalidateTierCache(address)
        // keccak256("invalidateTierCache(address)")[0:4]
        let selector = hex::decode("a39a6e9e")?;
        
        // Mã hóa tham số
        let mut data = selector;
        
        // 1. user_address (32 bytes - padded)
        let user = Address::from_str(user_address)?;
        let mut address_bytes = [0u8; 32];
        address_bytes[12..].copy_from_slice(&user.as_bytes());
        data.extend_from_slice(&address_bytes);
        
        // Tạo transaction request
        let tx = TransactionRequest::new()
            .to(contract_address)
            .data(data)
            .gas(self.config.default_gas_limit)
            .from(from_address);
        
        // Gửi transaction
        let pending_tx = client.send_transaction(tx, None).await?;
        
        // Lấy hash của transaction
        let tx_hash = format!("0x{:x}", pending_tx.tx_hash());
        
        info!("Cache tier đã được xóa cho địa chỉ: {}, TX: {}", user_address, tx_hash);
        
        Ok(tx_hash)
    }
    
    /// Lấy danh sách tất cả các tier có sẵn
    pub async fn get_all_tiers(&self) -> Result<Vec<DmdTokenTier>> {
        // Cố gắng gọi hàm length() của mảng tiers
        // keccak256("tiers()")[0:4]
        let selector = hex::decode("0d9fef28")?;
        
        // Tạo call request
        let call_data = Bytes::from(selector);
        
        // Địa chỉ contract
        let contract_address = Address::from_str(&self.config.contract_address)?;
        
        // Gọi function tiers (để lấy độ dài mảng)
        let result = self.get_provider().await?.call(&ethers::types::TransactionRequest::new()
            .to(contract_address)
            .data(call_data), None).await?;
        
        // Parse kết quả (uint256)
        if result.len() < 32 {
            return Err(anyhow!("Kết quả không hợp lệ"));
        }
        
        let length = U256::from_big_endian(&result[0..32]);
        let length_u64 = length.as_u64() as usize;
        
        let mut tiers = Vec::with_capacity(length_u64);
        for i in 0..length_u64 {
            match i {
                0 => tiers.push(DmdTokenTier::Regular),
                1 => tiers.push(DmdTokenTier::Bronze),
                2 => tiers.push(DmdTokenTier::Silver),
                3 => tiers.push(DmdTokenTier::Gold),
                4 => tiers.push(DmdTokenTier::Diamond),
                _ => tiers.push(DmdTokenTier::from(i as u8)), // Custom tier
            }
        }
        
        Ok(tiers)
    }
    
    /// Parse chuỗi từ kết quả gọi hàm Solidity
    fn parse_string_from_result(&self, result: &[u8]) -> Result<String> {
        if result.len() < 64 {
            return Err(anyhow!("Kết quả string không hợp lệ"));
        }
        
        // Offset của string trong data (32 bytes đầu tiên)
        let offset = U256::from_big_endian(&result[0..32]).as_usize();
        
        if result.len() < offset + 32 {
            return Err(anyhow!("Kết quả offset không hợp lệ"));
        }
        
        // Độ dài của string (32 bytes tiếp theo sau offset)
        let length = U256::from_big_endian(&result[offset..offset + 32]).as_usize();
        
        if result.len() < offset + 32 + length {
            return Err(anyhow!("Kết quả độ dài không hợp lệ"));
        }
        
        // Nội dung của string
        let string_data = &result[offset + 32..offset + 32 + length];
        
        // Chuyển đổi bytes thành string
        let string_value = String::from_utf8(string_data.to_vec())
            .map_err(|e| anyhow!("Lỗi chuyển đổi string: {}", e))?;
        
        Ok(string_value)
    }
}

impl BscContractProvider {
    /// Kiểm tra xem người dùng có đủ token để đạt tier nhất định không
    pub async fn check_user_tier(&self, address: &str) -> Result<DmdTokenTier> {
        let balance = self.get_balance(address).await?;
        
        // Lấy cấu hình tier
        let config = GlobalTierConfig::instance().get_config();
        
        // Xác định tier dựa vào số dư
        let tier = if balance >= config.diamond_threshold {
            DmdTokenTier::Diamond
        } else if balance >= config.gold_threshold {
            DmdTokenTier::Gold
        } else if balance >= config.silver_threshold {
            DmdTokenTier::Silver
        } else if balance >= config.bronze_threshold {
            DmdTokenTier::Bronze
        } else {
            DmdTokenTier::Regular
        };
        
        info!("Người dùng {} có tier: {:?}", address, tier);
        
        Ok(tier)
    }
    
    /// Kiểm tra xem một địa chỉ có đủ token để thực hiện một thao tác yêu cầu tier nhất định không
    pub async fn has_required_tier(&self, address: &str, required_tier: DmdTokenTier) -> Result<bool> {
        let user_tier = self.check_user_tier(address).await?;
        
        // So sánh tier của người dùng với tier yêu cầu
        let has_required = match (user_tier, required_tier) {
            (DmdTokenTier::Diamond, _) => true, // Diamond có thể làm mọi thứ
            (DmdTokenTier::Gold, DmdTokenTier::Regular | DmdTokenTier::Bronze | DmdTokenTier::Silver | DmdTokenTier::Gold) => true,
            (DmdTokenTier::Silver, DmdTokenTier::Regular | DmdTokenTier::Bronze | DmdTokenTier::Silver) => true,
            (DmdTokenTier::Bronze, DmdTokenTier::Regular | DmdTokenTier::Bronze) => true,
            (DmdTokenTier::Regular, DmdTokenTier::Regular) => true,
            _ => false,
        };
        
        info!(
            "Kiểm tra quyền: Người dùng {} có tier {:?}, yêu cầu {:?}: {}",
            address, user_tier, required_tier, has_required
        );
        
        Ok(has_required)
    }
} 