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

/// Manager quản lý dữ liệu oracle cho bridge
pub struct OracleManager {
    /// Các provider oracle
    providers: Vec<Box<dyn OracleProvider>>,
    /// Cache token price
    price_cache: Arc<Mutex<HashMap<String, (f64, u64)>>>,
    /// Thời gian hết hạn cache giá (ms)
    price_cache_ttl_ms: u64,
    /// Cấu hình đồng thuận
    consensus_config: ConsensusConfig,
    /// Dữ liệu từ nhiều nguồn để đồng thuận
    multi_source_data: Arc<Mutex<HashMap<(OracleDataType, String), Vec<OracleData>>>>,
}

impl OracleManager {
    /// Tạo oracle manager mới
    pub fn new(price_cache_ttl_ms: u64) -> Self {
        Self {
            providers: Vec::new(),
            price_cache: Arc::new(Mutex::new(HashMap::new())),
            price_cache_ttl_ms,
            consensus_config: ConsensusConfig::default(),
            multi_source_data: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    /// Tạo oracle manager với cấu hình đồng thuận tùy chỉnh
    pub fn with_consensus_config(price_cache_ttl_ms: u64, consensus_config: ConsensusConfig) -> Self {
        Self {
            providers: Vec::new(),
            price_cache: Arc::new(Mutex::new(HashMap::new())),
            price_cache_ttl_ms,
            consensus_config,
            multi_source_data: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    /// Thêm provider oracle
    pub fn add_provider(&mut self, provider: Box<dyn OracleProvider>) {
        self.providers.push(provider);
    }
    
    /// Xác thực giá token là hợp lệ
    fn validate_token_price(&self, price: f64) -> BridgeResult<()> {
        // Kiểm tra giá trị không âm
        if price < 0.0 {
            return Err(BridgeError::OracleError("Giá token không thể âm".to_string()));
        }
        
        // Kiểm tra giá trị không quá lớn (để tránh lỗi tràn số)
        const MAX_PRICE: f64 = 1_000_000_000.0; // 1 tỷ USD
        if price > MAX_PRICE {
            return Err(BridgeError::OracleError(format!("Giá token quá lớn: {}", price)));
        }
        
        // Kiểm tra giá trị không phải NaN hoặc Infinity
        if !price.is_finite() {
            return Err(BridgeError::OracleError("Giá token không hợp lệ (NaN hoặc Infinity)".to_string()));
        }
        
        Ok(())
    }
    
    /// Cập nhật giá token DMD cho một chain
    pub async fn update_token_price(&self, chain: &str, price: f64) -> BridgeResult<()> {
        // Xác thực giá token
        self.validate_token_price(price)?;
        
        // Cập nhật cache local
        {
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_millis() as u64;
                
            let mut cache = self.price_cache.lock().map_err(|_| BridgeError::OracleError("Cache lock failed".into()))?;
            cache.insert(chain.to_string(), (price, now));
        }
        
        // Cập nhật dữ liệu đa nguồn
        {
            let mut multi_data = self.multi_source_data.lock().map_err(|_| BridgeError::OracleError("Multi-source data lock failed".into()))?;
            let key = (OracleDataType::TokenPrice, chain.to_string());
            
            let entry = multi_data.entry(key.clone()).or_insert_with(Vec::new);
            
            // Thêm dữ liệu mới từ nguồn hiện tại
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_millis() as u64;
                
            let new_data = OracleData {
                data_type: OracleDataType::TokenPrice,
                chain: chain.to_string(),
                data: price.to_string(),
                timestamp: now,
                status: OracleUpdateStatus::Pending,
                tx_hash: None,
                confirmations: 1,
                source: "local".to_string(),
            };
            
            entry.push(new_data);
            
            // Giữ tối đa 10 dữ liệu gần nhất
            if entry.len() > 10 {
                // Sắp xếp theo thời gian giảm dần và loại bỏ các mục cũ nhất
                entry.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
                entry.truncate(10);
            }
        }
        
        // Cập nhật lên tất cả các oracle provider
        for provider in &self.providers {
            if provider.is_chain_supported(chain) {
                let data = OracleData {
                    data_type: OracleDataType::TokenPrice,
                    chain: chain.to_string(),
                    data: price.to_string(),
                    timestamp: SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or(Duration::from_secs(0))
                        .as_millis() as u64,
                    status: OracleUpdateStatus::Pending,
                    tx_hash: None,
                    confirmations: 1,
                    source: provider.name(),
                };
                
                match provider.update_data(data).await {
                    Ok(_) => info!("Updated price for {} on provider {}", chain, provider.name()),
                    Err(e) => error!("Failed to update price for {} on provider {}: {:?}", chain, provider.name(), e),
                }
            }
        }
        
        Ok(())
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
        let mut prices: Vec<f64> = Vec::new();
        for data in &recent_points {
            match data.data.parse::<f64>() {
                Ok(price) => prices.push(price),
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
        
        // Tính giá trung bình
        let avg_price: f64 = prices.iter().sum::<f64>() / prices.len() as f64;
        
        // Kiểm tra độ lệch
        let max_deviation = avg_price * self.consensus_config.max_deviation_percent / 100.0;
        
        for price in &prices {
            if (*price - avg_price).abs() > max_deviation {
                warn!("Giá {} lệch quá nhiều so với giá trung bình {} (lệch > {}%)",
                    price, avg_price, self.consensus_config.max_deviation_percent);
                // Trong triển khai thực tế, có thể xử lý các giá trị lệch
                // Ví dụ: loại bỏ giá trị lệch và tính lại trung bình
            }
        }
        
        // Trả về giá đồng thuận
        Ok(Some(avg_price))
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
        let mut manager = OracleManager::new(60000);
        manager.add_provider(Box::new(MockOracleProvider::new("TestOracle", vec!["ethereum", "bsc", "near"])));
        
        // Test cập nhật và lấy giá token
        manager.update_token_price("ethereum", 100.5).await.unwrap();
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