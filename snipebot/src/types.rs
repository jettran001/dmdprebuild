//! Types module
//!
//! Module này định nghĩa các kiểu dữ liệu dùng chung trong snipebot

use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use ethers::types::{Address, H256, U256};
use common::trading_actions::{TradeAction, TradeStatus};

/// Common type definitions for the SnipeBot project
///
/// This module contains core types used across the codebase,
/// ensuring consistency and preventing duplication.

/// Supported blockchain types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChainType {
    /// Ethereum Virtual Machine based chains
    EVM(u32),  // Parameter is chain_id
    
    /// Solana
    Solana,
    
    /// Other blockchains (stub for future)
    Other(u32),
}

impl ChainType {
    /// Get the chain ID for this chain type
    pub fn chain_id(&self) -> u32 {
        match self {
            ChainType::EVM(id) => *id,
            ChainType::Solana => 0, // Placeholder chain ID for Solana
            ChainType::Other(id) => *id,
        }
    }
    
    /// Check if this chain is an EVM-compatible chain
    pub fn is_evm(&self) -> bool {
        matches!(self, ChainType::EVM(_))
    }
    
    /// Get blockchain name based on chain ID
    pub fn get_name(&self) -> String {
        match self {
            ChainType::EVM(1) => "Ethereum".to_string(),
            ChainType::EVM(56) => "BSC".to_string(),
            ChainType::EVM(137) => "Polygon".to_string(),
            ChainType::EVM(42161) => "Arbitrum".to_string(),
            ChainType::EVM(10) => "Optimism".to_string(),
            ChainType::EVM(43114) => "Avalanche".to_string(),
            ChainType::EVM(id) => format!("EVM Chain ({})", id),
            ChainType::Solana => "Solana".to_string(),
            ChainType::Other(id) => format!("Chain ({})", id),
        }
    }
    
    /// Get the default base token for this chain
    pub fn default_base_token(&self) -> String {
        match self {
            ChainType::EVM(1) => "ETH".to_string(),
            ChainType::EVM(56) => "BNB".to_string(),
            ChainType::EVM(137) => "MATIC".to_string(),
            ChainType::EVM(42161) => "ETH".to_string(),
            ChainType::EVM(10) => "ETH".to_string(),
            ChainType::EVM(43114) => "AVAX".to_string(),
            ChainType::EVM(_) => "ETH".to_string(),
            ChainType::Solana => "SOL".to_string(),
            ChainType::Other(_) => "NATIVE".to_string(),
        }
    }
}

/// Token pair for trading
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TokenPair {
    /// Base token (e.g. ETH, BNB)
    pub base_token: String,
    
    /// Quote token (e.g. USDT, USDC)
    pub quote_token: String,
    
    /// Chain ID
    pub chain_id: u32,
}

impl TokenPair {
    /// Create a new token pair
    pub fn new(base_token: String, quote_token: String, chain_id: u32) -> Self {
        Self {
            base_token,
            quote_token,
            chain_id,
        }
    }
    
    /// Get pair string in format like ETH/USDT
    pub fn pair_string(&self) -> String {
        format!("{}/{}", self.base_token, self.quote_token)
    }
}

/// Re-export TradeAction from common module as TradeType for backward compatibility
/// This allows gradual migration to the new standardized type
pub use common::trading_actions::TradeAction as TradeType;

// Tạo một conversion để duy trì khả năng tương thích với code cũ
impl From<TradeAction> for TradeType {
    fn from(action: TradeAction) -> Self {
        action
    }
}

/// Trade parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeParams {
    /// Chain type and ID
    pub chain_type: ChainType,
    
    /// Token address
    pub token_address: String,
    
    /// Token pair
    pub token_pair: TokenPair,
    
    /// Trade amount in base currency
    pub amount: f64,
    
    /// Maximum acceptable slippage (%)
    pub slippage: f64,
    
    /// Trade type (buy/sell)
    pub trade_type: TradeType,
    
    /// Transaction deadline in minutes
    pub deadline_minutes: u32,
    
    /// Custom router address (if any)
    pub router_address: String,
    
    /// Custom gas limit
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_limit: Option<u64>,
    
    /// Custom gas price in gwei
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_price: Option<f64>,
    
    /// Strategy to use
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
    
    /// Stop loss percentage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_loss: Option<f64>,
    
    /// Take profit percentage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub take_profit: Option<f64>,
    
    /// Maximum hold time in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_hold_time: Option<u64>,
    
    /// Custom parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_params: Option<HashMap<String, String>>,
}

impl TradeParams {
    /// Get the chain ID
    pub fn chain_id(&self) -> u32 {
        self.chain_type.chain_id()
    }
    
    /// Check if this is a buy trade
    pub fn is_buy(&self) -> bool {
        self.trade_type == TradeType::Buy
    }
    
    /// Check if this is a sell trade
    pub fn is_sell(&self) -> bool {
        self.trade_type == TradeType::Sell
    }
    
    /// Get the base token for this trade based on chain type
    pub fn base_token(&self) -> String {
        self.chain_type.default_base_token()
    }
}

impl Default for TradeParams {
    fn default() -> Self {
        Self {
            chain_type: ChainType::EVM(1), // Default to Ethereum
            token_address: "".to_string(),
            token_pair: TokenPair {
                base_token: "".to_string(),
                quote_token: "".to_string(),
                chain_id: 0,
            },
            amount: 0.0,
            slippage: 1.0, // 1%
            trade_type: TradeType::Buy,
            deadline_minutes: 30,
            router_address: "".to_string(),
            gas_limit: None,
            gas_price: None,
            strategy: None,
            stop_loss: None,
            take_profit: None,
            max_hold_time: None,
            custom_params: None,
        }
    }
}

/// Trade result response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeResponse {
    /// Transaction hash
    pub transaction_hash: String,
    
    /// Success status
    pub success: bool,
    
    /// Transaction receipt (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_receipt: Option<TransactionReceipt>,
    
    /// Error message (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    
    /// Execution price
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_price: Option<f64>,
    
    /// Amount of tokens received
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_received: Option<f64>,
    
    /// Gas used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_used: Option<u64>,
    
    /// Gas cost in USD
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_cost_usd: Option<f64>,
}

/// Transaction receipt details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionReceipt {
    /// Transaction hash
    pub transaction_hash: String,
    
    /// Block number
    pub block_number: u64,
    
    /// Gas used
    pub gas_used: u64,
    
    /// Effective gas price
    pub effective_gas_price: f64,
    
    /// Logs
    pub logs: Vec<TransactionLog>,
    
    /// Status (1 = success, 0 = failure)
    pub status: u8,
}

/// Transaction log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionLog {
    /// Contract address that emitted the log
    pub address: String,
    
    /// Topics
    pub topics: Vec<String>,
    
    /// Log data
    pub data: String,
}

/// Blockchain position representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    /// Token address
    pub token_address: String,
    
    /// Chain ID
    pub chain_id: u32,
    
    /// Amount of tokens
    pub token_amount: f64,
    
    /// USD value
    pub usd_value: f64,
    
    /// Entry price in USD
    pub entry_price_usd: f64,
    
    /// Current price in USD
    pub current_price_usd: f64,
    
    /// Profit/loss percentage
    pub pnl_percent: f64,
}

/// General API response structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    /// Success status
    pub success: bool,
    
    /// Response data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    
    /// Error message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    
    /// Timestamp
    pub timestamp: u64,
}

impl<T> ApiResponse<T> {
    /// Create a successful response
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            timestamp: chrono::Utc::now().timestamp() as u64,
        }
    }
    
    /// Create an error response
    pub fn error(message: &str) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message.to_string()),
            timestamp: chrono::Utc::now().timestamp() as u64,
        }
    }
}

/// Transaction status
#[derive(Debug, Clone, PartialEq)]
pub struct TransactionStatus {
    /// Transaction status type
    pub status: TransactionStatusType,
    
    /// Whether the transaction has been confirmed
    pub confirmed: bool,
    
    /// How long the transaction has been pending (in seconds)
    pub pending_seconds: u64,
    
    /// Block number where transaction was included (if confirmed)
    pub block_number: Option<u64>,
    
    /// Gas used by the transaction (if confirmed)
    pub gas_used: Option<u64>,
    
    /// Effective gas price paid (if confirmed)
    pub effective_gas_price: Option<f64>,
}

/// Transaction status types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionStatusType {
    /// Transaction is pending
    Pending,
    /// Transaction is confirmed
    Confirmed,
    /// Transaction has failed
    Failed,
    /// Transaction not found
    NotFound,
}

impl TransactionStatus {
    /// Check if a transaction has been confirmed on the blockchain
    pub fn is_transaction_confirmed(&self) -> bool {
        self.confirmed
    }
    
    /// Create a new pending transaction status
    pub fn new_pending() -> Self {
        Self {
            status: TransactionStatusType::Pending,
            confirmed: false,
            pending_seconds: 0,
            block_number: None,
            gas_used: None,
            effective_gas_price: None,
        }
    }
    
    /// Create a new success transaction status
    pub fn new_success(block_number: u64, gas_used: u64, effective_gas_price: f64) -> Self {
        Self {
            status: TransactionStatusType::Confirmed,
            confirmed: true,
            pending_seconds: 0,
            block_number: Some(block_number),
            gas_used: Some(gas_used),
            effective_gas_price: Some(effective_gas_price),
        }
    }
    
    /// Create a new failed transaction status
    pub fn new_failed(block_number: u64) -> Self {
        Self {
            status: TransactionStatusType::Failed,
            confirmed: true,
            pending_seconds: 0,
            block_number: Some(block_number),
            gas_used: None,
            effective_gas_price: None,
        }
    }
}
