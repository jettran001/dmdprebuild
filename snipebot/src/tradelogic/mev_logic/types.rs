//! Types cho MEV Logic
//!
//! Module này định nghĩa các kiểu dữ liệu, struct, enum sử dụng trong
//! các chiến lược MEV và phân tích mempool.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use crate::types::{ChainType, TokenPair};

// Import from common module
use crate::tradelogic::common::types::{TraderBehaviorType, TraderExpertiseLevel, GasBehavior, TraderBehaviorAnalysis};

// Import from analys/mempool/types để tránh định nghĩa trùng lặp
use crate::analys::mempool::types::{
    SuspiciousPattern, MempoolAlertType, TransactionType, TransactionPriority,
    AlertSeverity, TokenInfo
};

/// MEV types and data models

/// Type of MEV opportunity detected - Sử dụng SuspiciousPattern từ mempool làm cơ sở
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MevOpportunityType {
    /// Price difference between DEXes
    Arbitrage,
    /// Sandwich a large transaction - dùng từ SuspiciousPattern
    Sandwich,
    /// Front run another transaction - dùng từ SuspiciousPattern
    FrontRun,
    /// New token creation - dùng từ MempoolAlertType
    NewToken,
    /// New liquidity added - dùng từ MempoolAlertType
    NewLiquidity,
    /// Opportunity related to liquidity removal - dùng từ MempoolAlertType
    LiquidityRemoval,
}

// Thêm impl để có thể chuyển đổi từ SuspiciousPattern sang MevOpportunityType
impl From<SuspiciousPattern> for MevOpportunityType {
    fn from(pattern: SuspiciousPattern) -> Self {
        match pattern {
            SuspiciousPattern::SandwichAttack => MevOpportunityType::Sandwich,
            SuspiciousPattern::FrontRunning => MevOpportunityType::FrontRun,
            SuspiciousPattern::SuddenLiquidityRemoval => MevOpportunityType::LiquidityRemoval,
            // Các trường hợp khác không ánh xạ trực tiếp vào MevOpportunityType
            _ => MevOpportunityType::Arbitrage, // Mặc định
        }
    }
}

// Thêm impl để có thể chuyển đổi từ MempoolAlertType sang MevOpportunityType
impl From<MempoolAlertType> for MevOpportunityType {
    fn from(alert_type: MempoolAlertType) -> Self {
        match alert_type {
            MempoolAlertType::NewToken => MevOpportunityType::NewToken,
            MempoolAlertType::LiquidityAdded => MevOpportunityType::NewLiquidity,
            MempoolAlertType::LiquidityRemoved => MevOpportunityType::LiquidityRemoval,
            MempoolAlertType::MevOpportunity => MevOpportunityType::Arbitrage,
            MempoolAlertType::SuspiciousTransaction(pattern) => Self::from(pattern),
            _ => MevOpportunityType::Arbitrage, // Mặc định
        }
    }
}

/// MEV execution method
#[derive(Debug, Clone, PartialEq)]
pub enum MevExecutionMethod {
    /// Flash loan transaction
    FlashLoan,
    /// Standard transaction
    StandardTransaction,
    /// Multiple swaps (flash swap)
    MultiSwap,
    /// Custom contract call
    CustomContract,
}

/// Thông tin cơ hội MEV
#[derive(Debug, Clone)]
pub struct MevOpportunity {
    /// Loại cơ hội MEV
    pub opportunity_type: MevOpportunityType,
    /// Blockchain và chain ID
    pub chain_id: u32,
    /// Thời gian phát hiện
    pub detected_at: u64,
    /// Hết hạn sau
    pub expires_at: u64,
    /// Đã thực thi chưa
    pub executed: bool,
    /// Lợi nhuận ước tính (USD)
    pub estimated_profit_usd: f64,
    /// Chi phí gas ước tính (USD)
    pub estimated_gas_cost_usd: f64,
    /// Lợi nhuận ròng ước tính (USD)
    pub estimated_net_profit_usd: f64,
    /// Cặp token liên quan
    pub token_pairs: Vec<TokenPair>,
    /// Độ rủi ro ước tính (0-100, càng cao càng rủi ro)
    pub risk_score: f64,
    /// Giao dịch liên quan
    pub related_transactions: Vec<String>,
    /// Phương thức thực thi
    pub execution_method: MevExecutionMethod,
    /// Tham số riêng cho từng loại cơ hội
    pub specific_params: HashMap<String, String>,
}

/// Configuration for MEV strategy
#[derive(Debug, Clone)]
pub struct MevConfig {
    /// Is MEV strategy enabled
    pub enabled: bool,
    /// Allowed opportunity types
    pub allowed_opportunity_types: HashSet<MevOpportunityType>,
    /// Minimum profit threshold (USD)
    pub min_profit_threshold_usd: f64,
    /// Maximum gas price (Gwei)
    pub max_gas_price_gwei: f64,
    /// Maximum capital per trade (ETH)
    pub max_capital_per_trade_eth: f64,
    /// Automatically execute opportunities (false = monitor only)
    pub auto_execute: bool,
    /// Maximum executions per minute
    pub max_executions_per_minute: u32,
    /// Whitelist of tokens allowed to trade
    pub token_whitelist: HashSet<String>,
    /// Allowed DEXes
    pub allowed_dexes: HashSet<String>,
    /// Maximum risk score allowed (0-100)
    pub max_risk_score: f64,
}

impl Default for MevConfig {
    fn default() -> Self {
        let mut allowed_opportunity_types = HashSet::new();
        allowed_opportunity_types.insert(MevOpportunityType::Arbitrage);
        allowed_opportunity_types.insert(MevOpportunityType::Sandwich);
        
        let mut allowed_dexes = HashSet::new();
        allowed_dexes.insert("uniswap".to_string());
        allowed_dexes.insert("sushiswap".to_string());
        allowed_dexes.insert("pancakeswap".to_string());
        
        Self {
            enabled: false, // Disabled by default, needs to be enabled manually
            allowed_opportunity_types,
            min_profit_threshold_usd: 20.0, // $20 minimum profit
            max_gas_price_gwei: 1000.0, // 1000 Gwei
            max_capital_per_trade_eth: 5.0, // 5 ETH maximum per trade
            auto_execute: false, // Monitor only, no execution
            max_executions_per_minute: 3, // Maximum 3 transactions per minute
            token_whitelist: HashSet::new(), // No whitelist by default
            allowed_dexes,
            max_risk_score: 60.0, // Allow medium risk
        }
    }
}

/// Dữ liệu thị trường cho phân tích và điều chỉnh chiến lược
#[derive(Debug, Clone)]
pub struct MarketData {
    /// Biến động giá trung bình 24h (%)
    pub volatility_24h: f64,
    /// Tổng khối lượng giao dịch 24h
    pub volume_24h: f64,
    /// DEX cung cấp thanh khoản tốt nhất
    pub best_liquidity_dex: Option<String>,
    /// Phân tích xu hướng giá 
    pub price_trend: Option<String>,
    /// Thời điểm đầu tiên phát hiện token
    pub first_seen: Option<u64>,
}

/// Kiểu MEV opportunity mới
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AdvancedMevOpportunityType {
    /// Just-In-Time (JIT) Liquidity
    JITLiquidity,
    /// Cross-domain MEV (đa chuỗi)
    CrossDomain,
    /// Backrunning
    Backrunning,
    /// Liquidation opportunity
    Liquidation,
    /// Order flow auction
    OrderFlowAuction,
}

/// Cấu hình cho JIT Liquidity
#[derive(Debug, Clone)]
pub struct JITLiquidityConfig {
    /// Enabled state
    pub enabled: bool,
    /// Minimum profit threshold (USD)
    pub min_profit_threshold_usd: f64,
    /// Maximum capital allocation (USD)
    pub max_capital_allocation_usd: f64,
    /// Target pools for JIT
    pub target_pools: std::collections::HashSet<String>,
    /// Blocks to monitor in advance 
    pub monitor_blocks_ahead: u64,
    /// Maximum transaction delay (ms)
    pub max_transaction_delay_ms: u64,
}

impl Default for JITLiquidityConfig {
    fn default() -> Self {
        let mut target_pools = HashSet::new();
        target_pools.insert("0xB4e16d0168e52d35CaCD2c6185b44281Ec28C9Dc".to_string()); // USDC-ETH Uniswap V2
        
        Self {
            enabled: false,
            min_profit_threshold_usd: 50.0,
            max_capital_allocation_usd: 25000.0,
            target_pools,
            monitor_blocks_ahead: 1,
            max_transaction_delay_ms: 200,
        }
    }
}

/// Cross-domain MEV configuration
#[derive(Debug, Clone)]
pub struct CrossDomainMevConfig {
    /// Enabled state
    pub enabled: bool,
    /// Supported chain pairs
    pub supported_chains: std::collections::HashSet<(u64, u64)>,
    /// Minimum profit threshold (USD)
    pub min_profit_threshold_usd: f64,
    /// Maximum latency tolerance (ms)
    pub max_latency_ms: u64,
    /// Bridge providers (key=provider name, value=endpoint URL)
    pub bridge_providers: std::collections::HashMap<String, String>,
    /// Maximum bridging cost allowed (USD)
    pub max_bridge_cost_usd: f64,
    /// Estimated bridging time per chain (key=chain ID, value=seconds)
    pub estimated_bridge_time: std::collections::HashMap<u64, u64>,
    /// Estimated bridge cost (USD)
    pub estimated_bridge_cost_usd: f64,
    /// Gas oracle endpoints
    pub gas_oracle_endpoints: std::collections::HashMap<u64, String>,
}

impl Default for CrossDomainMevConfig {
    fn default() -> Self {
        let mut supported_chains = HashSet::new();
        supported_chains.insert((1, 56)); // Ethereum <-> BSC
        supported_chains.insert((1, 137)); // Ethereum <-> Polygon
        
        let mut bridge_providers = HashMap::new();
        bridge_providers.insert("layerzero".to_string(), "https://api.layerzero.network".to_string());
        
        let mut estimated_bridge_time = HashMap::new();
        estimated_bridge_time.insert(1, 30); // Ethereum: 30s
        estimated_bridge_time.insert(56, 15); // BSC: 15s
        estimated_bridge_time.insert(137, 10); // Polygon: 10s
        
        let mut gas_oracle_endpoints = HashMap::new();
        gas_oracle_endpoints.insert(1, "https://api.etherscan.io/api?module=gastracker".to_string());
        
        Self {
            enabled: false,
            supported_chains,
            min_profit_threshold_usd: 100.0,
            max_latency_ms: 5000,
            bridge_providers,
            max_bridge_cost_usd: 50.0,
            estimated_bridge_time,
            estimated_bridge_cost_usd: 20.0,
            gas_oracle_endpoints,
        }
    }
}

/// Searcher strategy for submitting transactions
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SearcherStrategy {
    /// Standard MEV
    Standard,
    /// Private transaction
    Private,
    /// Builder API submission
    BuilderApi,
    /// Order flow auction
    OrderFlowAuction,
}

/// Searcher identity information
#[derive(Debug, Clone)]
pub struct SearcherIdentity {
    /// Searcher name/identifier
    pub name: String,
    /// Preferred strategy
    pub strategy: SearcherStrategy,
    /// Builder API endpoints
    pub builder_api_endpoints: std::collections::HashMap<u64, String>,
    /// Private pool endpoints
    pub private_pool_endpoints: std::collections::HashMap<u64, String>,
    /// Signer wallet (address only, not private key)
    pub signer_wallet: Option<String>,
    /// Average success rate (0-1)
    pub success_rate: f64,
} 