//! Bridge provider traits and interfaces
//!
//! This module defines standard traits for bridge providers, clients, and monitoring
//! systems used across the bridge ecosystem.

use async_trait::async_trait;
use anyhow::Result;

use super::chain::Chain;
use super::status::BridgeStatus;
use super::types::FeeEstimate;
use super::transaction::BridgeTransaction;

/// Bridge provider trait for implementing different bridge technologies
#[async_trait]
pub trait BridgeProvider: Send + Sync + 'static {
    /// Send cross-chain message or token
    /// 
    /// # Arguments
    /// * `from_chain` - Source chain
    /// * `to_chain` - Target chain
    /// * `receiver` - Receiver address on target chain
    /// * `payload` - Data payload to send
    /// 
    /// # Returns
    /// Transaction hash as a string if successful
    async fn send_message(&self, 
                         from_chain: Chain, 
                         to_chain: Chain, 
                         receiver: String, 
                         payload: Vec<u8>) -> Result<String>;
                         
    /// Get transaction status
    /// 
    /// # Arguments
    /// * `tx_hash` - Transaction hash to check
    /// 
    /// # Returns
    /// Current bridge status of the transaction
    async fn get_transaction_status(&self, tx_hash: &str) -> Result<BridgeStatus>;
    
    /// Estimate cross-chain transfer fee
    /// 
    /// # Arguments
    /// * `from_chain` - Source chain
    /// * `to_chain` - Target chain
    /// * `payload_size` - Size of payload in bytes
    /// 
    /// # Returns
    /// Fee estimate including amount, token and USD value
    async fn estimate_fee(&self, 
                         from_chain: Chain, 
                         to_chain: Chain, 
                         payload_size: usize) -> Result<FeeEstimate>;
                         
    /// Check if provider supports a specific chain
    /// 
    /// # Arguments
    /// * `chain` - Chain to check support for
    /// 
    /// # Returns
    /// True if the chain is supported by this provider
    fn supports_chain(&self, chain: Chain) -> bool;
    
    /// Get provider name
    /// 
    /// # Returns
    /// String identifier for this provider
    fn provider_name(&self) -> &str;
}

/// Bridge client trait for API communication
#[async_trait]
pub trait BridgeClient: Send + Sync + 'static {
    /// Get transaction status
    /// 
    /// # Arguments
    /// * `tx_hash` - Transaction hash to check
    /// 
    /// # Returns
    /// Current bridge status
    async fn get_bridge_status(&self, tx_hash: &str) -> Result<BridgeStatus>;
    
    /// Estimate fee for bridge transaction
    /// 
    /// # Arguments
    /// * `source_chain` - Source blockchain
    /// * `target_chain` - Target blockchain
    /// * `token_id` - Token ID or address (as string)
    /// * `amount` - Amount to transfer (as string)
    /// 
    /// # Returns
    /// Fee estimate
    async fn estimate_fee(&self, 
                         source_chain: Chain, 
                         target_chain: Chain, 
                         token_id: &str, 
                         amount: &str) -> Result<FeeEstimate>;
                         
    /// Retry a failed relay
    /// 
    /// # Arguments
    /// * `tx_hash` - Transaction hash to retry
    async fn retry_relay(&self, tx_hash: &str) -> Result<()>;
    
    /// Bridge tokens between chains
    /// 
    /// # Arguments
    /// * `source_chain` - Source blockchain
    /// * `target_chain` - Target blockchain
    /// * `token_id` - Token ID or address
    /// * `amount` - Amount to transfer
    /// * `sender` - Sender address
    /// * `receiver` - Receiver address
    /// 
    /// # Returns
    /// Bridge transaction details
    async fn bridge_tokens(&self,
                          source_chain: Chain,
                          target_chain: Chain,
                          token_id: &str,
                          amount: &str,
                          sender: &str,
                          receiver: &str) -> Result<BridgeTransaction>;
}

/// Bridge transaction monitor trait
#[async_trait]
pub trait BridgeMonitor: Send + Sync + 'static {
    /// Monitor a transaction until completion or failure
    /// 
    /// # Arguments
    /// * `tx_hash` - Transaction hash to monitor
    /// * `source_chain` - Source chain
    /// * `target_chain` - Target chain
    /// 
    /// # Returns
    /// Final transaction status
    async fn monitor_transaction(&self, 
                                tx_hash: &str, 
                                source_chain: Chain, 
                                target_chain: Chain) -> Result<BridgeStatus>;
                                
    /// Get all transactions for a given address
    /// 
    /// # Arguments
    /// * `address` - Address to query
    /// * `limit` - Maximum number of transactions to return
    /// 
    /// # Returns
    /// List of transactions
    async fn get_address_transactions(&self, address: &str, limit: usize) -> Result<Vec<BridgeTransaction>>;
    
    /// Get transaction details
    /// 
    /// # Arguments
    /// * `tx_hash` - Transaction hash
    /// 
    /// # Returns
    /// Transaction details if found
    async fn get_transaction(&self, tx_hash: &str) -> Result<Option<BridgeTransaction>>;
}

/// Bridge adapter trait for blockchain interaction
#[async_trait]
pub trait BridgeAdapter: Send + Sync + 'static {
    /// Register a new bridge provider
    /// 
    /// # Arguments
    /// * `provider_name` - Provider identifier
    /// * `provider` - Bridge provider implementation
    async fn register_provider(&self, provider_name: &str, provider: Box<dyn BridgeProvider>) -> Result<()>;
    
    /// Get a provider by name
    /// 
    /// # Arguments
    /// * `provider_name` - Provider identifier
    /// 
    /// # Returns
    /// Reference to the provider if found
    async fn get_provider(&self, provider_name: &str) -> Result<Box<dyn BridgeProvider>>;
    
    /// Bridge tokens between chains
    /// 
    /// # Arguments
    /// * `provider_name` - Provider to use
    /// * `source_chain` - Source chain
    /// * `target_chain` - Target chain
    /// * `token_address` - Token address or ID
    /// * `amount` - Amount to transfer
    /// * `receiver` - Receiver address
    /// 
    /// # Returns
    /// Bridge transaction
    async fn bridge_tokens(&self,
                          provider_name: &str,
                          source_chain: Chain,
                          target_chain: Chain,
                          token_address: &str,
                          amount: &str,
                          receiver: &str) -> Result<BridgeTransaction>;
                          
    /// Get list of supported chains
    fn get_supported_chains(&self) -> Vec<Chain>;
    
    /// Check if a chain is supported
    fn is_chain_supported(&self, chain: Chain) -> bool;
} 