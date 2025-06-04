//! TradeLogic module for DiamondChain SnipeBot
//!
//! Contains core trade execution logic, strategies and utilities.
//! Integrates with analysis services to make intelligent trading decisions.

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