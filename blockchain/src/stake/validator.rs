//! Module validator cho proof-of-stake
//!
//! Module này cung cấp các chức năng:
//! - Quản lý validator
//! - Xác thực giao dịch
//! - Phân phối rewards cho validator

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use async_trait::async_trait;
use ethers::types::{Address, U256};
use tracing::{info, warn, error, debug};
use std::collections::HashMap;
use chrono::{DateTime, Utc, Duration};
use uuid::Uuid;
use serde::{Serialize, Deserialize};

use crate::stake::{StakePoolConfig, UserStakeInfo};
use super::constants::*;

/// Loại cảnh báo validator
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidatorAlertType {
    /// Validator mất kết nối
    ConnectionLost,
    /// Validator không tham gia consensus
    MissedConsensus,
    /// Validator không ký blocks
    MissedSigning,
    /// Validator có hoạt động bất thường
    SuspiciousActivity,
    /// Validator bị phạt
    Penalized,
    /// Validator gần như bị cấm (penalty count cao)
    NearBan,
    /// Validator đã được khôi phục kết nối
    ConnectionRestored,
}

/// Thông tin cảnh báo validator
#[derive(Debug, Clone)]
pub struct ValidatorAlert {
    /// Loại cảnh báo
    pub alert_type: ValidatorAlertType,
    /// Địa chỉ validator
    pub validator_address: Address,
    /// Thời gian cảnh báo
    pub timestamp: DateTime<Utc>,
    /// Chi tiết cảnh báo
    pub details: String,
    /// Mức độ nghiêm trọng (1-5, 5 là nghiêm trọng nhất)
    pub severity: u8,
}

/// Kiểu của hàm thông báo cảnh báo
pub type AlertNotifierFn = fn(&ValidatorAlert) -> Result<(), String>;

/// Trạng thái hoạt động của validator
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidatorStatus {
    /// Đang hoạt động bình thường
    Active,
    /// Không hoạt động
    Inactive,
    /// Đang bị phạt
    Penalized,
    /// Đang bị cấm
    Banned,
    /// Đang chờ kích hoạt
    Pending,
}

/// Thông tin validator
#[derive(Debug, Clone)]
pub struct ValidatorInfo {
    /// Địa chỉ validator
    pub address: Address,
    /// Số lượng token đã stake
    pub staked_amount: U256,
    /// Thời gian bắt đầu làm validator
    pub start_time: u64,
    /// Số block đã xác thực
    pub blocks_validated: u64,
    /// Rewards đã nhận
    pub rewards_earned: U256,
    /// Trạng thái hoạt động
    pub is_active: bool,
    /// Trạng thái chi tiết
    pub status: ValidatorStatus,
    /// Thời gian kiểm tra kết nối cuối cùng
    pub last_connection_check: Option<DateTime<Utc>>,
    /// Thời gian tham gia consensus cuối cùng
    pub last_consensus_participation: Option<DateTime<Utc>>,
    /// Số lần bỏ lỡ consensus liên tiếp
    pub consecutive_missed_consensus: u32,
    /// Số lần mất kết nối liên tiếp
    pub consecutive_connection_failures: u32,
}

/// Snapshot của trạng thái validator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSnapshot {
    /// ID snapshot duy nhất
    pub id: String,
    /// Thời gian tạo snapshot
    pub created_at: DateTime<Utc>,
    /// Thông tin các validator tại thời điểm snapshot
    pub validators: Vec<ValidatorInfo>,
    /// Trạng thái giám sát của các validator
    pub monitoring_info: HashMap<String, ValidatorInfo>,
    /// Mô tả snapshot
    pub description: String,
    /// Phiên bản snapshot
    pub version: String,
    /// Hash kiểm tra tính toàn vẹn dữ liệu
    pub integrity_hash: String,
    /// Danh sách cảnh báo tại thời điểm snapshot
    pub alerts: Vec<ValidatorAlert>,
    /// Metadata bổ sung
    pub metadata: HashMap<String, String>,
    /// Dữ liệu hiệu suất validator (address -> performance data)
    pub performance_data: HashMap<String, ValidatorPerformance>,
}

/// Dữ liệu hiệu suất của validator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorPerformance {
    /// Địa chỉ validator
    pub address: String,
    /// Tỷ lệ uptime (0-100%)
    pub uptime_percentage: f64,
    /// Tỷ lệ tham gia consensus (0-100%)
    pub consensus_participation_rate: f64,
    /// Số block được xác thực
    pub blocks_validated: u64,
    /// Thời gian phản hồi trung bình (ms)
    pub avg_response_time_ms: u64,
    /// Số lỗi được ghi nhận
    pub error_count: u32,
    /// Thời gian cập nhật
    pub updated_at: DateTime<Utc>,
    /// Xu hướng hiệu suất (1: cải thiện, 0: ổn định, -1: giảm sút)
    pub performance_trend: i8,
    /// Số liệu theo dõi tài nguyên (CPU, RAM, Disk, Network)
    pub resource_metrics: HashMap<String, f64>,
}

impl ValidatorSnapshot {
    /// Tạo snapshot mới từ dữ liệu hiện tại
    pub fn new(validators: Vec<ValidatorInfo>, monitoring_info: HashMap<Address, ValidatorInfo>, description: String) -> Self {
        let snapshot = Self {
            id: Uuid::new_v4().to_string(),
            created_at: Utc::now(),
            validators,
            monitoring_info: monitoring_info.into_iter()
                .map(|(addr, info)| (format!("{:?}", addr), info))
                .collect(),
            description,
            version: env!("CARGO_PKG_VERSION").to_string(),
            integrity_hash: String::new(),
            alerts: Vec::new(),
            metadata: HashMap::new(),
            performance_data: HashMap::new(),
        };
        
        snapshot.generate_integrity_hash()
    }
    
    /// Tạo snapshot với đầy đủ thông tin
    pub fn new_full(
        validators: Vec<ValidatorInfo>, 
        monitoring_info: HashMap<Address, ValidatorInfo>, 
        description: String,
        alerts: Vec<ValidatorAlert>,
        metadata: HashMap<String, String>
    ) -> Self {
        let snapshot = Self {
            id: Uuid::new_v4().to_string(),
            created_at: Utc::now(),
            validators,
            monitoring_info: monitoring_info.into_iter()
                .map(|(addr, info)| (format!("{:?}", addr), info))
                .collect(),
            description,
            version: env!("CARGO_PKG_VERSION").to_string(),
            integrity_hash: String::new(),
            alerts,
            metadata,
            performance_data: HashMap::new(),
        };
        
        snapshot.generate_integrity_hash()
    }
    
    /// Chuyển đổi monitoring_info từ snapshot về HashMap<Address, ValidatorInfo>
    pub fn to_monitoring_info(&self) -> Result<HashMap<Address, ValidatorInfo>> {
        let mut result = HashMap::new();
        for (addr_str, info) in &self.monitoring_info {
            let addr = addr_str.parse::<Address>()
                .map_err(|e| anyhow::anyhow!("Không thể chuyển đổi địa chỉ từ snapshot: {}", e))?;
            result.insert(addr, info.clone());
        }
        Ok(result)
    }
    
    /// Tạo hash kiểm tra tính toàn vẹn dữ liệu
    pub fn generate_integrity_hash(mut self) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        // Tạm thời đặt integrity_hash về rỗng trước khi tính toán
        self.integrity_hash = String::new();
        
        let json = serde_json::to_string(&self).unwrap_or_default();
        
        let mut hasher = DefaultHasher::new();
        json.hash(&mut hasher);
        self.integrity_hash = format!("{:x}", hasher.finish());
        
        self
    }
    
    /// Kiểm tra tính toàn vẹn của snapshot
    pub fn verify_integrity(&self) -> bool {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let current_hash = self.integrity_hash.clone();
        
        // Tạo bản sao và tính toán lại hash
        let mut snapshot_copy = self.clone();
        snapshot_copy.integrity_hash = String::new();
        
        let json = serde_json::to_string(&snapshot_copy).unwrap_or_default();
        
        let mut hasher = DefaultHasher::new();
        json.hash(&mut hasher);
        let calculated_hash = format!("{:x}", hasher.finish());
        
        current_hash == calculated_hash
    }
    
    /// Thêm metadata cho snapshot
    pub fn add_metadata(&mut self, key: &str, value: &str) {
        self.metadata.insert(key.to_string(), value.to_string());
        // Cập nhật lại hash sau khi thay đổi
        *self = self.clone().generate_integrity_hash();
    }
    
    /// Thêm cảnh báo vào snapshot
    pub fn add_alert(&mut self, alert: ValidatorAlert) {
        self.alerts.push(alert);
        // Cập nhật lại hash sau khi thay đổi
        *self = self.clone().generate_integrity_hash();
    }
    
    /// Thêm dữ liệu hiệu suất cho validator
    pub fn add_performance_data(&mut self, address: Address, performance: ValidatorPerformance) {
        let addr_key = format!("{:?}", address);
        self.performance_data.insert(addr_key, performance);
        // Cập nhật lại hash sau khi thay đổi
        *self = self.clone().generate_integrity_hash();
    }
    
    /// Tính toán hiệu suất tổng thể của một validator
    pub fn calculate_overall_performance(&self, address: &Address) -> Option<f64> {
        let addr_key = format!("{:?}", address);
        
        if let Some(perf) = self.performance_data.get(&addr_key) {
            // Công thức: 40% uptime + 40% consensus + 20% response time (normalized and inverted)
            // Giá trị cao hơn = hiệu suất tốt hơn (0-100%)
            let response_time_factor = if perf.avg_response_time_ms > 0 {
                let normalized = (1000.0 - (perf.avg_response_time_ms as f64).min(1000.0)) / 10.0;
                normalized.max(0.0)
            } else {
                100.0 // Nếu không có dữ liệu thời gian phản hồi
            };
            
            let overall = perf.uptime_percentage * 0.4 + 
                         perf.consensus_participation_rate * 0.4 + 
                         response_time_factor * 0.2;
                         
            Some(overall)
        } else {
            None
        }
    }
    
    /// So sánh hai snapshot để tìm sự khác biệt
    pub fn diff(&self, other: &Self) -> HashMap<String, String> {
        let mut differences = HashMap::new();
        
        // So sánh số lượng validator
        if self.validators.len() != other.validators.len() {
            differences.insert(
                "validator_count".to_string(), 
                format!("{} -> {}", self.validators.len(), other.validators.len())
            );
        }
        
        // So sánh trạng thái validator
        for validator in &self.validators {
            if let Some(other_validator) = other.validators.iter().find(|v| v.address == validator.address) {
                if validator.status != other_validator.status {
                    differences.insert(
                        format!("validator_{:?}_status", validator.address),
                        format!("{:?} -> {:?}", validator.status, other_validator.status)
                    );
                }
                
                if validator.is_active != other_validator.is_active {
                    differences.insert(
                        format!("validator_{:?}_is_active", validator.address),
                        format!("{} -> {}", validator.is_active, other_validator.is_active)
                    );
                }
            } else {
                differences.insert(
                    format!("validator_{:?}", validator.address),
                    "Removed".to_string()
                );
            }
        }
        
        // Kiểm tra validator mới
        for validator in &other.validators {
            if !self.validators.iter().any(|v| v.address == validator.address) {
                differences.insert(
                    format!("validator_{:?}", validator.address),
                    "Added".to_string()
                );
            }
        }
        
        differences
    }
    
    /// So sánh hiệu suất validator giữa hai snapshot
    pub fn compare_performance(&self, other: &Self) -> HashMap<String, (f64, f64, f64)> {
        let mut performance_diff = HashMap::new();
        
        // Duyệt qua tất cả validator trong cả hai snapshot
        let mut all_validators = self.performance_data.keys().collect::<std::collections::HashSet<_>>();
        all_validators.extend(other.performance_data.keys());
        
        for addr in all_validators {
            let self_perf = match self.performance_data.get(addr) {
                Some(p) => (p.uptime_percentage, p.consensus_participation_rate, p.avg_response_time_ms as f64),
                None => (0.0, 0.0, 0.0), // Giá trị mặc định nếu không có trong snapshot này
            };
            
            let other_perf = match other.performance_data.get(addr) {
                Some(p) => (p.uptime_percentage, p.consensus_participation_rate, p.avg_response_time_ms as f64),
                None => (0.0, 0.0, 0.0), // Giá trị mặc định nếu không có trong snapshot kia
            };
            
            // Tính toán sự khác biệt (new - old)
            let diff = (
                other_perf.0 - self_perf.0, 
                other_perf.1 - self_perf.1,
                other_perf.2 - self_perf.2
            );
            
            performance_diff.insert(addr.clone(), diff);
        }
        
        performance_diff
    }
    
    /// Phát hiện validator có hiệu suất bất thường
    pub fn detect_abnormal_validators(&self, threshold_percentage: f64) -> Vec<(Address, f64, String)> {
        let mut abnormal_validators = Vec::new();
        
        for (addr_str, perf) in &self.performance_data {
            // Chuyển đổi địa chỉ từ chuỗi sang Address
            let addr = match addr_str.parse::<Address>() {
                Ok(a) => a,
                Err(_) => continue, // Bỏ qua nếu không thể parse
            };
            
            // Kiểm tra các tiêu chí bất thường
            if perf.uptime_percentage < threshold_percentage {
                abnormal_validators.push((
                    addr, 
                    perf.uptime_percentage,
                    format!("Uptime thấp: {}%", perf.uptime_percentage)
                ));
            }
            
            if perf.consensus_participation_rate < threshold_percentage {
                abnormal_validators.push((
                    addr,
                    perf.consensus_participation_rate,
                    format!("Tỷ lệ tham gia consensus thấp: {}%", perf.consensus_participation_rate)
                ));
            }
            
            if perf.avg_response_time_ms > 500 {
                let normalized = (perf.avg_response_time_ms as f64 / 10.0).min(100.0);
                abnormal_validators.push((
                    addr,
                    100.0 - normalized,
                    format!("Thời gian phản hồi cao: {}ms", perf.avg_response_time_ms)
                ));
            }
        }
        
        abnormal_validators
    }
    
    /// Xuất báo cáo hiệu suất của tất cả validator
    pub fn generate_performance_report(&self) -> String {
        let mut report = format!("BÁO CÁO HIỆU SUẤT VALIDATOR - {}\n", self.created_at.format("%Y-%m-%d %H:%M:%S"));
        report.push_str("=====================================\n\n");
        
        let mut validators = Vec::new();
        for (addr_str, perf) in &self.performance_data {
            // Tính điểm tổng thể dựa trên các tiêu chí
            let overall_score = perf.uptime_percentage * 0.4 + 
                               perf.consensus_participation_rate * 0.4 + 
                               (100.0 - (perf.avg_response_time_ms as f64 / 10.0).min(100.0)) * 0.2;
                               
            validators.push((addr_str, perf, overall_score));
        }
        
        // Sắp xếp theo điểm tổng thể giảm dần
        validators.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        
        for (i, (addr, perf, score)) in validators.iter().enumerate() {
            report.push_str(&format!("{}. Validator: {}\n", i + 1, addr));
            report.push_str(&format!("   Điểm tổng thể: {:.1}%\n", score));
            report.push_str(&format!("   Uptime: {:.1}%\n", perf.uptime_percentage));
            report.push_str(&format!("   Tỷ lệ tham gia consensus: {:.1}%\n", perf.consensus_participation_rate));
            report.push_str(&format!("   Thời gian phản hồi: {}ms\n", perf.avg_response_time_ms));
            report.push_str(&format!("   Blocks được xác thực: {}\n", perf.blocks_validated));
            report.push_str(&format!("   Số lỗi: {}\n", perf.error_count));
            report.push_str(&format!("   Xu hướng: {}\n\n", match perf.performance_trend {
                1 => "Đang cải thiện ⬆",
                0 => "Ổn định ➡",
                -1 => "Đang giảm sút ⬇",
                _ => "Không xác định"
            }));
        }
        
        report
    }
}

/// Trait cho validator
#[async_trait]
pub trait Validator: Send + Sync {
    /// Đăng ký làm validator
    async fn register_validator(&self, pool_address: Address, validator_address: Address) -> Result<()>;
    
    /// Hủy đăng ký validator
    async fn unregister_validator(&self, pool_address: Address, validator_address: Address) -> Result<()>;
    
    /// Xác thực giao dịch
    async fn validate_transaction(&self, pool_address: Address, transaction_hash: &str) -> Result<bool>;
    
    /// Lấy danh sách validator
    async fn get_validators(&self, pool_address: Address) -> Result<Vec<ValidatorInfo>>;
    
    /// Lấy thông tin validator
    async fn get_validator_info(&self, pool_address: Address, validator_address: Address) -> Result<ValidatorInfo>;
    
    /// Phân phối rewards cho validator
    async fn distribute_validator_rewards(&self, pool_address: Address) -> Result<()>;
    
    /// Kiểm tra trạng thái kết nối của validator
    async fn check_validator_connection(&self, validator_address: Address) -> Result<bool>;
    
    /// Kiểm tra sự tham gia consensus của validator
    async fn check_consensus_participation(&self, validator_address: Address) -> Result<bool>;
    
    /// Cập nhật trạng thái kết nối của validator
    async fn update_connection_status(&self, validator_address: Address, is_connected: bool) -> Result<()>;
    
    /// Cập nhật trạng thái tham gia consensus của validator
    async fn update_consensus_participation(&self, validator_address: Address, has_participated: bool) -> Result<()>;
    
    /// Thiết lập hàm thông báo cảnh báo
    fn set_alert_notifier(&mut self, notifier: AlertNotifierFn) -> Result<()>;
    
    /// Xóa hàm thông báo cảnh báo
    fn clear_alert_notifier(&mut self) -> Result<()>;
    
    /// Khởi động quá trình giám sát validator
    async fn start_monitoring(&self) -> Result<()>;
    
    /// Dừng quá trình giám sát validator
    async fn stop_monitoring(&self) -> Result<()>;
}

/// Implement Validator
pub struct ValidatorImpl {
    /// Cache cho validators
    validators: Arc<RwLock<Vec<ValidatorInfo>>>,
    /// Stake manager để tương tác với các stake pools
    stake_manager: Arc<dyn super::StakePoolManager>,
    /// Router để tương tác với các smart contracts
    router: Arc<dyn super::StakingRouter>,
    /// Thông tin giám sát validator
    monitoring_info: Arc<RwLock<HashMap<Address, ValidatorInfo>>>,
    /// Lịch sử cảnh báo
    alert_history: Arc<RwLock<Vec<ValidatorAlert>>>,
    /// Hàm thông báo cảnh báo
    alert_notifier: Option<AlertNotifierFn>,
    /// Đang giám sát không
    is_monitoring: Arc<RwLock<bool>>>,
    /// Tần suất kiểm tra (giây)
    monitoring_interval_seconds: u64,
    /// Ngưỡng cảnh báo mất kết nối liên tiếp
    connection_failure_threshold: u32,
    /// Ngưỡng cảnh báo bỏ lỡ consensus liên tiếp
    missed_consensus_threshold: u32,
    /// Danh sách các snapshot trạng thái
    snapshots: Arc<RwLock<Vec<ValidatorSnapshot>>>,
    /// Số lượng snapshot tối đa được lưu
    max_snapshots: usize,
    /// Tự động tạo snapshot trước khi cập nhật trạng thái
    auto_snapshot: bool,
    /// Handle để dừng task auto snapshot
    auto_snapshot_handle: Arc<RwLock<Option<tokio::task::AbortHandle>>>,
}

impl ValidatorImpl {
    /// Tạo validator mới
    pub fn new() -> Self {
        Self {
            validators: Arc::new(RwLock::new(Vec::new())),
            stake_manager: Arc::new(super::StakeManager::default()),
            router: Arc::new(super::routers::StakingRouterImpl::new()),
            monitoring_info: Arc::new(RwLock::new(HashMap::new())),
            alert_history: Arc::new(RwLock::new(Vec::new())),
            alert_notifier: None,
            is_monitoring: Arc::new(RwLock::new(false)),
            monitoring_interval_seconds: 60,
            connection_failure_threshold: 3,
            missed_consensus_threshold: 5,
            snapshots: Arc::new(RwLock::new(Vec::new())),
            max_snapshots: 10,
            auto_snapshot: true,
            auto_snapshot_handle: Arc::new(RwLock::new(None)),
        }
    }
    
    /// Tạo validator mặc định cho khởi tạo đệ quy
    fn default_validator() -> Self {
        Self {
            validators: Arc::new(RwLock::new(Vec::new())),
            stake_manager: Arc::new(super::StakeManager::default()),
            router: Arc::new(super::routers::StakingRouterImpl::new()),
            monitoring_info: Arc::new(RwLock::new(HashMap::new())),
            alert_history: Arc::new(RwLock::new(Vec::new())),
            alert_notifier: None,
            is_monitoring: Arc::new(RwLock::new(false)),
            monitoring_interval_seconds: 60,
            connection_failure_threshold: 3,
            missed_consensus_threshold: 5,
            snapshots: Arc::new(RwLock::new(Vec::new())),
            max_snapshots: 10,
            auto_snapshot: true,
            auto_snapshot_handle: Arc::new(RwLock::new(None)),
        }
    }
    
    /// Tạo validator mới với stake manager và router
    pub fn with_dependencies(
        stake_manager: Arc<dyn super::StakePoolManager>,
        router: Arc<dyn super::StakingRouter>,
    ) -> Self {
        Self {
            validators: Arc::new(RwLock::new(Vec::new())),
            stake_manager,
            router,
            monitoring_info: Arc::new(RwLock::new(HashMap::new())),
            alert_history: Arc::new(RwLock::new(Vec::new())),
            alert_notifier: None,
            is_monitoring: Arc::new(RwLock::new(false)),
            monitoring_interval_seconds: 60,
            connection_failure_threshold: 3,
            missed_consensus_threshold: 5,
            snapshots: Arc::new(RwLock::new(Vec::new())),
            max_snapshots: 10,
            auto_snapshot: true,
            auto_snapshot_handle: Arc::new(RwLock::new(None)),
        }
    }
    
    /// Thiết lập tần suất giám sát
    pub fn with_monitoring_interval(mut self, interval_seconds: u64) -> Self {
        self.monitoring_interval_seconds = interval_seconds;
        self
    }
    
    /// Thiết lập ngưỡng cảnh báo
    pub fn with_alert_thresholds(mut self, connection_threshold: u32, consensus_threshold: u32) -> Self {
        self.connection_failure_threshold = connection_threshold;
        self.missed_consensus_threshold = consensus_threshold;
        self
    }
    
    /// Thiết lập cấu hình cho cơ chế snapshot
    pub fn with_snapshot_config(mut self, max_snapshots: usize, auto_snapshot: bool) -> Self {
        self.max_snapshots = max_snapshots;
        self.auto_snapshot = auto_snapshot;
        self
    }
    
    /// Gửi cảnh báo về validator
    async fn send_alert(&self, validator_address: Address, alert_type: ValidatorAlertType, details: String, severity: u8) -> Result<()> {
        let alert = ValidatorAlert {
            alert_type,
            validator_address,
            timestamp: Utc::now(),
            details,
            severity,
        };
        
        // Lưu vào lịch sử cảnh báo
        {
            let mut history = self.alert_history.write().await;
            history.push(alert.clone());
        }
        
        // Gửi thông báo nếu có hàm notifier
        if let Some(notifier) = self.alert_notifier {
            if let Err(e) = notifier(&alert) {
                error!("Không thể gửi cảnh báo validator: {:?}", e);
            }
        }
        
        // Log cảnh báo
        match severity {
            1 => info!("Cảnh báo validator [{}]: {}", validator_address, details),
            2 => info!("Cảnh báo validator [{}]: {}", validator_address, details),
            3 => warn!("Cảnh báo validator [{}]: {}", validator_address, details),
            4 => error!("Cảnh báo nghiêm trọng validator [{}]: {}", validator_address, details),
            5 => error!("CẢNH BÁO KHẨN CẤP validator [{}]: {}", validator_address, details),
            _ => warn!("Cảnh báo không xác định validator [{}]: {}", validator_address, details),
        }
        
        Ok(())
    }
    
    /// Kiểm tra tất cả các validators
    async fn check_all_validators(&self) -> Result<()> {
        let validators = self.validators.read().await.clone();
        
        for validator in validators {
            // Kiểm tra kết nối
            let is_connected = self.check_validator_connection(validator.address).await?;
            self.update_connection_status(validator.address, is_connected).await?;
            
            // Kiểm tra sự tham gia consensus
            let has_participated = self.check_consensus_participation(validator.address).await?;
            self.update_consensus_participation(validator.address, has_participated).await?;
        }
        
        Ok(())
    }
    
    /// Bắt đầu vòng lặp giám sát
    async fn start_monitoring_loop(&self) -> Result<()> {
        let monitoring_interval = std::time::Duration::from_secs(self.monitoring_interval_seconds);
        let is_monitoring = self.is_monitoring.clone();
        let self_clone = self.clone();
        
        // Khởi chạy task giám sát trong background
        tokio::spawn(async move {
            info!("Bắt đầu giám sát validator với chu kỳ {} giây", self_clone.monitoring_interval_seconds);
            
            while *is_monitoring.read().await {
                if let Err(e) = self_clone.check_all_validators().await {
                    error!("Lỗi khi kiểm tra validator: {:?}", e);
                }
                
                tokio::time::sleep(monitoring_interval).await;
            }
            
            info!("Đã dừng giám sát validator");
        });
        
        Ok(())
    }
    
    /// Tạo snapshot của trạng thái hiện tại
    pub async fn create_snapshot(&self, description: &str) -> Result<String> {
        let validators = self.validators.read().await.clone();
        let monitoring_info = self.monitoring_info.read().await.clone();
        let alerts = self.alert_history.read().await.clone();
        
        let mut metadata = HashMap::new();
        metadata.insert("timestamp".to_string(), Utc::now().to_rfc3339());
        metadata.insert("validator_count".to_string(), validators.len().to_string());
        metadata.insert("alert_count".to_string(), alerts.len().to_string());
        metadata.insert("created_by".to_string(), "system".to_string());
        
        let snapshot = ValidatorSnapshot::new_full(
            validators,
            monitoring_info,
            description.to_string(),
            alerts,
            metadata
        );
        
        let snapshot_id = snapshot.id.clone();
        
        // Thêm snapshot vào danh sách và giữ giới hạn số lượng
        {
            let mut snapshots = self.snapshots.write().await;
            snapshots.push(snapshot);
            
            // Nếu vượt quá giới hạn, xóa snapshot cũ nhất
            if snapshots.len() > self.max_snapshots {
                snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                snapshots.truncate(self.max_snapshots);
            }
        }
        
        info!("Đã tạo snapshot trạng thái validator với ID: {}", snapshot_id);
        Ok(snapshot_id)
    }
    
    /// Lấy danh sách tất cả các snapshot
    pub async fn get_snapshots(&self) -> Result<Vec<ValidatorSnapshot>> {
        let snapshots = self.snapshots.read().await;
        Ok(snapshots.clone())
    }
    
    /// Lấy một snapshot theo ID
    pub async fn get_snapshot(&self, snapshot_id: &str) -> Result<Option<ValidatorSnapshot>> {
        let snapshots = self.snapshots.read().await;
        Ok(snapshots.iter()
            .find(|s| s.id == snapshot_id)
            .cloned())
    }
    
    /// Khôi phục trạng thái từ snapshot
    pub async fn restore_from_snapshot(&self, snapshot_id: &str) -> Result<()> {
        // Tìm snapshot theo ID
        let snapshot = {
            let snapshots = self.snapshots.read().await;
            let snapshot = snapshots.iter()
                .find(|s| s.id == snapshot_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Không tìm thấy snapshot với ID: {}", snapshot_id))?;
            snapshot
        };
        
        // Khôi phục trạng thái
        {
            let mut validators = self.validators.write().await;
            *validators = snapshot.validators.clone();
        }
        
        {
            let monitoring_info = snapshot.to_monitoring_info()?;
            let mut current_monitoring_info = self.monitoring_info.write().await;
            *current_monitoring_info = monitoring_info;
        }
        
        info!("Đã khôi phục trạng thái validator từ snapshot ID: {}", snapshot_id);
        
        // Tạo một cảnh báo về việc khôi phục
        let detail = format!("Trạng thái validator đã được khôi phục từ snapshot có ID: {}", snapshot_id);
        self.send_alert(
            Address::zero(), // Địa chỉ zero vì đây là thông báo hệ thống
            ValidatorAlertType::ConnectionRestored,
            detail,
            2 // Mức độ 2 - thông tin
        ).await?;
        
        Ok(())
    }
    
    /// Tạo snapshot tự động trước khi cập nhật trạng thái
    async fn auto_create_snapshot(&self, description: &str) -> Result<String> {
        if self.auto_snapshot {
            self.create_snapshot(description).await
        } else {
            // Nếu không bật auto_snapshot, trả về chuỗi rỗng
            Ok(String::new())
        }
    }
    
    /// Xóa một snapshot theo ID
    pub async fn delete_snapshot(&self, snapshot_id: &str) -> Result<bool> {
        let mut snapshots = self.snapshots.write().await;
        let initial_len = snapshots.len();
        
        snapshots.retain(|s| s.id != snapshot_id);
        
        let deleted = snapshots.len() < initial_len;
        if deleted {
            info!("Đã xóa snapshot ID: {}", snapshot_id);
        } else {
            warn!("Không tìm thấy snapshot ID: {} để xóa", snapshot_id);
        }
        
        Ok(deleted)
    }
    
    /// Xuất snapshot ra file JSON
    pub async fn export_snapshot(&self, snapshot_id: &str, file_path: &str) -> Result<()> {
        let snapshot = {
            let snapshots = self.snapshots.read().await;
            snapshots.iter()
                .find(|s| s.id == snapshot_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Không tìm thấy snapshot với ID: {}", snapshot_id))?
        };
        
        let json = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| anyhow::anyhow!("Không thể serialize snapshot: {}", e))?;
            
        std::fs::write(file_path, json)
            .map_err(|e| anyhow::anyhow!("Không thể ghi snapshot ra file: {}", e))?;
            
        info!("Đã xuất snapshot ID: {} ra file: {}", snapshot_id, file_path);
        
        Ok(())
    }
    
    /// Nhập snapshot từ file JSON
    pub async fn import_snapshot(&self, file_path: &str) -> Result<String> {
        let json = std::fs::read_to_string(file_path)
            .map_err(|e| anyhow::anyhow!("Không thể đọc file snapshot: {}", e))?;
            
        let snapshot: ValidatorSnapshot = serde_json::from_str(&json)
            .map_err(|e| anyhow::anyhow!("Không thể parse snapshot JSON: {}", e))?;
            
        let snapshot_id = snapshot.id.clone();
        
        // Thêm snapshot vào danh sách
        {
            let mut snapshots = self.snapshots.write().await;
            
            // Kiểm tra xem snapshot đã tồn tại chưa
            if snapshots.iter().any(|s| s.id == snapshot_id) {
                warn!("Snapshot với ID: {} đã tồn tại, sẽ thay thế", snapshot_id);
                snapshots.retain(|s| s.id != snapshot_id);
            }
            
            snapshots.push(snapshot);
        }
        
        info!("Đã nhập snapshot ID: {} từ file: {}", snapshot_id, file_path);
        
        Ok(snapshot_id)
    }
    
    /// So sánh hai snapshot để xem sự khác biệt
    pub async fn compare_snapshots(&self, snapshot_id1: &str, snapshot_id2: &str) -> Result<HashMap<String, String>> {
        let snapshots = self.snapshots.read().await;
        
        let snapshot1 = snapshots.iter()
            .find(|s| s.id == snapshot_id1)
            .ok_or_else(|| anyhow::anyhow!("Không tìm thấy snapshot với ID: {}", snapshot_id1))?;
            
        let snapshot2 = snapshots.iter()
            .find(|s| s.id == snapshot_id2)
            .ok_or_else(|| anyhow::anyhow!("Không tìm thấy snapshot với ID: {}", snapshot_id2))?;
            
        let differences = snapshot1.diff(snapshot2);
        
        if differences.is_empty() {
            info!("Hai snapshot {} và {} không có sự khác biệt", snapshot_id1, snapshot_id2);
        } else {
            info!("Phát hiện {} khác biệt giữa snapshot {} và {}", 
                  differences.len(), snapshot_id1, snapshot_id2);
        }
        
        Ok(differences)
    }
    
    /// Tạo snapshot tự động định kỳ
    pub async fn start_auto_snapshot(&self, interval_minutes: u64, retention_count: usize) -> Result<()> {
        let self_clone = self.clone();
        let interval = std::time::Duration::from_secs(interval_minutes * 60);
        
        // Tạo tokio task với AbortHandle
        let (abort_handle, abort_registration) = tokio::task::AbortHandle::new_pair();
        
        // Lưu abort_handle để có thể dừng task sau này
        {
            let mut handle_guard = self.auto_snapshot_handle.write().await;
            *handle_guard = Some(abort_handle);
        }
        
        let task = tokio::task::spawn(async move {
            info!("Bắt đầu tạo snapshot tự động mỗi {} phút", interval_minutes);
            
            loop {
                tokio::time::sleep(interval).await;
                
                let desc = format!("Auto snapshot at {}", Utc::now().to_rfc3339());
                match self_clone.create_snapshot(&desc).await {
                    Ok(id) => info!("Đã tạo snapshot tự động với ID: {}", id),
                    Err(e) => error!("Lỗi khi tạo snapshot tự động: {}", e),
                }
                
                // Giữ giới hạn số lượng snapshot
                let mut snapshots = self_clone.snapshots.write().await;
                if snapshots.len() > retention_count {
                    snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                    while snapshots.len() > retention_count {
                        if let Some(old) = snapshots.pop() {
                            info!("Đã xóa snapshot cũ: {}", old.id);
                        }
                    }
                }
            }
        }).with_abort_handle(abort_registration);
        
        // Không cần spawn task mới vì with_abort_handle đã làm việc đó
        
        Ok(())
    }
    
    /// Dừng tự động tạo snapshot
    pub async fn stop_auto_snapshot(&self) -> Result<()> {
        // Lấy handle từ trạng thái
        let handle_option = {
            let mut handle_guard = self.auto_snapshot_handle.write().await;
            handle_guard.take()
        };
        
        // Nếu có handle, dừng task
        if let Some(handle) = handle_option {
            info!("Đang dừng tự động tạo snapshot...");
            handle.abort();
            
            // Tạo snapshot cuối cùng
            let snapshot_desc = format!("Final snapshot after stopping auto-snapshot at {}", Utc::now().to_rfc3339());
            let snapshot_id = self.create_snapshot(&snapshot_desc).await?;
            
            // Thêm metadata
            if let Some(mut snapshot) = self.get_snapshot(&snapshot_id).await? {
                snapshot.add_metadata("stopped_by", "manual_request");
                snapshot.add_metadata("stop_time", &Utc::now().to_rfc3339());
                
                // Cập nhật snapshot đã sửa
                let mut snapshots = self.snapshots.write().await;
                if let Some(idx) = snapshots.iter().position(|s| s.id == snapshot_id) {
                    snapshots[idx] = snapshot;
                }
            }
            
            info!("Đã dừng tự động tạo snapshot, snapshot cuối: {}", snapshot_id);
            Ok(())
        } else {
            warn!("Không có task auto snapshot đang chạy để dừng");
            Ok(())
        }
    }
}

/// Cấu trúc cho lịch sử validator
#[derive(Debug, Clone)]
pub struct ValidatorHistory {
    /// Có bị cấm không
    pub is_banned: bool,
    /// Số lần bị phạt
    pub penalty_count: u32,
    /// Lý do bị phạt
    pub penalty_reasons: Vec<String>,
    /// Thời gian bị phạt cuối
    pub last_penalty_time: u64,
}

impl ValidatorImpl {
    /// Lấy lịch sử của validator
    pub async fn get_validator_history(&self, validator_address: Address) -> Result<Option<ValidatorHistory>> {
        // Kiểm tra trong cache local
        let validators = self.validators.read().await;
        if let Some(validator) = validators.iter().find(|v| v.address == validator_address) {
            // Nếu validator đã có trong hệ thống, kiểm tra lịch sử
            // TODO: Trong thực tế, cần truy vấn blockchain hoặc database
            
            // Mẫu dữ liệu testing
            if validator_address == Address::zero() {
                return Ok(Some(ValidatorHistory {
                    is_banned: true,
                    penalty_count: *MAX_ALLOWED_PENALTIES + 1,
                    penalty_reasons: vec!["Test banned validator".to_string()],
                    last_penalty_time: Self::get_current_time(),
                }));
            }
            
            // Trả về None nếu không tìm thấy lịch sử penalty
            return Ok(None);
        }
        
        // Nếu không có trong cache, truy vấn từ blockchain
        // TODO: Implement truy vấn từ blockchain
        Ok(None)
    }
    
    /// Lấy thời gian hiện tại (timestamp)
    fn get_current_time() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}

#[async_trait]
impl Validator for ValidatorImpl {
    async fn register_validator(&self, pool_address: Address, validator_address: Address) -> Result<()> {
        // Tạo snapshot trước khi đăng ký
        let snapshot_desc = format!("Trước khi đăng ký validator {}", validator_address);
        let snapshot_id = self.auto_create_snapshot(&snapshot_desc).await?;
        
        // Thực hiện đăng ký
        match self.router.register_validator(pool_address, validator_address).await {
            Ok(_) => {
                // Đăng ký thành công, cập nhật thông tin
                let validator_info = ValidatorInfo {
                    address: validator_address,
                    staked_amount: U256::zero(), // Sẽ cập nhật sau
                    start_time: chrono::Utc::now().timestamp() as u64,
                    blocks_validated: 0,
                    rewards_earned: U256::zero(),
                    is_active: true,
                    status: ValidatorStatus::Active,
                    last_connection_check: None,
                    last_consensus_participation: None,
                    consecutive_missed_consensus: 0,
                    consecutive_connection_failures: 0,
                };
                
                // Cập nhật danh sách validators
                {
                    let mut validators = self.validators.write().await;
                    validators.push(validator_info.clone());
                }
                
                // Cập nhật monitoring_info
                {
                    let mut monitoring_info = self.monitoring_info.write().await;
                    monitoring_info.insert(validator_address, validator_info);
                }
                
                info!("Đã đăng ký validator {} cho pool {}", validator_address, pool_address);
                Ok(())
            },
            Err(e) => {
                // Nếu đăng ký thất bại và có snapshot, khôi phục lại trạng thái
                if !snapshot_id.is_empty() {
                    warn!("Đăng ký validator thất bại, đang khôi phục từ snapshot: {}", snapshot_id);
                    if let Err(restore_err) = self.restore_from_snapshot(&snapshot_id).await {
                        error!("Không thể khôi phục từ snapshot {}: {}", snapshot_id, restore_err);
                    }
                }
                
                // Trả về lỗi ban đầu
                Err(e)
            }
        }
    }

    async fn unregister_validator(&self, pool_address: Address, validator_address: Address) -> Result<()> {
        // Tạo snapshot trước khi hủy đăng ký
        let snapshot_desc = format!("Trước khi hủy đăng ký validator {}", validator_address);
        let snapshot_id = self.auto_create_snapshot(&snapshot_desc).await?;
        
        // Thực hiện hủy đăng ký
        match self.router.unregister_validator(pool_address, validator_address).await {
            Ok(_) => {
                // Hủy đăng ký thành công, cập nhật thông tin
                {
                    let mut validators = self.validators.write().await;
                    validators.retain(|v| v.address != validator_address);
                }
                
                {
                    let mut monitoring_info = self.monitoring_info.write().await;
                    monitoring_info.remove(&validator_address);
                }
                
                info!("Đã hủy đăng ký validator {} khỏi pool {}", validator_address, pool_address);
                Ok(())
            },
            Err(e) => {
                // Nếu hủy đăng ký thất bại và có snapshot, khôi phục lại trạng thái
                if !snapshot_id.is_empty() {
                    warn!("Hủy đăng ký validator thất bại, đang khôi phục từ snapshot: {}", snapshot_id);
                    if let Err(restore_err) = self.restore_from_snapshot(&snapshot_id).await {
                        error!("Không thể khôi phục từ snapshot {}: {}", snapshot_id, restore_err);
                    }
                }
                
                // Trả về lỗi ban đầu
                Err(e)
            }
        }
    }

    async fn validate_transaction(&self, pool_address: Address, transaction_hash: &str) -> Result<bool> {
        // TODO: Implement validation logic
        // For now, return a simulated result
        Ok(true)
    }

    async fn get_validators(&self, pool_address: Address) -> Result<Vec<ValidatorInfo>> {
        let validators = self.validators.read().await;
        Ok(validators.clone())
    }

    async fn get_validator_info(&self, pool_address: Address, validator_address: Address) -> Result<ValidatorInfo> {
        let validators = self.validators.read().await;
        if let Some(validator) = validators.iter().find(|v| v.address == validator_address) {
            return Ok(validator.clone());
        }
        
        Err(anyhow::anyhow!("Validator not found: {:?}", validator_address))
    }

    async fn distribute_validator_rewards(&self, pool_address: Address) -> Result<()> {
        // TODO: Implement reward distribution logic
        Ok(())
    }
    
    async fn check_validator_connection(&self, validator_address: Address) -> Result<bool> {
        // TODO: Thực hiện kiểm tra kết nối thực tế đến validator
        // Đây là mã mẫu, trong thực tế cần thực hiện ping hoặc kiểm tra RPC
        
        // Giả lập kết quả kiểm tra: validator với địa chỉ kết thúc bằng 1 sẽ bị mất kết nối
        let address_str = format!("{:?}", validator_address);
        let is_connected = !address_str.ends_with('1');
        
        debug!("Kiểm tra kết nối validator {}: {}", validator_address, if is_connected { "đang kết nối" } else { "mất kết nối" });
        
        Ok(is_connected)
    }
    
    async fn check_consensus_participation(&self, validator_address: Address) -> Result<bool> {
        // TODO: Kiểm tra thực tế xem validator có tham gia consensus không
        // Có thể kiểm tra từ blockchain xem validator có ký block gần đây không
        
        // Giả lập kết quả: validator với địa chỉ kết thúc bằng 2 sẽ không tham gia consensus
        let address_str = format!("{:?}", validator_address);
        let has_participated = !address_str.ends_with('2');
        
        debug!("Kiểm tra tham gia consensus validator {}: {}", 
               validator_address, 
               if has_participated { "đã tham gia" } else { "bỏ lỡ" });
        
        Ok(has_participated)
    }
    
    async fn update_connection_status(&self, validator_address: Address, is_connected: bool) -> Result<()> {
        // Tạo snapshot trước khi cập nhật trạng thái
        let snapshot_desc = format!("Trước khi cập nhật trạng thái kết nối của validator {}", validator_address);
        let snapshot_id = self.auto_create_snapshot(&snapshot_desc).await?;
        
        // Tìm validator trong monitoring_info
        let mut updated_info = None;
        
        {
            let mut monitoring_info = self.monitoring_info.write().await;
            if let Some(info) = monitoring_info.get_mut(&validator_address) {
                // Cập nhật trạng thái kết nối
                if is_connected {
                    // Đặt lại số lần mất kết nối liên tiếp nếu kết nối thành công
                    info.consecutive_connection_failures = 0;
                    
                    // Cập nhật thời gian kiểm tra kết nối
                    info.last_connection_check = Some(Utc::now());
                    
                    // Phục hồi trạng thái active nếu validator đang bị penalized do mất kết nối
                    if info.status == ValidatorStatus::Penalized && info.consecutive_missed_consensus == 0 {
                        info.status = ValidatorStatus::Active;
                    }
                    
                    // Tạo cảnh báo phục hồi kết nối nếu trước đó có cảnh báo mất kết nối
                    if info.consecutive_connection_failures > 0 {
                        let alert = ValidatorAlert {
                            alert_type: ValidatorAlertType::ConnectionRestored,
                            validator_address,
                            timestamp: Utc::now(),
                            details: format!("Validator {} đã khôi phục kết nối", validator_address),
                            severity: 1, // Mức độ thấp vì đây là thông báo tốt
                        };
                        
                        let mut history = self.alert_history.write().await;
                        history.push(alert.clone());
                        
                        // Gửi thông báo nếu có
                        if let Some(notifier) = self.alert_notifier {
                            if let Err(e) = notifier(&alert) {
                                warn!("Không thể gửi thông báo phục hồi kết nối: {}", e);
                            }
                        }
                    }
                } else {
                    // Tăng số lần mất kết nối liên tiếp
                    info.consecutive_connection_failures += 1;
                    
                    // Cập nhật thời gian kiểm tra kết nối
                    info.last_connection_check = Some(Utc::now());
                    
                    // Kiểm tra nếu vượt ngưỡng cảnh báo
                    if info.consecutive_connection_failures >= self.connection_failure_threshold {
                        // Cập nhật trạng thái
                        info.is_active = false;
                        info.status = ValidatorStatus::Penalized;
                        
                        // Tạo cảnh báo
                        let alert = ValidatorAlert {
                            alert_type: ValidatorAlertType::ConnectionLost,
                            validator_address,
                            timestamp: Utc::now(),
                            details: format!(
                                "Validator {} mất kết nối {} lần liên tiếp, vượt ngưỡng {}",
                                validator_address, info.consecutive_connection_failures, self.connection_failure_threshold
                            ),
                            severity: 4, // Mức độ cao vì đây là vấn đề nghiêm trọng
                        };
                        
                        let mut history = self.alert_history.write().await;
                        history.push(alert.clone());
                        
                        // Gửi thông báo nếu có
                        if let Some(notifier) = self.alert_notifier {
                            if let Err(e) = notifier(&alert) {
                                error!("Không thể gửi cảnh báo mất kết nối: {}", e);
                            }
                        }
                    }
                }
                
                updated_info = Some(info.clone());
            } else {
                // Nếu không tìm thấy validator, tạo mới thông tin
                let validator_info = ValidatorInfo {
                    address: validator_address,
                    staked_amount: U256::zero(),
                    start_time: 0,
                    blocks_validated: 0,
                    rewards_earned: U256::zero(),
                    is_active: is_connected,
                    status: if is_connected { ValidatorStatus::Active } else { ValidatorStatus::Inactive },
                    last_connection_check: Some(Utc::now()),
                    last_consensus_participation: None,
                    consecutive_missed_consensus: 0,
                    consecutive_connection_failures: if is_connected { 0 } else { 1 },
                };
                
                monitoring_info.insert(validator_address, validator_info.clone());
                updated_info = Some(validator_info);
            }
        }
        
        // Cập nhật validators cache
        if let Some(updated) = updated_info {
            let mut validators = self.validators.write().await;
            
            // Tìm validator trong danh sách
            let validator_index = validators.iter().position(|v| v.address == validator_address);
            
            if let Some(index) = validator_index {
                // Cập nhật thông tin
                validators[index] = updated;
            } else {
                // Thêm mới nếu chưa có
                validators.push(updated);
            }
        }
        
        Ok(())
    }
    
    async fn update_consensus_participation(&self, validator_address: Address, has_participated: bool) -> Result<()> {
        // Tạo snapshot tự động trước khi cập nhật
        let snapshot_desc = format!("Trước khi cập nhật trạng thái consensus của validator {}", validator_address);
        let snapshot_id = self.auto_create_snapshot(&snapshot_desc).await?;
        
        // Tìm validator trong monitoring_info
        let mut updated_info = None;
        
        {
            let mut monitoring_info = self.monitoring_info.write().await;
            if let Some(info) = monitoring_info.get_mut(&validator_address) {
                // Cập nhật trạng thái tham gia consensus
                if has_participated {
                    // Đặt lại số lần bỏ lỡ consensus liên tiếp nếu tham gia thành công
                    info.consecutive_missed_consensus = 0;
                    
                    // Cập nhật thời gian tham gia consensus
                    info.last_consensus_participation = Some(Utc::now());
                    
                    // Phục hồi trạng thái active nếu validator đang bị penalized do bỏ lỡ consensus
                    if info.status == ValidatorStatus::Penalized && info.consecutive_connection_failures == 0 {
                        info.status = ValidatorStatus::Active;
                    }
                } else {
                    // Tăng số lần bỏ lỡ consensus liên tiếp
                    info.consecutive_missed_consensus += 1;
                    
                    // Cập nhật thời gian tham gia consensus
                    info.last_consensus_participation = Some(Utc::now());
                    
                    // Kiểm tra nếu vượt ngưỡng cảnh báo
                    if info.consecutive_missed_consensus >= self.missed_consensus_threshold {
                        // Cập nhật trạng thái
                        info.is_active = false;
                        info.status = ValidatorStatus::Penalized;
                        
                        // Tạo cảnh báo
                        let alert = ValidatorAlert {
                            alert_type: ValidatorAlertType::MissedConsensus,
                            validator_address,
                            timestamp: Utc::now(),
                            details: format!(
                                "Validator {} bỏ lỡ consensus {} lần liên tiếp, vượt ngưỡng {}",
                                validator_address, info.consecutive_missed_consensus, self.missed_consensus_threshold
                            ),
                            severity: 4, // Mức độ cao vì đây là vấn đề nghiêm trọng
                        };
                        
                        let mut history = self.alert_history.write().await;
                        history.push(alert.clone());
                        
                        // Gửi thông báo nếu có
                        if let Some(notifier) = self.alert_notifier {
                            if let Err(e) = notifier(&alert) {
                                error!("Không thể gửi cảnh báo bỏ lỡ consensus: {}", e);
                            }
                        }
                        
                        // Kiểm tra nếu cần tạo cảnh báo sắp bị cấm
                        if info.consecutive_missed_consensus >= self.missed_consensus_threshold * 2 {
                            let alert = ValidatorAlert {
                                alert_type: ValidatorAlertType::NearBan,
                                validator_address,
                                timestamp: Utc::now(),
                                details: format!(
                                    "Validator {} có nguy cơ bị cấm do bỏ lỡ consensus quá nhiều lần",
                                    validator_address
                                ),
                                severity: 5, // Mức độ rất cao
                            };
                            
                            let mut history = self.alert_history.write().await;
                            history.push(alert.clone());
                            
                            // Gửi thông báo nếu có
                            if let Some(notifier) = self.alert_notifier {
                                if let Err(e) = notifier(&alert) {
                                    error!("Không thể gửi cảnh báo sắp bị cấm: {}", e);
                                }
                            }
                            
                            // Cập nhật trạng thái thành Banned
                            info.status = ValidatorStatus::Banned;
                        }
                    }
                }
                
                updated_info = Some(info.clone());
            } else {
                // Nếu không tìm thấy validator, tạo mới thông tin
                let validator_info = ValidatorInfo {
                    address: validator_address,
                    staked_amount: U256::zero(),
                    start_time: 0,
                    blocks_validated: 0,
                    rewards_earned: U256::zero(),
                    is_active: has_participated,
                    status: if has_participated { ValidatorStatus::Active } else { ValidatorStatus::Inactive },
                    last_connection_check: None,
                    last_consensus_participation: Some(Utc::now()),
                    consecutive_missed_consensus: if has_participated { 0 } else { 1 },
                    consecutive_connection_failures: 0,
                };
                
                monitoring_info.insert(validator_address, validator_info.clone());
                updated_info = Some(validator_info);
            }
        }
        
        // Cập nhật validators cache
        if let Some(updated) = updated_info {
            let mut validators = self.validators.write().await;
            
            // Tìm validator trong danh sách
            let validator_index = validators.iter().position(|v| v.address == validator_address);
            
            if let Some(index) = validator_index {
                // Cập nhật thông tin
                validators[index] = updated;
            } else {
                // Thêm mới nếu chưa có
                validators.push(updated);
            }
        }
        
        Ok(())
    }
    
    fn set_alert_notifier(&mut self, notifier: AlertNotifierFn) -> Result<()> {
        self.alert_notifier = Some(notifier);
        info!("Đã thiết lập hàm thông báo cảnh báo validator");
        Ok(())
    }
    
    fn clear_alert_notifier(&mut self) -> Result<()> {
        self.alert_notifier = None;
        info!("Đã xóa hàm thông báo cảnh báo validator");
        Ok(())
    }
    
    async fn start_monitoring(&self) -> Result<()> {
        let mut is_monitoring = self.is_monitoring.write().await;
        if *is_monitoring {
            return Ok(());
        }
        
        *is_monitoring = true;
        drop(is_monitoring);
        
        self.start_monitoring_loop().await?;
        
        info!("Đã bắt đầu giám sát tự động cho validator");
        Ok(())
    }
    
    async fn stop_monitoring(&self) -> Result<()> {
        let mut is_monitoring = self.is_monitoring.write().await;
        *is_monitoring = false;
        
        info!("Đã dừng giám sát tự động cho validator");
        Ok(())
    }
}

// Thêm unit tests cho chức năng snapshot
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_create_and_restore_snapshot() {
        let validator = ValidatorImpl::new();
        
        // Thêm một số validator giả
        {
            let mut validators = validator.validators.write().await;
            validators.push(ValidatorInfo {
                address: Address::from_low_u64_be(1),
                staked_amount: U256::from(1000),
                start_time: 1000000,
                blocks_validated: 100,
                rewards_earned: U256::from(50),
                is_active: true,
                status: ValidatorStatus::Active,
                last_connection_check: Some(Utc::now()),
                last_consensus_participation: Some(Utc::now()),
                consecutive_missed_consensus: 0,
                consecutive_connection_failures: 0,
            });
        }
        
        {
            let mut monitoring_info = validator.monitoring_info.write().await;
            monitoring_info.insert(Address::from_low_u64_be(1), ValidatorInfo {
                address: Address::from_low_u64_be(1),
                staked_amount: U256::from(1000),
                start_time: 1000000,
                blocks_validated: 100,
                rewards_earned: U256::from(50),
                is_active: true,
                status: ValidatorStatus::Active,
                last_connection_check: Some(Utc::now()),
                last_consensus_participation: Some(Utc::now()),
                consecutive_missed_consensus: 0,
                consecutive_connection_failures: 0,
            });
        }
        
        // Tạo snapshot
        let snapshot_id = validator.create_snapshot("Test snapshot").await.unwrap();
        
        // Thay đổi trạng thái
        {
            let mut validators = validator.validators.write().await;
            validators[0].is_active = false;
            validators[0].status = ValidatorStatus::Inactive;
        }
        
        {
            let mut monitoring_info = validator.monitoring_info.write().await;
            if let Some(info) = monitoring_info.get_mut(&Address::from_low_u64_be(1)) {
                info.is_active = false;
                info.status = ValidatorStatus::Inactive;
            }
        }
        
        // Kiểm tra trạng thái đã thay đổi
        {
            let validators = validator.validators.read().await;
            assert_eq!(validators[0].is_active, false);
            assert_eq!(validators[0].status, ValidatorStatus::Inactive);
        }
        
        // Khôi phục từ snapshot
        validator.restore_from_snapshot(&snapshot_id).await.unwrap();
        
        // Kiểm tra trạng thái đã được khôi phục
        {
            let validators = validator.validators.read().await;
            assert_eq!(validators[0].is_active, true);
            assert_eq!(validators[0].status, ValidatorStatus::Active);
        }
        
        {
            let monitoring_info = validator.monitoring_info.read().await;
            let info = monitoring_info.get(&Address::from_low_u64_be(1)).unwrap();
            assert_eq!(info.is_active, true);
            assert_eq!(info.status, ValidatorStatus::Active);
        }
    }
    
    #[tokio::test]
    async fn test_snapshot_integrity() {
        let validator = ValidatorImpl::new();
        
        // Thêm một số validator giả
        {
            let mut validators = validator.validators.write().await;
            validators.push(ValidatorInfo {
                address: Address::from_low_u64_be(1),
                staked_amount: U256::from(1000),
                start_time: 1000000,
                blocks_validated: 100,
                rewards_earned: U256::from(50),
                is_active: true,
                status: ValidatorStatus::Active,
                last_connection_check: Some(Utc::now()),
                last_consensus_participation: Some(Utc::now()),
                consecutive_missed_consensus: 0,
                consecutive_connection_failures: 0,
            });
        }
        
        // Tạo snapshot
        let snapshot_id = validator.create_snapshot("Test integrity").await.unwrap();
        
        // Lấy snapshot vừa tạo
        let snapshot = validator.get_snapshot(&snapshot_id).await.unwrap().unwrap();
        
        // Kiểm tra tính toàn vẹn
        assert!(snapshot.verify_integrity(), "Tính toàn vẹn snapshot bị vi phạm");
    }
    
    #[tokio::test]
    async fn test_snapshot_comparison() {
        let validator = ValidatorImpl::new();
        
        // Thêm validator ban đầu
        {
            let mut validators = validator.validators.write().await;
            validators.push(ValidatorInfo {
                address: Address::from_low_u64_be(1),
                staked_amount: U256::from(1000),
                start_time: 1000000,
                blocks_validated: 100,
                rewards_earned: U256::from(50),
                is_active: true,
                status: ValidatorStatus::Active,
                last_connection_check: Some(Utc::now()),
                last_consensus_participation: Some(Utc::now()),
                consecutive_missed_consensus: 0,
                consecutive_connection_failures: 0,
            });
        }
        
        // Tạo snapshot ban đầu
        let snapshot_id1 = validator.create_snapshot("Snapshot 1").await.unwrap();
        
        // Thay đổi trạng thái
        {
            let mut validators = validator.validators.write().await;
            validators[0].is_active = false;
            validators[0].status = ValidatorStatus::Inactive;
            
            // Thêm validator mới
            validators.push(ValidatorInfo {
                address: Address::from_low_u64_be(2),
                staked_amount: U256::from(2000),
                start_time: 1000000,
                blocks_validated: 50,
                rewards_earned: U256::from(25),
                is_active: true,
                status: ValidatorStatus::Active,
                last_connection_check: Some(Utc::now()),
                last_consensus_participation: Some(Utc::now()),
                consecutive_missed_consensus: 0,
                consecutive_connection_failures: 0,
            });
        }
        
        // Tạo snapshot sau khi thay đổi
        let snapshot_id2 = validator.create_snapshot("Snapshot 2").await.unwrap();
        
        // So sánh hai snapshot
        let differences = validator.compare_snapshots(&snapshot_id1, &snapshot_id2).await.unwrap();
        
        assert!(differences.contains_key("validator_count"), "Không phát hiện sự thay đổi số lượng validator");
        assert!(differences.values().any(|v| v.contains("Added")), "Không phát hiện validator được thêm vào");
        assert!(differences.values().any(|v| v.contains("true -> false")), "Không phát hiện thay đổi trạng thái active");
        assert!(differences.values().any(|v| v.contains("Active -> Inactive")), "Không phát hiện thay đổi trạng thái validator");
    }
}
