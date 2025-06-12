//! Common module for shared utilities and types
//!
//! This module serves as a central point for shared code between different
//! parts of the system. It includes bridge types, middleware, network integration,
//! and other shared functionality.

pub mod bridge_types;
pub mod api;
pub mod middleware;
pub mod mpc;
pub mod network_integration;
pub mod performance;
pub mod trading_types;
pub mod trading_actions;

// Re-export bridge_types để dễ dàng import
pub use bridge_types::*;
// Re-export trading_types để dễ dàng import
pub use trading_types::*;
// Re-export trading_actions để dễ dàng import
pub use trading_actions::*; 