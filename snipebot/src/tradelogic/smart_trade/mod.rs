//! Smart Trade Module
//!
//! Triển khai các chiến lược giao dịch thông minh tự động với phân tích rủi ro và theo dõi thông minh.
//! Module này thuộc thành phần `tradelogic` của hệ thống và là trọng tâm của quá trình giao dịch tự động.
//!
//! # Các chiến lược giao dịch
//! Module cung cấp các chiến lược giao dịch nâng cao:
//! - **Logic tránh rủi ro**: Quét và phân tích token để phát hiện các rủi ro như honeypot, tax cao, ...
//! - **Logic mua bán nhanh**: Mua khi phát hiện lệnh lớn, bán khi tăng >5% hoặc sau 5 phút
//! - **Logic mua bán thông minh**: Áp dụng TSL (Trailing Stop Loss) với thời gian giữ dài hơn
//!
//! # Flow chính
//! `analys::mempool` -> `smart_trade::executor` -> `chain_adapters::evm_adapter` (execution)
//!
//! # Tính năng nổi bật
//! - Phát hiện honeypot và token không an toàn
//! - Adaptive timing (điều chỉnh tần suất kiểm tra dựa trên hoạt động)
//! - Trailing Stop Loss tự động
//! - Phân tích rủi ro dựa trên nhiều nguồn dữ liệu

// Re-export các module con
pub mod analys_client;
pub mod security;
pub mod anti_mev;
pub mod optimization;
pub mod optimizer;
pub mod alert;
pub mod types;
pub mod constants;
pub mod token_analysis;
pub mod trade_strategy;
pub mod utils;
pub mod executor;

// Re-export các thành phần chính
pub use executor::SmartTradeExecutor;
pub use types::{SmartTradeConfig, TradeResult, TradeStatus, TradeStrategy};
pub use self::token_analysis::TokenAnalysis;
pub use self::trade_strategy::TradeStrategyManager;
pub use self::optimization::{
    TradeOptimizer, GasOptimizationConfig, PoolInfo,
    BatchTradeResult, BatchTradeDetail, optimize_trade_order
};
pub use self::anti_mev::{
    AntiMevProtection, AntiMevConfig, MevProtectionResult,
    DetectedMevPattern, MevType, ProtectionMeasure
};
pub use self::analys_client::SmartTradeAnalysisClient;

/// Tạo mới SmartTradeExecutor với cấu hình mặc định
///
/// Hàm tiện ích để dễ dàng khởi tạo executor mà không cần gọi trực tiếp constructor
///
/// # Examples
/// ```
/// use snipebot::tradelogic::smart_trade;
///
/// let executor = smart_trade::create_smart_trade_executor();
/// ```
pub fn create_smart_trade_executor() -> SmartTradeExecutor {
    SmartTradeExecutor::new()
}

/// Tạo mới SmartTradeAnalysisClient để sử dụng các dịch vụ phân tích
///
/// Hàm tiện ích để dễ dàng khởi tạo client phân tích 
/// mà không cần gọi trực tiếp constructor
///
/// # Parameters
/// - `chain_adapters`: Map chứa các EVM adapter, key là chain_id
///
/// # Examples
/// ```
/// use snipebot::tradelogic::smart_trade;
/// use std::collections::HashMap;
/// use std::sync::Arc;
///
/// let chain_adapters = HashMap::new();
/// let analysis_client = smart_trade::create_analysis_client(chain_adapters);
/// ```
pub fn create_analysis_client(chain_adapters: std::collections::HashMap<u32, std::sync::Arc<crate::chain_adapters::evm_adapter::EvmAdapter>>) -> SmartTradeAnalysisClient {
    SmartTradeAnalysisClient::new(chain_adapters)
}
