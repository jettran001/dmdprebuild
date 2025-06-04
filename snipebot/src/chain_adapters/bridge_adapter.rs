//! Bridge adapter for cross-chain operations
//!
//! This adapter provides functionality to interact with various blockchain bridges
//! including LayerZero and Wormhole, enabling cross-chain token transfers and
//! message passing between different blockchains.
//!
//! Important: Cập nhật 2024-10-17 - Đã sửa import để sử dụng common::bridge_types
//! thay vì crate::bridge_types để thống nhất với mod.rs

use std::sync::Arc;
use anyhow::{Result, bail, Context};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use chrono::Utc;
use async_trait::async_trait;
use reqwest;
use std::time::Duration;

// Import from common bridge types
use common::bridge_types::{
    Chain,
    BridgeStatus,
    FeeEstimate,
    BridgeTransaction,
    MonitorConfig,
    BridgeProvider,
    BridgeAdapter as BridgeAdapterTrait,
    monitor_transaction,
};

/// LayerZero bridge provider implementation
pub struct LayerZeroBridge {
    /// Base URL for the bridge service
    base_url: String,
    /// HTTP client for making API calls
    client: reqwest::Client,
    /// Provider name
    name: String,
}

impl LayerZeroBridge {
    /// Create a new LayerZero bridge provider
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
            name: "LayerZero".to_string(),
        }
    }
}

#[async_trait]
impl BridgeProvider for LayerZeroBridge {
    async fn send_message(&self, 
                         from_chain: Chain, 
                         to_chain: Chain, 
                         receiver: String, 
                         payload: Vec<u8>) -> Result<String> {
        // Currently a placeholder implementation
        warn!("LayerZero send_message is a placeholder implementation");
        
        // In a real implementation, we would call the LayerZero API
        let url = format!("{}/bridge/relay", self.base_url);
        let request = serde_json::json!({
            "source_chain": from_chain.to_layerzero_id(),
            "target_chain": to_chain.to_layerzero_id(),
            "receiver": receiver,
            "payload": hex::encode(payload),
        });
        
        // Simulated success response
        Ok("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string())
    }
    
    async fn get_transaction_status(&self, tx_hash: &str) -> Result<BridgeStatus> {
        // Currently a placeholder implementation
        warn!("LayerZero get_transaction_status is a placeholder implementation");
        
        // In a real implementation, we would call the LayerZero API
        let url = format!("{}/bridge/status/{}", self.base_url, tx_hash);
        
        // Simulated success response
        Ok(BridgeStatus::Pending)
    }
    
    async fn estimate_fee(&self, 
                         from_chain: Chain, 
                         to_chain: Chain, 
                         payload_size: usize) -> Result<FeeEstimate> {
        // Currently a placeholder implementation
        warn!("LayerZero estimate_fee is a placeholder implementation");
        
        // In a real implementation, we would call the LayerZero API
        let url = format!("{}/bridge/estimate-fee", self.base_url);
        let params = &[
            ("source_chain", from_chain.to_layerzero_id().to_string()),
            ("target_chain", to_chain.to_layerzero_id().to_string()),
            ("payload_size", payload_size.to_string()),
        ];
        
        // Simulated fee estimate
        let fee_amount = match from_chain {
            Chain::Ethereum => "0.001",
            Chain::BSC => "0.001",
            Chain::Polygon => "0.01",
            Chain::Avalanche => "0.01",
            _ => "0.001",
        };
        
        let fee_token = match from_chain {
            Chain::Ethereum => "ETH",
            Chain::BSC => "BNB",
            Chain::Polygon => "MATIC",
            Chain::Avalanche => "AVAX",
            Chain::NEAR => "NEAR",
            Chain::Solana => "SOL",
        };
        
        Ok(FeeEstimate::new(
            fee_amount.to_string(), 
            fee_token.to_string(), 
            1.0 // Placeholder USD value
        ))
    }
    
    fn supports_chain(&self, chain: Chain) -> bool {
        chain.is_layerzero_supported()
    }
    
    fn provider_name(&self) -> &str {
        &self.name
    }
}

/// Wormhole bridge provider implementation
pub struct WormholeBridge {
    /// Base URL for the bridge service
    base_url: String,
    /// HTTP client for making API calls
    client: reqwest::Client,
    /// Provider name
    name: String,
}

impl WormholeBridge {
    /// Create a new Wormhole bridge provider
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
            name: "Wormhole".to_string(),
        }
    }
}

#[async_trait]
impl BridgeProvider for WormholeBridge {
    async fn send_message(&self, 
                         from_chain: Chain, 
                         to_chain: Chain, 
                         receiver: String, 
                         payload: Vec<u8>) -> Result<String> {
        // Currently a placeholder implementation
        warn!("Wormhole send_message is a placeholder implementation");
        
        // In a real implementation, we would call the Wormhole API
        let url = format!("{}/wormhole/send", self.base_url);
        let request = serde_json::json!({
            "source_chain": from_chain.as_str(),
            "target_chain": to_chain.as_str(),
            "receiver": receiver,
            "payload": hex::encode(payload),
        });
        
        // Simulated success response
        Ok("0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string())
    }
    
    async fn get_transaction_status(&self, tx_hash: &str) -> Result<BridgeStatus> {
        // Currently a placeholder implementation
        warn!("Wormhole get_transaction_status is a placeholder implementation");
        
        // In a real implementation, we would call the Wormhole API
        let url = format!("{}/wormhole/status/{}", self.base_url, tx_hash);
        
        // Simulated success response
        Ok(BridgeStatus::Confirmed)
    }
    
    async fn estimate_fee(&self, 
                         from_chain: Chain, 
                         to_chain: Chain, 
                         payload_size: usize) -> Result<FeeEstimate> {
        // Currently a placeholder implementation
        warn!("Wormhole estimate_fee is a placeholder implementation");
        
        // In a real implementation, we would call the Wormhole API
        let url = format!("{}/wormhole/estimate-fee", self.base_url);
        let params = &[
            ("source_chain", from_chain.as_str()),
            ("target_chain", to_chain.as_str()),
            ("payload_size", payload_size.to_string()),
        ];
        
        // Simulated fee estimate
        let fee_amount = match from_chain {
            Chain::Solana => "0.00001",
            _ => "0.001",
        };
        
        let fee_token = match from_chain {
            Chain::Ethereum => "ETH",
            Chain::BSC => "BNB",
            Chain::Polygon => "MATIC",
            Chain::Avalanche => "AVAX",
            Chain::NEAR => "NEAR",
            Chain::Solana => "SOL",
        };
        
        Ok(FeeEstimate::new(
            fee_amount.to_string(), 
            fee_token.to_string(), 
            0.5 // Placeholder USD value
        ))
    }
    
    fn supports_chain(&self, chain: Chain) -> bool {
        chain.is_wormhole_supported()
    }
    
    fn provider_name(&self) -> &str {
        &self.name
    }
}

/// SnipeBot implementation of the common BridgeAdapter 
/// Implements both crate::bridge_types::BridgeAdapter trait and 
/// additional functionality specific to SnipeBot
pub struct BridgeAdapter {
    /// Chain ID used for internal referencing
    chain_id: u32,
    /// Available bridge providers
    providers: RwLock<Vec<(String, Arc<dyn BridgeProvider>)>>,
    /// Recent transactions
    recent_txs: RwLock<Vec<BridgeTransaction>>,
    /// Maximum number of recent transactions to keep
    max_recent_txs: usize,
}

/// Implement the common BridgeAdapter trait
impl BridgeAdapterTrait for BridgeAdapter {
    async fn register_provider(&self, provider_name: &str, provider: Box<dyn BridgeProvider>) -> Result<()> {
        // Chuyển đổi Box<dyn BridgeProvider> thành Arc<dyn BridgeProvider>
        let arc_provider = Arc::from(provider);
        
        // Sử dụng phương thức nội bộ để đăng ký provider
        let mut providers = self.providers.write().await;
        providers.push((provider_name.to_string(), arc_provider));
        info!("Registered bridge provider: {}", provider_name);
        Ok(())
    }
    
    async fn get_provider(&self, provider_name: &str) -> Result<Box<dyn BridgeProvider>> {
        let providers = self.providers.read().await;
        
        for (name, provider) in providers.iter() {
            if name == provider_name {
                // Quản lý lại Arc để trả về Box
                return Ok(Box::new(provider.clone()));
            }
        }
        
        bail!("Bridge provider not found: {}", provider_name)
    }
    
    async fn bridge_tokens(
        &self,
        provider_name: &str,
        source_chain: Chain,
        target_chain: Chain,
        token_address: &str,
        amount: &str,
        receiver: &str
    ) -> Result<BridgeTransaction> {
        self.bridge_tokens_impl(provider_name, source_chain, target_chain, token_address, amount, receiver).await
    }
    
    fn get_supported_chains(&self) -> Vec<Chain> {
        vec![
            Chain::Ethereum,
            Chain::BSC,
            Chain::Polygon,
            Chain::Avalanche,
            Chain::NEAR,
            Chain::Solana,
        ]
    }
    
    fn is_chain_supported(&self, chain: Chain) -> bool {
        self.get_supported_chains().contains(&chain)
    }
}

impl BridgeAdapter {
    /// Create a new bridge adapter
    pub fn new(chain_id: u32) -> Self {
        Self {
            chain_id,
            providers: RwLock::new(Vec::new()),
            recent_txs: RwLock::new(Vec::new()),
            max_recent_txs: 100,
        }
    }
    
    /// Initialize the bridge adapter
    pub async fn init(&self) -> Result<()> {
        info!("Initializing bridge adapter for chain ID {}", self.chain_id);
        
        // In a real implementation, we would initialize connections to bridge providers
        // For now, this is a placeholder
        
        info!("Bridge adapter initialized for chain ID {}", self.chain_id);
        Ok(())
    }
    
    /// Register a bridge provider (compatibility wrapper for existing code)
    /// 
    /// Deprecated: Use register_provider from CommonBridgeAdapter trait instead
    #[deprecated(note = "Use register_provider from CommonBridgeAdapter trait instead")]
    pub async fn register_provider_legacy(&self, name: &str, provider: Arc<dyn BridgeProvider>) -> Result<()> {
        let mut providers = self.providers.write().await;
        providers.push((name.to_string(), provider));
        info!("Registered bridge provider: {}", name);
        Ok(())
    }
    
    /// Get a specific bridge provider by name (compatibility wrapper for existing code)
    /// 
    /// Deprecated: Use get_provider from CommonBridgeAdapter trait instead
    #[deprecated(note = "Use get_provider from CommonBridgeAdapter trait instead")]
    pub async fn get_provider_legacy(&self, name: &str) -> Result<Arc<dyn BridgeProvider>> {
        let providers = self.providers.read().await;
        
        for (provider_name, provider) in providers.iter() {
            if provider_name == name {
                return Ok(provider.clone());
            }
        }
        
        bail!("Bridge provider not found: {}", name)
    }
    
    /// Bridge tokens between chains using the specified provider (implementation)
    /// Used by both the legacy API and the new CommonBridgeAdapter trait
    async fn bridge_tokens_impl(&self, 
                             provider_name: &str,
                             source_chain: Chain, 
                             target_chain: Chain,
                             token_address: &str,
                             amount: &str,
                             receiver: &str) -> Result<BridgeTransaction> {
        // Validate inputs
        if provider_name.is_empty() {
            bail!("Provider name cannot be empty");
        }
        
        if token_address.is_empty() {
            bail!("Token address cannot be empty");
        }
        
        if amount.is_empty() {
            bail!("Amount cannot be empty");
        }
        
        // Validate amount format (ensure it's numeric)
        if amount.parse::<f64>().is_err() {
            bail!("Invalid amount format: {}", amount);
        }
        
        if receiver.is_empty() {
            bail!("Receiver address cannot be empty");
        }
        
        // Check if provider exists
        let provider = self.get_provider_legacy(provider_name).await
            .context(format!("Failed to get provider: {}", provider_name))?;
        
        // Check compatibility between chains and provider
        if provider_name.to_lowercase() == "layerzero" && !source_chain.is_layerzero_supported() {
            bail!("Source chain {} is not supported by LayerZero", source_chain);
        }
        
        if provider_name.to_lowercase() == "layerzero" && !target_chain.is_layerzero_supported() {
            bail!("Target chain {} is not supported by LayerZero", target_chain);
        }
        
        if provider_name.to_lowercase() == "wormhole" && !source_chain.is_wormhole_supported() {
            bail!("Source chain {} is not supported by Wormhole", source_chain);
        }
        
        if provider_name.to_lowercase() == "wormhole" && !target_chain.is_wormhole_supported() {
            bail!("Target chain {} is not supported by Wormhole", target_chain);
        }
        
        // Prepare the payload for token bridge
        let payload = self.prepare_token_payload(token_address, amount, receiver)
            .context("Failed to prepare token payload")?;
        
        // Send the bridge transaction
        let tx_hash = provider.send_message(
            source_chain,
            target_chain,
            receiver.to_string(),
            payload
        ).await
        .context("Failed to send bridge message")?;
        
        // Create transaction record
        let tx = BridgeTransaction::new(
            tx_hash.clone(),
            source_chain,
            target_chain,
            "self".to_string(), // In a real implementation, this would be the actual sender
            receiver.to_string(),
            amount.to_string(),
            token_address.to_string(),
            BridgeStatus::Pending,
        );
        
        // Store in recent transactions
        if let Err(e) = self.store_transaction(&tx).await {
            warn!("Failed to store transaction: {}", e);
            // Continue even if storage fails
        }
        
        info!(
            "Initiated token bridge from {} to {}, token: {}, amount: {}, tx: {}", 
            source_chain, target_chain, token_address, amount, tx_hash
        );
        
        Ok(tx)
    }
    
    /// Bridge tokens between chains using the specified provider (legacy API)
    ///
    /// # Arguments
    /// * `provider_name` - Name of the bridge provider to use
    /// * `source_chain` - Source blockchain
    /// * `target_chain` - Target blockchain
    /// * `token_address` - Token address or ID
    /// * `amount` - Amount to transfer (as string)
    /// * `receiver` - Receiver address on target chain
    /// 
    /// # Returns
    /// Bridge transaction details
    /// 
    /// Deprecated: Use bridge_tokens from CommonBridgeAdapter trait instead
    #[deprecated(note = "Use bridge_tokens from CommonBridgeAdapter trait instead")]
    pub async fn bridge_tokens(&self, 
                             provider_name: &str,
                             source_chain: Chain, 
                             target_chain: Chain,
                             token_address: &str,
                             amount: &str,
                             receiver: &str) -> Result<BridgeTransaction> {
        self.bridge_tokens_impl(provider_name, source_chain, target_chain, token_address, amount, receiver).await
    }
    
    /// Get status of a bridge transaction
    /// 
    /// # Arguments
    /// * `provider_name` - Name of the bridge provider
    /// * `tx_hash` - Transaction hash to check
    /// 
    /// # Returns
    /// Current bridge status of the transaction
    pub async fn get_transaction_status(&self, provider_name: &str, tx_hash: &str) -> Result<BridgeStatus> {
        // Validate inputs
        if provider_name.is_empty() {
            bail!("Provider name cannot be empty");
        }
        
        if tx_hash.is_empty() {
            bail!("Transaction hash cannot be empty");
        }
        
        // Check if provider exists
        let provider = self.get_provider_legacy(provider_name).await
            .context(format!("Failed to get provider: {}", provider_name))?;
        
        // Get status from provider
        let status = provider.get_transaction_status(tx_hash).await
            .context(format!("Failed to get transaction status for tx: {}", tx_hash))?;
        
        // Update status in our transaction store
        if let Err(e) = self.update_transaction_status(tx_hash, status.clone()).await {
            warn!("Failed to update transaction status: {}", e);
            // Continue monitoring even if update fails
        }
        
        Ok(status)
    }
    
    /// Estimate fee for bridge transaction
    /// 
    /// # Arguments
    /// * `provider_name` - Name of the bridge provider
    /// * `source_chain` - Source blockchain
    /// * `target_chain` - Target blockchain
    /// * `token_address` - Token address or ID
    /// * `amount` - Amount to transfer (as string)
    /// 
    /// # Returns
    /// Fee estimate including amount, token and USD value
    pub async fn estimate_bridge_fee(&self,
                                    provider_name: &str,
                                    source_chain: Chain,
                                    target_chain: Chain,
                                    token_address: &str,
                                    amount: &str) -> Result<FeeEstimate> {
        // Validate inputs
        if provider_name.is_empty() {
            bail!("Provider name cannot be empty");
        }
        
        if token_address.is_empty() {
            bail!("Token address cannot be empty");
        }
        
        if amount.is_empty() {
            bail!("Amount cannot be empty");
        }
        
        // Check if provider exists
        let provider = self.get_provider_legacy(provider_name).await
            .context(format!("Failed to get provider: {}", provider_name))?;
        
        // Check compatibility between chains
        if !provider.supports_chain(source_chain) {
            bail!("Source chain {} is not supported by provider {}", 
                 source_chain, provider_name);
        }
        
        if !provider.supports_chain(target_chain) {
            bail!("Target chain {} is not supported by provider {}", 
                 target_chain, provider_name);
        }
        
        // Use dummy receiver address for fee estimation
        let dummy_receiver = "0x0000000000000000000000000000000000000000";
        
        // Prepare the payload
        let payload = self.prepare_token_payload(token_address, amount, dummy_receiver)
            .context("Failed to prepare token payload for fee estimation")?;
        
        // Get fee estimate from provider
        let fee = provider.estimate_fee(source_chain, target_chain, payload.len()).await
            .context("Failed to estimate bridge fee")?;
            
        Ok(fee)
    }
    
    /// Monitor a bridge transaction with timeout and retries
    /// 
    /// # Arguments
    /// * `provider_name` - Name of the bridge provider
    /// * `tx_hash` - Transaction hash to monitor
    /// * `config` - Optional monitoring configuration
    /// 
    /// # Returns
    /// Final transaction status or error
    pub async fn monitor_transaction(&self, 
                                   provider_name: &str, 
                                   tx_hash: &str,
                                   config: Option<MonitorConfig>) -> Result<BridgeStatus> {
        // Validate inputs
        if provider_name.is_empty() {
            bail!("Provider name cannot be empty");
        }
        
        if tx_hash.is_empty() {
            bail!("Transaction hash cannot be empty");
        }
        
        // Get the provider
        let provider = self.get_provider_legacy(provider_name).await
            .context(format!("Failed to get provider: {}", provider_name))?;
            
        // Use the common monitor_transaction function from bridge_types
        let result = monitor_transaction(&*provider, tx_hash, config).await;
        
        // If monitoring was successful, update the transaction status
        if let Ok(status) = &result {
            if let Err(e) = self.update_transaction_status(tx_hash, status.clone()).await {
                warn!("Failed to update transaction status: {}", e);
                // Continue monitoring even if update fails
            }
        }
        
        result
    }
    
    /// Get recent transactions
    pub async fn get_recent_transactions(&self, limit: usize) -> Vec<BridgeTransaction> {
        let txs = self.recent_txs.read().await;
        let limit = std::cmp::min(limit, txs.len());
        txs.iter().take(limit).cloned().collect()
    }
    
    /// Internal helper to prepare token payload
    fn prepare_token_payload(&self, token_address: &str, amount: &str, receiver: &str) -> Result<Vec<u8>> {
        // In a real implementation, this would create the actual payload format
        // needed by the bridge protocol
        let payload = format!("{}:{}:{}", token_address, amount, receiver);
        Ok(payload.into_bytes())
    }
    
    /// Store transaction in recent transactions list
    async fn store_transaction(&self, tx: &BridgeTransaction) -> Result<()> {
        let mut txs = self.recent_txs.write().await;
        
        // Add new transaction
        txs.push(tx.clone());
        
        // Trim if exceeds max size
        if txs.len() > self.max_recent_txs {
            *txs = txs.iter().skip(txs.len() - self.max_recent_txs).cloned().collect();
        }
        
        Ok(())
    }
    
    /// Update transaction status
    async fn update_transaction_status(&self, tx_hash: &str, status: BridgeStatus) -> Result<()> {
        let mut txs = self.recent_txs.write().await;
        
        for tx in txs.iter_mut() {
            if tx.tx_hash == tx_hash {
                tx.update_status(status);
                return Ok(());
            }
        }
        
        bail!("Transaction not found in recent transactions: {}", tx_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_bridge_adapter_initialization() {
        let adapter = BridgeAdapter::new(1);
        assert_eq!(adapter.chain_id, 1);
        
        let result = adapter.init().await;
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_chain_conversion() {
        assert_eq!(Chain::Ethereum.to_layerzero_id(), 1);
        assert_eq!(Chain::BSC.to_layerzero_id(), 2);
        
        assert!(Chain::Ethereum.is_layerzero_supported());
        assert!(!Chain::Solana.is_layerzero_supported());
        
        assert!(Chain::Ethereum.is_wormhole_supported());
        assert!(!Chain::NEAR.is_wormhole_supported());
    }
} 