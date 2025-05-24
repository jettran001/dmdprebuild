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

// Thêm các module mới cho common gateway server
pub mod web3;
pub mod frontend;
pub mod performance;
pub mod mpc;
pub mod network_integration;

// Re-export một số tính năng thường dùng
pub use api::gateway::ApiGatewayService;
pub use auth::authentication::AuthenticationService;
pub use web3::blockchain_gateway::BlockchainGatewayService;
pub use performance::queue::QueueManagementService;
pub use mpc::session::MpcSessionService;
pub use network_integration::grpc::GrpcIntegrationService;
pub use network_integration::websocket::WebsocketIntegrationService;
