//! Module chain_adapters
//!
//! Module này chịu trách nhiệm kết nối với các blockchain khác nhau, bao gồm:
//! - EVM adapter: Tương tác với các blockchain EVM (Ethereum, BSC, Polygon)
//! - Solana adapter: Tương tác với Solana blockchain (stub implementation)
//! - Stellar adapter: Tương tác với Stellar blockchain (stub implementation)
//! - Sui adapter: Tương tác với Sui blockchain (stub implementation)
//! - TON adapter: Tương tác với TON blockchain (stub implementation)
//! - Bridge adapter: Tương tác với các bridge giữa các blockchain khác nhau

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use once_cell::sync::Lazy;

pub mod evm_adapter;
pub mod evm_extensions;
pub mod sol_adapter;
pub mod stellar_adapter;
pub mod sui_adapter; 
pub mod ton_adapter;
pub mod bridge_adapter;
pub mod bridge_provider;

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

// Import utils từ common thay vì utils nội bộ
pub use crate::tradelogic::common::utils as chain_utils;

// Re-export MevAdapterExtensions trait
pub use evm_extensions::MevAdapterExtensions;

/// Adapter Registry để quản lý các adapter cho các blockchain khác nhau
#[derive(Clone)]
pub struct AdapterRegistry {
    /// EVM adapters, indexed by chain ID
    evm_adapters: HashMap<u32, Arc<evm_adapter::EvmAdapter>>,
    /// Solana adapter
    sol_adapter: Option<Arc<sol_adapter::SolAdapter>>,
    /// Stellar adapter
    stellar_adapter: Option<Arc<stellar_adapter::StellarAdapter>>,
    /// Sui adapter
    sui_adapter: Option<Arc<sui_adapter::SuiAdapter>>,
    /// TON adapter
    ton_adapter: Option<Arc<ton_adapter::TonAdapter>>,
    /// Bridge adapter
    bridge_adapter: Option<Arc<bridge_adapter::BridgeAdapter>>,
}

impl AdapterRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            evm_adapters: HashMap::new(),
            sol_adapter: None,
            stellar_adapter: None,
            sui_adapter: None,
            ton_adapter: None,
            bridge_adapter: None,
        }
    }

    /// Register an EVM adapter for a specific chain ID
    pub fn register_evm_adapter(&mut self, chain_id: u32, adapter: Arc<evm_adapter::EvmAdapter>) {
        self.evm_adapters.insert(chain_id, adapter);
    }

    /// Get an EVM adapter for a specific chain ID
    pub fn get_evm_adapter(&self, chain_id: u32) -> Option<Arc<evm_adapter::EvmAdapter>> {
        self.evm_adapters.get(&chain_id).cloned()
    }

    /// Register a Solana adapter
    pub fn register_sol_adapter(&mut self, adapter: Arc<sol_adapter::SolAdapter>) {
        self.sol_adapter = Some(adapter);
    }

    /// Get the Solana adapter
    pub fn get_sol_adapter(&self) -> Option<Arc<sol_adapter::SolAdapter>> {
        self.sol_adapter.clone()
    }

    /// Register a Stellar adapter
    pub fn register_stellar_adapter(&mut self, adapter: Arc<stellar_adapter::StellarAdapter>) {
        self.stellar_adapter = Some(adapter);
    }

    /// Get the Stellar adapter
    pub fn get_stellar_adapter(&self) -> Option<Arc<stellar_adapter::StellarAdapter>> {
        self.stellar_adapter.clone()
    }

    /// Register a Sui adapter
    pub fn register_sui_adapter(&mut self, adapter: Arc<sui_adapter::SuiAdapter>) {
        self.sui_adapter = Some(adapter);
    }

    /// Get the Sui adapter
    pub fn get_sui_adapter(&self) -> Option<Arc<sui_adapter::SuiAdapter>> {
        self.sui_adapter.clone()
    }

    /// Register a TON adapter
    pub fn register_ton_adapter(&mut self, adapter: Arc<ton_adapter::TonAdapter>) {
        self.ton_adapter = Some(adapter);
    }

    /// Get the TON adapter
    pub fn get_ton_adapter(&self) -> Option<Arc<ton_adapter::TonAdapter>> {
        self.ton_adapter.clone()
    }

    /// Register a Bridge adapter
    pub fn register_bridge_adapter(&mut self, adapter: Arc<bridge_adapter::BridgeAdapter>) {
        self.bridge_adapter = Some(adapter);
    }

    /// Get the Bridge adapter
    pub fn get_bridge_adapter(&self) -> Option<Arc<bridge_adapter::BridgeAdapter>> {
        self.bridge_adapter.clone()
    }
}

/// Global adapter registry
static ADAPTER_REGISTRY: Lazy<Mutex<AdapterRegistry>> = Lazy::new(|| {
    Mutex::new(AdapterRegistry::new())
});

/// Get the global adapter registry
pub fn get_adapter_registry() -> Arc<AdapterRegistry> {
    match ADAPTER_REGISTRY.lock() {
        Ok(registry) => Arc::new(registry.clone()),
        Err(e) => {
            // Log error and return empty registry as fallback
            tracing::error!("Failed to acquire adapter registry lock: {}", e);
            Arc::new(AdapterRegistry::new())
        }
    }
} 