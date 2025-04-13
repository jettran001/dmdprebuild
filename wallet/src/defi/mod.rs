//! Module DeFi cung cấp các chức năng tài chính phi tập trung cho wallet.
//! Bao gồm các tính năng như farming, staking và tương tác với các dịch vụ DeFi.
//! Sẽ được phát triển chi tiết trong giai đoạn tiếp theo.

// Khai báo các module con
mod api;
mod farm;
mod stake;

// Re-exports
pub use api::DefiApi;
pub use farm::{FarmingManager, FarmingOpportunity};
pub use stake::{StakingManager, StakingOpportunity};
