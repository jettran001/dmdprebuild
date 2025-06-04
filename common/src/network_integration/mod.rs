//! Tích hợp với các dịch vụ mạng từ domain network
//!
//! Module này tích hợp và sử dụng các công nghệ từ domain network,
//! bao gồm gRPC, WebSocket, HTTP/2, Redis và P2P communications.

pub mod grpc;
pub mod websocket;
pub mod http2;
pub mod redis;
pub mod p2p;

/// Re-export các service chính
pub use grpc::GrpcIntegrationService;
pub use websocket::WebsocketIntegrationService;
pub use http2::Http2IntegrationService;
pub use redis::RedisIntegrationService;
pub use p2p::P2pIntegrationService; 