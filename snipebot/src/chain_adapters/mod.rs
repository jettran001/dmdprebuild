//! Module chain_adapters
//!
//! Module này chịu trách nhiệm kết nối với các blockchain khác nhau, bao gồm:
//! - EVM adapter: Tương tác với các blockchain EVM (Ethereum, BSC, Polygon)
//! - Solana adapter: Tương tác với Solana blockchain
//! - Các adapter khác sẽ được thêm trong tương lai

pub mod evm_adapter;
// pub mod sol_adapter; // TODO: Implement in the future
// pub mod stellar_adapter; // TODO: Implement in the future
// pub mod sui_adapter; // TODO: Implement in the future
// pub mod ton_adapter; // TODO: Implement in the future 