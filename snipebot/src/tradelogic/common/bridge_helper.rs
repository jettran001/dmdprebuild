//! Bridge helper functions
//!
//! This module provides helper functions for working with dynamic BridgeProvider objects,
//! ensuring compatibility with trait objects while maintaining type safety.

use std::sync::Arc;
use std::collections::HashMap;
use std::future::Future;

use anyhow::{Result, anyhow, Context};
use async_trait::async_trait;

// Import necessary types
use common::bridge_types::providers::BridgeProvider;
use common::bridge_types::status::BridgeStatus;
use common::bridge_types::types::MonitorConfig;

/// Chỉ định các trait cơ bản cho bridge provider
pub trait BridgeProvider: Send + Sync + 'static {
    /// Get chain ID (source)
    fn get_source_chain_id(&self) -> Option<u64>;
    
    /// Get chain ID (destination)
    fn get_destination_chain_id(&self) -> Option<u64>;
    
    /// Check if token is supported by this bridge
    fn is_token_supported(&self, token_address: &str) -> bool;
}

/// Mở rộng trait BridgeProvider với các phương thức async
#[async_trait]
pub trait BridgeProviderExt: BridgeProvider {
    /// Get estimated bridging cost
    async fn get_bridging_cost_estimate(&self, token_address: &str, amount: f64) -> Result<f64>;
    
    /// Get estimated bridging time
    async fn get_bridging_time_estimate(&self) -> Result<u64>;
    
    /// Bridge tokens from source to destination chain
    async fn bridge_tokens(&self, token_address: &str, amount: f64) -> Result<String>;
    
    /// Monitor transaction status
    async fn monitor_transaction(&self, tx_hash: &str, callback: impl Fn(&str) -> Result<()> + Send + Sync + 'static) -> Result<()>;
    
    /// Get transaction status
    async fn get_transaction_status(&self, tx_hash: &str) -> Result<String>;
}

/// Get source chain ID from BridgeProvider
/// 
/// Helper function to get source chain ID from dyn BridgeProvider
/// 
/// # Parameters
/// * `provider` - Reference to dynamic BridgeProvider
/// 
/// # Returns
/// Option containing source chain ID if available
pub fn get_source_chain_id(provider: &dyn BridgeProvider) -> Option<u64> {
    provider.get_source_chain_id()
}

/// Get destination chain ID from BridgeProvider
/// 
/// Helper function to get destination chain ID from dyn BridgeProvider
/// 
/// # Parameters
/// * `provider` - Reference to dynamic BridgeProvider
/// 
/// # Returns
/// Option containing destination chain ID if available
pub fn get_destination_chain_id(provider: &dyn BridgeProvider) -> Option<u64> {
    provider.get_destination_chain_id()
}

/// Check if token is supported by BridgeProvider
/// 
/// Helper function to check if a token is supported by a dynamic BridgeProvider
/// 
/// # Parameters
/// * `provider` - Reference to dynamic BridgeProvider
/// * `token_address` - Address of the token to check
/// 
/// # Returns
/// true if the token is supported, false otherwise
pub fn is_token_supported(provider: &dyn BridgeProvider, token_address: &str) -> bool {
    provider.is_token_supported(token_address)
}

/// Monitor a bridge transaction with custom callback
/// 
/// Helper function to monitor a transaction status with a callback function
/// 
/// # Parameters
/// * `provider` - Arc to a dynamic BridgeProvider
/// * `tx_hash` - Transaction hash to monitor
/// * `callback` - Callback function to execute when status changes
/// 
/// # Returns
/// Result indicating success or failure
pub async fn monitor_transaction(
    _provider: &Arc<dyn BridgeProvider>, 
    _tx_hash: &str,
    _callback: impl Fn(&str) -> Result<()> + Send + Sync + 'static
) -> Result<()> {
    // Implement a default fallback behavior when using trait objects
    Err(anyhow!("monitor_transaction not supported with dynamic BridgeProvider"))
}

/// Read a BridgeProvider from a vector of providers
///
/// Helper function to safely read a BridgeProvider from a vector
///
/// # Parameters
/// * `providers` - Vector of BridgeProvider trait objects
/// * `index` - Index to read from
///
/// # Returns
/// Option containing a reference to the provider if found
pub fn read_provider_from_vec(providers: &Vec<Arc<dyn BridgeProvider>>, index: usize) -> Option<&Arc<dyn BridgeProvider>> {
    providers.get(index)
}

/// Read a BridgeProvider from a HashMap
///
/// Helper function to safely read a BridgeProvider from a HashMap
///
/// # Parameters
/// * `provider_map` - HashMap containing BridgeProvider trait objects
/// * `key` - Key to look up
///
/// # Returns
/// Option containing a reference to the provider if found
pub fn read_provider_from_map<K: std::hash::Hash + Eq>(provider_map: &HashMap<K, Arc<dyn BridgeProvider>>, key: &K) -> Option<&Arc<dyn BridgeProvider>> {
    provider_map.get(key)
}

/// Blanket implementation for all BridgeProvider implementors
#[async_trait]
impl<T: BridgeProvider + ?Sized + Send + Sync> BridgeProviderExt for T {
    async fn get_bridging_cost_estimate(&self, _token_address: &str, _amount: f64) -> Result<f64> {
        Err(anyhow!("get_bridging_cost_estimate not implemented"))
    }
    
    async fn get_bridging_time_estimate(&self) -> Result<u64> {
        Err(anyhow!("get_bridging_time_estimate not implemented"))
    }
    
    async fn bridge_tokens(&self, _token_address: &str, _amount: f64) -> Result<String> {
        Err(anyhow!("bridge_tokens not implemented"))
    }
    
    async fn monitor_transaction(&self, _tx_hash: &str, _callback: impl Fn(&str) -> Result<()> + Send + Sync + 'static) -> Result<()> {
        Err(anyhow!("monitor_transaction not implemented"))
    }
    
    async fn get_transaction_status(&self, _tx_hash: &str) -> Result<String> {
        Err(anyhow!("get_transaction_status not implemented"))
    }
}

/// Monitor a bridge transaction
///
/// Helper function to monitor a transaction across chains via BridgeProvider
///
/// # Arguments
/// * `provider` - BridgeProvider trait object reference
/// * `tx_hash` - Transaction hash to monitor
/// * `config` - Optional monitoring configuration
///
/// # Returns
/// The latest bridge status of the transaction
pub async fn monitor_transaction_with_config(
    provider: &dyn BridgeProvider, 
    tx_hash: &str,
    config: Option<MonitorConfig>
) -> Result<BridgeStatus> {
    // Call the provider's get_transaction_status method
    provider.get_transaction_status(tx_hash)
        .await
        .context("Failed to get transaction status from bridge provider")
} 