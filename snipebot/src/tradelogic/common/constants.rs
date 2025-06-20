//! Constants for Trading Strategies
//! 
//! This module contains all thresholds, timeouts, and configuration constants 
//! used in the trading system. Organizing these values as constants allows
//! for easy modification and configuration of the trading behavior.
//!
//! Includes constants for:
//! - MEV strategies
//! - Smart Trading strategies
//! - General trading settings
//! - Token analysis thresholds
//! - Security parameters

// External imports
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};

// ========================================================================
// ========================= MEV CONSTANTS ================================
// ========================================================================

/// Khoảng cách giá tối thiểu để tận dụng cơ hội arbitrage (%)
pub const MIN_PRICE_DIFFERENCE_PERCENT: f64 = 0.5;

/// Lợi nhuận tối thiểu để thực thi cơ hội MEV (USD)
pub const MIN_PROFIT_THRESHOLD_USD: f64 = 20.0;

/// Ngưỡng giá trị giao dịch nhỏ nhất để theo dõi (ETH)
pub const MIN_TX_VALUE_ETH: f64 = 1.0;

/// Ngưỡng giá trị giao dịch lớn (ETH)
pub const HIGH_VALUE_TX_ETH: f64 = 10.0;

/// Hệ số tăng gas để chạy trước giao dịch (%)
pub const FRONT_RUN_GAS_BOOST: f64 = 1.2;

/// Gas price tối đa chấp nhận được (Gwei)
pub const MAX_GAS_PRICE_GWEI: f64 = 1000.0;

/// Khoảng thời gian quét MEV (mili giây)
pub const MEV_SCAN_INTERVAL_MS: u64 = 1000;

/// Thời gian tối đa cho một cơ hội MEV (mili giây)
pub const MAX_OPPORTUNITY_AGE_MS: u64 = 5000; 

/// Tỷ lệ lợi nhuận chia cho validator/miners (%)
pub const PROFIT_SHARE_PERCENT: f64 = 70.0;

/// Kích thước tối đa của bundle MEV
pub const MAX_BUNDLE_SIZE: usize = 3;

/// Số lượng giao dịch tối thiểu cần theo dõi cho sandwich
pub const MIN_TRANSACTIONS_FOR_SANDWICH: usize = 3;

/// Thời gian chờ tối đa cho giao dịch mempool (mili giây)
pub const MAX_MEMPOOL_WAIT_TIME_MS: u64 = 3000;

/// Số lượng DEX tối đa cần kiểm tra cho cơ hội arbitrage
pub const MAX_DEX_CHECK_COUNT: usize = 5;

/// Gas tối đa cho giao dịch arbitrage
pub const ARBITRAGE_GAS_LIMIT: u64 = 500000;

/// Gas tối đa cho giao dịch sandwich
pub const SANDWICH_GAS_LIMIT: u64 = 350000;

/// Thời gian chờ tối đa cho bundle được xác nhận (mili giây)
pub const BUNDLE_CONFIRMATION_TIMEOUT_MS: u64 = 15000; 

// ========================================================================
// ===================== SMART TRADE CONSTANTS ============================
// ========================================================================

/// Cấu trúc chứa các giá trị thời gian thực có thể điều chỉnh trong runtime
/// Các giá trị mặc định được định nghĩa nhưng có thể được thay đổi thông qua cấu hình
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigurableConstants {
    // ===== Quick Trade Strategy Constants =====
    /// Ngưỡng giao dịch tối thiểu (BNB) để kích hoạt quick trade
    pub quick_trade_min_bnb: f64,

    /// Phần trăm lợi nhuận mục tiêu (5%) cho quick trade
    pub quick_trade_target_profit: f64,

    /// Thời gian giữ tối đa cho quick trade (giây)
    pub quick_trade_max_hold_time: u64,

    // ===== Smart Trade Strategy Constants =====
    /// Thời gian giữ tối thiểu cho smart trade (giây)
    pub smart_trade_min_hold_time: u64,

    /// Thời gian giữ tối đa cho smart trade (giây) 
    pub smart_trade_max_hold_time: u64,

    /// Phần trăm trailing stop loss cho smart trade
    pub smart_trade_tsl_percent: f64,

    // ===== General Trading Constants =====
    /// Phần trăm ngưỡng ngăn lỗ áp dụng cho mọi loại giao dịch
    pub stop_loss_percent: f64,

    /// Số tiền tối đa (BNB) cho mỗi giao dịch để giới hạn rủi ro
    pub max_trade_amount_bnb: f64,

    /// Khoảng thời gian kiểm tra giá (milliseconds)
    pub price_check_interval_ms: u64,

    // ===== Token Analysis Constants =====
    /// Ngưỡng tax mua tối đa an toàn
    pub max_safe_buy_tax: f64,

    /// Ngưỡng tax bán tối đa an toàn
    pub max_safe_sell_tax: f64,

    /// Chênh lệch tax bán-mua nguy hiểm
    pub dangerous_tax_diff: f64,

    /// Thời gian khóa LP tối thiểu (giây)
    pub min_pool_lock_time: u64,

    /// Ngưỡng thanh khoản tối thiểu (USD)
    pub min_liquidity_threshold: f64,

    /// Phần trăm giảm thanh khoản đáng báo động
    pub liquidity_drop_alert: f64,

    // ===== Monitoring Constants =====
    /// Tần suất kiểm tra bất thường contract (milliseconds)
    pub contract_monitor_interval_ms: u64,

    /// Tần suất kiểm tra tax (milliseconds)
    pub tax_check_interval_ms: u64,

    /// Ngưỡng whale (phần trăm của tổng cung) cho việc theo dõi 
    pub whale_threshold_percent: f64,

    // ===== Security Constants =====
    /// Tối thiểu ngày khóa LP an toàn
    pub min_safe_liquidity_lock_days: u64,

    /// Tối đa phần trăm token trong ví dev/team an toàn
    pub max_safe_ownership_percentage: f64,

    /// Tối đa thời gian delay giữa các giao dịch (giây)
    pub max_transfer_delay_seconds: u64,

    // ===== Market Analysis Constants =====
    /// Phần trăm biến động giá cảnh báo trong 24h
    pub volatility_warning_threshold: f64,

    /// Phần trăm tăng volume đáng chú ý trong 24h
    pub volume_surge_threshold: f64,

    // ===== MEV Protection Constants =====
    /// Ngưỡng rủi ro MEV (thang điểm 0-1)
    pub mev_risk_threshold: f64,

    /// Hệ số tăng gas để chống front-running
    pub frontrun_gas_boost: f64,
}

impl Default for ConfigurableConstants {
    fn default() -> Self {
        Self {
            // Quick Trade Strategy
            quick_trade_min_bnb: 0.1,
            quick_trade_target_profit: 5.0,
            quick_trade_max_hold_time: 300,

            // Smart Trade Strategy
            smart_trade_min_hold_time: 1200,
            smart_trade_max_hold_time: 1800,
            smart_trade_tsl_percent: 2.0,

            // General Trading
            stop_loss_percent: 5.0,
            max_trade_amount_bnb: 0.5,
            price_check_interval_ms: 1000,

            // Token Analysis
            max_safe_buy_tax: 10.0,
            max_safe_sell_tax: 10.0,
            dangerous_tax_diff: 5.0,
            min_pool_lock_time: 30 * 24 * 60 * 60, // 30 days in seconds
            min_liquidity_threshold: 5000.0,
            liquidity_drop_alert: 30.0,

            // Monitoring
            contract_monitor_interval_ms: 30000,
            tax_check_interval_ms: 60000,
            whale_threshold_percent: 3.0,

            // Security
            min_safe_liquidity_lock_days: 30,
            max_safe_ownership_percentage: 15.0,
            max_transfer_delay_seconds: 60,

            // Market Analysis
            volatility_warning_threshold: 20.0,
            volume_surge_threshold: 300.0,

            // MEV Protection
            mev_risk_threshold: 0.7,
            frontrun_gas_boost: 1.1,
        }
    }
}

// Đối tượng global cho constants có thể điều chỉnh
lazy_static::lazy_static! {
    pub static ref CONSTANTS: Arc<RwLock<ConfigurableConstants>> = Arc::new(RwLock::new(ConfigurableConstants::default()));
}

/// Cập nhật constants từ cấu hình
pub async fn update_constants(config: &ConfigurableConstants) {
    let mut constants = CONSTANTS.write().await;
    *constants = config.clone();
}

/// Trả về bản sao của constants hiện tại
pub async fn get_current_constants() -> ConfigurableConstants {
    CONSTANTS.read().await.clone()
}

// =========================================================================
// ================= SMART TRADE STANDARD CONSTANTS ========================
// =========================================================================

/// Ngưỡng giao dịch tối thiểu (BNB) để kích hoạt quick trade
pub const QUICK_TRADE_MIN_BNB: f64 = 0.1;

/// Phần trăm lợi nhuận mục tiêu (5%) cho quick trade
pub const QUICK_TRADE_TARGET_PROFIT: f64 = 5.0;

/// Thời gian giữ tối đa cho quick trade (5 phút, đơn vị: giây)
pub const QUICK_TRADE_MAX_HOLD_TIME: u64 = 300;

// ===== Smart Trade Strategy Constants =====
/// Thời gian giữ tối thiểu cho smart trade (20 phút, đơn vị: giây)
pub const SMART_TRADE_MIN_HOLD_TIME: u64 = 1200;

/// Thời gian giữ tối đa cho smart trade (30 phút, đơn vị: giây) 
pub const SMART_TRADE_MAX_HOLD_TIME: u64 = 1800;

/// Phần trăm trailing stop loss (2%) cho smart trade
pub const SMART_TRADE_TSL_PERCENT: f64 = 2.0;

// ===== General Trading Constants =====
/// Phần trăm ngưỡng ngăn lỗ (5%) áp dụng cho mọi loại giao dịch
pub const STOP_LOSS_PERCENT: f64 = 5.0;

/// Số tiền tối đa (0.5 BNB) cho mỗi giao dịch để giới hạn rủi ro
pub const MAX_TRADE_AMOUNT_BNB: f64 = 0.5;

/// Khoảng thời gian kiểm tra giá (1 giây, đơn vị: milliseconds)
pub const PRICE_CHECK_INTERVAL_MS: u64 = 1000;

// ===== Token Analysis Constants =====
/// Ngưỡng tax mua tối đa an toàn (10%)
pub const MAX_SAFE_BUY_TAX: f64 = 10.0;

/// Ngưỡng tax bán tối đa an toàn (10%)
pub const MAX_SAFE_SELL_TAX: f64 = 10.0;

/// Chênh lệch tax bán-mua nguy hiểm (>5%)
pub const DANGEROUS_TAX_DIFF: f64 = 5.0;

/// Thời gian khóa LP tối thiểu (30 ngày, đơn vị: giây)
pub const MIN_POOL_LOCK_TIME: u64 = 30 * 24 * 60 * 60;

/// Ngưỡng thanh khoản tối thiểu ($5000)
pub const MIN_LIQUIDITY_THRESHOLD: f64 = 5000.0;

/// Phần trăm giảm thanh khoản đáng báo động (30%)
pub const LIQUIDITY_DROP_ALERT: f64 = 30.0;

// ===== Monitoring Constants =====
/// Tần suất kiểm tra bất thường contract (30 giây, đơn vị: milliseconds)
pub const CONTRACT_MONITOR_INTERVAL_MS: u64 = 30000;

/// Tần suất kiểm tra tax (60 giây, đơn vị: milliseconds)
pub const TAX_CHECK_INTERVAL_MS: u64 = 60000;

/// Ngưỡng whale (3% của tổng cung) cho việc theo dõi 
pub const WHALE_THRESHOLD_PERCENT: f64 = 3.0;

// ===== Security Constants =====
/// Tối thiểu ngày khóa LP an toàn (30 ngày)
pub const MIN_SAFE_LIQUIDITY_LOCK_DAYS: u64 = 30;

/// Tối đa phần trăm token trong ví dev/team an toàn (15%)
pub const MAX_SAFE_OWNERSHIP_PERCENTAGE: f64 = 15.0;

/// Tối đa thời gian delay giữa các giao dịch (60 giây)
pub const MAX_TRANSFER_DELAY_SECONDS: u64 = 60;

// ===== Market Analysis Constants =====
/// Phần trăm biến động giá cảnh báo trong 24h (20%)
pub const VOLATILITY_WARNING_THRESHOLD: f64 = 20.0;

/// Phần trăm tăng volume đáng chú ý trong 24h (300%)
pub const VOLUME_SURGE_THRESHOLD: f64 = 300.0;

// ===== MEV Protection Constants =====
/// Ngưỡng rủi ro MEV (0.7 trên thang điểm 0-1)
pub const MEV_RISK_THRESHOLD: f64 = 0.7;

/// Hệ số tăng gas để chống front-running (tăng 10%)
pub const FRONTRUN_GAS_BOOST: f64 = 1.1;

// Gas and network settings
/// Default gas limit for token approval transactions
pub const DEFAULT_GAS_LIMIT_APPROVAL: u64 = 60000;

/// Default gas limit for swap transactions
pub const DEFAULT_GAS_LIMIT_SWAP: u64 = 350000;

/// Gas price multiplier for high priority transactions
pub const GAS_PRICE_MULTIPLIER_HIGH: f64 = 1.2;

/// Gas price multiplier for medium priority transactions
pub const GAS_PRICE_MULTIPLIER_MEDIUM: f64 = 1.1;

/// Gas price multiplier for low priority transactions
pub const GAS_PRICE_MULTIPLIER_LOW: f64 = 1.0;

// Slippage settings
/// Default slippage percentage for buy transactions
pub const DEFAULT_BUY_SLIPPAGE: f64 = 2.0;

/// Default slippage percentage for sell transactions
pub const DEFAULT_SELL_SLIPPAGE: f64 = 3.0;

/// Maximum allowed slippage percentage
pub const MAX_SLIPPAGE: f64 = 20.0;

// Timeout settings
/// Default transaction timeout in seconds
pub const DEFAULT_TX_TIMEOUT: u64 = 180;

/// Default polling interval for transaction status in seconds
pub const DEFAULT_POLLING_INTERVAL: u64 = 5;

/// Maximum wait time for transaction confirmation in seconds
pub const MAX_WAIT_TIME: u64 = 300;

// Trading parameters
/// Default take profit percentage
pub const DEFAULT_TAKE_PROFIT_PERCENT: f64 = 30.0;

/// Default stop loss percentage
pub const DEFAULT_STOP_LOSS_PERCENT: f64 = 15.0;

/// Default trade expiration time in seconds (1 day)
pub const DEFAULT_TRADE_EXPIRE_TIME: u64 = 24 * 60 * 60;

// Error messages
/// Unknown failure reason message
pub const UNKNOWN_FAILURE_REASON: &str = "Unknown reason";

/// High slippage warning format
pub const HIGH_SLIPPAGE_FORMAT: &str = "Abnormally high sell slippage: {}%";

/// Default test amount for transaction simulation
pub const DEFAULT_TEST_AMOUNT: &str = "0.01";

/// Slippage threshold to detect honeypot tokens
pub const HONEYPOT_SLIPPAGE_THRESHOLD: f64 = 50.0;

/// Insufficient liquidity error format
pub const INSUFFICIENT_LIQUIDITY_FORMAT: &str = "Insufficient liquidity: {}";

/// Default maximum wait time for transaction
pub const DEFAULT_MAX_WAIT_TIME: u64 = 300;

/// Default gas price multiplier
pub const DEFAULT_GAS_PRICE_MULTIPLIER: f64 = 1.1;
