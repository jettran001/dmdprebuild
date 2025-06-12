//! Chain enum and related functionality
//!
//! This module defines the `Chain` enum representing different blockchain networks
//! supported by the bridge system, along with methods to convert between different
//! chain ID formats and check compatibility.

use std::fmt;
use std::str::FromStr;
use serde::{Serialize, Deserialize};

/// Supported blockchain networks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Chain {
    /// Binance Smart Chain
    BSC,
    /// NEAR Protocol
    NEAR,
    /// Solana
    Solana,
    /// Ethereum
    Ethereum,
    /// Polygon
    Polygon,
    /// Avalanche
    Avalanche,
}

impl Chain {
    /// Convert to LayerZero chain ID
    pub fn to_layerzero_id(&self) -> u16 {
        match self {
            Chain::BSC => 2,         // BSC in LayerZero
            Chain::NEAR => 115,      // NEAR in LayerZero
            Chain::Ethereum => 1,    // Ethereum in LayerZero
            Chain::Polygon => 4,     // Polygon in LayerZero
            Chain::Avalanche => 3,   // Avalanche in LayerZero
            Chain::Solana => 0,      // Solana doesn't use LayerZero but uses Wormhole
        }
    }
    
    /// Get string representation of the chain
    pub fn as_str(&self) -> &'static str {
        match self {
            Chain::BSC => "bsc",
            Chain::NEAR => "near",
            Chain::Solana => "solana",
            Chain::Ethereum => "ethereum",
            Chain::Polygon => "polygon",
            Chain::Avalanche => "avalanche",
        }
    }
    
    /// Convert from LayerZero chain ID to Chain
    pub fn from_layerzero_id(id: u16) -> Option<Self> {
        match id {
            1 => Some(Chain::Ethereum),
            2 => Some(Chain::BSC),
            3 => Some(Chain::Avalanche),
            4 => Some(Chain::Polygon),
            115 => Some(Chain::NEAR),
            _ => None,
        }
    }
    
    /// Check if chain is supported by LayerZero
    pub fn is_layerzero_supported(&self) -> bool {
        match self {
            Chain::Solana => false,  // Solana doesn't support LayerZero
            _ => true,              // Other chains support it
        }
    }
    
    /// Check if chain is supported by Wormhole
    pub fn is_wormhole_supported(&self) -> bool {
        match self {
            Chain::NEAR => false,    // NEAR doesn't support Wormhole
            _ => true,              // Other chains support it
        }
    }
    
    /// Get a list of all supported chains
    pub fn supported_chains() -> Vec<Self> {
        vec![
            Self::BSC,
            Self::NEAR,
            Self::Solana,
            Self::Ethereum,
            Self::Polygon,
            Self::Avalanche
        ]
    }
    
    /// DEPRECATED: Use `str::parse()` or `FromStr::from_str()` instead
    #[deprecated(since = "0.1.0", note = "Use `str::parse()` or `FromStr::from_str()` instead")]
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "bsc" => Some(Self::BSC),
            "near" => Some(Self::NEAR),
            "solana" => Some(Self::Solana),
            "ethereum" => Some(Self::Ethereum),
            "polygon" => Some(Self::Polygon),
            "avalanche" => Some(Self::Avalanche),
            _ => None,
        }
    }
}

impl fmt::Display for Chain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for Chain {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "bsc" => Ok(Self::BSC),
            "near" => Ok(Self::NEAR),
            "solana" => Ok(Self::Solana),
            "ethereum" => Ok(Self::Ethereum),
            "polygon" => Ok(Self::Polygon),
            "avalanche" => Ok(Self::Avalanche),
            _ => Err(format!("Unknown chain: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_chain_conversions() {
        // Test LayerZero conversions
        assert_eq!(Chain::Ethereum.to_layerzero_id(), 1);
        assert_eq!(Chain::BSC.to_layerzero_id(), 2);
        assert_eq!(Chain::from_layerzero_id(1), Some(Chain::Ethereum));
        assert_eq!(Chain::from_layerzero_id(999), None);
        
        // Test string conversions
        assert_eq!(Chain::Ethereum.as_str(), "ethereum");
        assert_eq!(Chain::from_str("ethereum"), Some(Chain::Ethereum));
        assert_eq!(Chain::from_str("ETHEREUM"), Some(Chain::Ethereum));
        assert_eq!(Chain::from_str("unknown"), None);
        
        // Test Display
        assert_eq!(Chain::Ethereum.to_string(), "ethereum");
    }
    
    #[test]
    fn test_support_checks() {
        assert!(Chain::Ethereum.is_layerzero_supported());
        assert!(!Chain::Solana.is_layerzero_supported());
        
        assert!(Chain::Ethereum.is_wormhole_supported());
        assert!(!Chain::NEAR.is_wormhole_supported());
    }
} 