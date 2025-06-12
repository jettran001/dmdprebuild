//! Constants cho MEV Logic
//!
//! Module này định nghĩa các hằng số, ngưỡng và giá trị mặc định
//! sử dụng trong các chiến lược MEV.

// === Các hằng số và ngưỡng ===

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