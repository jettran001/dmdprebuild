//! Type definitions for Smart Trading System
//!
//! This module contains all structs, enums, and type aliases needed for the smart trading functionality.
//! These types are essential for the operation of the trade logic and are used throughout the module.

// External imports
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use tokio::sync::RwLock;

// Internal imports
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::MempoolAnalyzer;
use crate::analys::risk_analyzer::{
    RiskAnalyzer, 
    RiskFactor, 
    TradeRecommendation, 
    TradeRiskAnalysis
};
use crate::analys::token_status::{
    AdvancedTokenAnalysis, 
    ContractInfo, 
    LiquidityEvent, 
    LiquidityEventType, 
    TokenSafety, 
    TokenStatus
};

// Import from common module
use crate::tradelogic::common::types::{TokenIssue, TraderBehaviorAnalysis, GasBehavior, TradeStatus};

/// Loại chiến lược giao dịch
///
/// Định nghĩa các loại chiến lược giao dịch khác nhau mà SmartTradeExecutor có thể sử dụng.
/// Mỗi chiến lược có các tham số và hành vi riêng.
#[derive(Debug, Clone, PartialEq)]
pub enum TradeStrategy {
    /// Giao dịch thủ công: Chỉ mua bán theo lệnh trực tiếp của người dùng,
    /// không có chức năng tự động bán.
    Manual,

    /// Giao dịch nhanh: Mua khi phát hiện lệnh lớn trong mempool, bán nhanh khi 
    /// đạt lợi nhuận mục tiêu (mặc định 5%) hoặc sau thời gian ngắn (5 phút).
    /// Thích hợp cho các cơ hội thoáng qua, ít rủi ro.
    Quick,

    /// Giao dịch thông minh: Sử dụng Trailing Stop Loss (TSL), có thời gian giữ 
    /// lâu hơn và chiến lược thoát vị thế tinh vi hơn. Thích hợp cho các token
    /// có tiềm năng tăng trưởng dài hơn.
    Smart,
}

/// Kết quả của một giao dịch đã hoàn thành
/// 
/// Chứa thông tin về kết quả của một giao dịch, bao gồm giá mua/bán, lợi nhuận,
/// thời gian, và lý do kết thúc giao dịch. Dùng để lưu lịch sử và phân tích hiệu suất.
#[derive(Debug, Clone)]
pub struct TradeResult {
    /// ID giao dịch duy nhất
    pub trade_id: String,
    
    /// Giá mua vào (theo đơn vị base token: ETH/BNB)
    pub entry_price: f64,
    
    /// Giá bán ra (theo đơn vị base token: ETH/BNB), None nếu chưa bán
    pub exit_price: Option<f64>,
    
    /// Lợi nhuận theo phần trăm, None nếu chưa bán
    pub profit_percent: Option<f64>,
    
    /// Lợi nhuận quy đổi ra USD, None nếu chưa bán
    pub profit_usd: Option<f64>,
    
    /// Thời gian mua (Unix timestamp, giây)
    pub entry_time: u64,
    
    /// Thời gian bán (Unix timestamp, giây), None nếu chưa bán
    pub exit_time: Option<u64>,
    
    /// Trạng thái hiện tại của giao dịch
    pub status: TradeStatus,
    
    /// Lý do bán/hủy giao dịch, None nếu chưa kết thúc
    pub exit_reason: Option<String>,
    
    /// Tổng chi phí gas đã sử dụng (USD)
    pub gas_cost_usd: f64,
}

/// Thông tin theo dõi giao dịch đang diễn ra
/// 
/// Đại diện cho một giao dịch đang được theo dõi và quản lý bởi SmartTradeExecutor.
/// Chứa tất cả thông tin cần thiết để thực hiện các chiến lược tự động như 
/// trailing stop loss, take profit, và stop loss.
#[derive(Debug, Clone)]
pub struct TradeTracker {
    /// ID giao dịch duy nhất, được tạo khi giao dịch bắt đầu
    pub trade_id: String,
    
    /// Địa chỉ contract của token (0x...)
    pub token_address: String,
    
    /// Chain ID của blockchain (ví dụ: 1 = Ethereum, 56 = BSC)
    pub chain_id: u32,
    
    /// Chiến lược được áp dụng cho giao dịch này
    pub strategy: TradeStrategy,
    
    /// Giá mua vào ban đầu (theo đơn vị base token: ETH/BNB)
    pub entry_price: f64,
    
    /// Số lượng token đã mua (số token thực tế)
    pub token_amount: f64,
    
    /// Số lượng base token (ETH/BNB) đã đầu tư
    pub invested_amount: f64,
    
    /// Giá cao nhất token đã đạt được từ khi mua (cho trailing stop loss)
    pub highest_price: f64,
    
    /// Thời điểm mua (Unix timestamp, giây)
    pub entry_time: u64,
    
    /// Thời điểm tối đa được phép giữ token (Unix timestamp, giây)
    pub max_hold_time: u64,
    
    /// Ngưỡng lợi nhuận mục tiêu để bán (phần trăm, ví dụ: 5.0 = 5%)
    pub take_profit_percent: f64,
    
    /// Ngưỡng lỗ tối đa chấp nhận được (phần trăm, ví dụ: 5.0 = 5%)
    pub stop_loss_percent: f64,
    
    /// Phần trăm trailing stop loss nếu có (ví dụ: 2.0 = 2%)
    pub trailing_stop_percent: Option<f64>,
    
    /// Hash giao dịch mua trên blockchain
    pub buy_tx_hash: String,
    
    /// Hash giao dịch bán trên blockchain (None nếu chưa bán)
    pub sell_tx_hash: Option<String>,
    
    /// Trạng thái hiện tại của giao dịch
    pub status: TradeStatus,
    
    /// Lý do bán/hủy nếu giao dịch đã kết thúc (None nếu chưa kết thúc)
    pub exit_reason: Option<String>,
}

/// Cấu hình cho SmartTradeExecutor
///
/// Chứa các tham số cấu hình cho module smart trade, bao gồm:
/// - Các ngưỡng giao dịch (tối thiểu, tối đa)
/// - Tỉ lệ đầu tư
/// - Chiến lược mặc định
/// - Whitelist/blacklist token
/// - v.v.
#[derive(Debug, Clone)]
pub struct SmartTradeConfig {
    /// Có bật tính năng Smart Trade hay không (true = bật)
    pub enabled: bool,
    
    /// Cho phép tự động thực hiện giao dịch (true = tự động, false = chỉ phân tích)
    pub auto_trade: bool,
    
    /// Ngưỡng tối thiểu cho mỗi giao dịch (BNB/ETH), dưới mức này sẽ không giao dịch
    pub min_trade_amount: f64,
    
    /// Ngưỡng tối đa cho mỗi giao dịch (BNB/ETH), tránh rủi ro quá lớn
    pub max_trade_amount: f64,
    
    /// Tỉ lệ đầu tư trên vốn khả dụng (0.0-1.0), ví dụ: 0.1 = 10% vốn
    pub capital_per_trade_ratio: f64,
    
    /// Số lượng giao dịch tối đa được phép thực hiện cùng lúc
    pub max_concurrent_trades: u32,
    
    /// Danh sách token được phép giao dịch (whitelist), nếu trống = tất cả token
    pub token_whitelist: HashSet<String>,
    
    /// Danh sách token cấm giao dịch (blacklist), luôn được áp dụng
    pub token_blacklist: HashSet<String>,
    
    /// Chiến lược giao dịch mặc định sẽ áp dụng
    pub default_strategy: TradeStrategy,
    
    /// Điểm rủi ro tối đa cho phép (0-100), token có điểm rủi ro cao hơn sẽ bị bỏ qua
    pub max_risk_score: f64,
    
    /// Danh sách DEX được phép giao dịch (uniswap, pancakeswap, ...)
    pub allowed_dexes: HashSet<String>,
}

impl Default for SmartTradeConfig {
    fn default() -> Self {
        let mut allowed_dexes = HashSet::new();
        allowed_dexes.insert("uniswap".to_string());
        allowed_dexes.insert("pancakeswap".to_string());
        allowed_dexes.insert("sushiswap".to_string());
        
        Self {
            enabled: false, // Tắt mặc định, cần kích hoạt thủ công
            auto_trade: false, // Chỉ phân tích, không tự động giao dịch
            min_trade_amount: 0.01, // Tối thiểu 0.01 BNB/ETH
            max_trade_amount: 0.5, // Tối đa 0.5 BNB/ETH
            capital_per_trade_ratio: 0.05, // 5% vốn cho mỗi giao dịch
            max_concurrent_trades: 5, // Tối đa 5 giao dịch cùng lúc
            token_whitelist: HashSet::new(), // Không giới hạn token
            token_blacklist: HashSet::new(), // Không có token bị cấm
            default_strategy: TradeStrategy::Quick, // Mặc định dùng chiến lược nhanh
            max_risk_score: 40.0, // Điểm rủi ro tối đa 40/100
            allowed_dexes,
        }
    }
}

// Re-export types from common module that are used frequently here for convenience
pub use crate::tradelogic::common::types::TraderBehaviorType;
pub use crate::tradelogic::common::types::TraderExpertiseLevel;
