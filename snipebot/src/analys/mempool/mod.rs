//! Phân tích giao dịch trong mempool để phát hiện cơ hội và rủi ro.
//!
//! Module này theo dõi và phân tích các giao dịch chưa được xác nhận (pending)
//! trong mempool của blockchain để:
//! - Phát hiện sớm các token mới được tạo
//! - Phát hiện giao dịch thêm thanh khoản lớn
//! - Theo dõi và tận dụng cơ hội MEV
//! - Phát hiện giao dịch đáng ngờ từ ví lớn (whale)
//! - Cảnh báo rủi ro giao dịch dựa trên pattern
//!
//! # Flow
//! `blockchain` -> `snipebot/mempool_analyzer` -> `tradelogic`

//! Mempool analysis module
//!
//! This module is responsible for monitoring and analyzing mempool transactions
//! to identify opportunities and risks.

// Re-export main types
pub mod types;
pub use types::*;

// Re-export utility functions
pub mod utils;
pub use utils::*;

// Analysis components
pub mod analyzer;
pub mod detection;

// Lọc và phân loại giao dịch
pub mod filter;

// Đánh giá độ ưu tiên và phân tích gas
pub mod priority;

// Phát hiện cơ hội arbitrage và MEV
pub mod arbitrage;

// Re-export main analyzer for convenience
pub use analyzer::MempoolAnalyzer;

// Re-export các hàm phân tích chính
pub use detection::{
    detect_front_running,
    detect_sandwich_attack,
    detect_high_frequency_trading,
    detect_sudden_liquidity_removal,
    detect_whale_movement,
    detect_token_liquidity_pattern
};

pub use filter::get_filtered_transactions;

pub use priority::{
    estimate_confirmation_time,
    gas_multiplier_to_string,
    calculate_transaction_priority,
    format_gas_price,
    analyze_gas_price_trend
};

pub use arbitrage::{
    find_mev_bots,
    find_sandwich_attacks,
    find_frontrunning_transactions
}; 