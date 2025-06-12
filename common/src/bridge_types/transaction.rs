//! Bridge transaction data structures
//!
//! This module defines the `BridgeTransaction` struct and related types for handling
//! cross-chain bridge transactions and transfers.

use serde::{Serialize, Deserialize};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use super::chain::Chain;
use super::status::BridgeStatus;

/// Bridge transaction information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeTransaction {
    /// Transaction hash
    pub tx_hash: String,
    
    /// Source blockchain
    pub source_chain: Chain,
    
    /// Target blockchain
    pub target_chain: Chain,
    
    /// Sender address
    pub sender: String,
    
    /// Receiver address
    pub receiver: String,
    
    /// Amount being transferred (as string to ensure compatibility with different blockchains)
    pub amount: String,
    
    /// Token ID or address (as string to ensure compatibility with different blockchains)
    pub token_id: String,
    
    /// Current transaction status
    pub status: BridgeStatus,
    
    /// Transaction timestamp (UNIX timestamp in seconds)
    pub timestamp: u64,
}

impl BridgeTransaction {
    /// Create a new bridge transaction
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tx_hash: String,
        source_chain: Chain,
        target_chain: Chain,
        sender: String,
        receiver: String,
        amount: String,
        token_id: String,
        status: BridgeStatus,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
            
        Self {
            tx_hash,
            source_chain,
            target_chain,
            sender,
            receiver,
            amount,
            token_id,
            status,
            timestamp,
        }
    }
    
    /// Create a new bridge transaction with a specific timestamp
    #[allow(clippy::too_many_arguments)]
    pub fn with_timestamp(
        tx_hash: String,
        source_chain: Chain,
        target_chain: Chain,
        sender: String,
        receiver: String,
        amount: String,
        token_id: String,
        status: BridgeStatus,
        timestamp: u64,
    ) -> Self {
        Self {
            tx_hash,
            source_chain,
            target_chain,
            sender,
            receiver,
            amount,
            token_id,
            status,
            timestamp,
        }
    }
    
    /// Create a transaction in pending status
    #[allow(clippy::too_many_arguments)]
    pub fn pending(
        tx_hash: String,
        source_chain: Chain,
        target_chain: Chain,
        sender: String,
        receiver: String,
        amount: String,
        token_id: String,
    ) -> Self {
        Self::new(
            tx_hash,
            source_chain,
            target_chain,
            sender,
            receiver,
            amount,
            token_id,
            BridgeStatus::Pending,
        )
    }
    
    /// Calculate how long the transaction has been in the current status
    pub fn time_in_status(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
            
        now.saturating_sub(self.timestamp)
    }
    
    /// Update the transaction status
    pub fn update_status(&mut self, new_status: BridgeStatus) {
        // Update timestamp when status changes
        if self.status != new_status {
            self.status = new_status;
            self.timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_secs();
        }
    }
    
    /// Check if transaction has been in its current status for too long
    pub fn is_stuck(&self, timeout_seconds: u64) -> bool {
        if self.status.is_terminal() {
            return false;  // Terminal statuses are never considered stuck
        }
        
        self.time_in_status() > timeout_seconds
    }
    
    /// Get unique identifier for this transaction
    pub fn id(&self) -> String {
        format!(
            "{}_{}_{}_{}",
            self.tx_hash,
            self.source_chain,
            self.target_chain,
            self.timestamp
        )
    }
}

impl fmt::Display for BridgeTransaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Bridge [{}] {} -> {}, Status: {}, Amount: {} [{}], Sender: {}, Receiver: {}",
            self.tx_hash,
            self.source_chain,
            self.target_chain,
            self.status,
            self.amount,
            self.token_id,
            self.sender,
            self.receiver
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn create_test_transaction() -> BridgeTransaction {
        BridgeTransaction::new(
            "0x1234567890abcdef".to_string(),
            Chain::Ethereum,
            Chain::BSC,
            "0xSender".to_string(),
            "0xReceiver".to_string(),
            "1.5".to_string(),
            "0xTokenAddress".to_string(),
            BridgeStatus::Pending,
        )
    }
    
    #[test]
    fn test_transaction_creation() {
        let tx = create_test_transaction();
        
        assert_eq!(tx.tx_hash, "0x1234567890abcdef");
        assert_eq!(tx.source_chain, Chain::Ethereum);
        assert_eq!(tx.target_chain, Chain::BSC);
        assert_eq!(tx.status, BridgeStatus::Pending);
        assert_eq!(tx.amount, "1.5");
        assert_eq!(tx.token_id, "0xTokenAddress");
    }
    
    #[test]
    fn test_status_update() {
        let mut tx = create_test_transaction();
        let original_timestamp = tx.timestamp;
        
        // Wait a moment to ensure timestamp changes
        std::thread::sleep(std::time::Duration::from_millis(10));
        
        tx.update_status(BridgeStatus::Confirmed);
        assert_eq!(tx.status, BridgeStatus::Confirmed);
        assert!(tx.timestamp > original_timestamp);
    }
    
    #[test]
    fn test_is_stuck() {
        let tx = BridgeTransaction::with_timestamp(
            "0x1234567890abcdef".to_string(),
            Chain::Ethereum,
            Chain::BSC,
            "0xSender".to_string(),
            "0xReceiver".to_string(),
            "1.5".to_string(),
            "0xTokenAddress".to_string(),
            BridgeStatus::Pending,
            // Set timestamp to 100 seconds ago
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_secs() - 100,
        );
        
        assert!(!tx.is_stuck(120)); // Not stuck yet (< 120s)
        assert!(tx.is_stuck(50));   // Stuck (> 50s)
    }
} 