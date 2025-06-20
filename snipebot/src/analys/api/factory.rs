//! Factory functions for Analysis API providers
//!
//! This module contains factory functions to create various analysis providers.
//! These functions follow the dependency injection pattern, allowing for easier testing
//! and more flexible configurations.

use std::sync::Arc;
use std::collections::HashMap;
use crate::analys::risk_analyzer::RiskAnalyzer;
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::tradelogic::traits::{
    MempoolAnalysisProvider,
    TokenAnalysisProvider,
    RiskAnalysisProvider,
    MevOpportunityProvider,
};

use super::{
    mempool_api::MempoolAnalysisProviderImpl,
    token_api::TokenAnalysisProviderImpl,
    risk_api::RiskAnalysisProviderImpl,
    mev_opportunity_api::MevOpportunityProviderImpl,
};

/// Create a new mempool analysis provider
///
/// # Parameters
/// * `chain_adapters` - Map of chain ID to EVM adapter for blockchain interaction
///
/// # Returns
/// An implementation of the `MempoolAnalysisProvider` trait
pub fn create_mempool_analysis_provider(
    chain_adapters: HashMap<u32, Arc<EvmAdapter>>
) -> Arc<dyn MempoolAnalysisProvider> {
    Arc::new(MempoolAnalysisProviderImpl::new(chain_adapters))
}

/// Create a new token analysis provider
///
/// # Parameters
/// * `chain_adapters` - Map of chain ID to EVM adapter for blockchain interaction
///
/// # Returns
/// An implementation of the `TokenAnalysisProvider` trait
pub fn create_token_analysis_provider(
    chain_adapters: HashMap<u32, Arc<EvmAdapter>>
) -> Arc<dyn TokenAnalysisProvider> {
    Arc::new(TokenAnalysisProviderImpl::new(chain_adapters))
}

/// Create a new risk analysis provider
///
/// # Parameters
/// * `risk_analyzer` - The risk analyzer to use for risk analysis.
///                     This allows dependency injection and easier testing.
///
/// # Returns
/// An implementation of the `RiskAnalysisProvider` trait
pub fn create_risk_analysis_provider(
    risk_analyzer: Option<Arc<RiskAnalyzer>> = None
) -> Arc<dyn RiskAnalysisProvider> {
    // Use provided risk analyzer or create a new one
    let analyzer = risk_analyzer.unwrap_or_else(|| Arc::new(RiskAnalyzer::new()));
    Arc::new(RiskAnalysisProviderImpl::new(analyzer))
}

/// Create a new MEV opportunity provider
///
/// # Parameters
/// * `mempool_provider` - Provider for mempool analysis
/// * `token_provider` - Provider for token analysis
/// * `risk_provider` - Provider for risk analysis
///
/// # Returns
/// An implementation of the `MevOpportunityProvider` trait that
/// integrates mempool, token, and risk analysis
pub fn create_mev_opportunity_provider(
    mempool_provider: Arc<dyn MempoolAnalysisProvider>,
    token_provider: Arc<dyn TokenAnalysisProvider>,
    risk_provider: Arc<dyn RiskAnalysisProvider>
) -> Arc<dyn MevOpportunityProvider> {
    Arc::new(MevOpportunityProviderImpl::new(
        mempool_provider,
        token_provider,
        risk_provider
    ))
} 