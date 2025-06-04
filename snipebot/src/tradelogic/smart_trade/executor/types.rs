/// Kiểu dữ liệu nội bộ cho SmartTradeExecutor
///
/// Module này định nghĩa các struct, enum và type alias
/// được sử dụng bởi các module con trong executor.

use std::collections::HashMap;
use std::time::Duration;
use serde::{Serialize, Deserialize};
use anyhow::Result;
use tokio::task::JoinHandle;

use crate::types::{TokenPair, TradeParams, TradeType};
use crate::tradelogic::traits::OpportunityPriority;

/// Task handle để quản lý task chạy ngầm
pub struct TaskHandle {
    /// Handle để join hoặc abort task
    pub join_handle: JoinHandle<()>,
    
    /// Task ID
    pub task_id: String,
    
    /// Mô tả nhiệm vụ
    pub description: String,
}

/// Trạng thái của một giao dịch
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TradeStatus {
    /// Giao dịch mới khởi tạo
    Initialized,
    
    /// Đang phân tích token/giao dịch
    Analyzing,
    
    /// Giao dịch được phê duyệt, đang chờ thực thi
    Approved,
    
    /// Đang thực thi
    Executing,
    
    /// Đang theo dõi
    Monitoring,
    
    /// Hoàn thành thành công
    Completed,
    
    /// Thất bại
    Failed,
    
    /// Đã hủy
    Cancelled,
}

/// Chiến lược giao dịch
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TradeStrategy {
    /// Giao dịch đơn
    SingleTrade,
    
    /// Dollar cost averaging
    DCA,
    
    /// Giao dịch theo lưới
    Grid,
    
    /// Vào lệnh theo thời gian trung bình
    TWAP,
    
    /// Theo giá trung bình khối lượng
    VWAP,
    
    /// Chiến lược tùy chỉnh
    Custom(String),
}

/// Kết quả của một giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeResult {
    /// ID giao dịch
    pub trade_id: String,
    
    /// Tham số giao dịch
    pub params: TradeParams,
    
    /// Trạng thái giao dịch
    pub status: TradeStatus,
    
    /// Thời gian khởi tạo (Unix timestamp)
    pub created_at: u64,
    
    /// Thời gian hoàn thành (Unix timestamp)
    pub completed_at: Option<u64>,
    
    /// Hash giao dịch
    pub tx_hash: Option<String>,
    
    /// Số tiền thực tế giao dịch
    pub actual_amount: Option<f64>,
    
    /// Giá thực tế
    pub actual_price: Option<f64>,
    
    /// Phí giao dịch
    pub fee: Option<f64>,
    
    /// Lợi nhuận/lỗ
    pub profit_loss: Option<f64>,
    
    /// Lỗi (nếu thất bại)
    pub error: Option<String>,
    
    /// Blockchain explorer URL
    pub explorer_url: Option<String>,
}

/// Theo dõi giao dịch
#[derive(Debug, Clone)]
pub struct TradeTracker {
    /// ID giao dịch
    pub trade_id: String,
    
    /// Tham số giao dịch
    pub params: TradeParams,
    
    /// Trạng thái giao dịch
    pub status: TradeStatus,
    
    /// Thời gian khởi tạo (Unix timestamp)
    pub created_at: u64,
    
    /// Thời gian cập nhật gần nhất (Unix timestamp)
    pub updated_at: u64,
    
    /// Chiến lược giao dịch
    pub strategy: TradeStrategy,
    
    /// Các thông tin bổ sung
    pub metadata: HashMap<String, String>,
    
    /// Giá ban đầu
    pub initial_price: Option<f64>,
    
    /// Giá mục tiêu
    pub target_price: Option<f64>,
    
    /// Giá stop loss
    pub stop_loss: Option<f64>,
    
    /// Hash giao dịch (nếu đã thực thi)
    pub tx_hash: Option<String>,
    
    /// Số lần thử lại
    pub retry_count: u32,
    
    /// Các giao dịch con (cho chiến lược DCA, Grid)
    pub sub_trades: Vec<TradeResult>,
    
    /// Task handle để quản lý task background
    #[serde(skip)]
    pub task_handle: Option<TaskHandle>,
}

/// Loại cảnh báo
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AlertType {
    /// Cảnh báo giá
    Price,
    
    /// Cảnh báo khối lượng
    Volume,
    
    /// Cảnh báo rủi ro
    Risk,
    
    /// Cảnh báo pattern
    Pattern,
    
    /// Cảnh báo cơ hội
    Opportunity,
    
    /// Cảnh báo kỹ thuật
    Technical,
    
    /// Cảnh báo fundamentals
    Fundamental,
}

/// Cấu trúc cảnh báo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// ID cảnh báo
    pub alert_id: String,
    
    /// Loại cảnh báo
    pub alert_type: AlertType,
    
    /// Token pair
    pub token_pair: TokenPair,
    
    /// Chain ID
    pub chain_id: u32,
    
    /// Thông điệp
    pub message: String,
    
    /// Mức độ nghiêm trọng (0-100)
    pub severity: u8,
    
    /// Thời gian cảnh báo (Unix timestamp)
    pub timestamp: u64,
    
    /// Metadata
    pub metadata: HashMap<String, String>,
    
    /// Đã xem
    pub viewed: bool,
}

/// Cơ hội giao dịch phát hiện được
#[derive(Debug, Clone)]
pub struct TradeOpportunity {
    /// ID cơ hội
    pub opportunity_id: String,
    
    /// Token pair
    pub token_pair: TokenPair,
    
    /// Chain ID
    pub chain_id: u32,
    
    /// Loại giao dịch (mua/bán)
    pub trade_type: TradeType,
    
    /// Ưu tiên
    pub priority: OpportunityPriority,
    
    /// Điểm tin cậy (0-100)
    pub confidence: u8,
    
    /// Lợi nhuận dự kiến (%)
    pub expected_profit: f64,
    
    /// Khối lượng khuyến nghị
    pub recommended_amount: f64,
    
    /// Thời gian phát hiện (Unix timestamp)
    pub discovered_at: u64,
    
    /// Thời gian hết hạn (Unix timestamp)
    pub expires_at: Option<u64>,
    
    /// Đã xử lý
    pub processed: bool,
}

/// Bộ lọc cảnh báo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertFilter {
    /// Loại cảnh báo
    pub alert_types: Option<Vec<AlertType>>,
    
    /// Mức độ nghiêm trọng tối thiểu (0-100)
    pub min_severity: Option<u8>,
    
    /// Chỉ hiển thị chưa xem
    pub unviewed_only: bool,
    
    /// Giới hạn số lượng
    pub limit: Option<usize>,
}

/// Trạng thái chứng thực
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VerificationStatus {
    /// Chưa xác minh
    Unverified,
    
    /// Đã xác minh
    Verified,
    
    /// Xác minh một phần
    PartiallyVerified,
    
    /// Xác minh thất bại
    VerificationFailed,
}

/// Trạng thái của SmartTradeExecutor
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutorState {
    /// Đã dừng, không xử lý giao dịch
    Stopped,
    
    /// Đang dừng, đang hoàn thành các giao dịch hiện tại
    Stopping,
    
    /// Đang chạy, sẵn sàng xử lý giao dịch
    Running,
}

/// Loại giao dịch
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeType {
    /// Giao dịch mua
    Buy,
    
    /// Giao dịch bán
    Sell,
    
    /// Cung cấp thanh khoản
    AddLiquidity,
    
    /// Rút thanh khoản
    RemoveLiquidity,
    
    /// Stake token
    Stake,
    
    /// Unstake token
    Unstake,
    
    /// Claim rewards
    Claim,
}

/// Cấu hình cho position manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionConfig {
    /// Kích thước tối đa cho mỗi vị thế (% tổng tài sản)
    pub max_position_size: f64,
    
    /// Kích thước tối thiểu cho mỗi vị thế (theo USD)
    pub min_position_size_usd: f64,
    
    /// Số lượng vị thế tối đa cùng lúc
    pub max_concurrent_positions: usize,
    
    /// Map chain ID đến kích thước giao dịch tối thiểu
    pub min_trade_amount_map: HashMap<u32, f64>,
    
    /// Map chain ID đến kích thước giao dịch tối đa
    pub max_trade_amount_map: HashMap<u32, f64>,
}

impl Default for PositionConfig {
    fn default() -> Self {
        Self {
            max_position_size: 0.1, // 10% tổng tài sản
            min_position_size_usd: 100.0, // $100
            max_concurrent_positions: 10,
            min_trade_amount_map: HashMap::new(),
            max_trade_amount_map: HashMap::new(),
        }
    }
}

/// Cấu hình quản lý rủi ro
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    /// Ngưỡng rủi ro tối đa chấp nhận được (0-100)
    pub max_risk_threshold: u8,
    
    /// Tỷ lệ stop loss mặc định
    pub default_stop_loss: f64,
    
    /// Tỷ lệ take profit mặc định
    pub default_take_profit: f64,
    
    /// Tỷ lệ rủi ro / phần thưởng tối thiểu
    pub min_risk_reward_ratio: f64,
    
    /// Tỷ lệ phần trăm vốn tối đa có thể mất trong một ngày
    pub max_daily_drawdown: f64,
    
    /// Tỷ lệ risk-per-trade
    pub risk_per_trade: f64,
    
    /// Timeout cho các giao dịch đang chờ xử lý (giây)
    pub pending_tx_timeout: u64,
    
    /// Map chain ID đến ngưỡng rủi ro tùy chỉnh
    pub chain_risk_thresholds: HashMap<u32, u8>,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_risk_threshold: 70,
            default_stop_loss: 0.05, // 5%
            default_take_profit: 0.1, // 10%
            min_risk_reward_ratio: 1.5,
            max_daily_drawdown: 0.05, // 5%
            risk_per_trade: 0.01, // 1%
            pending_tx_timeout: 3600, // 1 giờ
            chain_risk_thresholds: HashMap::new(),
        }
    }
}

/// Cấu hình chiến lược giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// ID của chiến lược
    pub id: String,
    
    /// Tên của chiến lược
    pub name: String,
    
    /// Loại chiến lược
    pub strategy_type: StrategyType,
    
    /// Chain ID được áp dụng
    pub chain_id: u32,
    
    /// Token được giao dịch
    pub token_address: String,
    
    /// Tham số tùy chỉnh cho chiến lược
    pub params: HashMap<String, String>,
    
    /// Đã kích hoạt hay chưa
    pub enabled: bool,
}

/// Loại chiến lược giao dịch
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrategyType {
    /// Chiến lược grid
    Grid,
    
    /// Chiến lược DCA (Dollar Cost Averaging)
    DCA,
    
    /// Chiến lược TWAP (Time Weighted Average Price)
    TWAP,
    
    /// Chiến lược theo momentum
    Momentum,
    
    /// Chiến lược theo xu hướng
    Trend,
}

/// Cấu hình cho SmartTradeExecutor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorConfig {
    /// ID của executor
    pub id: String,
    
    /// Tên của executor
    pub name: String,
    
    /// Mô tả
    pub description: Option<String>,
    
    /// Cấu hình position
    pub position_config: PositionConfig,
    
    /// Cấu hình risk
    pub risk_config: RiskConfig,
    
    /// Các cấu hình chiến lược
    pub strategy_configs: Vec<StrategyConfig>,
    
    /// Các tham số tùy chỉnh
    pub custom_params: HashMap<String, String>,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: "SmartTradeExecutor".to_string(),
            description: Some("Executor cho giao dịch thông minh tự động".to_string()),
            position_config: PositionConfig::default(),
            risk_config: RiskConfig::default(),
            strategy_configs: Vec::new(),
            custom_params: HashMap::new(),
        }
    }
} 