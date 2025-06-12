// Common library for the Diamondchain project
//
// This crate provides shared functionality used across other crates in the workspace.
// It includes utility functions, common traits, error handling, and other shared components.

// Re-export modules
pub mod api;
pub mod middleware;
pub mod mpc;
pub mod network_integration;
pub mod performance;

// Re-export individual modules
pub mod cache;
pub mod config;
pub mod error;
pub mod logger;
pub mod module_map;
pub mod router;
pub mod sdk;

// Bridge types for cross-chain operations
pub mod bridge_types;

// Re-export bridge_types để dễ dàng sử dụng
pub use bridge_types::{Chain, BridgeStatus, BridgeTransaction, FeeEstimate, MonitorConfig, BridgeProvider, BridgeAdapter};

// Note: Previous re-exports were removed as they referenced non-existent modules/items
