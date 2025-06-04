//! Common trading action definitions
//!
//! This module contains standardized trading action enums used across the project,
//! ensuring consistency and preventing duplication between modules.

use serde::{Serialize, Deserialize};

/// Standardized trading action enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeAction {
    /// Buy operation
    Buy,
    
    /// Sell operation
    Sell,
    
    /// Liquidity provision
    AddLiquidity,
    
    /// Liquidity removal
    RemoveLiquidity,
    
    /// Swap tokens (token to token)
    Swap,
    
    /// Bridge tokens cross-chain
    Bridge,
    
    /// Flash swap/loan
    FlashSwap,
    
    /// Approve tokens for spending by router
    Approve,
}

impl Default for TradeAction {
    fn default() -> Self {
        Self::Buy
    }
}

/// Trade status enum for tracking execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeStatus {
    /// Trade is pending execution
    Pending,
    /// Trade is being submitted
    Submitting,
    /// Trade has been submitted to blockchain
    Submitted,
    /// Trade is being executed/in progress
    Executing,
    /// Trade has been completed successfully
    Completed,
    /// Trade has failed
    Failed,
    /// Trade was canceled by user
    Canceled,
    /// Trade was rejected due to validation/safety checks
    Rejected,
}

impl Default for TradeStatus {
    fn default() -> Self {
        Self::Pending
    }
} 