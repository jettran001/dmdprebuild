/// Phân tích giao dịch trong mempool để phát hiện cơ hội và rủi ro.
///
/// Module này theo dõi và phân tích các giao dịch chưa được xác nhận (pending)
/// trong mempool của blockchain để:
/// - Phát hiện sớm các token mới được tạo
/// - Phát hiện giao dịch thêm thanh khoản lớn
/// - Theo dõi và tận dụng cơ hội MEV
/// - Phát hiện giao dịch đáng ngờ từ ví lớn (whale)
/// - Cảnh báo rủi ro giao dịch dựa trên pattern
///
/// # Flow
/// `blockchain` -> `snipebot/mempool_analyzer` -> `tradelogic`

// Định nghĩa các kiểu dữ liệu cho phân tích mempool
mod types;

// Phân tích mempool và các giao dịch chưa được xác nhận
mod analyzer;

// Lọc và phân loại giao dịch
mod filter;

// Đánh giá độ ưu tiên và phân tích gas
mod priority;

// Phát hiện cơ hội arbitrage và MEV
mod arbitrage;

// Phát hiện pattern đáng ngờ (front-running, sandwich,...)
mod detection;

// Các tiện ích chung
mod utils;

// Re-export các struct và trait quan trọng
pub use types::{
    MempoolTransaction, TransactionType, TransactionPriority, 
    MempoolAlert, MempoolAlertType, AlertSeverity, SuspiciousPattern,
    NewTokenInfo, TokenTransferInfo, TransactionCluster,
    TransactionPredictionData, TokenInfo, TransactionFilterOptions,
    TransactionSortCriteria
};

// Re-export MempoolAnalyzer như API chính
pub use analyzer::MempoolAnalyzer;

// Re-export các hàm phân tích chính
pub use detection::{
    detect_front_running, detect_sandwich_attack, detect_high_frequency_trading
};

pub use filter::{
    get_filtered_transactions
};

pub use priority::{
    estimate_confirmation_time
};

pub use arbitrage::{
    find_mev_bots
};

pub use utils::{
    hash_data, normalize_address, gas_multiplier_to_string
}; 