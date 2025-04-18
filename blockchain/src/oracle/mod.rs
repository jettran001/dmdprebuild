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

/// Cấu trúc lưu trữ chi tiết lỗi nguồn dữ liệu 
#[derive(Debug, Clone)]
pub struct SourceErrorLog {
    /// ID lỗi
    pub id: String,
    /// Thời gian xảy ra
    pub timestamp: DateTime<Utc>,
    /// Loại dữ liệu được yêu cầu
    pub data_type: OracleDataType,
    /// Blockchain liên quan
    pub chain: DmdChain,
    /// Tên nguồn dữ liệu
    pub source_name: String,
    /// Thông báo lỗi
    pub error_message: String,
    /// Mã lỗi (nếu có)
    pub error_code: Option<String>,
    /// Thời gian truy vấn (ms)
    pub query_time_ms: Option<u64>,
    /// Số lần retry
    pub retry_count: u32,
}

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
    /// Cache lưu trữ lịch sử lỗi từ các nguồn dữ liệu
    error_log_cache: Arc<RwLock<Vec<SourceErrorLog>>>,
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
            error_log_cache: Arc::new(RwLock::new(Vec::new())),
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
    
    /// Ghi log chi tiết lỗi nguồn dữ liệu
    async fn log_source_error(&self, data_type: OracleDataType, chain: DmdChain, source_name: String, error_message: String, retry_count: u32) {
        let error_log = SourceErrorLog {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            data_type,
            chain,
            source_name,
            error_message,
            error_code: None,
            query_time_ms: None,
            retry_count,
        };
        
        // Ghi log lỗi
        warn!(
            "Chi tiết lỗi [ID: {}] từ nguồn {}: data_type={:?}, chain={:?}, message={}",
            error_log.id, error_log.source_name, error_log.data_type, error_log.chain, error_log.error_message
        );
        
        // Lưu vào cache
        let mut error_logs = self.error_log_cache.write().await;
        error_logs.push(error_log);
        
        // Giới hạn kích thước cache để tránh quá tải bộ nhớ
        if error_logs.len() > 1000 {
            // Giữ lại 500 lỗi gần nhất
            *error_logs = error_logs.drain(error_logs.len() - 500..).collect();
        }
    }
    
    /// Lấy lịch sử lỗi từ các nguồn dữ liệu
    pub async fn get_source_error_logs(&self, limit: Option<usize>) -> Vec<SourceErrorLog> {
        let error_logs = self.error_log_cache.read().await;
        let limit = limit.unwrap_or(100).min(error_logs.len());
        error_logs.iter().rev().take(limit).cloned().collect()
    }
    
    /// Lấy lịch sử lỗi cho loại dữ liệu và chain cụ thể
    pub async fn get_source_error_logs_filtered(
        &self, 
        data_type: Option<OracleDataType>, 
        chain: Option<DmdChain>,
        source_name: Option<String>,
        limit: Option<usize>
    ) -> Vec<SourceErrorLog> {
        let error_logs = self.error_log_cache.read().await;
        
        let filtered_logs: Vec<SourceErrorLog> = error_logs.iter()
            .filter(|log| {
                let type_match = data_type.as_ref().map_or(true, |dt| &log.data_type == dt);
                let chain_match = chain.as_ref().map_or(true, |ch| &log.chain == ch);
                let source_match = source_name.as_ref().map_or(true, |sn| &log.source_name == sn);
                type_match && chain_match && source_match
            })
            .cloned()
            .collect();
        
        let limit = limit.unwrap_or(100).min(filtered_logs.len());
        filtered_logs.into_iter().rev().take(limit).collect()
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
        
        // Nếu không có nguồn dữ liệu
        if data_sources.is_empty() {
            return Err(OracleError::NoDataSources);
        }
        
        // Thu thập các lỗi từ tất cả các nguồn
        let mut error_details = HashMap::new();
        let mut attempted_sources = 0;
        let start_time = std::time::Instant::now();
        
        for source in data_sources.iter() {
            if source.supports(&data_type, &chain) {
                attempted_sources += 1;
                let source_name = source.name().to_string();
                
                // Theo dõi thời gian truy vấn
                let query_start = std::time::Instant::now();
                let retry_count = 0; // Ban đầu chưa retry
                
                match source.fetch_data(data_type.clone(), chain).await {
                    Ok(data) => {
                        // Đo thời gian truy vấn
                        let query_time = query_start.elapsed().as_millis() as u64;
                        debug!("Lấy dữ liệu từ nguồn {} thành công sau {}ms", source_name, query_time);
                        
                        // Cập nhật vào cache
                        let mut data_cache = self.data_cache.write().await;
                        data_cache.insert(cache_key, data.clone());
                        
                        // Nếu có nguồn dữ liệu khác trước đó đã thất bại, log chi tiết
                        if !error_details.is_empty() {
                            let previous_failures = error_details.len();
                            debug!(
                                "Đã lấy dữ liệu thành công từ nguồn {} sau khi {} nguồn khác thất bại", 
                                source_name, previous_failures
                            );
                            
                            // Lưu các lỗi vào log lỗi cho dù đã có dữ liệu thành công
                            for (src_name, error_msg) in &error_details {
                                self.log_source_error(
                                    data_type.clone(), 
                                    chain,
                                    src_name.clone(),
                                    error_msg.clone(),
                                    retry_count
                                ).await;
                            }
                        }
                        
                        return Ok(data);
                    },
                    Err(e) => {
                        // Đo thời gian truy vấn thất bại
                        let query_time = query_start.elapsed().as_millis() as u64;
                        
                        // Ghi log chi tiết lỗi từ nguồn cụ thể
                        let error_message = format!("Lỗi: {} (thời gian: {}ms)", e, query_time);
                        warn!(
                            "Không thể lấy dữ liệu [{:?} / {:?}] từ nguồn {}: {} (mất {}ms)", 
                            data_type, chain, source_name, error_message, query_time
                        );
                        
                        // Thêm vào danh sách lỗi chi tiết
                        error_details.insert(source_name.clone(), error_message.clone());
                        
                        // Ghi log lỗi ngay lập tức thay vì đợi đến cuối
                        self.log_source_error(
                            data_type.clone(), 
                            chain,
                            source_name,
                            error_message,
                            retry_count
                        ).await;
                    }
                }
            }
        }
        
        // Đo tổng thời gian thực hiện
        let total_time = start_time.elapsed().as_millis() as u64;
        
        // Nếu không có nguồn dữ liệu nào hỗ trợ loại dữ liệu này
        if attempted_sources == 0 {
            let error = OracleError::UnsupportedDataType(format!(
                "Không có nguồn dữ liệu nào hỗ trợ {:?} trên chain {:?}",
                data_type, chain
            ));
            
            error!("{}", error);
            return Err(error);
        }
        
        // Tạo thông báo lỗi chi tiết với thông tin từng nguồn
        let mut detailed_error = format!(
            "Không thể lấy dữ liệu {:?} cho chain {:?} từ {} nguồn (mất {}ms). Chi tiết lỗi:\n",
            data_type, chain, error_details.len(), total_time
        );
        
        for (source_name, error_msg) in &error_details {
            detailed_error.push_str(&format!("- Nguồn {}: {}\n", source_name, error_msg));
        }
        
        // Log lỗi tổng hợp
        error!("{}", detailed_error);
        
        // Trả về lỗi chi tiết
        Err(OracleError::MultiSourceFailure(detailed_error))
    }
    
    /// Tạo báo cáo từ nhiều dữ liệu
    pub async fn create_report(&self, data_types: Vec<OracleDataType>, chain: DmdChain, reporter: &str) -> OracleResult<OracleReport> {
        let mut data_list = Vec::new();
        let mut failed_data_types = Vec::new();
        let mut error_details = HashMap::new();
        
        // Lấy tất cả dữ liệu cần thiết
        for data_type in &data_types {
            match self.fetch_data(data_type.clone(), chain).await {
                Ok(data) => data_list.push(data),
                Err(e) => {
                    // Ghi log chi tiết lỗi
                    let error_message = format!("Không thể lấy dữ liệu [{:?}] từ chain [{:?}]: {}", data_type, chain, e);
                    warn!("{}", error_message);
                    
                    // Thu thập thông tin lỗi
                    failed_data_types.push(data_type.clone());
                    error_details.insert(format!("{:?}", data_type), error_message);
                }
            }
        }
        
        // Kiểm tra nếu không lấy được dữ liệu nào
        if data_list.is_empty() {
            let mut detailed_error = format!(
                "Không thể tạo báo cáo cho chain {:?}: Tất cả {} loại dữ liệu đều thất bại. Chi tiết lỗi:\n",
                chain, failed_data_types.len()
            );
            
            for (data_type, error) in &error_details {
                detailed_error.push_str(&format!("- {}: {}\n", data_type, error));
            }
            
            error!("{}", detailed_error);
            return Err(OracleError::MultiSourceFailure(detailed_error));
        }
        
        // Tạo báo cáo với dữ liệu thu thập được
        let mut report = OracleReport {
            id: self.generate_report_id(),
            data: data_list,
            reporter: reporter.to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            status: OracleReportStatus::Pending,
        };
        
        // Nếu có lỗi nhưng vẫn lấy được một số dữ liệu
        if !failed_data_types.is_empty() {
            let missing_data_count = failed_data_types.len();
            let total_data_count = data_types.len();
            let success_rate = (total_data_count - missing_data_count) as f64 / total_data_count as f64 * 100.0;
            
            // Đánh dấu báo cáo là không đầy đủ nếu thiếu quá nhiều dữ liệu
            if success_rate < 70.0 {
                report.status = OracleReportStatus::Incomplete;
                
                // Log cảnh báo với chi tiết các dữ liệu bị thiếu
                let warning_message = format!(
                    "Báo cáo ID {} cho chain {:?} không đầy đủ (chỉ có {:.1}% dữ liệu). Các loại dữ liệu bị thiếu: {:?}",
                    report.id, chain, success_rate, failed_data_types
                );
                warn!("{}", warning_message);
                
                // Thêm chi tiết lỗi vào báo cáo
                report.add_error_details(failed_data_types, error_details);
            } else {
                // Báo cáo vẫn chấp nhận được nhưng thiếu một số dữ liệu
                info!(
                    "Báo cáo ID {} cho chain {:?} tạo thành công nhưng thiếu {}/{} loại dữ liệu ({:.1}% đầy đủ)",
                    report.id, chain, missing_data_count, total_data_count, success_rate
                );
            }
        } else {
            // Báo cáo đầy đủ
            info!("Tạo báo cáo ID {} cho chain {:?} thành công với tất cả {} loại dữ liệu", report.id, chain, data_types.len());
        }
        
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
        
        // Phân tích lỗi nguồn dữ liệu
        self.analyze_error_logs().await;
        
        info!("Hoàn thành đồng bộ hóa dữ liệu giữa các module");
        Ok(())
    }
    
    /// Phân tích lỗi từ các nguồn dữ liệu
    async fn analyze_error_logs(&self) {
        // Lấy lỗi trong 24 giờ qua
        let now = Utc::now();
        let one_day_ago = now - chrono::Duration::days(1);
        
        let error_logs = self.error_log_cache.read().await;
        let recent_errors: Vec<&SourceErrorLog> = error_logs.iter()
            .filter(|log| log.timestamp > one_day_ago)
            .collect();
        
        if recent_errors.is_empty() {
            return;
        }
        
        // Thống kê lỗi theo nguồn dữ liệu
        let mut source_error_counts: HashMap<String, u32> = HashMap::new();
        let mut chain_error_counts: HashMap<DmdChain, u32> = HashMap::new();
        let mut data_type_error_counts: HashMap<OracleDataType, u32> = HashMap::new();
        
        for log in &recent_errors {
            *source_error_counts.entry(log.source_name.clone()).or_insert(0) += 1;
            *chain_error_counts.entry(log.chain.clone()).or_insert(0) += 1;
            *data_type_error_counts.entry(log.data_type.clone()).or_insert(0) += 1;
        }
        
        // Log thông tin thống kê
        info!("Phân tích lỗi nguồn dữ liệu trong 24 giờ qua");
        info!("Tổng số lỗi: {}", recent_errors.len());
        
        // Xuất thông tin về nguồn dữ liệu có nhiều lỗi nhất
        if !source_error_counts.is_empty() {
            let mut source_errors: Vec<(String, u32)> = source_error_counts.into_iter().collect();
            source_errors.sort_by(|a, b| b.1.cmp(&a.1));
            
            info!("Top 3 nguồn dữ liệu có nhiều lỗi nhất:");
            for (idx, (source, count)) in source_errors.iter().take(3).enumerate() {
                info!("  {}. {}: {} lỗi", idx + 1, source, count);
            }
        }
        
        // Xuất thông tin về chain có nhiều lỗi nhất
        if !chain_error_counts.is_empty() {
            let mut chain_errors: Vec<(DmdChain, u32)> = chain_error_counts.into_iter().collect();
            chain_errors.sort_by(|a, b| b.1.cmp(&a.1));
            
            info!("Top 3 blockchain có nhiều lỗi nhất:");
            for (idx, (chain, count)) in chain_errors.iter().take(3).enumerate() {
                info!("  {}. {:?}: {} lỗi", idx + 1, chain, count);
            }
        }
        
        // Cảnh báo nếu có nguồn dữ liệu bị lỗi nhiều
        for (source, count) in source_errors.iter().take(3) {
            let error_rate = (*count as f64) / (recent_errors.len() as f64) * 100.0;
            if error_rate > 80.0 {
                warn!("CẢNH BÁO: Nguồn dữ liệu {} có tỷ lệ lỗi cao ({:.2}%)", source, error_rate);
            }
        }
    }
    
    /// Xuất log lỗi ra file
    pub async fn export_error_logs(&self, path: &str) -> OracleResult<()> {
        use std::fs::File;
        use std::io::Write;
        
        let error_logs = self.error_log_cache.read().await;
        if error_logs.is_empty() {
            return Ok(());
        }
        
        // Tạo nội dung cho file
        let mut content = String::from("# Oracle Source Error Logs\n");
        content.push_str(&format!("## Exported at: {}\n\n", Utc::now()));
        content.push_str("| ID | Timestamp | Source | Data Type | Chain | Error Message | Query Time |\n");
        content.push_str("|:---|:----------|:-------|:----------|:------|:--------------|:----------|\n");
        
        for log in error_logs.iter() {
            let query_time = log.query_time_ms.map_or("N/A".to_string(), |t| format!("{}ms", t));
            content.push_str(&format!(
                "| {} | {} | {} | {:?} | {:?} | {} | {} |\n",
                log.id,
                log.timestamp,
                log.source_name,
                log.data_type,
                log.chain,
                log.error_message.replace("|", "\\|"), // Escape pipe characters
                query_time
            ));
        }
        
        // Ghi ra file
        match File::create(path) {
            Ok(mut file) => {
                match file.write_all(content.as_bytes()) {
                    Ok(_) => {
                        info!("Đã xuất {} log lỗi ra file {}", error_logs.len(), path);
                        Ok(())
                    },
                    Err(e) => Err(OracleError::InternalError(format!("Không thể ghi file: {}", e)))
                }
            },
            Err(e) => Err(OracleError::InternalError(format!("Không thể tạo file: {}", e)))
        }
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