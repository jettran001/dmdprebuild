//! TradeLogic module for DiamondChain SnipeBot
//!
//! Contains core trade execution logic, strategies and utilities.
//! Integrates with analysis services to make intelligent trading decisions.

/// Module tradelogic - Core logic cho các chiến lược giao dịch
///
/// Module này chứa các core logic và implemention của các chiến lược giao dịch
/// bao gồm manual trading, smart trading, và MEV logic.

// Re-export traits
pub mod traits;
pub use traits::*;

// Core logic and implementations
pub mod coordinator;
pub mod manual_trade;
pub mod smart_trade;
pub mod mev_logic;
pub mod common;
pub mod stubs;
pub mod utils;  // Thêm module utils mới

// Re-export analys API providers
pub use crate::analys::api::{
    create_mempool_analysis_provider,
    create_token_analysis_provider,
    create_risk_analysis_provider,
    create_mev_opportunity_provider,
    MempoolAnalysisProviderImpl,
    TokenAnalysisProviderImpl,
    RiskAnalysisProviderImpl,
    MevOpportunityProviderImpl,
};

// Import core traits
use crate::chain_adapters::evm_adapter::EvmAdapter;
use std::sync::Arc;
use std::collections::HashMap;
use anyhow::Result;

/// Create all analysis providers needed for trade logic
/// 
/// Helper function to create all analysis providers at once with shared chain adapters
/// 
/// # Parameters
/// * `chain_adapters` - Map of chain ID to EVM adapter for blockchain interaction
/// 
/// # Returns
/// A tuple containing all analysis providers
pub fn create_analysis_providers(
    chain_adapters: HashMap<u32, Arc<EvmAdapter>>
) -> Result<(
    Arc<dyn MempoolAnalysisProvider>,
    Arc<dyn TokenAnalysisProvider>,
    Arc<dyn RiskAnalysisProvider>,
    Arc<dyn MevOpportunityProvider>
)> {
    let mempool_provider = create_mempool_analysis_provider(chain_adapters.clone());
    let token_provider = create_token_analysis_provider(chain_adapters.clone());
    let risk_provider = create_risk_analysis_provider();
    let mev_provider = create_mev_opportunity_provider(
        mempool_provider.clone(),
        token_provider.clone(),
        risk_provider.clone()
    );
    
    Ok((mempool_provider, token_provider, risk_provider, mev_provider))
}

// Crate-level module declarations for trade logic
// Re-exports important traits and types for public use

// Standard imports
use std::sync::Arc;

// All module declarations
pub mod traits;             // Core traits for all trade logic
pub mod coordinator;        // Trade coordinator implementation
pub mod manual_trade;       // Manual trading implementation 
pub mod stubs;              // Stub implementations for testing
pub mod utils;              // Unified utilities (merged from all utils files)
pub mod smart_trade;        // Smart trade module
pub mod common;             // Common module for shared code
pub mod mev_logic;          // MEV logic module

// Common types re-exports
pub use common::types::{TradeAction, TradeStatus, TokenIssue, TokenIssueDetail, TokenIssueType, SecurityCheckResult};

// Re-export core traits
pub use traits::{
    TradeExecutor, TradeCoordinator, RiskManager, 
    StrategyOptimizer, CrossChainTrader,
    MempoolAnalysisProvider, TokenAnalysisProvider,
    RiskAnalysisProvider, MevOpportunityProvider,
};

// Re-export coordinator
pub use coordinator::{TradeCoordinatorImpl, ExecutorRegistration};

// Reduced re-exports from the utils module
pub use utils::{
    current_time_seconds,
    generate_trade_id,
    calculate_percentage_change,
    format_address,
};

// Re-export from smart_trade 
pub use smart_trade::types::{
    TradeTracker, TradeResult, SwapData,
    TradeStrategy, DcaPosition, StrategyParams,
};

/// Get a reference to the global coordinator
/// This function is used across modules to access the coordinator
pub fn get_coordinator() -> Arc<TradeCoordinatorImpl> {
    crate::lib::get_global_coordinator()
}

// Common resource code
pub(crate) mod resource_manager {
    use std::sync::Arc;
    use once_cell::sync::Lazy;
    use tokio::sync::Semaphore;

    /// Priority levels for resource allocation
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum ResourcePriority {
        Low,
        Medium,
        High,
        Critical,
    }

    // Global resource manager
    static RESOURCE_MANAGER: Lazy<Arc<ResourceManager>> = 
        Lazy::new(|| Arc::new(ResourceManager::new()));

    /// Get the global resource manager
    pub fn get_resource_manager() -> Arc<ResourceManager> {
        RESOURCE_MANAGER.clone()
    }

    /// Resource manager for controlling access to limited resources
    pub struct ResourceManager {
        api_semaphore: Semaphore,
        network_semaphore: Semaphore,
        compute_semaphore: Semaphore,
    }

    impl ResourceManager {
        /// Create a new resource manager with default limits
        pub fn new() -> Self {
            Self {
                api_semaphore: Semaphore::new(10),
                network_semaphore: Semaphore::new(20),
                compute_semaphore: Semaphore::new(5),
            }
        }
    }
}

pub use resource_manager::{ResourcePriority, get_resource_manager}; 