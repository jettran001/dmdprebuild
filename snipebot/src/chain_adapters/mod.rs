//! Module chain_adapters
//!
//! Module này chịu trách nhiệm kết nối với các blockchain khác nhau, bao gồm:
//! - EVM adapter: Tương tác với các blockchain EVM (Ethereum, BSC, Polygon)
//! - Solana adapter: Tương tác với Solana blockchain (stub implementation)
//! - Stellar adapter: Tương tác với Stellar blockchain (stub implementation)
//! - Sui adapter: Tương tác với Sui blockchain (stub implementation)
//! - TON adapter: Tương tác với TON blockchain (stub implementation)
//! - Bridge adapter: Tương tác với các bridge giữa các blockchain khác nhau

pub mod evm_adapter;
pub mod sol_adapter;
pub mod stellar_adapter;
pub mod sui_adapter; 
pub mod ton_adapter;
pub mod bridge_adapter;

// Re-export các loại dữ liệu quan trọng từ common::bridge_types cho API
pub use common::bridge_types::{
    Chain,
    BridgeStatus,
    BridgeTransaction,
    FeeEstimate,
    MonitorConfig,
    BridgeProvider
};

// Re-export BridgeAdapter implementation từ module bridge_adapter
pub use bridge_adapter::BridgeAdapter;

// Re-export BridgeAdapter trait từ common nhưng với tên khác để tránh xung đột
pub use common::bridge_types::BridgeAdapter as BridgeAdapterTrait; 