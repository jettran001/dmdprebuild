//! Common trading types
//!
//! This module contains common type definitions used across the trading system,
//! particularly for MEV detection, trader behavior analysis, and transaction monitoring.
//! Having these definitions centralized prevents code duplication between modules.

use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// Information about a transaction that may potentially fail
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PotentialFailingTx {
    /// Transaction hash
    pub tx_hash: String,
    
    /// From address
    pub from: String,
    
    /// To address
    pub to: String,
    
    /// Configured gas limit
    pub gas_limit: u64,
    
    /// Estimated gas required
    pub estimated_gas: u64,
    
    /// Reason for potential failure
    pub reason: FailureReason,
    
    /// Prediction confidence (0-100)
    pub confidence: u8,
}

/// Reasons why a transaction might fail
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureReason {
    /// Insufficient gas limit
    InsufficientGas,
    
    /// Gas price too low compared to network
    GasPriceTooLow,
    
    /// Insufficient balance
    InsufficientBalance,
    
    /// Slippage too low for high volatility token
    SlippageToLow,
    
    /// Invalid nonce
    InvalidNonce,
    
    /// Contract execution error
    ContractExecution,
    
    /// Unknown reason
    Unknown,
}

/// Trader profile information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraderProfile {
    /// Trader's address
    pub address: String,
    
    /// Total number of trades
    pub trade_count: usize,
    
    /// Average trade value
    pub avg_trade_value: f64,
    
    /// Number of successful trades
    pub successful_trades: usize,
    
    /// Number of failed trades
    pub failed_trades: usize,
    
    /// Average token hold time (seconds)
    pub avg_hold_time: f64,
    
    /// Preferred tokens
    pub preferred_tokens: Vec<String>,
    
    /// Risk appetite
    pub risk_appetite: RiskAppetite,
    
    /// Trading pattern
    pub trading_pattern: TradingPattern,
    
    /// Whether this is a bot
    pub is_bot: bool,
    
    /// Whether this is an arbitrageur
    pub is_arbitrageur: bool,
}

/// Trader's risk appetite
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskAppetite {
    /// Low risk (prefer blue-chip)
    Low,
    
    /// Moderate risk (balanced)
    Moderate,
    
    /// Accept high risk (prefer small altcoin)
    High,
    
    /// Unknown (insufficient data)
    Unknown,
}

/// Trading pattern of a trader
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradingPattern {
    /// Scalping (very short holding time)
    Scalping,
    
    /// Day trading (holding time <1 day)
    DayTrading,
    
    /// Swing trading (holding time a few days)
    SwingTrading,
    
    /// Long-term investing (holding time >1 week)
    LongTermInvesting,
    
    /// Neutral (no clear trend)
    Neutral,
    
    /// Unknown
    Unknown,
}

impl Default for TraderProfile {
    fn default() -> Self {
        Self {
            address: String::new(),
            trade_count: 0,
            avg_trade_value: 0.0,
            successful_trades: 0,
            failed_trades: 0,
            avg_hold_time: 0.0,
            preferred_tokens: Vec::new(),
            risk_appetite: RiskAppetite::Unknown,
            trading_pattern: TradingPattern::Unknown,
            is_bot: false,
            is_arbitrageur: false,
        }
    }
}

impl Default for PotentialFailingTx {
    fn default() -> Self {
        Self {
            tx_hash: String::new(),
            from: String::new(),
            to: String::new(),
            gas_limit: 0,
            estimated_gas: 0,
            reason: FailureReason::Unknown,
            confidence: 0,
        }
    }
} 