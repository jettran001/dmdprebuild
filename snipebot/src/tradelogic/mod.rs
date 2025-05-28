//! Trading Logic Module
//!
//! This module is responsible for all trading logic, including:
//! - Manual trading: User-initiated trades via API
//! - Smart trading: Automated trading with intelligent strategies
//! - MEV logic: Detection and utilization of MEV opportunities
//!
//! The module follows a trait-based design pattern to ensure modularity and extensibility.
//! Each strategy implements common interfaces defined in the `traits` module.

pub mod smart_trade;     // Intelligent automated trading with risk management
pub mod mev_logic;       // MEV detection and execution from mempool analysis
pub mod common;          // Shared code between different trading strategies
pub mod traits;          // Common traits that all trading strategies must implement
pub mod manual_trade;    // User-initiated trading through API requests
pub mod coordinator;     // Coordinator for managing shared state between executors
pub mod stubs;           // Stub implementations for traits that do not have complete implementations

// Re-export main traits for easy access
pub use traits::{
    TradeExecutor, RiskManager, StrategyOptimizer, CrossChainTrader,
    MempoolAnalysisProvider, TokenAnalysisProvider, RiskAnalysisProvider, MevOpportunityProvider,
    TradeCoordinator
};

// Re-export factory functions (not implementations)
pub use coordinator::create_trade_coordinator;
pub use smart_trade::create_smart_trade_executor;
pub use mev_logic::strategy::create_mev_strategy;
pub use manual_trade::create_manual_trade_executor;
pub use stubs::{
    create_stub_risk_manager,
    create_stub_strategy_optimizer,
    create_stub_cross_chain_trader
}; 