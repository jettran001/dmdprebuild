/// Common functionality shared between smart_trade and mev_logic
///
/// This module contains code that is shared between different trade logic modules
/// to avoid duplication and ensure consistency.

pub mod types;
pub mod utils;
pub mod analysis;
pub mod gas;
pub mod trade_execution;

// Re-exports for convenience
pub use types::*;
pub use utils::*;
pub use analysis::*;
pub use trade_execution::*;

// Re-export gas utilities
pub use gas::{
    calculate_optimal_gas_price,
    calculate_optimal_gas_for_mev,
    TransactionPriority,
    NetworkCongestion,
}; 