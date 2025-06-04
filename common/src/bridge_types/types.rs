//! Common bridge type definitions
//!
//! This module contains common type definitions used across the bridge system,
//! including fee estimations, monitoring configurations, and other shared types.

use serde::{Serialize, Deserialize};
use std::time::Duration;

/// Fee estimate for bridge transactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeEstimate {
    /// Fee amount in native tokens (as string to ensure compatibility with different blockchains)
    pub fee_amount: String,
    
    /// Fee token symbol
    pub fee_token: String,
    
    /// Fee in USD
    pub fee_usd: f64,
}

impl FeeEstimate {
    /// Create a new fee estimate
    pub fn new(fee_amount: String, fee_token: String, fee_usd: f64) -> Self {
        Self {
            fee_amount,
            fee_token,
            fee_usd,
        }
    }
    
    /// Create a zero fee estimate
    pub fn zero(token: &str) -> Self {
        Self {
            fee_amount: "0".to_string(),
            fee_token: token.to_string(),
            fee_usd: 0.0,
        }
    }
}

/// Configuration for transaction monitoring
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MonitorConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    
    /// Delay between retries in seconds
    pub retry_delay: Duration,
    
    /// Monitor interval in seconds
    pub monitor_interval: Duration,
    
    /// Maximum monitoring time in seconds
    pub max_monitor_time: Duration,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            retry_delay: Duration::from_secs(30),
            monitor_interval: Duration::from_secs(30),
            max_monitor_time: Duration::from_secs(3600), // 1 hour
        }
    }
}

impl MonitorConfig {
    /// Create a new monitoring configuration
    pub fn new(max_retries: u32, retry_delay: Duration, monitor_interval: Duration, max_monitor_time: Duration) -> Self {
        Self {
            max_retries,
            retry_delay,
            monitor_interval,
            max_monitor_time,
        }
    }
    
    /// Create a new monitoring configuration from seconds
    pub fn from_seconds(max_retries: u32, retry_delay_secs: u64, monitor_interval_secs: u64, max_monitor_time_secs: u64) -> Self {
        Self {
            max_retries,
            retry_delay: Duration::from_secs(retry_delay_secs),
            monitor_interval: Duration::from_secs(monitor_interval_secs),
            max_monitor_time: Duration::from_secs(max_monitor_time_secs),
        }
    }
}

/// Bridge metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeMetrics {
    /// Total number of bridge transactions
    pub total_transactions: u64,
    
    /// Number of successful transactions
    pub successful_transactions: u64,
    
    /// Number of failed transactions
    pub failed_transactions: u64,
    
    /// Average transaction completion time (seconds)
    pub avg_completion_time_seconds: f64,
    
    /// Total fees paid (in USD)
    pub total_fees_usd: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_fee_estimate() {
        let fee = FeeEstimate::new("0.01".to_string(), "ETH".to_string(), 15.0);
        assert_eq!(fee.fee_amount, "0.01");
        assert_eq!(fee.fee_token, "ETH");
        assert_eq!(fee.fee_usd, 15.0);
        
        let zero_fee = FeeEstimate::zero("BNB");
        assert_eq!(zero_fee.fee_amount, "0");
        assert_eq!(zero_fee.fee_token, "BNB");
        assert_eq!(zero_fee.fee_usd, 0.0);
    }
    
    #[test]
    fn test_monitor_config() {
        let config = MonitorConfig::default();
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.retry_delay, Duration::from_secs(30));
        assert_eq!(config.monitor_interval, Duration::from_secs(30));
        assert_eq!(config.max_monitor_time, Duration::from_secs(3600));
        
        let custom = MonitorConfig::new(3, Duration::from_secs(10), Duration::from_secs(20), Duration::from_secs(300));
        assert_eq!(custom.max_retries, 3);
        assert_eq!(custom.retry_delay, Duration::from_secs(10));
        assert_eq!(custom.monitor_interval, Duration::from_secs(20));
        assert_eq!(custom.max_monitor_time, Duration::from_secs(300));
        
        let from_secs = MonitorConfig::from_seconds(4, 15, 25, 400);
        assert_eq!(from_secs.max_retries, 4);
        assert_eq!(from_secs.retry_delay, Duration::from_secs(15));
        assert_eq!(from_secs.monitor_interval, Duration::from_secs(25));
        assert_eq!(from_secs.max_monitor_time, Duration::from_secs(400));
    }
    }