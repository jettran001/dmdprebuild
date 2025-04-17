//! Module Oracle cho DMD Token trên đa blockchain
//! 
//! Oracle này thực hiện vai trò trung tâm trong việc kiểm soát và đồng bộ hóa dữ liệu 
//! giữa các contract DMD trên các blockchain khác nhau, cung cấp các dịch vụ như:
//! - Theo dõi giá trị token trên các chain
//! - Xác thực giao dịch cross-chain
//! - Phát hiện bất thường và gian lận
//! - Cung cấp dữ liệu off-chain cho các smart contract
//! - Kiểm soát trung tâm cho tất cả các module: bridge, farm, stake, exchange

mod error;
mod types;
mod provider;
mod chainlink;
mod onchain;

// Re-exports
pub use error::{OracleError, OracleResult};
pub use types::{OracleData, OracleDataType, OracleReport, OracleReportStatus};
pub use provider::OracleDataSource;
pub use chainlink::ChainlinkDataSource;
pub use onchain::OnChainDataSource;

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};
use ethers::types::{Address, U256, H256};
use uuid::Uuid;
use tracing::{info, debug, warn, error};

// Internal imports 
use crate::smartcontracts::dmd_token::DmdChain;
use crate::bridge::types::BridgeConfig;
use crate::bridge::error::BridgeError;
use crate::stake::StakeManager;
use crate::farm::FarmManager;
use crate::exchange::PairManager;

/// Oracle xác minh và kiểm soát trung tâm cho các contract DMD
#[derive(Clone)]
pub struct DmdOracle {
    /// Cấu hình
    config: types::OracleConfig,
    /// Các nguồn dữ liệu
    data_sources: Arc<RwLock<Vec<Arc<dyn OracleDataSource>>>>,
    /// Cache dữ liệu
    data_cache: Arc<RwLock<HashMap<String, OracleData>>>,
    /// Cache báo cáo
    report_cache: Arc<RwLock<HashMap<String, OracleReport>>>,
    /// Bridge Manager (optional)
    bridge_manager: Option<Arc<RwLock<dyn BridgeManagerInterface + Send + Sync>>>,
    /// Stake Manager (optional)
    stake_manager: Option<Arc<RwLock<StakeManager>>>,
    /// Farm Manager (optional)
    farm_manager: Option<Arc<RwLock<FarmManager>>>,
    /// Exchange Manager (optional)
    exchange_manager: Option<Arc<RwLock<PairManager>>>,
}

/// Trait cho việc tương tác với Bridge Manager
#[async_trait::async_trait]
pub trait BridgeManagerInterface: Send + Sync + 'static {
    /// Lấy thông tin về số lượng token bị khóa trong bridge
    async fn get_locked_tokens(&self, chain: DmdChain) -> Result<U256, BridgeError>;
    
    /// Xác thực giao dịch cross-chain
    async fn verify_transaction(&self, tx_hash: &str, source_chain: DmdChain, target_chain: DmdChain) -> Result<bool, BridgeError>;
    
    /// Cập nhật phí bridge dựa trên dữ liệu từ oracle
    async fn update_bridge_fees(&self, fees: HashMap<DmdChain, U256>) -> Result<(), BridgeError>;
}

impl DmdOracle {
    /// Tạo Oracle mới
    pub fn new(config: types::OracleConfig) -> Self {
        Self {
            config,
            data_sources: Arc::new(RwLock::new(Vec::new())),
            data_cache: Arc::new(RwLock::new(HashMap::new())),
            report_cache: Arc::new(RwLock::new(HashMap::new())),
            bridge_manager: None,
            stake_manager: None,
            farm_manager: None,
            exchange_manager: None,
        }
    }
    
    /// Đăng ký Bridge Manager
    pub fn register_bridge_manager<T: BridgeManagerInterface + 'static>(&mut self, manager: Arc<RwLock<T>>) -> &mut Self {
        self.bridge_manager = Some(manager as Arc<RwLock<dyn BridgeManagerInterface + Send + Sync>>);
        self
    }
    
    /// Đăng ký Stake Manager
    pub fn register_stake_manager(&mut self, manager: Arc<RwLock<StakeManager>>) -> &mut Self {
        self.stake_manager = Some(manager);
        self
    }
    
    /// Đăng ký Farm Manager
    pub fn register_farm_manager(&mut self, manager: Arc<RwLock<FarmManager>>) -> &mut Self {
        self.farm_manager = Some(manager);
        self
    }
    
    /// Đăng ký Exchange Manager
    pub fn register_exchange_manager(&mut self, manager: Arc<RwLock<PairManager>>) -> &mut Self {
        self.exchange_manager = Some(manager);
        self
    }
    
    /// Thêm nguồn dữ liệu
    pub async fn add_data_source<T: OracleDataSource + 'static>(&self, source: T) -> OracleResult<()> {
        let mut data_sources = self.data_sources.write().await;
        data_sources.push(Arc::new(source));
        Ok(())
    }
    
    /// Tạo ID duy nhất cho dữ liệu
    fn generate_data_id(&self, data_type: &OracleDataType, chain: &DmdChain) -> String {
        format!("{}_{}_{}",
            Uuid::new_v4().to_string().split('-').next().unwrap(),
            format!("{:?}", data_type).to_lowercase(),
            format!("{:?}", chain).to_lowercase()
        )
    }
    
    /// Tạo ID duy nhất cho báo cáo
    fn generate_report_id(&self) -> String {
        Uuid::new_v4().to_string()
    }
    
    /// Lấy dữ liệu từ nguồn dữ liệu
    pub async fn fetch_data(&self, data_type: OracleDataType, chain: DmdChain) -> OracleResult<OracleData> {
        // Kiểm tra cache trước
        let cache_key = format!("{:?}_{:?}", data_type, chain);
        let data_cache = self.data_cache.read().await;
        
        if let Some(cached_data) = data_cache.get(&cache_key) {
            // Kiểm tra dữ liệu có còn hiệu lực không (dưới 5 phút)
            let now = Utc::now();
            if (now - cached_data.timestamp).num_seconds() < 300 {
                return Ok(cached_data.clone());
            }
        }
        drop(data_cache);
        
        // Không có trong cache hoặc đã hết hạn, lấy từ nguồn dữ liệu
        let data_sources = self.data_sources.read().await;
        for source in data_sources.iter() {
            if source.supports(&data_type, &chain) {
                match source.fetch_data(data_type.clone(), chain).await {
                    Ok(data) => {
                        // Cập nhật vào cache
                        let mut data_cache = self.data_cache.write().await;
                        data_cache.insert(cache_key, data.clone());
                        return Ok(data);
                    },
                    Err(e) => {
                        // Ghi log lỗi nhưng tiếp tục với nguồn dữ liệu khác
                        warn!("Không thể lấy dữ liệu từ nguồn {}: {}", source.name(), e);
                    }
                }
            }
        }
        
        Err(OracleError::DataNotFound(format!(
            "Không tìm thấy dữ liệu cho {:?} trên chain {:?}",
            data_type, chain
        )))
    }
    
    /// Tạo báo cáo từ nhiều dữ liệu
    pub async fn create_report(&self, data_types: Vec<OracleDataType>, chain: DmdChain, reporter: &str) -> OracleResult<OracleReport> {
        let mut data_list = Vec::new();
        
        // Lấy tất cả dữ liệu cần thiết
        for data_type in data_types {
            match self.fetch_data(data_type, chain).await {
                Ok(data) => data_list.push(data),
                Err(e) => return Err(e),
            }
        }
        
        // Tạo báo cáo
        let report = OracleReport {
            id: self.generate_report_id(),
            data: data_list,
            reporter: reporter.to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            status: OracleReportStatus::Pending,
        };
        
        // Lưu vào cache
        let mut report_cache = self.report_cache.write().await;
        report_cache.insert(report.id.clone(), report.clone());
        
        Ok(report)
    }
    
    /// Xác thực báo cáo
    pub async fn verify_report(&self, report_id: &str) -> OracleResult<OracleReportStatus> {
        let mut report_cache = self.report_cache.write().await;
        
        if let Some(report) = report_cache.get_mut(report_id) {
            // Thực hiện xác thực dữ liệu
            // Trong thực tế, sẽ kiểm tra chữ ký, nguồn dữ liệu, v.v.
            
            // Giả lập quá trình xác thực thành công
            report.status = OracleReportStatus::Confirmed;
            report.updated_at = Utc::now();
            
            Ok(report.status.clone())
        } else {
            Err(OracleError::DataNotFound(format!("Không tìm thấy báo cáo có ID: {}", report_id)))
        }
    }
    
    /// Lấy giá token trên một chain cụ thể
    pub async fn get_token_price(&self, chain: DmdChain) -> OracleResult<f64> {
        let data = self.fetch_data(OracleDataType::TokenPrice, chain).await?;
        
        // Chuyển đổi giá trị chuỗi thành f64
        data.value.parse::<f64>()
            .map_err(|e| OracleError::InvalidData(format!("Không thể chuyển đổi giá token: {}", e)))
    }
    
    /// Xác minh giao dịch cross-chain
    pub async fn verify_cross_chain_tx(&self, source_chain: DmdChain, target_chain: DmdChain, tx_hash: &str) -> OracleResult<bool> {
        // Thử sử dụng Bridge Manager trước nếu có
        if let Some(bridge_manager) = &self.bridge_manager {
            match bridge_manager.read().await.verify_transaction(tx_hash, source_chain, target_chain).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    warn!("Không thể xác minh giao dịch qua Bridge Manager: {}", e);
                    // Tiếp tục với cách xác minh thông qua Oracle
                }
            }
        }
        
        // Sử dụng Oracle Data Source nếu không có hoặc Bridge Manager thất bại
        let data_type = OracleDataType::CrossChainVerification;
        
        // Lấy thông tin xác minh từ chain nguồn
        let source_data = self.fetch_data(data_type.clone(), source_chain).await
            .map_err(|_| OracleError::DataNotFound(format!("Không tìm thấy giao dịch {} trên chain {:?}", tx_hash, source_chain)))?;
        
        // Lấy thông tin xác minh từ chain đích
        let target_data = self.fetch_data(data_type, target_chain).await
            .map_err(|_| OracleError::DataNotFound(format!("Không tìm thấy giao dịch {} trên chain {:?}", tx_hash, target_chain)))?;
        
        // Kiểm tra tính nhất quán của dữ liệu
        // Trong thực tế, sẽ có thuật toán phức tạp hơn để xác minh
        let result = source_data.value == target_data.value;
        
        Ok(result)
    }
    
    /// Kiểm tra tính nhất quán của token trên các chain
    pub async fn check_token_consistency(&self) -> OracleResult<HashMap<DmdChain, U256>> {
        let mut results = HashMap::new();
        let chains = vec![
            DmdChain::Ethereum,
            DmdChain::BinanceSmartChain,
            DmdChain::Polygon,
            DmdChain::Avalanche,
            DmdChain::Near,
        ];
        
        // Lấy tổng cung trên mỗi chain
        for chain in chains {
            match self.fetch_data(OracleDataType::TotalSupply, chain).await {
                Ok(data) => {
                    let total_supply = data.value.parse::<u128>()
                        .map_err(|e| OracleError::InvalidData(format!("Không thể chuyển đổi tổng cung: {}", e)))?;
                    
                    results.insert(chain, U256::from(total_supply));
                },
                Err(e) => {
                    warn!("Không thể lấy tổng cung trên chain {:?}: {}", chain, e);
                    // Không thêm chain này vào kết quả
                }
            }
        }
        
        Ok(results)
    }
    
    /// Phát hiện bất thường trong lưu lượng token
    pub async fn detect_anomalies(&self) -> OracleResult<Vec<OracleData>> {
        let mut anomalies = Vec::new();
        let chains = vec![
            DmdChain::Ethereum,
            DmdChain::BinanceSmartChain,
            DmdChain::Polygon,
            DmdChain::Avalanche,
            DmdChain::Near,
        ];
        
        for chain in chains {
            match self.fetch_data(OracleDataType::AnomalyAlert, chain).await {
                Ok(data) => {
                    // Nếu có dữ liệu bất thường, thêm vào danh sách
                    if data.value != "0" && data.value != "false" {
                        anomalies.push(data);
                    }
                },
                Err(_) => {
                    // Bỏ qua chain không có dữ liệu
                }
            }
        }
        
        Ok(anomalies)
    }
    
    /// Cập nhật phí bridge
    pub async fn update_bridge_fees(&self) -> OracleResult<()> {
        if let Some(bridge_manager) = &self.bridge_manager {
            // Lấy thông tin về phí gas trên tất cả các chain
            let chains = vec![
                DmdChain::Ethereum,
                DmdChain::BinanceSmartChain,
                DmdChain::Polygon,
                DmdChain::Avalanche,
                DmdChain::Near,
            ];
            
            let mut fees = HashMap::new();
            for chain in chains {
                match self.fetch_data(OracleDataType::GasFee, chain).await {
                    Ok(data) => {
                        if let Ok(fee) = data.value.parse::<u64>() {
                            fees.insert(chain, U256::from(fee));
                        }
                    },
                    Err(_) => continue,
                }
            }
            
            if !fees.is_empty() {
                match bridge_manager.write().await.update_bridge_fees(fees).await {
                    Ok(_) => {
                        info!("Cập nhật phí bridge thành công");
                        return Ok(());
                    },
                    Err(e) => {
                        return Err(OracleError::InternalError(format!("Không thể cập nhật phí bridge: {}", e)));
                    }
                }
            }
        }
        
        Err(OracleError::DataNotFound("Không có dữ liệu về phí hoặc không có Bridge Manager".to_string()))
    }
    
    /// Đồng bộ dữ liệu giữa các module
    pub async fn sync_all(&self) -> OracleResult<()> {
        info!("Bắt đầu đồng bộ hóa dữ liệu giữa các module...");
        
        // Cập nhật phí bridge
        if let Err(e) = self.update_bridge_fees().await {
            warn!("Không thể cập nhật phí bridge: {}", e);
        }
        
        // Kiểm tra tính nhất quán của token
        if let Err(e) = self.check_token_consistency().await {
            warn!("Không thể kiểm tra tính nhất quán của token: {}", e);
        }
        
        // Phát hiện bất thường
        match self.detect_anomalies().await {
            Ok(anomalies) if !anomalies.is_empty() => {
                warn!("Phát hiện {} bất thường trong hệ thống", anomalies.len());
                // TODO: Xử lý cảnh báo
            },
            Err(e) => {
                warn!("Không thể phát hiện bất thường: {}", e);
            },
            _ => {}
        }
        
        info!("Hoàn thành đồng bộ hóa dữ liệu giữa các module");
        Ok(())
    }
    
    /// Khởi động dịch vụ tự động cập nhật
    pub async fn start_auto_sync(&self, interval_seconds: u64) -> OracleResult<()> {
        let oracle = self.clone();
        
        // Khởi động một task chạy ngầm để tự động đồng bộ hóa
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(interval_seconds));
            loop {
                interval.tick().await;
                if let Err(e) = oracle.sync_all().await {
                    error!("Lỗi trong quá trình tự động đồng bộ hóa: {}", e);
                }
            }
        });
        
        info!("Đã khởi động dịch vụ tự động đồng bộ hóa dữ liệu (mỗi {} giây)", interval_seconds);
        Ok(())
    }
}

/// Tạo một instance Oracle mặc định với các cấu hình và nguồn dữ liệu cơ bản
pub async fn create_default_oracle() -> OracleResult<DmdOracle> {
    // Tạo cấu hình mặc định
    let config = types::OracleConfig::default();
    
    // Tạo Oracle
    let mut oracle = DmdOracle::new(config);
    
    // Thêm các nguồn dữ liệu mặc định
    oracle.add_data_source(chainlink::ChainlinkDataSource::new(
        "https://api.chainlink.com", 
        Some("your-api-key".to_string())
    )).await?;
    
    oracle.add_data_source(onchain::OnChainDataSource::new()).await?;
    
    Ok(oracle)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_oracle_data_sources() {
        // Tạo Oracle mặc định
        let oracle = create_default_oracle().await.unwrap();
        
        // Lấy giá token
        let price_result = oracle.get_token_price(DmdChain::Ethereum).await;
        assert!(price_result.is_ok());
        
        if let Ok(price) = price_result {
            assert!(price > 0.0);
        }
    }
} 