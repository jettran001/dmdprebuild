//! 🧭 Module quản lý người dùng miễn phí (free user) cho wallet module.
//! Triển khai các tính năng theo mô hình:
//! - types: Định nghĩa dữ liệu
//! - manager: Quản lý tổng thể
//! - auth: Xác thực người dùng
//! - limits: Kiểm tra và áp dụng giới hạn
//! - records: Ghi nhận hoạt động

// Exports public types
mod types;
pub use types::*;

// Exports manager core
mod manager;
pub use manager::*;

// Auth related functionality
mod auth;
pub(crate) use auth::*;

// Limit checking functionality
mod limits;
pub(crate) use limits::*;

// Activity records functionality
mod records;
pub(crate) use records::*;

// Queries functionality
mod queries;
pub(crate) use queries::*;

// Test utilities
#[cfg(test)]
mod test_utils;

// Re-exports publicly accessible items
pub use manager::FreeUserManager;
pub use types::{FreeUserData, UserStatus, TransactionType, SnipeResult};