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
pub mod types;
pub mod strategy;
pub mod strategy_implementation;
pub mod opportunity;
pub mod execution;
pub mod analysis;
pub mod analyzer;
pub mod trader_behavior;
pub mod bundle;
pub mod jit_liquidity;
pub mod cross_domain;
pub mod traits;
pub use crate::tradelogic::utils as utils;

// Re-export main components
pub use types::{
    MevConfig, MevOpportunity, MevOpportunityType, MevExecutionMethod,
    ArbitrageOpportunity, SandwichOpportunity, FrontrunOpportunity,
    TraderBehaviorAnalysis, TraderBehaviorType, TraderExpertiseLevel,
    GasBehavior, TokenPriceImpact, TradeRoute, MevStrategyType,
    AdvancedMevOpportunityType, JITLiquidityConfig, CrossDomainMevConfig,
    SearcherStrategy, SearcherIdentity
};

pub use opportunity::{OpportunityManager, OpportunityFilter};
pub use strategy::MevStrategy;
pub use execution::execute_mev_opportunity;
pub use trader_behavior::analyze_trader;

// Re-export analyzer functions from analys/mempool/analyzer.rs
pub use crate::analys::mempool::analyzer::MempoolAnalyzer;

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

// Re-export bundle components
pub use bundle::{BundleManager, MevBundle, BundleStatus, create_bundle};

// Re-export from common/bridge_types
pub use crate::tradelogic::traits::BridgeProvider;
pub use crate::tradelogic::traits::BridgeStatus;

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

// Re-export analysis functions from common/analysis
pub use crate::tradelogic::common::analysis::{
    detect_sandwich_pattern,
    detect_front_running_pattern,
    detect_abnormal_liquidity_pattern,
    detect_arbitrage_opportunities,
    find_transactions_for_token,
    detect_new_tokens
};

// Import constants from common module instead of local constants.rs
pub use crate::tradelogic::common::constants as mev_constants; 