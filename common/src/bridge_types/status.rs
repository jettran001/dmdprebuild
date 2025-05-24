//! Bridge transaction status types
//!
//! This module defines the `BridgeStatus` enum representing the status of a bridge transaction
//! across chains, along with helper methods for status handling and conversion.

use serde::{Serialize, Deserialize};
use std::fmt;

/// Status of a bridge transaction
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BridgeStatus {
    /// Transaction is pending
    Pending,
    /// Transaction is confirmed on source chain but not yet completed on target chain
    Confirmed,
    /// Transaction failed with error message
    Failed(String),
    /// Transaction completed successfully on both chains
    Completed,
}

impl BridgeStatus {
    /// Check if the transaction status is in a terminal state (Completed or Failed)
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed(_))
    }

    /// Check if the transaction was successful
    pub fn is_successful(&self) -> bool {
        matches!(self, Self::Completed)
    }
    
    /// Get failure reason if available
    pub fn failure_reason(&self) -> Option<&str> {
        match self {
            Self::Failed(reason) => Some(reason),
            _ => None,
        }
    }
    
    /// Convert from string representation
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "pending" => Some(Self::Pending),
            "confirmed" => Some(Self::Confirmed),
            "completed" => Some(Self::Completed),
            s if s.starts_with("failed:") => {
                let reason = s[7..].trim().to_string();
                Some(Self::Failed(reason))
            }
            _ => None,
        }
    }
}

impl fmt::Display for BridgeStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "Pending"),
            Self::Confirmed => write!(f, "Confirmed"),
            Self::Completed => write!(f, "Completed"),
            Self::Failed(reason) => write!(f, "Failed: {}", reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_status_properties() {
        assert!(BridgeStatus::Completed.is_terminal());
        assert!(BridgeStatus::Failed("test".into()).is_terminal());
        assert!(!BridgeStatus::Pending.is_terminal());
        assert!(!BridgeStatus::Confirmed.is_terminal());
        
        assert!(BridgeStatus::Completed.is_successful());
        assert!(!BridgeStatus::Failed("test".into()).is_successful());
        
        assert_eq!(
            BridgeStatus::Failed("test reason".into()).failure_reason(),
            Some("test reason")
        );
        assert_eq!(BridgeStatus::Completed.failure_reason(), None);
    }
    
    #[test]
    fn test_status_display() {
        assert_eq!(BridgeStatus::Pending.to_string(), "Pending");
        assert_eq!(BridgeStatus::Completed.to_string(), "Completed");
        assert_eq!(
            BridgeStatus::Failed("error message".into()).to_string(),
            "Failed: error message"
        );
    }
} 