//! Common functionality shared between smart_trade and mev_logic
//!
//! This module contains code that is shared between different trade logic modules
//! to avoid duplication and ensure consistency.

pub mod types;
pub mod analysis;
pub mod gas;
pub mod mev_detection;
pub mod trade_execution;
pub mod constants;
pub mod health_monitor;
pub mod rate_limiter;
pub mod resource_manager;
pub mod throttle;
pub mod network_reliability;
pub mod bridge_helper;

// Re-exports for convenience
pub use types::*;

// Re-export specific functions from utils to avoid ambiguity
pub use crate::tradelogic::utils::{
    analyze_transaction_pattern,
    calculate_price_impact,
    is_proxy_contract,
    format_token_address,
    is_pump_and_dump,
    calculate_risk_score,
    current_time_seconds,
    calculate_percentage_change,
    eth_to_usd,
    usd_to_eth,
    calculate_profit,
    wait_for_transaction,
    convert_to_token_units,
    calculate_moving_average,
    find_price_extremes,
    calculate_rsi,
    detect_price_trend,
    generate_trade_id,
    validate_trade_params,
    simulate_trade_execution,
    execute_blockchain_transaction,
    perform_token_security_check,
    get_token_metadata,
    check_trade_conditions,
};

// Re-export specific functions from analysis to avoid ambiguity
pub use analysis::{
    analyze_trader_behavior,
    detect_token_issues,
    is_proxy_contract as analysis_is_proxy_contract,
    convert_token_safety_to_security_check,
    analyze_token_security,
};

pub use trade_execution::*;

// Re-export specific functions from gas to avoid ambiguity
pub use gas::{
    NetworkCongestion,
    TransactionPriority,
    GasHistory,
    get_gas_history,
    calculate_optimal_gas_price,
    calculate_optimal_gas_for_mev,
    get_gas_price,
    get_pending_transaction_count,
    get_gas_price_sync,
};

// Re-export specific types and functions from mev_detection to avoid ambiguity
pub use mev_detection::{
    MevAttackType,
    MevAnalysisResult,
    analyze_mempool_for_mev,
    calculate_optimal_gas_price as mev_calculate_optimal_gas_price,
    calculate_optimal_slippage,
    generate_anti_mev_tx_params,
    AntiMevTxParams,
};

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

// Re-export constants
pub use constants::*; 