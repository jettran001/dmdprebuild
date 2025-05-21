//! Common traits for trading strategies
//!
//! This module defines the core traits that all trading strategies must implement
//! to ensure a standardized interface across different trading approaches.
//! Follows the trait-based design pattern to allow for modularity and extensibility.

// External imports
use std::sync::Arc;
use async_trait::async_trait;

// Standard library imports
use std::collections::HashMap;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};
use crate::analys::token_status::TokenSafety;
use crate::analys::mempool::{MempoolTransaction, TransactionType, MempoolAlert};
use crate::analys::risk_analyzer::{TradeRiskAnalysis, RiskFactor};
use crate::tradelogic::common::types::{RiskScore, SecurityCheckResult, TokenIssue};
use crate::tradelogic::mev_logic::types::{MevOpportunityType, MevExecutionMethod};
use crate::tradelogic::mev_logic::opportunity::MevOpportunity;

// Third party imports
use anyhow::Result;

/// Base trait for all trade executors
/// 
/// This trait defines the standard interface that all trade execution strategies
/// must implement, ensuring consistent behavior across different trading approaches.
#[async_trait]
pub trait TradeExecutor: Send + Sync + 'static {
    /// Configuration type for this executor
    type Config: Default + Clone + Send + Sync + 'static;
    
    /// Result type for this executor's trades
    type TradeResult: Clone + Send + Sync + 'static;
    
    /// Start the trading executor
    async fn start(&self) -> Result<()>;
    
    /// Stop the trading executor
    async fn stop(&self) -> Result<()>;
    
    /// Update the executor's configuration
    async fn update_config(&self, config: Self::Config) -> Result<()>;
    
    /// Add a new blockchain to monitor
    async fn add_chain(&mut self, chain_id: u32, adapter: Arc<EvmAdapter>) -> Result<()>;
    
    /// Execute a trade with the specified parameters
    async fn execute_trade(&self, params: TradeParams) -> Result<Self::TradeResult>;
    
    /// Get the current status of an active trade
    async fn get_trade_status(&self, trade_id: &str) -> Result<Option<Self::TradeResult>>;
    
    /// Get all active trades
    async fn get_active_trades(&self) -> Result<Vec<Self::TradeResult>>;
    
    /// Get trade history within a specific time range
    async fn get_trade_history(&self, from_timestamp: u64, to_timestamp: u64) -> Result<Vec<Self::TradeResult>>;
    
    /// Evaluate a token for potential trading
    async fn evaluate_token(&self, chain_id: u32, token_address: &str) -> Result<Option<TokenSafety>>;
}

/// Risk management trait for trading strategies
///
/// This trait defines methods for assessing and managing risks in trading operations.
#[async_trait]
pub trait RiskManager: Send + Sync + 'static {
    /// Calculate risk score for a potential trade (0-100)
    async fn calculate_risk_score(&self, chain_id: u32, token_address: &str) -> Result<u8>;
    
    /// Check if a trade meets the risk criteria
    async fn meets_risk_criteria(&self, chain_id: u32, token_address: &str, max_risk: u8) -> Result<bool>;
    
    /// Get detailed risk analysis
    async fn get_risk_analysis(&self, chain_id: u32, token_address: &str) -> Result<HashMap<String, f64>>;
    
    /// Update risk parameters based on market conditions
    async fn adapt_risk_parameters(&self, market_volatility: f64) -> Result<()>;
}

/// Trade strategy optimization trait
///
/// This trait defines methods for optimizing trading strategies based on
/// historical performance and market conditions.
#[async_trait]
pub trait StrategyOptimizer: Send + Sync + 'static {
    /// Optimize strategy parameters based on historical performance
    async fn optimize_parameters(&self, history_days: u32) -> Result<()>;
    
    /// Backtest strategy with specific parameters
    async fn backtest(&self, parameters: HashMap<String, f64>, days: u32) -> Result<f64>;
    
    /// Get strategy performance metrics
    async fn get_performance_metrics(&self) -> Result<HashMap<String, f64>>;
    
    /// Suggest optimal parameters for current market conditions
    async fn suggest_optimal_parameters(&self) -> Result<HashMap<String, f64>>;
}

/// Cross-chain trading interface
///
/// This trait defines methods for executing trades across different blockchains.
#[async_trait]
pub trait CrossChainTrader: Send + Sync + 'static {
    /// Find arbitrage opportunities between chains
    async fn find_arbitrage_opportunities(&self) -> Result<Vec<(ChainType, ChainType, String, f64)>>;
    
    /// Execute cross-chain arbitrage
    async fn execute_cross_chain_arbitrage(&self, from_chain: ChainType, to_chain: ChainType, token: &str) -> Result<String>;
    
    /// Get supported bridges between chains
    async fn get_supported_bridges(&self, from_chain: ChainType, to_chain: ChainType) -> Result<Vec<String>>;
    
    /// Estimate cross-chain transfer time and cost
    async fn estimate_cross_chain_transfer(&self, from_chain: ChainType, to_chain: ChainType, token: &str, amount: f64) -> Result<(u64, f64)>;
}

/// Mempool Analysis Provider
///
/// Trait cho phép MEV logic sử dụng các dịch vụ phân tích mempool
#[async_trait]
pub trait MempoolAnalysisProvider: Send + Sync + 'static {
    /// Lấy danh sách giao dịch mempool theo các tiêu chí
    async fn get_pending_transactions(&self, 
        chain_id: u32, 
        filter_types: Option<Vec<TransactionType>>,
        limit: Option<usize>
    ) -> Result<Vec<MempoolTransaction>>;
    
    /// Phát hiện các mẫu giao dịch đáng ngờ
    async fn detect_suspicious_patterns(&self, 
        chain_id: u32, 
        lookback_blocks: u32
    ) -> Result<Vec<MempoolAlert>>;
    
    /// Phát hiện cơ hội MEV từ mempool
    async fn detect_mev_opportunities(&self, 
        chain_id: u32,
        opportunity_types: Option<Vec<MevOpportunityType>>
    ) -> Result<Vec<MevOpportunity>>;
    
    /// Đăng ký callback khi phát hiện cơ hội MEV mới
    async fn subscribe_to_opportunities(&self, 
        chain_id: u32,
        callback: Arc<dyn Fn(MevOpportunity) -> Result<()> + Send + Sync + 'static>
    ) -> Result<String>;
    
    /// Hủy đăng ký callback
    async fn unsubscribe(&self, subscription_id: &str) -> Result<()>;
    
    /// Dự đoán thời gian xác nhận của giao dịch dựa trên gas
    async fn estimate_confirmation_time(&self, 
        chain_id: u32,
        gas_price_gwei: f64, 
        gas_limit: u64
    ) -> Result<u64>;
    
    /// Phân tích gas percentile và đề xuất giá gas
    async fn get_gas_suggestions(&self, chain_id: u32) -> Result<HashMap<String, f64>>;
}

/// Token Analysis Provider
///
/// Trait cho phép MEV logic sử dụng các dịch vụ phân tích token
#[async_trait]
pub trait TokenAnalysisProvider: Send + Sync + 'static {
    /// Phân tích an toàn token
    async fn analyze_token_safety(&self, 
        chain_id: u32, 
        token_address: &str
    ) -> Result<TokenSafety>;
    
    /// Kiểm tra các vấn đề bảo mật của token
    async fn check_token_issues(&self, 
        chain_id: u32, 
        token_address: &str
    ) -> Result<Vec<TokenIssue>>;
    
    /// Đánh giá xem token có an toàn để giao dịch không
    async fn is_safe_to_trade(&self, 
        chain_id: u32, 
        token_address: &str, 
        max_risk_score: u8
    ) -> Result<bool>;
    
    /// Phân tích chi tiết token với kết quả đầy đủ
    async fn get_detailed_token_analysis(&self, 
        chain_id: u32, 
        token_address: &str
    ) -> Result<SecurityCheckResult>;
    
    /// Theo dõi thay đổi thanh khoản của token
    async fn monitor_token_liquidity(&self, 
        chain_id: u32, 
        token_address: &str, 
        callback: Arc<dyn Fn(f64, bool) -> Result<()> + Send + Sync + 'static>
    ) -> Result<String>;
    
    /// Hủy theo dõi token
    async fn stop_monitoring(&self, monitoring_id: &str) -> Result<()>;
}

/// Risk Analysis Provider
///
/// Trait cho phép MEV logic sử dụng dịch vụ phân tích rủi ro
#[async_trait]
pub trait RiskAnalysisProvider: Send + Sync + 'static {
    /// Phân tích rủi ro giao dịch
    async fn analyze_trade_risk(&self, 
        chain_id: u32, 
        token_address: &str, 
        trade_amount_usd: f64
    ) -> Result<TradeRiskAnalysis>;
    
    /// Tính toán điểm rủi ro tổng hợp cho MEV opportunity
    async fn calculate_opportunity_risk(&self, 
        opportunity: &MevOpportunity
    ) -> Result<RiskScore>;
    
    /// Kiểm tra xem cơ hội có đáp ứng tiêu chí rủi ro không
    async fn opportunity_meets_criteria(&self, 
        opportunity: &MevOpportunity, 
        max_risk_score: u8
    ) -> Result<bool>;
    
    /// Đánh giá rủi ro của phương pháp thực thi
    async fn evaluate_execution_method_risk(&self, 
        chain_id: u32,
        method: MevExecutionMethod
    ) -> Result<u8>;
    
    /// Thích nghi ngưỡng rủi ro dựa trên biến động thị trường
    async fn adapt_risk_threshold(&self, 
        base_threshold: u8, 
        market_volatility: f64
    ) -> Result<u8>;
}

/// MEV Opportunity Provider
///
/// Trait để truyền các cơ hội MEV từ analys module đến mev_logic
#[async_trait]
pub trait MevOpportunityProvider: Send + Sync + 'static {
    /// Lấy tất cả các cơ hội MEV hiện có
    async fn get_all_opportunities(&self, chain_id: u32) -> Result<Vec<MevOpportunity>>;
    
    /// Lấy các cơ hội MEV theo loại
    async fn get_opportunities_by_type(&self, 
        chain_id: u32, 
        opportunity_type: MevOpportunityType
    ) -> Result<Vec<MevOpportunity>>;
    
    /// Đăng ký nhận thông báo cho cơ hội mới
    async fn register_opportunity_listener(&self, 
        callback: Arc<dyn Fn(MevOpportunity) -> Result<()> + Send + Sync + 'static>
    ) -> Result<String>;
    
    /// Hủy đăng ký listener
    async fn unregister_listener(&self, listener_id: &str) -> Result<()>;
    
    /// Cập nhật trạng thái cơ hội
    async fn update_opportunity_status(&self, 
        opportunity_id: &str, 
        executed: bool, 
        tx_hash: Option<String>
    ) -> Result<()>;
    
    /// Lọc cơ hội theo điều kiện profit, risk, và loại
    async fn filter_opportunities(&self, 
        chain_id: u32,
        min_profit_usd: f64,
        max_risk: u8,
        types: Option<Vec<MevOpportunityType>>
    ) -> Result<Vec<MevOpportunity>>;
}

/// Trade Coordinator for managing shared state between executors
///
/// This trait defines methods for coordinating between different trade executors
/// including opportunity sharing, trade allocation, and global state management.
#[async_trait]
pub trait TradeCoordinator: Send + Sync + 'static {
    /// Register a trade executor
    async fn register_executor(&self, executor_id: &str, executor_type: ExecutorType) -> Result<()>;
    
    /// Unregister a trade executor
    async fn unregister_executor(&self, executor_id: &str) -> Result<()>;
    
    /// Share a new trading opportunity with other executors
    async fn share_opportunity(&self, 
        from_executor: &str,
        opportunity: SharedOpportunity,
    ) -> Result<()>;
    
    /// Subscribe to opportunity updates
    async fn subscribe_to_opportunities(&self, 
        executor_id: &str,
        callback: Arc<dyn Fn(SharedOpportunity) -> Result<()> + Send + Sync + 'static>,
    ) -> Result<String>;
    
    /// Unsubscribe from opportunity updates
    async fn unsubscribe_from_opportunities(&self, subscription_id: &str) -> Result<()>;
    
    /// Reserve an opportunity for an executor
    async fn reserve_opportunity(&self, 
        opportunity_id: &str, 
        executor_id: &str,
        priority: OpportunityPriority,
    ) -> Result<bool>;
    
    /// Release a previously reserved opportunity
    async fn release_opportunity(&self, opportunity_id: &str, executor_id: &str) -> Result<()>;
    
    /// Get all available opportunities
    async fn get_all_opportunities(&self) -> Result<Vec<SharedOpportunity>>;
    
    /// Get statistics about opportunity sharing and usage
    async fn get_sharing_statistics(&self) -> Result<SharingStatistics>;
    
    /// Update global optimization parameters based on market conditions
    async fn update_global_parameters(&self, parameters: HashMap<String, f64>) -> Result<()>;
    
    /// Get current global parameters
    async fn get_global_parameters(&self) -> Result<HashMap<String, f64>>;
}

/// Type of executor
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExecutorType {
    /// Smart trade executor
    SmartTrade,
    
    /// MEV bot
    MevBot,
    
    /// Manual trade executor
    ManualTrade,
    
    /// Custom executor
    Custom(String),
}

/// Priority level for opportunity reservation
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum OpportunityPriority {
    /// Low priority (will be overridden by higher priority)
    Low = 0,
    
    /// Medium priority
    Medium = 50,
    
    /// High priority
    High = 100,
    
    /// Critical priority (cannot be overridden)
    Critical = 200,
}

/// Shared opportunity information between executors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedOpportunity {
    /// Unique ID for the opportunity
    pub id: String,
    
    /// Chain ID
    pub chain_id: u32,
    
    /// Type of opportunity
    pub opportunity_type: SharedOpportunityType,
    
    /// Token addresses involved
    pub tokens: Vec<String>,
    
    /// Estimated profit in USD
    pub estimated_profit_usd: f64,
    
    /// Risk score (0-100)
    pub risk_score: u8,
    
    /// Time sensitivity (how quickly action is needed, in seconds)
    pub time_sensitivity: u64,
    
    /// Source executor that discovered the opportunity
    pub source: String,
    
    /// Creation timestamp
    pub created_at: u64,
    
    /// Custom data specific to opportunity type
    pub custom_data: HashMap<String, String>,
    
    /// Current reservation status
    pub reservation: Option<OpportunityReservation>,
}

/// Type of shared opportunity
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SharedOpportunityType {
    /// MEV opportunity
    Mev(MevOpportunityType),
    
    /// New token listing
    NewToken,
    
    /// Liquidity change
    LiquidityChange,
    
    /// Price movement
    PriceMovement,
    
    /// Custom opportunity type
    Custom(String),
}

/// Reservation information for an opportunity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpportunityReservation {
    /// Executor that reserved the opportunity
    pub executor_id: String,
    
    /// Priority of the reservation
    pub priority: OpportunityPriority,
    
    /// When the reservation was made
    pub reserved_at: u64,
    
    /// When the reservation expires
    pub expires_at: u64,
}

/// Statistics about opportunity sharing
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SharingStatistics {
    /// Total opportunities shared
    pub total_shared: u64,
    
    /// Opportunities by type
    pub by_type: HashMap<SharedOpportunityType, u64>,
    
    /// Opportunities by source
    pub by_source: HashMap<String, u64>,
    
    /// Opportunities executed
    pub executed: u64,
    
    /// Opportunities expired
    pub expired: u64,
    
    /// Average reservation time (seconds)
    pub avg_reservation_time: f64,
    
    /// Conflicts (when multiple executors wanted same opportunity)
    pub reservation_conflicts: u64,
} 