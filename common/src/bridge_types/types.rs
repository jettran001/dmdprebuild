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
    
    /// Initial delay between retries in seconds
    pub initial_delay: u64,
    
    /// Factor by which the delay increases with each retry (exponential backoff)
    pub backoff_factor: f32,
    
    /// Maximum total timeout for transaction monitoring in seconds
    pub max_timeout: u64,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            initial_delay: 30,
            backoff_factor: 1.5,
            max_timeout: 3600, // 1 hour
        }
    }
}

impl MonitorConfig {
    /// Create a new monitoring configuration
    pub fn new(max_retries: u32, initial_delay: u64, backoff_factor: f32, max_timeout: u64) -> Self {
        Self {
            max_retries,
            initial_delay,
            backoff_factor,
            max_timeout,
        }
    }
    
    /// Get initial delay as Duration
    pub fn initial_delay_duration(&self) -> Duration {
        Duration::from_secs(self.initial_delay)
    }
    
    /// Get max timeout as Duration
    pub fn max_timeout_duration(&self) -> Duration {
        Duration::from_secs(self.max_timeout)
    }
    
    /// Calculate delay for a specific retry attempt (with exponential backoff)
    pub fn delay_for_retry(&self, retry_number: u32) -> Duration {
        let delay_secs = self.initial_delay as f32 * self.backoff_factor.powi(retry_number as i32);
        Duration::from_secs(delay_secs as u64)
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
        assert_eq!(config.initial_delay, 30);
        assert_eq!(config.backoff_factor, 1.5);
        assert_eq!(config.max_timeout, 3600);
        
        let custom = MonitorConfig::new(3, 10, 2.0, 300);
        assert_eq!(custom.max_retries, 3);
        
        // Test exponential backoff
        assert_eq!(custom.delay_for_retry(0).as_secs(), 10);  // Initial delay
        assert_eq!(custom.delay_for_retry(1).as_secs(), 20);  // 10 * 2.0
        assert_eq!(custom.delay_for_retry(2).as_secs(), 40);  // 10 * 2.0^2
    }
} 