//! API Module for Analysis Services
//!
//! This module provides API implementations that the MEV logic and other
//! modules can use to access analysis functionality in a standardized way.

mod mempool_api;
mod token_api;
mod risk_api;
mod mev_opportunity_api;
mod factory;
#[cfg(test)]
pub mod usage_example;

// Re-export primary API implementations
pub use mempool_api::MempoolAnalysisProviderImpl;
pub use token_api::TokenAnalysisProviderImpl;
pub use risk_api::RiskAnalysisProviderImpl;
pub use mev_opportunity_api::MevOpportunityProviderImpl;

// Re-export factory functions
pub use factory::{
    create_mempool_analysis_provider,
    create_token_analysis_provider,
    create_risk_analysis_provider,
    create_mev_opportunity_provider,
};