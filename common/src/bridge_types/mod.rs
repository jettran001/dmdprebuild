//! Bridge types for cross-chain operations
//!
//! This module contains types, traits, and utilities for cross-chain bridge operations
//! that are shared between the blockchain and snipebot domains. By centralizing these
//! definitions, we avoid code duplication and ensure consistency across the system.

mod chain;
mod status;
mod types;
mod transaction;
mod providers;
mod monitor;

// Re-export all items from submodules
pub use chain::*;
pub use status::*;
pub use types::*;
pub use transaction::*;
pub use providers::*;
pub use monitor::*;

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