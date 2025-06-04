//! Bridge transaction monitoring utilities
//!
//! This module provides shared logic for monitoring bridge transactions across
//! multiple blockchains, with support for timeout, retry and backoff strategies.

use std::time::{Duration, Instant};
use anyhow::{Result, Context, bail, anyhow};
use futures::future::{self, BoxFuture};
use tracing::{debug, warn, error, info};

use super::status::BridgeStatus;
use super::types::MonitorConfig;
use super::providers::BridgeProvider;

/// Monitor a transaction with the given provider until completion, failure, or timeout
///
/// # Arguments
/// * `provider` - Bridge provider implementation
/// * `tx_hash` - Transaction hash to monitor
/// * `config` - Optional monitoring configuration
///
/// # Returns
/// Final transaction status or error
pub async fn monitor_transaction<P>(
    provider: &P,
    tx_hash: &str,
    config: Option<MonitorConfig>
) -> Result<BridgeStatus>
where
    P: BridgeProvider + ?Sized,
{
    let config = config.unwrap_or_default();
    
    if tx_hash.is_empty() {
        bail!("Transaction hash cannot be empty");
    }
    
    let start_time = Instant::now();
    let deadline = start_time + Duration::from_secs(config.max_timeout);
    
    let mut retries = 0;
    let mut delay = config.initial_delay;
    
    info!(
        "Monitoring transaction {} with provider {}, max timeout: {}s",
        tx_hash, provider.provider_name(), config.max_timeout
    );
    
    loop {
        // Check if we've exceeded the max timeout
        if Instant::now() > deadline {
            let elapsed = start_time.elapsed().as_secs();
            let error_msg = format!(
                "Monitoring timeout exceeded for transaction: {} after {}s",
                tx_hash, elapsed
            );
            error!("{}", error_msg);
            return Err(anyhow!(error_msg));
        }
        
        // Check transaction status
        match provider.get_transaction_status(tx_hash).await {
            Ok(status) => {
                let elapsed = start_time.elapsed().as_secs();
                debug!(
                    "Transaction {} status: {:?} after {}s",
                    tx_hash, status, elapsed
                );
                
                // Check if transaction is in a terminal state
                match status {
                    BridgeStatus::Completed => {
                        info!(
                            "Transaction {} completed successfully after {}s", 
                            tx_hash, elapsed
                        );
                        return Ok(status);
                    },
                    BridgeStatus::Failed(reason) => {
                        error!(
                            "Transaction {} failed after {}s: {}", 
                            tx_hash, elapsed, reason
                        );
                        return Ok(status);
                    },
                    _ => {
                        // Transaction still in progress, wait and retry
                        debug!(
                            "Transaction {} still in progress (status: {:?}) after {}s, waiting {}s...",
                            tx_hash, status, elapsed, delay
                        );
                        tokio::time::sleep(Duration::from_secs(delay)).await;
                        
                        // Increase delay for next check using exponential backoff
                        delay = (delay as f32 * config.backoff_factor) as u64;
                    }
                }
            },
            Err(e) => {
                warn!("Failed to get status for transaction {}: {}", tx_hash, e);
                
                retries += 1;
                if retries > config.max_retries {
                    let elapsed = start_time.elapsed().as_secs();
                    error!(
                        "Max retries ({}) exceeded for transaction {} after {}s",
                        config.max_retries, tx_hash, elapsed
                    );
                    return Err(e).context(format!(
                        "Max retries exceeded ({})",
                        config.max_retries
                    ));
                }
                
                // Wait before retrying
                tokio::time::sleep(Duration::from_secs(delay)).await;
                
                // Increase delay for next check using exponential backoff
                delay = (delay as f32 * config.backoff_factor) as u64;
            }
        }
    }
}

/// Monitor multiple transactions concurrently
///
/// # Arguments
/// * `provider` - Bridge provider implementation
/// * `tx_hashes` - List of transaction hashes to monitor
/// * `config` - Optional monitoring configuration
///
/// # Returns
/// Map of transaction hashes to their final status
pub async fn monitor_transactions<P>(
    provider: &P,
    tx_hashes: &[String],
    config: Option<MonitorConfig>
) -> Result<Vec<(String, Result<BridgeStatus>)>>
where
    P: BridgeProvider + ?Sized,
{
    let config = config.unwrap_or_default();
    
    if tx_hashes.is_empty() {
        return Ok(Vec::new());
    }
    
    // Create monitoring futures for each transaction
    let mut futures = Vec::with_capacity(tx_hashes.len());
    
    for tx_hash in tx_hashes {
        let tx_hash = tx_hash.clone();
        let provider_ref = provider;
        let config_clone = config.clone();
        
        let future: BoxFuture<'static, (String, Result<BridgeStatus>)> = Box::pin(async move {
            let result = monitor_transaction(provider_ref, &tx_hash, Some(config_clone)).await;
            (tx_hash, result)
        });
        
        futures.push(future);
    }
    
    // Execute all monitoring futures concurrently
    let results = future::join_all(futures).await;
    
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::chain::Chain;
    use super::super::types::FeeEstimate;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    
    struct MockProvider {
        name: String,
        status_sequence: Vec<BridgeStatus>,
        call_count: Arc<AtomicUsize>,
        should_fail: bool,
    }
    
    #[async_trait]
    impl BridgeProvider for MockProvider {
        async fn send_message(
            &self,
            _from_chain: Chain,
            _to_chain: Chain,
            _receiver: String,
            _payload: Vec<u8>
        ) -> Result<String> {
            Ok("mock_tx_hash".to_string())
        }
        
        async fn get_transaction_status(&self, _tx_hash: &str) -> Result<BridgeStatus> {
            let count = self.call_count.fetch_add(1, Ordering::SeqCst);
            
            if self.should_fail {
                return Err(anyhow!("Mock provider error"));
            }
            
            if count < self.status_sequence.len() {
                Ok(self.status_sequence[count].clone())
            } else {
                // Return last status if we've gone past the sequence
                Ok(self.status_sequence.last().cloned().unwrap_or(BridgeStatus::Pending))
            }
        }
        
        async fn estimate_fee(
            &self,
            _from_chain: Chain,
            _to_chain: Chain,
            _payload_size: usize
        ) -> Result<FeeEstimate> {
            Ok(FeeEstimate::zero("ETH"))
        }
        
        fn supports_chain(&self, _chain: Chain) -> bool {
            true
        }
        
        fn provider_name(&self) -> &str {
            &self.name
        }
    }
    
    #[tokio::test]
    async fn test_monitor_transaction_success() {
        let provider = MockProvider {
            name: "test_provider".to_string(),
            status_sequence: vec![
                BridgeStatus::Pending,
                BridgeStatus::Confirmed,
                BridgeStatus::Completed,
            ],
            call_count: Arc::new(AtomicUsize::new(0)),
            should_fail: false,
        };
        
        let config = MonitorConfig {
            max_retries: 3,
            initial_delay: 1,
            backoff_factor: 1.0,
            max_timeout: 10,
        };
        
        let result = monitor_transaction(&provider, "test_tx", Some(config)).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), BridgeStatus::Completed);
        assert_eq!(provider.call_count.load(Ordering::SeqCst), 3);
    }
    
    #[tokio::test]
    async fn test_monitor_transaction_failure() {
        let provider = MockProvider {
            name: "test_provider".to_string(),
            status_sequence: vec![
                BridgeStatus::Pending,
                BridgeStatus::Confirmed,
                BridgeStatus::Failed("Test failure".to_string()),
            ],
            call_count: Arc::new(AtomicUsize::new(0)),
            should_fail: false,
        };
        
        let config = MonitorConfig {
            max_retries: 3,
            initial_delay: 1,
            backoff_factor: 1.0,
            max_timeout: 10,
        };
        
        let result = monitor_transaction(&provider, "test_tx", Some(config)).await;
        assert!(result.is_ok());
        let status = result.unwrap();
        assert!(matches!(status, BridgeStatus::Failed(_)));
        if let BridgeStatus::Failed(reason) = status {
            assert_eq!(reason, "Test failure");
        }
    }
    
    #[tokio::test]
    async fn test_monitor_transaction_provider_error() {
        let provider = MockProvider {
            name: "test_provider".to_string(),
            status_sequence: vec![],
            call_count: Arc::new(AtomicUsize::new(0)),
            should_fail: true,
        };
        
        let config = MonitorConfig {
            max_retries: 2,
            initial_delay: 1,
            backoff_factor: 1.0,
            max_timeout: 10,
        };
        
        let result = monitor_transaction(&provider, "test_tx", Some(config)).await;
        assert!(result.is_err());
        assert_eq!(provider.call_count.load(Ordering::SeqCst), 3); // Initial + 2 retries
    }
} 