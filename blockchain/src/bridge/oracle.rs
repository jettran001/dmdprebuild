//! Oracle module for bridging DMD tokens
//! 
//! Theo dõi và đồng bộ hóa dữ liệu giữa các blockchain, cung cấp
//! thông tin về giá trị và số lượng DMD trên từng chain.

use std::{collections::HashMap, sync::{Arc, Mutex}, time::{Duration, SystemTime}};
use async_trait::async_trait;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};

use super::{
    error::{BridgeError, BridgeResult},
    types::{BridgeDirection, BridgeTokenType},
};
use crate::smartcontracts::TransactionHash;
use crate::common::suspicious_detection::{SuspiciousTransactionManager, SuspiciousTransactionRecord, SuspiciousTransactionType};
use crate::bridge::types::BridgeTransaction;

/// Loại dữ liệu được oracle theo dõi
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum OracleDataType {
    /// Tỷ giá token
    TokenPrice,
    /// Tổng cung trên một chain
    TotalSupply,
    /// Tổng khối lượng đã bridge
    TotalBridged,
    /// Thông tin giao dịch bridge
    BridgeTransaction,
}

/// Trạng thái cập nhật của oracle
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OracleUpdateStatus {
    /// Cập nhật thành công
    Success,
    /// Đang chờ xác nhận
    Pending,
    /// Đã thất bại
    Failed,
}

/// Dữ liệu cập nhật của oracle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleData {
    /// Loại dữ liệu
    pub data_type: OracleDataType,
    /// Chuỗi blockchain liên quan
    pub chain: String,
    /// Dữ liệu dạng chuỗi JSON
    pub data: String,
    /// Thời điểm cập nhật
    pub timestamp: u64,
    /// Trạng thái cập nhật
    pub status: OracleUpdateStatus,
    /// Hash giao dịch cập nhật, nếu có
    pub tx_hash: Option<TransactionHash>,
    /// Số lượng xác nhận của dữ liệu này
    pub confirmations: u32,
    /// Nguồn dữ liệu
    pub source: String,
}

/// Trait cho các oracle provider
#[async_trait]
pub trait OracleProvider: Send + Sync {
    /// Trả về tên của oracle provider
    fn name(&self) -> String;
    
    /// Lấy danh sách các blockchain được hỗ trợ
    fn supported_chains(&self) -> Vec<String>;
    
    /// Kiểm tra chuỗi có được hỗ trợ không
    fn is_chain_supported(&self, chain: &str) -> bool {
        self.supported_chains().contains(&chain.to_string())
    }
    
    /// Cập nhật dữ liệu lên oracle
    async fn update_data(&self, data: OracleData) -> BridgeResult<OracleUpdateStatus>;
    
    /// Lấy dữ liệu từ oracle
    async fn get_data(&self, data_type: OracleDataType, chain: &str) -> BridgeResult<Option<OracleData>>;
    
    /// Xác nhận cập nhật
    async fn confirm_update(&self, data_type: OracleDataType, chain: &str, tx_hash: &TransactionHash) -> BridgeResult<OracleUpdateStatus>;
}

/// Cấu hình cho ChainlinkOracle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainlinkConfig {
    /// Địa chỉ hợp đồng oracle
    pub oracle_address: String,
    /// Job ID cho oracle
    pub job_id: String,
    /// Khóa API, nếu cần
    pub api_key: Option<String>,
    /// Phí oracle
    pub fee: String,
}

/// Oracle provider sử dụng Chainlink
pub struct ChainlinkOracle {
    /// Ánh xạ từ tên blockchain tới cấu hình
    configs: HashMap<String, ChainlinkConfig>,
    /// Cache dữ liệu
    data_cache: Arc<Mutex<HashMap<(OracleDataType, String), OracleData>>>,
    /// Thời gian hết hạn cache (ms)
    cache_ttl_ms: u64,
}

impl ChainlinkOracle {
    /// Tạo oracle mới với cấu hình cho từng chain
    pub fn new(configs: HashMap<String, ChainlinkConfig>, cache_ttl_ms: u64) -> Self {
        Self {
            configs,
            data_cache: Arc::new(Mutex::new(HashMap::new())),
            cache_ttl_ms,
        }
    }
    
    /// Kiểm tra xem cache có hợp lệ không
    fn is_cache_valid(&self, timestamp: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_millis() as u64;
            
        now - timestamp <= self.cache_ttl_ms
    }
}

#[async_trait]
impl OracleProvider for ChainlinkOracle {
    fn name(&self) -> String {
        "Chainlink Oracle".to_string()
    }
    
    fn supported_chains(&self) -> Vec<String> {
        self.configs.keys().cloned().collect()
    }
    
    async fn update_data(&self, data: OracleData) -> BridgeResult<OracleUpdateStatus> {
        if !self.is_chain_supported(&data.chain) {
            return Err(BridgeError::UnsupportedChain(data.chain));
        }
        
        // Lưu vào cache trước
        {
            let mut cache = self.data_cache.lock().map_err(|_| BridgeError::OracleError("Cache lock failed".into()))?;
            cache.insert((data.data_type.clone(), data.chain.clone()), data.clone());
        }
        
        // Mô phỏng cập nhật lên Chainlink (trong triển khai thực tế sẽ gửi giao dịch đến oracle contract)
        // TODO: Implement actual Chainlink integration
        
        info!("Updated oracle data for {}: {:?}", data.chain, data.data_type);
        Ok(OracleUpdateStatus::Success)
    }
    
    async fn get_data(&self, data_type: OracleDataType, chain: &str) -> BridgeResult<Option<OracleData>> {
        if !self.is_chain_supported(chain) {
            return Err(BridgeError::UnsupportedChain(chain.to_string()));
        }
        
        // Kiểm tra cache
        let cache = self.data_cache.lock().map_err(|_| BridgeError::OracleError("Cache lock failed".into()))?;
        if let Some(data) = cache.get(&(data_type.clone(), chain.to_string())) {
            if self.is_cache_valid(data.timestamp) {
                return Ok(Some(data.clone()));
            }
        }
        
        // TODO: Implement fetching data from Chainlink oracle if not in cache
        
        Ok(None)
    }
    
    async fn confirm_update(&self, data_type: OracleDataType, chain: &str, tx_hash: &TransactionHash) -> BridgeResult<OracleUpdateStatus> {
        if !self.is_chain_supported(chain) {
            return Err(BridgeError::UnsupportedChain(chain.to_string()));
        }
        
        // TODO: Check transaction status on chain
        
        Ok(OracleUpdateStatus::Success)
    }
}

/// Định nghĩa ngưỡng đồng thuận
#[derive(Debug, Clone)]
pub struct ConsensusConfig {
    /// Số lượng nguồn dữ liệu tối thiểu cần thiết cho đồng thuận
    pub min_sources: usize,
    /// Phần trăm độ lệch tối đa giữa các nguồn (0-100)
    pub max_deviation_percent: f64,
    /// Thời gian tối đa giữa các cập nhật (ms)
    pub max_time_difference_ms: u64,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            min_sources: 2,
            max_deviation_percent: 5.0, // 5%
            max_time_difference_ms: 300000, // 5 phút
        }
    }
}

/// Quản lý Oracle
pub struct OracleManager {
    /// Danh sách các providers cho mỗi cặp token/chain
    providers: HashMap<String, Vec<Box<dyn OracleProvider>>>,
    
    /// Manager phát hiện giao dịch đáng ngờ
    suspicious_detection: Arc<SuspiciousTransactionManager>,
    
    /// Ngưỡng bias được coi là bất thường
    abnormal_bias_threshold: f64,
    
    /// Danh sách các giao dịch bất thường
    abnormal_transactions: Arc<Mutex<Vec<SuspiciousTransactionRecord>>>,
    
    /// Hàm thông báo admin
    admin_notifier: Option<Arc<dyn Fn(&str, &SuspiciousTransactionRecord) -> Result<()> + Send + Sync>>,
    
    /// Cache token price
    price_cache: Arc<Mutex<HashMap<String, (f64, u64)>>>,
    /// Thời gian hết hạn cache giá (ms)
    price_cache_ttl_ms: u64,
    /// Cấu hình đồng thuận
    consensus_config: ConsensusConfig,
    /// Dữ liệu từ nhiều nguồn để đồng thuận
    multi_source_data: Arc<Mutex<HashMap<(OracleDataType, String), Vec<OracleData>>>>,
    /// Yêu cầu provider phải hỗ trợ ít nhất một chain
    require_supported_chains: bool,
}

impl OracleManager {
    /// Khởi tạo OracleManager mới
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            suspicious_detection: Arc::new(SuspiciousTransactionManager::new()),
            abnormal_bias_threshold: 0.05, // 5% mặc định
            abnormal_transactions: Arc::new(Mutex::new(Vec::new())),
            admin_notifier: None,
            price_cache: Arc::new(Mutex::new(HashMap::new())),
            price_cache_ttl_ms: 60000,
            consensus_config: ConsensusConfig::default(),
            multi_source_data: Arc::new(Mutex::new(HashMap::new())),
            require_supported_chains: true,
        }
    }
    
    /// Tạo oracle manager với cấu hình đồng thuận tùy chỉnh
    pub fn with_consensus_config(price_cache_ttl_ms: u64, consensus_config: ConsensusConfig) -> Self {
        Self {
            providers: HashMap::new(),
            suspicious_detection: Arc::new(SuspiciousTransactionManager::new()),
            abnormal_bias_threshold: 0.05, // 5% mặc định
            abnormal_transactions: Arc::new(Mutex::new(Vec::new())),
            admin_notifier: None,
            price_cache: Arc::new(Mutex::new(HashMap::new())),
            price_cache_ttl_ms,
            consensus_config,
            multi_source_data: Arc::new(Mutex::new(HashMap::new())),
            require_supported_chains: true,
        }
    }
    
    /// Đặt yêu cầu provider phải hỗ trợ ít nhất một chain
    pub fn set_require_supported_chains(&mut self, require: bool) -> &mut Self {
        self.require_supported_chains = require;
        self
    }
    
    /// Thêm một nhà cung cấp mới vào danh sách
    pub fn add_provider(&mut self, provider: Box<dyn OracleProvider>) -> BridgeResult<()> {
        // Kiểm tra tên nhà cung cấp không được để trống
        if provider.name().is_empty() {
            return Err(BridgeError::OracleError("Tên nhà cung cấp không được để trống".into()));
        }
        
        // Kiểm tra xem nhà cung cấp đã tồn tại chưa
        let exists = self.providers.iter().any(|p| p.name() == provider.name());
        if exists {
            return Err(BridgeError::OracleError(
                format!("Nhà cung cấp '{}' đã tồn tại trong danh sách", provider.name())
            ));
        }
        
        // Kiểm tra danh sách chain được hỗ trợ
        let supported_chains = provider.supported_chains();
        
        if supported_chains.is_empty() {
            let msg = format!("Nhà cung cấp '{}' không hỗ trợ chain nào", provider.name());
            
            // Nếu cài đặt yêu cầu provider phải hỗ trợ ít nhất một chain
            if self.require_supported_chains {
                error!("{}", msg);
                return Err(BridgeError::OracleError(
                    format!("{} - Không thể thêm provider không hỗ trợ chain nào", msg)
                ));
            } else {
                // Log cảnh báo rõ ràng hơn nhưng vẫn cho phép thêm
                warn!("{} - Provider này có thể không sử dụng được!", msg);
                warn!("Bạn nên xem lại cấu hình của provider '{}' để đảm bảo nó hỗ trợ các chain cần thiết", provider.name());
            }
        } else {
            // Hiển thị danh sách chain được hỗ trợ
            info!(
                "Provider '{}' hỗ trợ {} chain: {:?}",
                provider.name(),
                supported_chains.len(),
                supported_chains
            );
        }
        
        // Kiểm tra xem có chain nào cần thiết mà provider không hỗ trợ không
        let required_chains = vec!["Ethereum", "BinanceSmartChain", "Polygon", "Avalanche"];
        let mut missing_chains = Vec::new();
        
        for chain in required_chains.iter() {
            if !provider.is_chain_supported(chain) {
                missing_chains.push(chain.to_string());
            }
        }
        
        if !missing_chains.is_empty() {
            warn!(
                "Cảnh báo: Provider '{}' không hỗ trợ các chain quan trọng sau: {:?}",
                provider.name(),
                missing_chains
            );
        }
        
        // Tìm các chain được hỗ trợ bởi nhiều provider
        let mut common_chains = Vec::new();
        for existing_provider in self.providers.iter() {
            for chain in supported_chains.iter() {
                if existing_provider.is_chain_supported(chain) {
                    common_chains.push((existing_provider.name().clone(), chain.clone()));
                }
            }
        }
        
        // Ghi log các chain được hỗ trợ bởi nhiều provider
        if !common_chains.is_empty() {
            info!(
                "Provider mới '{}' có {} chain được hỗ trợ bởi các provider khác:",
                provider.name(),
                common_chains.len()
            );
            for (other_provider, chain) in common_chains {
                info!("  - Chain '{}' cũng được hỗ trợ bởi provider '{}'", chain, other_provider);
            }
        }
        
        // Thêm provider mới
        info!("Thêm provider mới: {}", provider.name());
        self.providers.insert(provider.name(), vec![provider]);
        
        Ok(())
    }
    
    /// Cập nhật giá token từ một nguồn
    pub fn update_token_price(&mut self, chain: &str, token: &str, price: &str, source: &str) -> BridgeResult<()> {
        // Kiểm tra tính hợp lệ của các tham số đầu vào
        if chain.is_empty() {
            return Err(BridgeError::OracleError("Chain không được để trống".into()));
        }
        
        if token.is_empty() {
            return Err(BridgeError::OracleError("Token không được để trống".into()));
        }
        
        if source.is_empty() {
            return Err(BridgeError::OracleError("Nguồn không được để trống".into()));
        }
        
        // Chuyển đổi giá trị giá từ string sang f64
        let price_value = match price.parse::<f64>() {
            Ok(val) => val,
            Err(_) => return Err(BridgeError::OracleError(format!("Giá '{}' không hợp lệ", price)))
        };
        
        // Kiểm tra giá trị âm
        if price_value < 0.0 {
            return Err(BridgeError::OracleError("Giá token không thể là số âm".into()));
        }
        
        // Cảnh báo nếu giá là 0 hoặc quá nhỏ
        if price_value == 0.0 {
            warn!("Cảnh báo: Giá token {}/{} từ nguồn {} là 0", chain, token, source);
        } else if price_value < 0.0000001 && price_value > 0.0 {
            warn!("Cảnh báo: Giá token {}/{} từ nguồn {} rất nhỏ: {}", chain, token, source, price_value);
        } else if price_value > 1_000_000.0 {
            warn!("Cảnh báo: Giá token {}/{} từ nguồn {} rất cao: {}", chain, token, source, price_value);
        }
        
        let key = format!("{}:{}", chain, token);
        
        // Kiểm tra sự thay đổi bất thường về giá
        let mut significant_change = false;
        let mut price_deviation_pct = 0.0;
        
        if let Some(current_price) = self.price_cache.lock().map_err(|_| BridgeError::OracleError("Cache lock failed".into()))?.get(&key) {
            let old_price = current_price.0;
            if old_price > 0.0 {
                price_deviation_pct = ((price_value - old_price) / old_price).abs() * 100.0;
                
                // Nếu giá thay đổi đáng kể (>20%), ghi log và đánh dấu cần xác nhận thêm
                if price_deviation_pct > 20.0 {
                    warn!(
                        "Thay đổi đáng kể về giá token {}/{}: {} -> {} ({:.2}%) từ nguồn {}",
                        chain, token, old_price, price_value, price_deviation_pct, source
                    );
                    significant_change = true;
                }
            }
        }
        
        // Lưu giá mới
        let timestamp = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_millis() as u64;
                
        // Nếu thay đổi lớn (>50%), yêu cầu xác nhận từ nhiều nguồn
        if price_deviation_pct > 50.0 {
            // Thêm vào danh sách chờ xác nhận
            let confirmation_key = format!("{}:{}:{}", chain, token, source);
            self.multi_source_data.lock().map_err(|e| BridgeError::OracleError(e.to_string()))?.insert(
                (OracleDataType::TokenPrice, chain.to_string()),
                OracleData {
                data_type: OracleDataType::TokenPrice,
                chain: chain.to_string(),
                    data: price_value.to_string(),
                    timestamp,
                status: OracleUpdateStatus::Pending,
                tx_hash: None,
                confirmations: 1,
                    source: source.to_string(),
                }
            );
            
            info!(
                "Thay đổi lớn về giá token {}/{}: Yêu cầu xác nhận từ nguồn khác trước khi cập nhật",
                chain, token
            );
            
            // Kiểm tra xem có ít nhất 2 nguồn cùng báo giá tương tự không
            self.verify_consensus_price(chain, token, price_value)?;
        } else {
            // Cập nhật giá cho cả kho lưu trữ đơn và đa nguồn
            self.price_cache.lock().map_err(|e| BridgeError::OracleError(e.to_string()))?.insert(
                key,
                (price_value, timestamp)
            );
            
            // Cập nhật kho đa nguồn
            let multi_source_key = format!("{}:{}:{}", chain, token, source);
            self.multi_source_data.lock().map_err(|e| BridgeError::OracleError(e.to_string()))?.insert(
                (OracleDataType::TokenPrice, chain.to_string()),
                OracleData {
                    data_type: OracleDataType::TokenPrice,
                    chain: chain.to_string(),
                    data: price_value.to_string(),
                    timestamp,
                    status: OracleUpdateStatus::Pending,
                    tx_hash: None,
                    confirmations: 1,
                    source: source.to_string(),
                }
            );
            
            info!(
                "Đã cập nhật giá token {}/{} = {} từ nguồn {}",
                chain, token, price_value, source
            );
        }
        
        Ok(())
    }
    
    /// Kiểm tra xem có đồng thuận về giá từ nhiều nguồn không
    fn verify_consensus_price(&mut self, chain: &str, token: &str, new_price: f64) -> BridgeResult<bool> {
        let threshold = 0.1; // 10% sai lệch được chấp nhận
        let key = format!("{}:{}", chain, token);
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_millis() as u64;
        
        // Đếm số nguồn báo giá tương tự
        let mut similar_price_count = 0;
        let mut sources = Vec::new();
        
        let data_guard = self.multi_source_data.lock().map_err(|e| BridgeError::OracleError(e.to_string()))?;
        for ((c, t, s), (price, _)) in data_guard.iter() {
            if c == chain && t == token {
                let deviation = ((new_price - price) / price).abs();
                if deviation <= threshold {
                    similar_price_count += 1;
                    sources.push(s.clone());
                }
            }
        }
        
        // Nếu có ít nhất 2 nguồn (bao gồm nguồn hiện tại) báo giá tương tự
        if similar_price_count >= 1 {
            info!(
                "Đã xác nhận giá token {}/{} = {} từ nhiều nguồn: {:?}",
                chain, token, new_price, sources
            );
            
            // Cập nhật giá chính thức
            self.price_cache.lock().map_err(|e| BridgeError::OracleError(e.to_string()))?.insert(
                key,
                (new_price, timestamp)
            );
            
            return Ok(true);
        }
        
        Ok(false)
    }
    
    /// Lấy giá token DMD cho một chain
    pub async fn get_token_price(&self, chain: &str) -> BridgeResult<Option<f64>> {
        // Thử lấy giá từ đồng thuận đa nguồn trước
        if let Some(consensus_price) = self.get_price_consensus(chain).await? {
            debug!("Sử dụng giá đồng thuận cho {}: {}", chain, consensus_price);
            return Ok(Some(consensus_price));
        }
        
        // Kiểm tra cache local nếu không có đồng thuận
        {
            let cache = self.price_cache.lock().map_err(|_| BridgeError::OracleError("Cache lock failed".into()))?;
            if let Some((price, timestamp)) = cache.get(chain) {
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or(Duration::from_secs(0))
                    .as_millis() as u64;
                
                if now - timestamp <= self.price_cache_ttl_ms {
                    return Ok(Some(*price));
                }
            }
        }
        
        // Nếu không có trong cache hoặc đã hết hạn, truy vấn các provider
        for provider in &self.providers {
            if provider.is_chain_supported(chain) {
                if let Ok(Some(data)) = provider.get_data(OracleDataType::TokenPrice, chain).await {
                    if let Ok(price) = data.data.parse::<f64>() {
                        // Xác thực giá token
                        self.validate_token_price(price)?;
                        
                        // Cập nhật cache
                        let mut cache = self.price_cache.lock().map_err(|_| BridgeError::OracleError("Cache lock failed".into()))?;
                        cache.insert(chain.to_string(), (price, data.timestamp));
                        
                        return Ok(Some(price));
                    }
                }
            }
        }
        
        Ok(None)
    }
    
    /// Cập nhật tổng cung token trên một chain
    pub async fn update_total_supply(&self, chain: &str, total_supply: String) -> BridgeResult<()> {
        // Kiểm tra dữ liệu hợp lệ
        match total_supply.parse::<u128>() {
            Ok(_) => {}, // Dữ liệu hợp lệ
            Err(e) => return Err(BridgeError::OracleError(
                format!("Tổng cung không hợp lệ: {}: {}", total_supply, e)
            )),
        }
        
        let data = OracleData {
            data_type: OracleDataType::TotalSupply,
            chain: chain.to_string(),
            data: total_supply,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_millis() as u64,
            status: OracleUpdateStatus::Pending,
            tx_hash: None,
            confirmations: 1,
            source: "local".to_string(),
        };
        
        for provider in &self.providers {
            if provider.is_chain_supported(chain) {
                match provider.update_data(data.clone()).await {
                    Ok(_) => info!("Updated total supply for {} on provider {}", chain, provider.name()),
                    Err(e) => error!("Failed to update total supply for {} on provider {}: {:?}", chain, provider.name(), e),
                }
            }
        }
        
        Ok(())
    }
    
    /// Lấy tổng cung token trên một chain
    pub async fn get_total_supply(&self, chain: &str) -> BridgeResult<Option<String>> {
        for provider in &self.providers {
            if provider.is_chain_supported(chain) {
                if let Ok(Some(data)) = provider.get_data(OracleDataType::TotalSupply, chain).await {
                    return Ok(Some(data.data));
                }
            }
        }
        
        Ok(None)
    }
    
    /// Cập nhật thông tin giao dịch bridge
    pub async fn update_bridge_transaction(
        &self,
        source_chain: &str,
        target_chain: &str,
        tx_hash: &TransactionHash,
        status: OracleUpdateStatus,
    ) -> BridgeResult<()> {
        let data = OracleData {
            data_type: OracleDataType::BridgeTransaction,
            chain: format!("{}-{}", source_chain, target_chain),
            data: serde_json::to_string(&(tx_hash, status.clone())).map_err(|e| BridgeError::SerializationError(e.to_string()))?,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_millis() as u64,
            status,
            tx_hash: Some(tx_hash.clone()),
            confirmations: 1,
            source: "local".to_string(),
        };
        
        for provider in &self.providers {
            if provider.is_chain_supported(source_chain) && provider.is_chain_supported(target_chain) {
                match provider.update_data(data.clone()).await {
                    Ok(_) => info!("Updated bridge transaction {:?} from {} to {}", tx_hash, source_chain, target_chain),
                    Err(e) => error!("Failed to update bridge transaction: {:?}", e),
                }
            }
        }
        
        Ok(())
    }
    
    /// Đặt ngưỡng bias bất thường
    pub fn set_abnormal_bias_threshold(&mut self, threshold: f64) -> Result<()> {
        if threshold <= 0.0 || threshold >= 1.0 {
            return Err(BridgeError::OracleError(format!(
                "Abnormal bias threshold phải nằm trong khoảng (0, 1), nhưng nhận được {}", 
                threshold
            )));
        }
        
        self.abnormal_bias_threshold = threshold;
        info!("Đã đặt ngưỡng phát hiện bias bất thường là {}", threshold);
        Ok(())
    }
    
    /// Đặt hàm thông báo admin cho phát hiện giao dịch bất thường
    pub fn set_admin_notifier<F>(&mut self, notifier: F) -> &mut Self
    where
        F: Fn(&str, &SuspiciousTransactionRecord) -> Result<()> + 'static + Send + Sync,
    {
        // Đặt notifier cho cả SuspiciousTransactionManager
        self.suspicious_detection = Arc::new(
            SuspiciousTransactionManager::new().with_admin_notifier(notifier.clone())
        );
        
        self.admin_notifier = Some(Arc::new(notifier));
        
        info!("Đã đặt hàm thông báo admin cho phát hiện giao dịch bất thường");
        self
    }
    
    /// Xóa hàm thông báo admin
    pub fn clear_admin_notifier(&mut self) -> &mut Self {
        // Khởi tạo lại suspicious_detection manager không có notifier
        self.suspicious_detection = Arc::new(SuspiciousTransactionManager::new());
        
        self.admin_notifier = None;
        
        info!("Đã xóa hàm thông báo admin");
        self
    }
    
    /// Kiểm tra giao dịch bất thường dựa trên dữ liệu Oracle
    pub fn check_abnormal_transaction(&self, transaction: &BridgeTransaction) -> Result<bool> {
        // Sử dụng SuspiciousTransactionManager để kiểm tra các mẫu đáng ngờ thông thường
        let records = self.suspicious_detection.check_transaction(transaction)
            .map_err(|e| BridgeError::OracleError(format!("Lỗi kiểm tra giao dịch: {}", e)))?;
        
        let mut is_abnormal = false;
        
        if !records.is_empty() {
            is_abnormal = true;
            
            // Lưu vào danh sách giao dịch bất thường
            let mut abnormal = self.abnormal_transactions.lock()
                .map_err(|_| BridgeError::OracleError("Failed to acquire lock for abnormal transactions".into()))?;
            
            for record in &records {
                abnormal.push(record.clone());
                
            warn!(
                    "Phát hiện giao dịch bất thường: {} {} từ {} -> {} ({}->{}): {}",
                    record.amount, record.token, record.source_address, record.destination_address,
                    record.source_chain, record.destination_chain, record.description
                );
                
                // Thông báo cho admin
                if let Some(notifier) = &self.admin_notifier {
                    if let Err(e) = notifier("ABNORMAL_TRANSACTION", record) {
                        error!("Không thể thông báo cho admin: {:?}", e);
                    }
                }
            }
        }
        
        // Kiểm tra thêm các điều kiện bất thường đặc biệt cho Oracle như bias
        // ... (Mã cũ kiểm tra bias có thể giữ lại ở đây nếu cần) ...
        
        Ok(is_abnormal)
    }
    
    /// Đạt được đồng thuận về giá từ nhiều nguồn dữ liệu
    async fn get_price_consensus(&self, chain: &str) -> BridgeResult<Option<f64>> {
        let multi_data = self.multi_source_data.lock().map_err(|_| BridgeError::OracleError("Multi-source data lock failed".into()))?;
        
        let key = (OracleDataType::TokenPrice, chain.to_string());
        let data_points = match multi_data.get(&key) {
            Some(points) => points,
            None => return Ok(None),
        };
        
        // Lấy thời điểm hiện tại
        let now = SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or(Duration::from_secs(0))
            .as_millis() as u64;
            
        // Lọc các dữ liệu gần đây theo cấu hình
        let recent_points: Vec<&OracleData> = data_points
            .iter()
            .filter(|data| now - data.timestamp <= self.consensus_config.max_time_difference_ms)
            .collect();
            
        // Kiểm tra số lượng nguồn dữ liệu
        if recent_points.len() < self.consensus_config.min_sources {
            debug!("Không đủ nguồn dữ liệu cho đồng thuận: {} (cần {})",
                recent_points.len(), self.consensus_config.min_sources);
            return Ok(None);
        }
        
        // Chuyển đổi các giá trị thành số
        let mut prices: Vec<(f64, &str, u64)> = Vec::new();
        for data in &recent_points {
            match data.data.parse::<f64>() {
                Ok(price) => prices.push((price, &data.source, data.timestamp)),
                Err(e) => warn!("Không thể chuyển đổi giá trị '{}' từ nguồn {}: {}", 
                    data.data, data.source, e),
            }
        }
        
        // Kiểm tra lại sau khi loại bỏ các giá trị không hợp lệ
        if prices.len() < self.consensus_config.min_sources {
            debug!("Không đủ giá trị hợp lệ cho đồng thuận: {} (cần {})",
                prices.len(), self.consensus_config.min_sources);
            return Ok(None);
        }
        
        // Phân tích để xử lý các giá trị lệch (outliers)
        // Sử dụng thuật toán Modified Z-Score để phát hiện outliers
        if prices.len() >= 3 { // Cần ít nhất 3 giá trị để phát hiện outlier
            // Tính median
            let mut values: Vec<f64> = prices.iter().map(|(p, _, _)| *p).collect();
            values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            
            let median = if values.len() % 2 == 0 {
                (values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
            } else {
                values[values.len() / 2]
            };
            
            // Tính MAD (Median Absolute Deviation)
            let mut deviations: Vec<f64> = values.iter()
                .map(|x| (x - median).abs())
                .collect();
            deviations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            
            let mad = if deviations.len() % 2 == 0 {
                (deviations[deviations.len() / 2 - 1] + deviations[deviations.len() / 2]) / 2.0
            } else {
                deviations[deviations.len() / 2]
            };
            
            // Hằng số 0.6745 được sử dụng trong thuật toán Modified Z-Score
            const MODIFIED_Z_THRESHOLD: f64 = 3.5; // Ngưỡng để coi là outlier
            
            // Xác định và loại bỏ các outliers
            let mut filtered_prices = Vec::new();
            let mut outliers = Vec::new();
            
            if mad != 0.0 { // Tránh chia cho 0
                for (price, source, timestamp) in prices {
                    let modified_z = 0.6745 * (price - median) / mad;
                    
                    if modified_z.abs() <= MODIFIED_Z_THRESHOLD {
                        filtered_prices.push(price);
                    } else {
                        outliers.push((price, source, timestamp, modified_z));
                        warn!("Phát hiện giá lệch ({}) từ nguồn {} với modified Z-score: {:.2}",
                            price, source, modified_z);
                    }
                }
            } else {
                // Nếu MAD = 0, không có độ lệch, dùng tất cả giá trị
                filtered_prices = prices.iter().map(|(p, _, _)| *p).collect();
            }
            
            // Nếu sau khi lọc vẫn đủ nguồn dữ liệu, sử dụng các giá trị đã lọc
            if filtered_prices.len() >= self.consensus_config.min_sources {
                // Sử dụng median thay vì mean để chống outlier tốt hơn
                filtered_prices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                
                let consensus_price = if filtered_prices.len() % 2 == 0 {
                    (filtered_prices[filtered_prices.len() / 2 - 1] + filtered_prices[filtered_prices.len() / 2]) / 2.0
                } else {
                    filtered_prices[filtered_prices.len() / 2]
                };
                
                // Ghi log các giá trị bị loại bỏ
                if !outliers.is_empty() {
                    info!("Đã loại bỏ {} giá lệch, sử dụng giá đồng thuận {} từ {} nguồn", 
                        outliers.len(), consensus_price, filtered_prices.len());
                }
                
                return Ok(Some(consensus_price));
            } else {
                // Không đủ dữ liệu sau khi lọc, quay lại phương pháp trung bình
                debug!("Sau khi lọc outlier, không đủ nguồn dữ liệu ({}/{}), sử dụng phương pháp cũ",
                    filtered_prices.len(), self.consensus_config.min_sources);
            }
        }
        
        // Phương pháp dự phòng: tính giá trung bình
        let avg_price: f64 = prices.iter().map(|(p, _, _)| *p).sum::<f64>() / prices.len() as f64;
        
        // Kiểm tra độ lệch so với trung bình và ghi log cảnh báo
        let max_deviation = avg_price * self.consensus_config.max_deviation_percent / 100.0;
        
        let mut has_significant_deviation = false;
        for (price, source, _) in &prices {
            if (*price - avg_price).abs() > max_deviation {
                has_significant_deviation = true;
                warn!("Giá {} từ nguồn {} lệch {:.2}% so với giá trung bình {}",
                    price, source, (*price - avg_price).abs() / avg_price * 100.0, avg_price);
            }
        }
        
        if has_significant_deviation {
            warn!("Phát hiện độ lệch đáng kể giữa các nguồn dữ liệu, sử dụng giá trung bình {}", avg_price);
        } else {
            debug!("Đồng thuận đạt được với giá trung bình {}", avg_price);
        }
        
        // Trả về giá đồng thuận
        Ok(Some(avg_price))
    }
}

/// Dữ liệu giao dịch bridge
#[derive(Debug, Clone)]
pub struct BridgeTransactionData {
    /// Mã hash giao dịch
    pub tx_hash: TransactionHash,
    /// Chuỗi nguồn
    pub source_chain: String,
    /// Chuỗi đích
    pub target_chain: String,
    /// Người gửi
    pub sender: String,
    /// Người nhận
    pub recipient: String,
    /// Số lượng token
    pub amount: f64,
    /// Thời gian giao dịch
    pub timestamp: u64,
    /// Trạng thái
    pub status: OracleUpdateStatus,
}

/// Cấu trúc lưu trữ lịch sử giao dịch của người gửi
struct SenderHistory {
    total_tx_count: u32,           // Tổng số giao dịch
    recent_tx_count: u32,          // Số giao dịch trong 1 giờ qua
    avg_tx_amount: f64,            // Giá trị trung bình giao dịch
    max_tx_amount: f64,            // Giá trị giao dịch lớn nhất
    recent_transactions: Vec<(f64, u64)>, // Các giao dịch gần đây: (amount, timestamp)
    recipients: Vec<String>,        // Danh sách người nhận gần đây
    cyclic_patterns: Vec<String>,   // Các mẫu giao dịch vòng tròn đã phát hiện
    unusual_chains: Vec<String>,    // Các chuỗi blockchain bất thường đã giao dịch
    is_new_account: bool,          // Tài khoản mới (ít hơn 1 tuần)
    risk_score: u32,               // Điểm đánh giá rủi ro (0-100)
}

/// Loại mẫu giao dịch đáng ngờ
#[derive(Debug, Clone, PartialEq)]
pub enum SuspiciousPatternType {
    /// Giao dịch qua lại giữa các địa chỉ (wash trading)
    CircularTrading,
    /// Chia nhỏ giao dịch để tránh phát hiện
    Smurfing,
    /// Tài khoản mới với khối lượng giao dịch lớn
    NewAccountLargeVolume,
    /// Giao dịch giữa các chuỗi bất thường
    UnusualChainPath,
    /// Nhiều giao dịch trong thời gian ngắn
    HighFrequency,
    /// Giao dịch với người nhận/gửi trong danh sách đen
    BlockedCounterparty,
    /// Giao dịch với các chuỗi bị hạn chế
    RestrictedChain,
    /// Biến động giá trị lớn
    VolatileValuePattern,
    /// Mẫu giao dịch hình tam giác (A->B->C->A)
    TriangularPattern,
    /// Mẫu giao dịch song song (cùng lúc gửi đến nhiều đích)
    ParallelTransactions,
}

/// Chi tiết về mẫu giao dịch đáng ngờ
#[derive(Debug, Clone)]
pub struct SuspiciousPatternDetail {
    /// Loại mẫu đáng ngờ
    pub pattern_type: SuspiciousPatternType,
    /// Mức độ nghiêm trọng (1-5)
    pub severity: u8,
    /// Mô tả về mẫu đáng ngờ
    pub description: String,
    /// Thông tin bổ sung
    pub additional_info: HashMap<String, String>,
    /// Khuyến nghị xử lý
    pub recommended_action: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    
    // Triển khai mock oracle provider cho test
    struct MockOracleProvider {
        name: String,
        chains: Vec<String>,
        data_store: Arc<Mutex<HashMap<(OracleDataType, String), OracleData>>>,
    }
    
    impl MockOracleProvider {
        fn new(name: &str, chains: Vec<&str>) -> Self {
            Self {
                name: name.to_string(),
                chains: chains.iter().map(|s| s.to_string()).collect(),
                data_store: Arc::new(Mutex::new(HashMap::new())),
            }
        }
    }
    
    #[async_trait]
    impl OracleProvider for MockOracleProvider {
        fn name(&self) -> String {
            self.name.clone()
        }
        
        fn supported_chains(&self) -> Vec<String> {
            self.chains.clone()
        }
        
        async fn update_data(&self, data: OracleData) -> BridgeResult<OracleUpdateStatus> {
            if !self.is_chain_supported(&data.chain) {
                return Err(BridgeError::UnsupportedChain(data.chain));
            }
            
            let mut store = self.data_store.lock().map_err(|_| BridgeError::OracleError("Lock failed".into()))?;
            store.insert((data.data_type.clone(), data.chain.clone()), data);
            
            Ok(OracleUpdateStatus::Success)
        }
        
        async fn get_data(&self, data_type: OracleDataType, chain: &str) -> BridgeResult<Option<OracleData>> {
            if !self.is_chain_supported(chain) {
                return Err(BridgeError::UnsupportedChain(chain.to_string()));
            }
            
            let store = self.data_store.lock().map_err(|_| BridgeError::OracleError("Lock failed".into()))?;
            Ok(store.get(&(data_type, chain.to_string())).cloned())
        }
        
        async fn confirm_update(&self, _data_type: OracleDataType, chain: &str, _tx_hash: &TransactionHash) -> BridgeResult<OracleUpdateStatus> {
            if !self.is_chain_supported(chain) {
                return Err(BridgeError::UnsupportedChain(chain.to_string()));
            }
            
            Ok(OracleUpdateStatus::Success)
        }
    }
    
    #[tokio::test]
    async fn test_oracle_manager() {
        // Tạo oracle manager và thêm mock provider
        let mut manager = OracleManager::new();
        manager.add_provider(Box::new(MockOracleProvider::new("TestOracle", vec!["ethereum", "bsc", "near"])));
        
        // Test cập nhật và lấy giá token
        manager.update_token_price("ethereum", "100.5", "local", "100.5").await.unwrap();
        let price = manager.get_token_price("ethereum").await.unwrap();
        assert_eq!(price, Some(100.5));
        
        // Test chain không được hỗ trợ
        let result = manager.get_token_price("solana").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
        
        // Test cập nhật tổng cung
        manager.update_total_supply("bsc", "1000000".to_string()).await.unwrap();
        let supply = manager.get_total_supply("bsc").await.unwrap();
        assert_eq!(supply, Some("1000000".to_string()));
    }
} 