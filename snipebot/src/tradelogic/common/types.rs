//! Common type definitions shared between trade logic modules
//!
//! This module contains types that are used by both smart_trade and mev_logic,
//! preventing code duplication and ensuring consistency.

use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use common::trading_actions::{TradeAction, TradeStatus};
pub use common::trading_actions::TradeAction as GlobalTradeType;

/// Trader behavior type
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TraderBehaviorType {
    /// Arbitrage trader
    Arbitrageur,
    /// MEV bot
    MevBot,
    /// Retail investor
    Retail,
    /// Institutional investor
    Institutional,
    /// Market maker
    MarketMaker,
    /// Whale (large value transactions)
    Whale,
    /// High frequency trader
    HighFrequencyTrader,
    /// Unknown type
    Unknown,
}

/// Trader expertise level
#[derive(Debug, Clone, PartialEq, Ord, PartialOrd, Eq, Hash, Serialize, Deserialize)]
pub enum TraderExpertiseLevel {
    /// Professional level
    Professional,
    /// Intermediate level
    Intermediate,
    /// Beginner level
    Beginner,
    /// Automated (bot)
    Automated,
    /// Unknown level
    Unknown,
}

/// Gas usage behavior of a trader
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasBehavior {
    /// Average gas price (Gwei)
    pub average_gas_price: f64,
    /// Highest gas price used (Gwei)
    pub highest_gas_price: f64,
    /// Lowest gas price used (Gwei)
    pub lowest_gas_price: f64,
    /// Has gas strategy (e.g. increases gas when needed)
    pub has_gas_strategy: bool,
    /// Transaction success rate
    pub success_rate: f64,
}

/// Trader behavior analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraderBehaviorAnalysis {
    /// Trader address
    pub address: String,
    /// Detected behavior type
    pub behavior_type: TraderBehaviorType,
    /// Expertise level
    pub expertise_level: TraderExpertiseLevel,
    /// Transaction frequency (transactions/hour)
    pub transaction_frequency: f64,
    /// Average transaction value (ETH)
    pub average_transaction_value: f64,
    /// Common transaction types
    pub common_transaction_types: Vec<crate::analys::mempool::TransactionType>,
    /// Gas usage behavior
    pub gas_behavior: GasBehavior,
    /// Frequently traded tokens
    pub frequently_traded_tokens: Vec<String>,
    /// Preferred DEXes
    pub preferred_dexes: Vec<String>,
    /// Active hours (0-23, UTC hours)
    pub active_hours: Vec<u8>,
    /// Prediction score for next behavior (0-100, higher is more reliable)
    pub prediction_score: f64,
    /// Additional notes
    pub additional_notes: Option<String>,
}

/// Token issue types for security analysis
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TokenIssue {
    // === Basic issues ===
    /// Token cannot be sold (honeypot)
    Honeypot,
    /// Abnormally high tax (>10%)
    HighTax,
    /// Tax changes over time
    DynamicTax,
    /// Low liquidity (<$5000)
    LowLiquidity,
    /// Contract not verified on chain explorer
    UnverifiedContract,
    /// Owner has full control over token
    OwnerWithFullControl,
    
    // === Advanced issues ===
    /// Ownership not renounced
    OwnershipNotRenounced,
    /// Backdoor to regain ownership
    OwnershipRenounceBackdoor,
    /// Contract is a changeable proxy
    ProxyContract,
    /// Has blacklist function
    BlacklistFunction,
    /// Has whitelist function
    WhitelistFunction,
    /// Has cooldown between transactions
    TradingCooldown,
    /// Maximum transaction amount limit
    MaxTransactionLimit,
    /// Maximum wallet holding limit
    MaxWalletLimit,
    /// Ability to mint unlimited tokens
    UnlimitedMintAuthority,
    /// Undisclosed fees
    HiddenFees,
    /// Abnormal liquidity events
    AbnormalLiquidityEvents,
    /// Calls to external contracts
    ExternalCalls,
    /// Uses delegatecall (dangerous)
    DelegateCall,
    /// Source code doesn't match bytecode
    InconsistentSourceCode,
    /// Interacts with malicious external contracts
    MaliciousExternalContract,
    /// Upgradeable logic after launch
    UpgradeableLogic,
    /// Fake ownership renounce
    FakeRenounce,
    /// Transfer restrictions
    TransferRestriction,
    /// Contract can self-destruct
    ContractSelfDestruct,
    /// Arbitrary code execution
    ArbitraryCodeExecution,
}

/// Standardized risk score with levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    /// Very low risk (0-20)
    VeryLow,
    /// Low risk (21-40)
    Low,
    /// Medium risk (41-60)
    Medium,
    /// High risk (61-80)
    High,
    /// Very high risk (81-100)
    VeryHigh,
}

impl RiskLevel {
    /// Create a risk level from a numerical score
    pub fn from_score(score: u8) -> Self {
        match score {
            0..=20 => Self::VeryLow,
            21..=40 => Self::Low,
            41..=60 => Self::Medium,
            61..=80 => Self::High,
            _ => Self::VeryHigh,
        }
    }
    
    /// Get the minimum score for this risk level
    pub fn min_score(&self) -> u8 {
        match self {
            Self::VeryLow => 0,
            Self::Low => 21,
            Self::Medium => 41,
            Self::High => 61,
            Self::VeryHigh => 81,
        }
    }
    
    /// Get the maximum score for this risk level
    pub fn max_score(&self) -> u8 {
        match self {
            Self::VeryLow => 20,
            Self::Low => 40,
            Self::Medium => 60,
            Self::High => 80,
            Self::VeryHigh => 100,
        }
    }
}

/// Token safety analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskScore {
    /// Overall risk score (0-100, higher is riskier)
    pub score: u64,
    
    /// Detailed risk factors identified
    pub factors: Vec<RiskFactor>,
    
    /// Is the token detected as honeypot
    pub is_honeypot: bool,
    
    /// Does the token have severe security issues
    pub has_severe_issues: bool,
}

impl Default for RiskScore {
    fn default() -> Self {
        Self {
            score: 50, // Default score is moderate risk
            factors: Vec::new(),
            is_honeypot: false,
            has_severe_issues: false,
        }
    }
}

/// Detailed risk factor for a token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskFactor {
    /// Risk category
    pub category: RiskCategory,
    
    /// Risk weight (contribution to overall score)
    pub weight: u8,
    
    /// Description of the risk
    pub description: String,
    
    /// Priority level (high, medium, low)
    pub priority: RiskPriority,
}

impl Default for RiskFactor {
    fn default() -> Self {
        Self {
            category: RiskCategory::Unknown,
            weight: 0,
            description: String::new(),
            priority: RiskPriority::Low,
        }
    }
}

/// Risk category types
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RiskCategory {
    /// Liquidity-related risks
    Liquidity,
    
    /// Contract security risks
    Security,
    
    /// Ownership/control risks
    Ownership,
    
    /// Market manipulation risks
    MarketManipulation,
    
    /// Regulatory/compliance risks
    Compliance,
    
    /// Unknown risk category
    Unknown,
}

/// Risk priority levels
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskPriority {
    /// High priority risk (critical)
    High,
    
    /// Medium priority risk (concerning)
    Medium,
    
    /// Low priority risk (minor)
    Low,
}

/// Security check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityCheckResult {
    /// Overall security score (0-100, higher is more secure)
    pub score: u8,
    
    /// Is the token a potential honeypot
    pub is_honeypot: bool,
    
    /// List of identified issues
    pub issues: Vec<TokenIssueDetail>,
    
    /// Contract verification status
    pub is_contract_verified: bool,
    
    /// Is there a significant rug pull risk
    pub rug_pull_risk: bool,
}

impl Default for SecurityCheckResult {
    fn default() -> Self {
        Self {
            score: 0,
            is_honeypot: false,
            issues: Vec::new(),
            is_contract_verified: false,
            rug_pull_risk: false,
        }
    }
}

/// Token issues detected during security analysis
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TokenIssueDetail {
    /// Type of issue
    pub issue_type: TokenIssueType,
    
    /// Description of the issue
    pub description: String,
    
    /// Severity of the issue (0-100, higher is more severe)
    pub severity: u8,
}

impl Default for TokenIssueDetail {
    fn default() -> Self {
        Self {
            issue_type: TokenIssueType::Other,
            description: String::new(),
            severity: 0,
        }
    }
}

/// Types of token security issues
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TokenIssueType {
    /// Hidden owner functions
    HiddenOwner,
    
    /// Malicious transfer function
    MaliciousTransfer,
    
    /// Honeypot-like characteristics
    Honeypot,
    
    /// Fee manipulation capability
    FeeManipulation,
    
    /// Blacklist/whitelist capability
    Blacklist,
    
    /// Unverified contract code
    UnverifiedCode,
    
    /// Minting capability
    MintFunction,
    
    /// Proxy/upgradeable functionality
    Proxy,
    
    /// Flash loan attack vulnerability
    FlashLoanVulnerability,
    
    /// Other miscellaneous issues
    Other,
}

/// Generic execution method for trades
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionMethod {
    /// Standard transaction
    Standard,
    /// Private transaction (via private RPC)
    Private,
    /// Flash transaction bundle
    FlashBundle,
    /// Custom contract execution
    CustomContract,
    /// Cross-chain bridge
    CrossChain,
    /// Batched transaction
    Batched,
}

impl Default for ExecutionMethod {
    fn default() -> Self {
        Self::Standard
    }
}

/// Security issue found in token/transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityIssue {
    /// Issue code (unique identifier)
    pub code: String,
    /// Issue name/title
    pub name: String,
    /// Detailed description
    pub description: String,
    /// Severity level
    pub severity: SecurityIssueSeverity,
}

/// Severity level for security issues
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityIssueSeverity {
    /// Informational only
    Info,
    /// Low severity
    Low,
    /// Medium severity
    Medium,
    /// High severity
    High,
    /// Critical severity
    Critical,
}

/// Common price and trade pair information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceInfo {
    /// Token address
    pub token: String,
    /// Chain ID where price is from
    pub chain_id: u32,
    /// Current price in base token
    pub price: f64,
    /// Price 24h ago
    pub price_24h: Option<f64>,
    /// Price change percentage (24h)
    pub price_change_24h: Option<f64>,
    /// Trading volume (24h)
    pub volume_24h: Option<f64>,
    /// Market cap
    pub market_cap: Option<f64>,
    /// Liquidity in USD
    pub liquidity_usd: Option<f64>,
    /// Last updated timestamp
    pub updated_at: u64,
}

/// Common trade parameters applicable to all strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommonTradeParams {
    /// Maximum acceptable slippage (percentage)
    pub max_slippage: f64,
    /// Gas price multiplier (1.0 = network average)
    pub gas_price_multiplier: f64,
    /// Gas limit multiplier (1.0 = estimated gas)
    pub gas_limit_multiplier: f64,
    /// Deadline in seconds
    pub deadline_seconds: u64,
    /// Whether to allow partial fills
    pub allow_partial_fill: bool,
    /// Maximum risk score acceptable (0-100)
    pub max_risk_score: u8,
    /// Custom parameters for specific execution methods
    pub custom_params: HashMap<String, String>,
}

impl Default for CommonTradeParams {
    fn default() -> Self {
        Self {
            max_slippage: 1.0,
            gas_price_multiplier: 1.0,
            gas_limit_multiplier: 1.2,
            deadline_seconds: 300,
            allow_partial_fill: true,
            max_risk_score: 60,
            custom_params: HashMap::new(),
        }
    }
}

/// Common interface for all trade results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeResultCommon {
    /// Unique trade ID
    pub id: String,
    /// Chain ID where the trade was executed
    pub chain_id: u32,
    /// Transaction hash
    pub tx_hash: Option<String>,
    /// Trade status
    pub status: TradeStatus,
    /// Token address
    pub token_address: String,
    /// Trade type (Buy/Sell/etc)
    pub trade_action: TradeAction,
    /// Amount in base currency
    pub base_amount: f64,
    /// Amount in tokens
    pub token_amount: f64,
    /// Execution price
    pub execution_price: Option<f64>,
    /// Gas used
    pub gas_used: Option<f64>,
    /// Gas cost in base currency
    pub gas_cost: Option<f64>,
    /// Created at timestamp
    pub created_at: u64,
    /// Executed at timestamp
    pub executed_at: Option<u64>,
    /// Error message if failed
    pub error: Option<String>,
    /// Security check result
    pub security_check: Option<SecurityCheckResult>,
}

/// Result of a trade operation
/// 
/// This struct represents the result of a trade operation, including
/// all relevant information about the trade such as status, amounts,
/// transaction details, and errors if any.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeResult {
    /// Common trade result fields shared across all trade types
    pub common: TradeResultCommon,
    
    /// Specific details based on trade type
    pub details: HashMap<String, String>,
    
    /// Gas price used (in Gwei)
    pub gas_price_gwei: Option<f64>,
    
    /// Gas limit used
    pub gas_limit: Option<u64>,
    
    /// Execution method used
    pub execution_method: ExecutionMethod,
    
    /// Price impact percentage
    pub price_impact: Option<f64>,
    
    /// Slippage percentage used
    pub slippage: Option<f64>,
    
    /// Router address used for the trade
    pub router_address: Option<String>,
    
    /// Pool address used for the trade
    pub pool_address: Option<String>,
    
    /// Block number when the trade was executed
    pub block_number: Option<u64>,
    
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl Default for TradeResult {
    fn default() -> Self {
        Self {
            common: TradeResultCommon::default(),
            details: HashMap::new(),
            gas_price_gwei: None,
            gas_limit: None,
            execution_method: ExecutionMethod::default(),
            price_impact: None,
            slippage: None,
            router_address: None,
            pool_address: None,
            block_number: None,
            metadata: HashMap::new(),
        }
    }
}

impl TradeResult {
    /// Create a new trade result with the given ID and chain ID
    pub fn new(id: String, chain_id: u32) -> Self {
        let mut result = Self::default();
        result.common.id = id;
        result.common.chain_id = chain_id;
        result.common.created_at = current_time_seconds();
        result
    }
    
    /// Set the trade as successful
    pub fn set_success(&mut self, tx_hash: String) {
        self.common.status = TradeStatus::Completed;
        self.common.tx_hash = Some(tx_hash);
        self.common.executed_at = Some(current_time_seconds());
    }
    
    /// Set the trade as failed with an error message
    pub fn set_failed(&mut self, error: String) {
        self.common.status = TradeStatus::Failed;
        self.common.error = Some(error);
        self.common.executed_at = Some(current_time_seconds());
    }
    
    /// Set the trade as pending with a transaction hash
    pub fn set_pending(&mut self, tx_hash: String) {
        self.common.status = TradeStatus::Pending;
        self.common.tx_hash = Some(tx_hash);
    }
    
    /// Check if the trade is completed
    pub fn is_completed(&self) -> bool {
        matches!(self.common.status, TradeStatus::Completed)
    }
    
    /// Check if the trade is failed
    pub fn is_failed(&self) -> bool {
        matches!(self.common.status, TradeStatus::Failed)
    }
    
    /// Check if the trade is pending
    pub fn is_pending(&self) -> bool {
        matches!(self.common.status, TradeStatus::Pending)
    }
}

/// Helper function to get current time in seconds
fn current_time_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_else(|e| {
            error!("Failed to get current time: {}", e);
            0 // Fallback to epoch start
        })
}

// Export common constants
pub const DEFAULT_SLIPPAGE: f64 = 1.0; // 1%
pub const DEFAULT_GAS_PRICE_MULTIPLIER: f64 = 1.05; // 5% above base
pub const DEFAULT_GAS_LIMIT_MULTIPLIER: f64 = 1.2; // 20% buffer
pub const DEFAULT_DEADLINE_SECONDS: u64 = 300; // 5 minutes
pub const DEFAULT_MAX_RISK_SCORE: u8 = 50; // Medium risk
pub const MIN_LIQUIDITY_USD: f64 = 10000.0; // $10,000
pub const MAX_ACCEPTABLE_BUY_TAX: f64 = 10.0; // 10%
pub const MAX_ACCEPTABLE_SELL_TAX: f64 = 10.0; // 10% 