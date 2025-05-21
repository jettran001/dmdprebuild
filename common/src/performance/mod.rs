//! Tối ưu hóa hiệu năng và xử lý đồng thời
//!
//! Module này quản lý queuing, concurrency control và caching để tối ưu hóa latency.
//! Sử dụng tokio::sync::Mutex cùng Redis queue để tránh double-sign và race-condition.

pub mod queue;
pub mod concurrency;
pub mod cache;
pub mod optimization;
pub mod monitoring;

/// Re-export các service chính
pub use queue::QueueManagementService;
pub use concurrency::ConcurrencyControlService;
pub use cache::CacheService;
pub use optimization::PerformanceOptimizationService;
pub use monitoring::MonitoringService; 