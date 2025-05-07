//! # Plugins Module
//! 
//! Module này chứa các plugin cho domain network.
//! Mỗi plugin implement trait Plugin từ core/engine.rs và sử dụng các service chuẩn hóa từ infra.

// Re-export plugin types
pub use crate::core::engine::{Plugin, PluginType, PluginError};

// Exported plugins
pub mod redis;
pub mod ws;
pub mod libp2p;
pub mod wasm;
pub mod webrtc;
pub mod grpc;
pub mod pool;

// Re-export các plugin chuẩn hóa cho dễ import
pub use self::{
    redis::RedisPlugin,
    ws::WebSocketPlugin,
    wasm::WasmPlugin,
    grpc::GrpcPlugin,
    libp2p::Libp2pPlugin,
};

// ## Hướng dẫn import plugin chuẩn hóa
// ```rust
// use crate::plugins::{RedisPlugin, WebSocketPlugin, WasmPlugin, GrpcPlugin, Libp2pPlugin};
// ``` 