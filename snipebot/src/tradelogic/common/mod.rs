/// Common functionality shared between smart_trade and mev_logic
///
/// This module contains code that is shared between different trade logic modules
/// to avoid duplication and ensure consistency.

pub mod types;
pub mod utils;
pub mod analysis;
pub mod gas;
pub mod trade_execution;
pub mod mev_detection;
pub mod resource_manager;
pub mod network_reliability;
pub mod health_monitor;
pub mod rate_limiter;
pub mod throttle;
pub mod bridge_helper;

// Re-exports for convenience
pub use types::*;
pub use utils::*;
pub use analysis::*;
pub use trade_execution::*;
pub use gas::*;
pub use mev_detection::*;
pub use resource_manager::{
    ResourceManager,
    ResourcePriority,
    ResourceType,
    get_resource_manager,
    GLOBAL_RESOURCE_MANAGER,
};

// Re-export network reliability
pub use network_reliability::{
    NetworkStatus,
    CircuitBreakerConfig,
    CircuitState,
    NetworkReliabilityManager,
    RetryStrategy,
    get_network_manager,
    GLOBAL_NETWORK_MANAGER,
};

// Re-export health monitoring
pub use health_monitor::{
    SystemHealth,
    HealthAlert,
    AlertSeverity,
    HealthMonitor,
    HealthMonitorConfig,
    get_health_monitor,
    GLOBAL_HEALTH_MONITOR,
};

// Re-export rate limiting
pub use rate_limiter::{
    RateLimitType,
    RateLimitConfig,
    RequestPriority,
    RateLimiter,
    ApiRateLimiterManager,
    get_api_rate_limiter,
    GLOBAL_API_RATE_LIMITER,
};

pub use throttle::{
    ThrottleConfig, TransactionThrottler, get_global_throttler,
    execute_with_throttling, update_global_throttler_config, update_global_system_metrics
};

// Re-export bridge helper functions
pub use bridge_helper::{
    get_source_chain_id,
    get_destination_chain_id,
    is_token_supported,
    monitor_transaction,
    read_provider_from_vec,
    read_provider_from_map,
}; 