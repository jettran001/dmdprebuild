//! Infrastructure module for network services
//!
//! Module này cung cấp các thành phần hạ tầng và cấu trúc cho network services:
//! - Config
//! - Service traits
//! - Error handling
//!
//! ## Examples:
//!
//! ```rust
//! use network::config::NetworkConfig;
//! use network::config::types::{RedisConfig, IpfsConfig};
//! use network::infra::service_traits::ServiceError;
//! ```

// Chỉ export các file chuẩn hóa và file logic thực sự
pub mod service_traits;
pub mod service_mocks;
pub mod redis_pool;
pub mod toml_profile;

// Cấu trúc config đã được di chuyển sang network/config
// Không khai báo mod config tại đây để tránh xung đột import

// Các file config cũ đã bị xóa:
// - config_types.rs -> thay thế bởi config/types.rs
// - config_loader.rs -> thay thế bởi config/loader.rs
// - config.rs -> thay thế bởi config/mod.rs

// Metrics và telemetry chưa được triển khai, sẽ bổ sung sau khi có file thực tế 