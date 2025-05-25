//! # Bridge Types Module
//!
//! Định nghĩa các kiểu dữ liệu, traits và hàm utility cho hệ thống bridge
//! giữa các blockchain.

pub mod chain;
pub mod status;
pub mod transaction;
pub mod types;
pub mod providers;
pub mod monitoring;

pub use chain::Chain;
pub use status::BridgeStatus;
pub use transaction::BridgeTransaction;
pub use types::{FeeEstimate, MonitorConfig};
pub use providers::{BridgeProvider, BridgeClient, BridgeMonitor, BridgeAdapter};
pub use monitoring::monitor_transaction;

// Documentation for core concepts
/// Bridge operations between blockchains, enabling cross-chain token transfers
/// and message passing.
///
/// The bridge system consists of:
/// * `Chain` - Enum of supported blockchains
/// * `BridgeStatus` - Status of bridge transactions
/// * `BridgeTransaction` - Details of a bridge transaction
/// * `BridgeProvider` - Trait for bridge providers (LayerZero, Wormhole)
/// * `BridgeClient` - Trait for API clients
/// * `BridgeMonitor` - Trait for transaction monitoring
pub struct BridgeDocs; 