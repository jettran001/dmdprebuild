pub mod config;
pub mod logger;
pub mod error;
pub mod auth;
pub mod api;
pub mod router;

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
