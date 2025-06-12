//! Bridge transaction monitoring utilities
//!
//! This module provides standard functions for monitoring bridge transactions across chains.
//! These functions can be used by both blockchain and snipebot domains to avoid code duplication.

// use std::time::Duration;  // Unused import
use tokio::time::sleep;
use anyhow::{Result};
use tracing::{debug, error, info, warn};

use super::status::BridgeStatus;
use super::types::MonitorConfig;
use super::transaction::BridgeTransaction;

/// Standard bridge transaction monitoring function
///
/// This function can be used by both blockchain and snipebot domains to monitor
/// bridge transactions without duplicating code.
///
/// # Arguments
/// * `tx_hash` - Transaction hash to monitor
/// * `get_status_fn` - Function to get transaction status
/// * `tx_info` - Optional transaction information
/// * `config` - Optional monitoring configuration
///
/// # Returns
/// Final transaction status
pub async fn monitor_transaction<F, R>(
    tx_hash: &str,
    mut get_status_fn: F,
    tx_info: Option<BridgeTransaction>,
    config: Option<MonitorConfig>
) -> Result<BridgeStatus> 
where 
    F: FnMut(&str) -> R,
    R: std::future::Future<Output = Result<BridgeStatus>>
{
    // Use default config if none provided
    let config = config.unwrap_or_default();
    
    // Start time for monitoring
    let start_time = std::time::Instant::now();
    let max_duration = config.max_monitor_time;
    let mut retries = 0;
    let mut last_status = BridgeStatus::Pending;
    
    // Log start of monitoring
    if let Some(tx) = &tx_info {
        info!(
            "Starting monitoring for transaction {} from {} to {}",
            tx_hash, tx.source_chain.as_str(), tx.target_chain.as_str()
        );
    } else {
        info!("Starting monitoring for transaction {}", tx_hash);
    }
    
    // Main monitoring loop
    loop {
        // Check if we've exceeded max monitoring time
        if start_time.elapsed() > max_duration {
            warn!("Max monitoring time exceeded for transaction {}", tx_hash);
            break;
        }
        
        // Get status
        match get_status_fn(tx_hash).await {
            Ok(status) => {
                last_status = status.clone();
                
                // Log status change
                debug!("Transaction {} status: {:?}", tx_hash, status);
                
                // Check if we're done
                match status {
                    BridgeStatus::Completed => {
                        info!("Transaction {} completed successfully", tx_hash);
                        return Ok(status.clone());
                    }
                    BridgeStatus::Failed(ref reason) => {
                        // Check if we should retry
                        if retries < config.max_retries {
                            retries += 1;
                            warn!(
                                "Transaction {} failed: {}. Retrying ({}/{})",
                                tx_hash, reason, retries, config.max_retries
                            );
                            sleep(config.retry_delay).await;
                            continue;
                        } else {
                            error!(
                                "Transaction {} failed after {} retries: {}", 
                                tx_hash, retries, reason
                            );
                            return Ok(status.clone());
                        }
                    }
                    _ => {
                        // Continue monitoring
                    }
                }
            }
            Err(e) => {
                error!("Error getting status for transaction {}: {}", tx_hash, e);
                if retries < config.max_retries {
                    retries += 1;
                    warn!(
                        "Error monitoring transaction {}. Retrying ({}/{})",
                        tx_hash, retries, config.max_retries
                    );
                    sleep(config.retry_delay).await;
                    continue;
                } else {
                    return Err(anyhow::anyhow!("Failed to monitor transaction {} after {} retries: {}", tx_hash, retries, e));
                }
            }
        }
        
        // Wait before checking again
        sleep(config.monitor_interval).await;
    }
    
    // Return last known status
    Ok(last_status)
} 