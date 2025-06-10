//! Types cho MEV Logic
//!
//! Module này định nghĩa các kiểu dữ liệu, struct, enum sử dụng trong
//! các chiến lược MEV và phân tích mempool.

use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::types::TokenPair;

// Import types từ analys/mempool/types để tránh định nghĩa trùng lặp
use crate::analys::mempool::types::{
    SuspiciousPattern, MempoolAlertType, MempoolTransaction
};

// Import from common module
use crate::tradelogic::common::types::{TraderBehaviorType, TraderExpertiseLevel, GasBehavior, TraderBehaviorAnalysis};
// Import TradeStatus để thêm phương thức update_status
use common::trading_actions::TradeStatus;

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
    /// Back run another transaction
    Backrun,
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MevExecutionMethod {
    /// Flash loan transaction
    FlashLoan,
    /// Standard transaction
    Standard,
    /// Multiple swaps (flash swap)
    MultiSwap,
    /// Custom contract call
    CustomContract,
    /// Thực thi thông qua MEV bundle
    MevBundle,
    /// Thực thi thông qua private RPC
    PrivateRPC,
    /// Thực thi thông qua builder API
    BuilderAPI,
}

// Thêm implementation cho MevExecutionMethod
impl MevExecutionMethod {
    /// Validate execution method for a specific chain
    pub fn validate_for_chain(&self, chain_id: u32) -> Result<(), anyhow::Error> {
        use anyhow::anyhow;
        
        match self {
            Self::FlashLoan => {
                // Kiểm tra hỗ trợ flash loan trên chain
                match chain_id {
                    1 | 56 | 137 | 42161 | 10 => Ok(()), // Các chain chính hỗ trợ flash loan
                    _ => Err(anyhow!("Flash loan not supported on chain {}", chain_id))
                }
            },
            Self::MevBundle => {
                // Kiểm tra hỗ trợ MEV bundle trên chain
                match chain_id {
                    1 | 5 | 11155111 => Ok(()), // Ethereum và testnet hỗ trợ MEV bundle
                    _ => Err(anyhow!("MEV bundle not supported on chain {}", chain_id))
                }
            },
            Self::PrivateRPC => {
                // Kiểm tra hỗ trợ private RPC trên chain
                match chain_id {
                    1 | 56 | 137 | 42161 | 10 | 5 | 11155111 => Ok(()), // Nhiều chain hỗ trợ
                    _ => Err(anyhow!("Private RPC not configured for chain {}", chain_id))
                }
            },
            Self::BuilderAPI => {
                // Kiểm tra hỗ trợ builder API trên chain
                match chain_id {
                    1 | 5 | 11155111 => Ok(()), // Ethereum và testnet hỗ trợ builder API
                    _ => Err(anyhow!("Builder API not supported on chain {}", chain_id))
                }
            },
            Self::Standard | Self::MultiSwap | Self::CustomContract => Ok(()), // Các phương thức còn lại được hỗ trợ trên tất cả các chain
        }
    }
}

/// Định nghĩa trạng thái của một cơ hội MEV
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MevOpportunityStatus {
    /// Cơ hội đã được phát hiện nhưng chưa xử lý
    Detected,
    /// Cơ hội đang được đánh giá
    Evaluating,
    /// Cơ hội đang được thực thi
    Executing,
    /// Cơ hội đã được thực thi thành công
    Executed,
    /// Thực thi cơ hội thất bại
    Failed,
    /// Cơ hội đã hết hạn
    Expired,
    /// Cơ hội bị từ chối (ví dụ: không đủ lợi nhuận)
    Rejected,
}

/// Thông tin cơ hội MEV
#[derive(Debug, Clone)]
pub struct MevOpportunity {
    /// ID giao dịch
    pub id: String,
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
    /// Token pair chính (nếu có)
    pub token_pair: Option<TokenPair>,
    /// Độ rủi ro ước tính (0-100, càng cao càng rủi ro)
    pub risk_score: f64,
    /// Giao dịch liên quan
    pub related_transactions: Vec<String>,
    /// Phương thức thực thi
    pub execution_method: MevExecutionMethod,
    /// Tham số riêng cho từng loại cơ hội
    pub specific_params: HashMap<String, String>,
    /// Các tham số cụ thể (định dạng khác)
    pub parameters: HashMap<String, String>,
    /// Transaction hash nếu đã thực thi
    pub execution_tx_hash: Option<String>,
    /// Trạng thái giao dịch TradeStatus
    pub status: Option<TradeStatus>,
    /// Trạng thái cơ hội MEV
    pub mev_status: MevOpportunityStatus,
    /// Giao dịch mục tiêu (dùng cho sandwich và backrun)
    pub target_transaction: Option<MempoolTransaction>,
    /// Dữ liệu bundle giao dịch (dùng cho MevBundle)
    pub bundle_data: Option<Vec<u8>>,
    /// Giao dịch đầu tiên trong sandwich
    pub first_transaction: Option<Vec<u8>>,
    /// Giao dịch thứ hai trong sandwich
    pub second_transaction: Option<Vec<u8>>,
    /// Lợi nhuận kỳ vọng (ETH)
    pub expected_profit: f64,
    /// Lợi nhuận thực tế sau khi thực thi (ETH)
    pub actual_profit: Option<f64>,
}

// Thêm constructor cho MevOpportunity để dễ dàng tạo instance
impl MevOpportunity {
    /// Tạo mới MevOpportunity với các tham số cơ bản
    pub fn new(
        opportunity_type: MevOpportunityType,
        chain_id: u32,
        estimated_profit_usd: f64,
        estimated_gas_cost_usd: f64,
        token_pairs: Vec<TokenPair>,
        risk_score: f64,
        execution_method: MevExecutionMethod,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
            
        Self {
            id: uuid::Uuid::new_v4().to_string(), // Generate a unique ID
            opportunity_type,
            chain_id,
            detected_at: now,
            expires_at: now + 60, // Mặc định hết hạn sau 60 giây
            executed: false,
            estimated_profit_usd,
            estimated_gas_cost_usd,
            estimated_net_profit_usd: estimated_profit_usd - estimated_gas_cost_usd,
            token_pairs,
            token_pair: token_pairs.first().cloned(),
            risk_score,
            related_transactions: Vec::new(),
            execution_method,
            specific_params: HashMap::new(),
            parameters: HashMap::new(),
            execution_tx_hash: None,
            status: None,
            target_transaction: None,
            bundle_data: None,
            first_transaction: None,
            second_transaction: None,
            expected_profit: 0.0,
            actual_profit: None,
            mev_status: MevOpportunityStatus::Detected,
        }
    }
    
    /// Tạo cơ hội MEV mới với validation
    pub fn new_with_validation(
        chain_id: u32,
        opportunity_type: MevOpportunityType,
        detected_at: u64,
        expires_at: u64,
        estimated_profit_usd: f64,
        estimated_gas_cost_usd: f64,
        execution_method: MevExecutionMethod,
        token_pair: Option<TokenPair>,
        parameters: HashMap<String, String>,
        risk_score: u8,
    ) -> Result<Self, anyhow::Error> {
        use anyhow::anyhow;
        
        // Validate opportunity type parameters
        opportunity_type.validate_parameters(&parameters)?;
        
        // Validate execution method
        execution_method.validate_for_chain(chain_id)?;
        
        // Validate profitability
        if estimated_profit_usd <= 0.0 {
            return Err(anyhow!("Estimated profit must be positive"));
        }
        
        // Validate gas cost
        if estimated_gas_cost_usd < 0.0 {
            return Err(anyhow!("Estimated gas cost cannot be negative"));
        }
        
        // Validate timestamps
        if detected_at >= expires_at {
            return Err(anyhow!("Expiration time must be after detection time"));
        }
        
        let token_pairs = if let Some(tp) = &token_pair {
            vec![tp.clone()]
        } else {
            Vec::new()
        };
        
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(), // Generate a unique ID
            chain_id,
            opportunity_type,
            detected_at,
            expires_at,
            executed: false,
            estimated_profit_usd,
            estimated_gas_cost_usd,
            estimated_net_profit_usd: estimated_profit_usd - estimated_gas_cost_usd,
            token_pairs,
            token_pair,
            risk_score: risk_score as f64,
            related_transactions: Vec::new(),
            execution_method,
            specific_params: HashMap::new(),
            parameters,
            execution_tx_hash: None,
            status: None,
            target_transaction: None,
            bundle_data: None,
            first_transaction: None,
            second_transaction: None,
            expected_profit: 0.0,
            actual_profit: None,
            mev_status: MevOpportunityStatus::Detected,
        })
    }
    
    /// Kiểm tra tính hợp lệ của cơ hội
    pub fn validate(&self) -> Result<(), anyhow::Error> {
        use anyhow::anyhow;
        
        // Kiểm tra các tham số dựa trên loại cơ hội
        self.opportunity_type.validate_parameters(&self.parameters)?;
        
        // Kiểm tra phương thức thực thi
        self.execution_method.validate_for_chain(self.chain_id)?;
        
        // Kiểm tra thời gian hết hạn
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
            
        if self.expires_at <= now {
            return Err(anyhow!("Opportunity has expired"));
        }
        
        // Kiểm tra lợi nhuận
        if self.estimated_profit_usd <= self.estimated_gas_cost_usd {
            return Err(anyhow!("Opportunity is not profitable after gas costs"));
        }
        
        Ok(())
    }
    
    /// Tính lợi nhuận ròng (sau khi trừ phí gas)
    pub fn net_profit_usd(&self) -> f64 {
        (self.estimated_profit_usd - self.estimated_gas_cost_usd).max(0.0_f64)
    }
    
    /// Tính tỷ lệ lợi nhuận trên chi phí gas
    pub fn profit_to_gas_ratio(&self) -> f64 {
        if self.estimated_gas_cost_usd <= 0.0 {
            return f64::INFINITY;
        }
        self.estimated_profit_usd / self.estimated_gas_cost_usd
    }
    
    /// Kiểm tra xem cơ hội có đáng thực hiện dựa trên ngưỡng lợi nhuận
    pub fn is_worth_executing(&self, min_profit_usd: f64, min_profit_gas_ratio: f64) -> bool {
        let net_profit = self.net_profit_usd();
        let profit_gas_ratio = self.profit_to_gas_ratio();
        
        net_profit >= min_profit_usd && profit_gas_ratio >= min_profit_gas_ratio
    }
    
    /// Đánh dấu cơ hội đã thực thi với tx hash
    pub fn mark_as_executed(&mut self, tx_hash: String) {
        self.executed = true;
        self.execution_tx_hash = Some(tx_hash);
        self.status = Some(TradeStatus::Completed);
        self.mev_status = MevOpportunityStatus::Executed;
    }
    
    /// Cập nhật trạng thái của cơ hội
    pub fn update_status(&mut self, status: TradeStatus) {
        self.status = Some(status);
        
        // Nếu trạng thái là Completed, đánh dấu là đã thực thi
        if status == TradeStatus::Completed {
            self.executed = true;
            self.mev_status = MevOpportunityStatus::Executed;
        }
    }
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
    pub target_pools: HashSet<String>,
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
    pub supported_chains: HashSet<(u64, u64)>,
    /// Minimum profit threshold (USD)
    pub min_profit_threshold_usd: f64,
    /// Maximum latency tolerance (ms)
    pub max_latency_ms: u64,
    /// Bridge providers (key=provider name, value=endpoint URL)
    pub bridge_providers: HashMap<String, String>,
    /// Maximum bridging cost allowed (USD)
    pub max_bridge_cost_usd: f64,
    /// Estimated bridging time per chain (key=chain ID, value=seconds)
    pub estimated_bridge_time: HashMap<u64, u64>,
    /// Estimated bridge cost (USD)
    pub estimated_bridge_cost_usd: f64,
    /// Gas oracle endpoints
    pub gas_oracle_endpoints: HashMap<u64, String>,
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
    pub builder_api_endpoints: HashMap<u64, String>,
    /// Private pool endpoints
    pub private_pool_endpoints: HashMap<u64, String>,
    /// Signer wallet (address only, not private key)
    pub signer_wallet: Option<String>,
    /// Average success rate (0-1)
    pub success_rate: f64,
}

impl MevOpportunityType {
    /// Validate opportunity type with specific parameters
    pub fn validate_parameters(&self, params: &HashMap<String, String>) -> Result<(), anyhow::Error> {
        use anyhow::anyhow;
        
        match self {
            Self::Arbitrage => {
                // Kiểm tra các tham số bắt buộc cho arbitrage
                Self::require_params(params, &["from_token", "to_token", "price_diff"])?;
                
                // Xác thực giá trị price_diff
                if let Some(price_diff) = params.get("price_diff") {
                    if let Ok(value) = price_diff.parse::<f64>() {
                        if value <= 0.0 {
                            return Err(anyhow!("Price difference must be positive"));
                        }
                    } else {
                        return Err(anyhow!("Invalid price_diff format, must be a number"));
                    }
                }
                
                Ok(())
            },
            Self::Sandwich => {
                // Kiểm tra các tham số bắt buộc cho sandwich
                Self::require_params(params, &["from_token", "to_token", "target_tx_hash", "amount"])?;
                
                // Xác thực giá trị amount
                if let Some(amount) = params.get("amount") {
                    if let Ok(value) = amount.parse::<f64>() {
                        if value <= 0.0 {
                            return Err(anyhow!("Amount must be positive"));
                        }
                    } else {
                        return Err(anyhow!("Invalid amount format, must be a number"));
                    }
                }
                
                // Xác thực định dạng tx_hash
                if let Some(tx_hash) = params.get("target_tx_hash") {
                    if !tx_hash.starts_with("0x") || tx_hash.len() != 66 {
                        return Err(anyhow!("Invalid transaction hash format"));
                    }
                }
                
                Ok(())
            },
            Self::FrontRun => {
                // Kiểm tra các tham số bắt buộc cho front run
                Self::require_params(params, &["target_tx_hash"])?;
                
                // Xác thực định dạng tx_hash
                if let Some(tx_hash) = params.get("target_tx_hash") {
                    if !tx_hash.starts_with("0x") || tx_hash.len() != 66 {
                        return Err(anyhow!("Invalid transaction hash format"));
                    }
                }
                
                Ok(())
            },
            Self::NewToken => {
                // Kiểm tra các tham số bắt buộc cho new token
                Self::require_params(params, &["token_address", "pair_address"])?;
                
                // Xác thực định dạng token_address
                if let Some(address) = params.get("token_address") {
                    if !address.starts_with("0x") || address.len() != 42 {
                        return Err(anyhow!("Invalid token address format"));
                    }
                }
                
                // Xác thực định dạng pair_address
                if let Some(address) = params.get("pair_address") {
                    if !address.starts_with("0x") || address.len() != 42 {
                        return Err(anyhow!("Invalid pair address format"));
                    }
                }
                
                Ok(())
            },
            Self::NewLiquidity | Self::LiquidityRemoval => {
                // Kiểm tra các tham số bắt buộc cho liquidity
                Self::require_params(params, &["token_address", "pair_address", "amount"])?;
                
                // Xác thực giá trị amount
                if let Some(amount) = params.get("amount") {
                    if let Ok(value) = amount.parse::<f64>() {
                        if value <= 0.0 {
                            return Err(anyhow!("Amount must be positive"));
                        }
                    } else {
                        return Err(anyhow!("Invalid amount format, must be a number"));
                    }
                }
                
                Ok(())
            }
        }
    }
    
    /// Yêu cầu các tham số cần thiết cho opportunity type
    fn require_params(params: &HashMap<String, String>, required: &[&str]) -> Result<(), anyhow::Error> {
        use anyhow::anyhow;
        
        for &param in required {
            if !params.contains_key(param) {
                return Err(anyhow!("Missing required parameter: {}", param));
            }
        }
        
        Ok(())
    }
}

// Implement validation cho MevConfig
impl MevConfig {
    /// Validate MevConfig
    pub fn validate(&self) -> Result<(), anyhow::Error> {
        use anyhow::anyhow;
        
        // Kiểm tra ngưỡng lợi nhuận
        if self.min_profit_threshold_usd < 0.0 {
            return Err(anyhow!("Minimum profit threshold cannot be negative"));
        }
        
        // Kiểm tra max_gas_price_gwei
        if self.max_gas_price_gwei <= 0.0 {
            return Err(anyhow!("Maximum gas price must be positive"));
        }
        
        // Kiểm tra max_capital_per_trade_eth
        if self.max_capital_per_trade_eth <= 0.0 {
            return Err(anyhow!("Maximum capital per trade must be positive"));
        }
        
        // Kiểm tra max_executions_per_minute
        if self.max_executions_per_minute == 0 {
            return Err(anyhow!("Maximum executions per minute cannot be zero"));
        }
        
        // Kiểm tra max_risk_score
        if self.max_risk_score < 0.0 || self.max_risk_score > 100.0 {
            return Err(anyhow!("Risk score must be between 0 and 100"));
        }
        
        Ok(())
    }
} 