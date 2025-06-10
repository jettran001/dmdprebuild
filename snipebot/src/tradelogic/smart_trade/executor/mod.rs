/// Smart Trade Executor - Module phối hợp
///
/// Module này quản lý và điều phối các module con của SmartTradeExecutor,
/// cung cấp các hàm factory để tạo executor và các tiện ích khởi tạo khác.
///
/// # Cấu trúc module
/// - `core`: Định nghĩa chính của SmartTradeExecutor và TradeExecutor trait implementation
/// - `trade_handler`: Logic xử lý giao dịch cụ thể (mua, bán, swap)
/// - `market_monitor`: Logic giám sát thị trường và giá cả
/// - `risk_manager`: Logic quản lý rủi ro và đánh giá an toàn
/// - `position_manager`: Logic quản lý vị thế giao dịch
/// - `strategy`: Các chiến lược giao dịch
/// - `utils`: Các tiện ích phụ trợ
/// - `types`: Các kiểu dữ liệu nội bộ
// Re-export các module con
pub mod core;
pub mod types;
pub mod trade_handler;
pub mod risk_manager;
pub mod market_monitor;
pub mod strategy;
pub mod position_manager;
pub mod utils;

// Re-export các thành phần chính để module cha có thể sử dụng
pub use core::SmartTradeExecutor;
pub use types::{ExecutorConfig, ExecutorState, TradeStatus, TradeType};
pub use trade_handler::TradeHandler;
pub use risk_manager::RiskManager;
pub use market_monitor::MarketMonitor;
pub use strategy::{Strategy, StrategyType, StrategyConfig};
pub use position_manager::PositionManager;

// Standard imports
use std::sync::Arc;
use anyhow::Result;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::tradelogic::traits::TradeCoordinator;
use super::types::SmartTradeConfig;

/// Tạo SmartTradeExecutor mới với cấu hình mặc định
pub async fn create_smart_trade_executor(
    chain_adapters: Arc<Vec<Arc<EvmAdapter>>>,
    config: SmartTradeConfig,
) -> Result<SmartTradeExecutor> {
    let mut adapters_map = std::collections::HashMap::new();
    
    // Thêm các chain adapter vào map
    for adapter in chain_adapters.iter() {
        let chain_id = adapter.chain_id();
        adapters_map.insert(chain_id, adapter.clone());
    }
    
    // Khởi tạo executor
    SmartTradeExecutor::new(adapters_map, config).await
}

/// Tạo SmartTradeExecutor và đăng ký với coordinator
pub async fn create_and_register_executor(
    chain_adapters: Arc<Vec<Arc<EvmAdapter>>>,
    config: SmartTradeConfig,
    coordinator: Arc<dyn TradeCoordinator>,
) -> Result<SmartTradeExecutor> {
    let executor = create_smart_trade_executor(chain_adapters, config).await?;
    
    // Thiết lập coordinator
    executor.set_coordinator(coordinator).await?;
    
    // Trả về executor đã được cấu hình
    Ok(executor)
} 