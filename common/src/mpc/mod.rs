//! Multi-Party Computation (MPC)
//!
//! Module này quản lý threshold MPC cho ký và quản lý key, với tối ưu hóa
//! cache session/token MPC để không phải handshake lại mỗi lần.

pub mod session;
pub mod cache;
pub mod signing;
pub mod key_management;
pub mod threshold;

/// Re-export các service chính
pub use session::MpcSessionService;
pub use cache::MpcCacheService;
pub use signing::MpcSigningService;
pub use key_management::MpcKeyManagementService;
pub use threshold::MpcThresholdService; 