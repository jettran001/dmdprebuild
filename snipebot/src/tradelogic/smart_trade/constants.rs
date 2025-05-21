//! Constants for Smart Trading Strategies
//! 
//! This module contains all thresholds, timeouts, and configuration constants 
//! used in the smart trading system. Organizing these values as constants allows
//! for easy modification and configuration of the trading behavior.

// ===== Quick Trade Strategy Constants =====
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
