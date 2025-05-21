//! API Module for Analysis Services
//!
//! This module provides API implementations that the MEV logic and other
//! modules can use to access analysis functionality in a standardized way.

mod mempool_api;
mod token_api;
mod risk_api;
mod mev_opportunity_api;

// Re-export primary API implementations
pub use mempool_api::MempoolAnalysisProviderImpl;
pub use token_api::TokenAnalysisProviderImpl;
pub use risk_api::RiskAnalysisProviderImpl;
pub use mev_opportunity_api::MevOpportunityProviderImpl;

// Factory functions to create API providers
use std::sync::Arc;
use std::collections::HashMap;
use crate::analys::mempool::MempoolAnalyzer;
use crate::analys::risk_analyzer::RiskAnalyzer;
use crate::chain_adapters::evm_adapter::EvmAdapter;

/// Create a new mempool analysis provider
pub fn create_mempool_analysis_provider(
    chain_adapters: HashMap<u32, Arc<EvmAdapter>>
) -> Arc<dyn crate::tradelogic::traits::MempoolAnalysisProvider> {
    Arc::new(mempool_api::MempoolAnalysisProviderImpl::new(chain_adapters))
}

/// Create a new token analysis provider
pub fn create_token_analysis_provider(
    chain_adapters: HashMap<u32, Arc<EvmAdapter>>
) -> Arc<dyn crate::tradelogic::traits::TokenAnalysisProvider> {
    Arc::new(token_api::TokenAnalysisProviderImpl::new(chain_adapters))
}

/// Create a new risk analysis provider
pub fn create_risk_analysis_provider() -> Arc<dyn crate::tradelogic::traits::RiskAnalysisProvider> {
    Arc::new(risk_api::RiskAnalysisProviderImpl::new(
        Arc::new(RiskAnalyzer::new())
    ))
}

/// Create a new MEV opportunity provider
pub fn create_mev_opportunity_provider(
    mempool_provider: Arc<dyn crate::tradelogic::traits::MempoolAnalysisProvider>,
    token_provider: Arc<dyn crate::tradelogic::traits::TokenAnalysisProvider>,
    risk_provider: Arc<dyn crate::tradelogic::traits::RiskAnalysisProvider>
) -> Arc<dyn crate::tradelogic::traits::MevOpportunityProvider> {
    Arc::new(mev_opportunity_api::MevOpportunityProviderImpl::new(
        mempool_provider,
        token_provider,
        risk_provider
    ))
} 