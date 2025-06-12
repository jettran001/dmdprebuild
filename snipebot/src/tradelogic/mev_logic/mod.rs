/// MEV logic module for detecting and utilizing MEV (Maximal Extractable Value) opportunities.
///
/// This module focuses on:
/// - Detecting MEV opportunities from mempool transactions
/// - Leveraging arbitrage opportunities between DEXes
/// - Executing sandwich trades
/// - Front-running valuable transactions
/// - Detecting new tokens and liquidity
/// - Just-In-Time liquidity provision
/// - Cross-domain MEV opportunities
/// - Advanced backrunning and order flow tracking
mod types;
mod strategy;
mod opportunity;
mod trader_behavior;
mod execution;
mod analysis;
mod analyzer;
mod bot;
mod constants;
mod bundle;
mod jit_liquidity;
mod cross_domain;
mod utils;

// Re-export main components
pub use types::{
    MevOpportunityType, 
    MevExecutionMethod,
    TraderBehaviorType,
    TraderExpertiseLevel,
    AdvancedMevOpportunityType,
    JITLiquidityConfig,
    CrossDomainMevConfig,
    SearcherStrategy,
    SearcherIdentity
};
pub use strategy::MevStrategy;
pub use opportunity::MevOpportunity;
pub use trader_behavior::{TraderBehaviorAnalysis, GasBehavior};
pub use execution::execute_mev_opportunity;
pub use bot::MevBot;
pub use bundle::{BundleManager, MevBundle, BundleStatus};
pub use types::MevConfig;

// Re-export JIT Liquidity
pub use jit_liquidity::{
    JITLiquidityProvider,
    JITLiquidityOpportunity,
    PoolInfo
};

// Re-export Cross-domain MEV
pub use cross_domain::{
    CrossDomainMevManager,
    CrossDomainArbitrage
};
// Re-export từ common/bridge_types
pub use common::bridge_types::{
    BridgeProvider,
    BridgeStatus
};

// Re-export advanced analyzer components
pub use analyzer::{
    detect_backrunning_opportunity,
    detect_flash_loan_opportunity,
    detect_liquidation_opportunity,
    detect_jit_opportunities,
    analyze_order_flow,
    LiquidationOpportunity,
    OrderFlowOpportunity
};

// Factory function
pub use strategy::create_mev_strategy;

// Re-export trader behavior analysis function
pub use trader_behavior::analyze_traders_batch; 