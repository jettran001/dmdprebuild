//! Common traits for trading strategies
//!
//! This module defines the core traits that all trading strategies must implement
//! to ensure a standardized interface across different trading approaches.
//! Follows the trait-based design pattern to allow for modularity and extensibility.
//!
//! The traits in this module define contracts for all main components of the trading system:
//! - TradeExecutor: The core contract for any trading strategy
//! - RiskManager: Risk assessment and management
//! - StrategyOptimizer: Optimization of trading parameters
//! - CrossChainTrader: Cross-chain trading capabilities
//! - Analysis Providers: Various data analysis capabilities (mempool, token, risk)
//! - TradeCoordinator: Coordination between different trading strategies

// External imports
use std::sync::Arc;
use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;

// Standard library imports

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::{TradeParams, ChainType};
use crate::analys::token_status::TokenSafety;
use crate::analys::mempool::{MempoolTransaction, TransactionType, MempoolAlert};
use crate::analys::risk_analyzer::TradeRiskAnalysis;
use crate::tradelogic::common::types::{RiskScore, SecurityCheckResult, TokenIssue};
use crate::tradelogic::mev_logic::types::{MevOpportunityType, MevExecutionMethod};
use crate::tradelogic::mev_logic::opportunity::MevOpportunity;

// Third party imports
use anyhow::Result;
use serde::{Serialize, Deserialize};

/// Base trait for all trade executors
/// 
/// This trait defines the standard interface that all trade execution strategies
/// must implement, ensuring consistent behavior across different trading approaches.
/// 
/// Trade executors are responsible for:
/// - Executing trades based on provided parameters
/// - Maintaining trade state and history
/// - Evaluating trading opportunities
/// - Managing risk and optimization within their domain
/// 
/// All implementations should be thread-safe and suitable for async contexts.
#[async_trait]
pub trait TradeExecutor: Send + Sync + 'static {
    /// Configuration type for this executor
    type Config: Default + Clone + Send + Sync + 'static;
    
    /// Result type for this executor's trades
    type TradeResult: Clone + Send + Sync + 'static;
    
    /// Start the trading executor
    /// 
    /// Initializes the executor and starts any background tasks required
    /// for monitoring or processing trade opportunities.
    async fn start(&self) -> Result<()>;
    
    /// Stop the trading executor
    /// 
    /// Gracefully stops all background tasks and releases resources.
    /// Should complete any pending trades before stopping.
    async fn stop(&self) -> Result<()>;
    
    /// Update the executor's configuration
    /// 
    /// Changes the behavior of the executor based on the new configuration.
    /// Should validate the configuration before applying it.
    async fn update_config(&self, config: Self::Config) -> Result<()>;
    
    /// Add a new blockchain to monitor
    /// 
    /// Adds support for a new blockchain by providing the appropriate adapter.
    /// Should validate that the chain is supported before adding.
    async fn add_chain(&mut self, chain_id: u32, adapter: Arc<EvmAdapter>) -> Result<()>;
    
    /// Execute a trade with the specified parameters
    /// 
    /// Core method for initiating a trade based on the provided parameters.
    /// Returns a trade result that can be used to track the trade status.
    async fn execute_trade(&self, params: TradeParams) -> Result<Self::TradeResult>;
    
    /// Get the current status of an active trade
    /// 
    /// Retrieves the current state of a trade by its ID.
    /// Returns None if the trade doesn't exist.
    async fn get_trade_status(&self, trade_id: &str) -> Result<Option<Self::TradeResult>>;
    
    /// Get all active trades
    /// 
    /// Returns a list of all trades that are currently in progress.
    /// Active trades include those that are pending, executing, or submitted.
    async fn get_active_trades(&self) -> Result<Vec<Self::TradeResult>>;
    
    /// Get trade history within a specific time range
    /// 
    /// Retrieves historical trade data between the specified timestamps.
    /// Used for performance analysis and reporting.
    async fn get_trade_history(&self, from_timestamp: u64, to_timestamp: u64) -> Result<Vec<Self::TradeResult>>;
    
    /// Evaluate a token for potential trading
    /// 
    /// Analyzes a token to determine if it's suitable for trading.
    /// Returns token safety information if available.
    async fn evaluate_token(&self, chain_id: u32, token_address: &str) -> Result<Option<TokenSafety>>;
}

/// Risk management trait for trading strategies
///
/// This trait defines methods for assessing and managing risks in trading operations.
/// It provides a standardized way to calculate risk scores, check risk criteria,
/// and adapt risk parameters based on market conditions.
///
/// Risk managers are responsible for:
/// - Calculating risk scores for tokens and trades
/// - Determining if trades meet risk criteria
/// - Providing detailed risk analysis
/// - Adapting risk parameters based on market conditions
#[async_trait]
pub trait RiskManager: Send + Sync + 'static {
    /// Calculate risk score for a potential trade (0-100)
    /// 
    /// Evaluates the risk level of trading a specific token on a given chain.
    /// Returns a score from 0 (lowest risk) to 100 (highest risk).
    async fn calculate_risk_score(&self, chain_id: u32, token_address: &str) -> Result<u8>;
    
    /// Check if a trade meets the risk criteria
    /// 
    /// Determines if a potential trade meets the specified maximum risk threshold.
    /// Returns true if the trade is within acceptable risk limits.
    async fn meets_risk_criteria(&self, chain_id: u32, token_address: &str, max_risk: u8) -> Result<bool>;
    
    /// Get detailed risk analysis
    /// 
    /// Provides a detailed breakdown of various risk factors for a token.
    /// Returns a map of risk factor names to their scores.
    async fn get_risk_analysis(&self, chain_id: u32, token_address: &str) -> Result<HashMap<String, f64>>;
    
    /// Update risk parameters based on market conditions
    /// 
    /// Adjusts internal risk parameters based on current market volatility.
    /// Higher volatility typically requires stricter risk management.
    async fn adapt_risk_parameters(&self, market_volatility: f64) -> Result<()>;
}

/// Trade strategy optimization trait
///
/// This trait defines methods for optimizing trading strategies based on
/// historical performance and market conditions. It allows for automatic
/// adjustment of strategy parameters to improve performance over time.
///
/// Strategy optimizers are responsible for:
/// - Analyzing historical performance to identify optimal parameters
/// - Backtesting strategies with different parameters
/// - Measuring performance metrics
/// - Suggesting optimal parameters for current market conditions
#[async_trait]
pub trait StrategyOptimizer: Send + Sync + 'static {
    /// Optimize strategy parameters based on historical performance
    /// 
    /// Analyzes past trading performance to find optimal strategy parameters.
    /// Uses the specified number of days of historical data for optimization.
    async fn optimize_parameters(&self, history_days: u32) -> Result<()>;
    
    /// Backtest strategy with specific parameters
    /// 
    /// Tests the strategy against historical data using the provided parameters.
    /// Returns a performance score (typically profit/loss or Sharpe ratio).
    async fn backtest(&self, parameters: HashMap<String, f64>, days: u32) -> Result<f64>;
    
    /// Get strategy performance metrics
    /// 
    /// Retrieves various performance metrics for the current strategy.
    /// May include metrics like win rate, profit factor, drawdown, etc.
    async fn get_performance_metrics(&self) -> Result<HashMap<String, f64>>;
    
    /// Suggest optimal parameters for current market conditions
    /// 
    /// Recommends optimal strategy parameters based on current market conditions.
    /// Uses recent market data and historical performance to make recommendations.
    async fn suggest_optimal_parameters(&self) -> Result<HashMap<String, f64>>;
}

/// Cross-chain trading interface base trait
///
/// This trait defines core methods for executing trades across different blockchains.
/// The trait is designed to be compatible with trait objects (dyn CrossChainTrader).
/// It was separated from the full API to allow for dynamic dispatch while maintaining
/// backward compatibility with existing code.
/// 
/// Cross-chain traders are responsible for:
/// - Finding arbitrage opportunities between different blockchains
/// - Executing cross-chain trades via bridges
/// - Managing bridge interactions and monitoring cross-chain transfers
#[async_trait]
pub trait CrossChainTrader: Send + Sync + 'static {
    /// Find arbitrage opportunities between chains
    /// 
    /// Scans for price discrepancies between different blockchains.
    /// Returns a list of potential arbitrage opportunities with their estimated profits.
    /// Each tuple contains (source chain, destination chain, token address, profit percentage).
    async fn find_arbitrage_opportunities(&self) -> Result<Vec<(ChainType, ChainType, String, f64)>>;
    
    /// Execute cross-chain arbitrage
    /// 
    /// Performs an arbitrage trade between two different blockchains.
    /// Returns the transaction hash of the initiated bridge transaction.
    async fn execute_cross_chain_arbitrage(&self, from_chain: ChainType, to_chain: ChainType, token: &str) -> Result<String>;
    
    /// Get supported bridges between chains
    /// 
    /// Lists all available bridge protocols between the specified chains.
    /// Returns the names of supported bridge protocols (e.g., "LayerZero", "Wormhole").
    async fn get_supported_bridges(&self, from_chain: ChainType, to_chain: ChainType) -> Result<Vec<String>>;
    
    /// Estimate cross-chain transfer time and cost
    /// 
    /// Calculates the estimated time and cost for a cross-chain transfer.
    /// Returns a tuple of (estimated time in seconds, estimated cost in USD).
    async fn estimate_cross_chain_transfer(&self, from_chain: ChainType, to_chain: ChainType, token: &str, amount: f64) -> Result<(u64, f64)>;
}

/// Extended Cross-chain trading interface
///
/// This trait extends the base CrossChainTrader trait with additional methods
/// that aren't compatible with trait objects due to having default implementations.
/// These methods are provided for backward compatibility with the previous CrossDomainBridge API.
#[async_trait]
pub trait CrossChainTraderExt: CrossChainTrader {
    /// Get source chain ID (compatibility with CrossDomainBridge)
    /// 
    /// Returns the source chain ID for this bridge.
    /// Returns None if not configured for a specific source chain.
    fn source_chain_id(&self) -> Option<u64> {
        None
    }
    
    /// Get destination chain ID (compatibility with CrossDomainBridge)
    /// 
    /// Returns the destination chain ID for this bridge.
    /// Returns None if not configured for a specific destination chain.
    fn destination_chain_id(&self) -> Option<u64> {
        None
    }
    
    /// Check if token is supported (compatibility with CrossDomainBridge)
    /// 
    /// Verifies if the given token can be bridged using this implementation.
    /// Returns true if the token is supported, false otherwise.
    fn is_token_supported(&self, _token_address: &str) -> bool {
        false
    }
    
    /// Get bridging cost estimate (compatibility with CrossDomainBridge)
    /// 
    /// Calculates the estimated cost to bridge the specified token amount.
    /// Returns the estimated cost in USD.
    async fn get_bridging_cost_estimate(&self, _token_address: &str, _amount: f64) -> Result<f64> {
        Err(anyhow::anyhow!("Not implemented in this CrossChainTrader implementation"))
    }
    
    /// Get bridging time estimate in seconds (compatibility with CrossDomainBridge)
    /// 
    /// Calculates the estimated time to complete a bridge transaction.
    /// Returns the estimated time in seconds.
    async fn get_bridging_time_estimate(&self) -> Result<u64> {
        Err(anyhow::anyhow!("Not implemented in this CrossChainTrader implementation"))
    }
    
    /// Bridge tokens across chains (compatibility with CrossDomainBridge)
    /// 
    /// Initiates a token bridge transaction from the source to destination chain.
    /// Returns the transaction hash of the bridge transaction.
    async fn bridge_tokens(&self, _token_address: &str, _amount: f64) -> Result<String> {
        Err(anyhow::anyhow!("Not implemented in this CrossChainTrader implementation"))
    }
}

impl<T: CrossChainTrader + ?Sized> CrossChainTraderExt for T {}

/// Bridge status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeStatus {
    /// Whether the bridge is operational
    pub operational: bool,
    
    /// Current congestion level (0-100)
    pub congestion: u8,
    
    /// Average confirmation time in seconds
    pub avg_confirmation_time: u64,
    
    /// Any active alerts or issues
    pub alerts: Vec<String>,
}

/// Bridge fee estimation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeEstimate {
    /// Base fee in USD
    pub base_fee_usd: f64,
    
    /// Gas fee in USD
    pub gas_fee_usd: f64,
    
    /// Total fee in USD
    pub total_fee_usd: f64,
    
    /// Fee in source token amount
    pub fee_in_token: f64,
}

/// Bridge transaction parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeTransaction {
    /// Source token address
    pub token_address: String,
    
    /// Amount to bridge
    pub amount: u128,
    
    /// Recipient address on destination chain
    pub recipient: String,
    
    /// Maximum acceptable fee in USD
    pub max_fee_usd: f64,
    
    /// Maximum acceptable time in seconds
    pub max_time_seconds: u64,
}

/// Trait cho bridge provider
#[async_trait]
pub trait BridgeProvider: Send + Sync {
    /// Lấy chain ID nguồn
    fn source_chain_id(&self) -> u64;
    
    /// Lấy chain ID đích
    fn destination_chain_id(&self) -> u64;
    
    /// Lấy tên bridge
    fn name(&self) -> &str;
    
    /// Lấy trạng thái bridge
    async fn get_bridge_status(&self) -> Result<BridgeStatus>;
    
    /// Ước tính phí bridge
    async fn estimate_bridge_fee(&self, token_address: &str, amount: u128) -> Result<FeeEstimate>;
    
    /// Thực thi bridge transaction
    async fn execute_bridge(&self, transaction: BridgeTransaction) -> Result<String>;
}

/// Mempool Analysis Provider
///
/// Trait cho phép MEV logic sử dụng các dịch vụ phân tích mempool.
/// Định nghĩa giao diện chuẩn cho việc truy xuất, phân tích và theo dõi 
/// các giao dịch trong mempool để phát hiện cơ hội MEV.
///
/// Các provider này chịu trách nhiệm:
/// - Truy xuất và lọc các giao dịch từ mempool của blockchain
/// - Phát hiện các mẫu đáng ngờ hoặc cơ hội MEV
/// - Phân tích chi phí gas và dự đoán thời gian xác nhận
/// - Thông báo khi phát hiện cơ hội mới (callback)
#[async_trait]
pub trait MempoolAnalysisProvider: Send + Sync + 'static {
    /// Lấy danh sách giao dịch mempool theo các tiêu chí
    /// 
    /// Truy xuất các giao dịch đang chờ từ mempool theo loại giao dịch
    /// và giới hạn số lượng kết quả trả về.
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain cần truy vấn
    /// * `filter_types` - Các loại giao dịch cần lọc (None = tất cả)
    /// * `limit` - Giới hạn số lượng kết quả (None = không giới hạn)
    /// 
    /// # Returns
    /// Danh sách các giao dịch mempool thỏa mãn điều kiện lọc
    async fn get_pending_transactions(&self, 
        chain_id: u32, 
        filter_types: Option<Vec<TransactionType>>,
        limit: Option<usize>
    ) -> Result<Vec<MempoolTransaction>>;
    
    /// Phát hiện các mẫu giao dịch đáng ngờ
    /// 
    /// Phân tích mempool để tìm các mẫu giao dịch đáng ngờ như sandwich attack,
    /// front-running, hoặc các dấu hiệu rug pull.
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain cần phân tích
    /// * `lookback_blocks` - Số block gần đây để phân tích cùng với mempool
    /// 
    /// # Returns
    /// Danh sách các cảnh báo về mẫu giao dịch đáng ngờ
    async fn detect_suspicious_patterns(&self, 
        chain_id: u32, 
        lookback_blocks: u32
    ) -> Result<Vec<MempoolAlert>>;
    
    /// Phát hiện cơ hội MEV từ mempool
    /// 
    /// Tìm kiếm các cơ hội MEV cụ thể từ các giao dịch trong mempool,
    /// như arbitrage, sandwich attack, liquidation, hoặc JIT.
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain cần phân tích
    /// * `opportunity_types` - Các loại cơ hội MEV cần tìm (None = tất cả)
    /// 
    /// # Returns
    /// Danh sách các cơ hội MEV được phát hiện
    async fn detect_mev_opportunities(&self, 
        chain_id: u32,
        opportunity_types: Option<Vec<MevOpportunityType>>
    ) -> Result<Vec<MevOpportunity>>;
    
    /// Đăng ký callback khi phát hiện cơ hội MEV mới
    /// 
    /// Thiết lập một callback để được thông báo khi có cơ hội MEV mới được phát hiện,
    /// cho phép phản ứng nhanh với các cơ hội trên blockchain.
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain cần theo dõi
    /// * `callback` - Hàm callback được gọi khi phát hiện cơ hội
    /// 
    /// # Returns
    /// ID đăng ký để có thể hủy đăng ký sau này
    async fn subscribe_to_opportunities(&self, 
        chain_id: u32,
        callback: Arc<dyn Fn(MevOpportunity) -> Result<()> + Send + Sync + 'static>
    ) -> Result<String>;
    
    /// Hủy đăng ký callback
    /// 
    /// Hủy đăng ký callback đã thiết lập trước đó.
    /// 
    /// # Tham số
    /// * `subscription_id` - ID đăng ký cần hủy
    async fn unsubscribe(&self, subscription_id: &str) -> Result<()>;
    
    /// Dự đoán thời gian xác nhận của giao dịch dựa trên gas
    /// 
    /// Ước tính thời gian (giây) cho đến khi một giao dịch được xác nhận,
    /// dựa trên giá gas hiện tại và ngưỡng gas đề xuất của giao dịch.
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain cần dự đoán
    /// * `gas_price_gwei` - Giá gas của giao dịch (Gwei)
    /// * `gas_limit` - Gas limit của giao dịch
    /// 
    /// # Returns
    /// Thời gian ước tính (giây) cho đến khi giao dịch được xác nhận
    async fn estimate_confirmation_time(&self, 
        chain_id: u32,
        gas_price_gwei: f64, 
        gas_limit: u64
    ) -> Result<u64>;
    
    /// Phân tích gas percentile và đề xuất giá gas
    /// 
    /// Cung cấp các đề xuất giá gas dựa trên phần trăm giao dịch 
    /// được xác nhận trong block gần đây.
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain cần phân tích
    /// 
    /// # Returns
    /// Bảng các đề xuất giá gas (fast, standard, slow) theo Gwei
    async fn get_gas_suggestions(&self, chain_id: u32) -> Result<HashMap<String, f64>>;
}

/// Token Analysis Provider
///
/// Trait cho phép MEV logic sử dụng các dịch vụ phân tích token.
/// Định nghĩa giao diện chuẩn cho việc đánh giá độ an toàn, phát hiện 
/// các vấn đề bảo mật, và theo dõi thanh khoản của token.
///
/// Các provider này chịu trách nhiệm:
/// - Phân tích smart contract của token để phát hiện các vấn đề bảo mật
/// - Đánh giá độ an toàn và rủi ro của token
/// - Theo dõi sự thay đổi thanh khoản và phát hiện dấu hiệu rug pull
/// - Cung cấp thông tin chi tiết về token để ra quyết định giao dịch
#[async_trait]
pub trait TokenAnalysisProvider: Send + Sync + 'static {
    /// Phân tích an toàn token
    /// 
    /// Phân tích smart contract của token để đánh giá mức độ an toàn.
    /// Kiểm tra các tính năng như honeypot, tax cao, blacklist, v.v.
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain chứa token
    /// * `token_address` - Địa chỉ của token cần phân tích
    /// 
    /// # Returns
    /// Thông tin về độ an toàn của token
    async fn analyze_token_safety(&self, 
        chain_id: u32, 
        token_address: &str
    ) -> Result<TokenSafety>;
    
    /// Kiểm tra các vấn đề bảo mật của token
    /// 
    /// Trả về danh sách các vấn đề bảo mật cụ thể được phát hiện trong token.
    /// Các vấn đề có thể bao gồm honeypot, tax động, proxy contract, v.v.
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain chứa token
    /// * `token_address` - Địa chỉ của token cần kiểm tra
    /// 
    /// # Returns
    /// Danh sách các vấn đề bảo mật được phát hiện
    async fn check_token_issues(&self, 
        chain_id: u32, 
        token_address: &str
    ) -> Result<Vec<TokenIssue>>;
    
    /// Đánh giá xem token có an toàn để giao dịch không
    /// 
    /// Kiểm tra nhanh để xác định xem token có đạt ngưỡng an toàn tối thiểu không.
    /// Sử dụng điểm rủi ro để so sánh với ngưỡng chấp nhận được.
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain chứa token
    /// * `token_address` - Địa chỉ của token cần đánh giá
    /// * `max_risk_score` - Điểm rủi ro tối đa chấp nhận được (0-100)
    /// 
    /// # Returns
    /// true nếu token đủ an toàn để giao dịch, false nếu không
    async fn is_safe_to_trade(&self, 
        chain_id: u32, 
        token_address: &str, 
        max_risk_score: u8
    ) -> Result<bool>;
    
    /// Phân tích chi tiết token với kết quả đầy đủ
    /// 
    /// Thực hiện phân tích toàn diện và cung cấp kết quả chi tiết về token,
    /// bao gồm điểm an toàn, danh sách vấn đề, trạng thái xác minh, v.v.
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain chứa token
    /// * `token_address` - Địa chỉ của token cần phân tích
    /// 
    /// # Returns
    /// Kết quả kiểm tra bảo mật chi tiết
    async fn get_detailed_token_analysis(&self, 
        chain_id: u32, 
        token_address: &str
    ) -> Result<SecurityCheckResult>;
    
    /// Theo dõi thay đổi thanh khoản của token
    /// 
    /// Thiết lập callback để nhận thông báo khi thanh khoản của token thay đổi đáng kể,
    /// hữu ích để phát hiện rug pull hoặc các sự kiện đột biến khác.
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain chứa token
    /// * `token_address` - Địa chỉ của token cần theo dõi
    /// * `callback` - Hàm callback được gọi khi thanh khoản thay đổi, với tham số:
    ///   * `f64` - Giá trị thanh khoản mới (USD)
    ///   * `bool` - true nếu là thay đổi đáng ngờ (rug pull)
    /// 
    /// # Returns
    /// ID theo dõi để có thể hủy sau này
    async fn monitor_token_liquidity(&self, 
        chain_id: u32, 
        token_address: &str, 
        callback: Arc<dyn Fn(f64, bool) -> Result<()> + Send + Sync + 'static>
    ) -> Result<String>;
    
    /// Hủy theo dõi token
    /// 
    /// Hủy việc theo dõi thanh khoản token đã thiết lập trước đó.
    /// 
    /// # Tham số
    /// * `monitoring_id` - ID theo dõi cần hủy
    async fn stop_monitoring(&self, monitoring_id: &str) -> Result<()>;
}

/// Risk Analysis Provider
///
/// Trait cho phép MEV logic sử dụng dịch vụ phân tích rủi ro.
/// Định nghĩa giao diện chuẩn để đánh giá rủi ro của các giao dịch
/// và cơ hội MEV.
///
/// Các provider này chịu trách nhiệm:
/// - Phân tích rủi ro của các giao dịch token
/// - Đánh giá rủi ro của các cơ hội MEV
/// - Xác định mức rủi ro phù hợp với tình hình thị trường
/// - Đưa ra các khuyến nghị về rủi ro cho các phương pháp thực thi
#[async_trait]
pub trait RiskAnalysisProvider: Send + Sync + 'static {
    /// Phân tích rủi ro giao dịch
    /// 
    /// Đánh giá toàn diện rủi ro của một giao dịch tiềm năng,
    /// bao gồm rủi ro token và rủi ro thị trường.
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain chứa token
    /// * `token_address` - Địa chỉ của token cần giao dịch
    /// * `trade_amount_usd` - Giá trị giao dịch dự kiến (USD)
    /// 
    /// # Returns
    /// Phân tích rủi ro chi tiết của giao dịch
    async fn analyze_trade_risk(&self, 
        chain_id: u32, 
        token_address: &str, 
        trade_amount_usd: f64
    ) -> Result<TradeRiskAnalysis>;
    
    /// Tính toán điểm rủi ro tổng hợp cho MEV opportunity
    /// 
    /// Tính toán điểm rủi ro dựa trên nhiều yếu tố của một cơ hội MEV,
    /// bao gồm rủi ro token, thời gian thực hiện, và phức tạp của giao dịch.
    /// 
    /// # Tham số
    /// * `opportunity` - Cơ hội MEV cần đánh giá
    /// 
    /// # Returns
    /// Điểm rủi ro tổng hợp cho cơ hội
    async fn calculate_opportunity_risk(&self, 
        opportunity: &MevOpportunity
    ) -> Result<RiskScore>;
    
    /// Kiểm tra xem cơ hội có đáp ứng tiêu chí rủi ro không
    /// 
    /// Đánh giá nhanh để xác định xem cơ hội MEV có nằm trong ngưỡng
    /// rủi ro cho phép hay không.
    /// 
    /// # Tham số
    /// * `opportunity` - Cơ hội MEV cần đánh giá
    /// * `max_risk_score` - Điểm rủi ro tối đa chấp nhận được (0-100)
    /// 
    /// # Returns
    /// true nếu cơ hội đáp ứng tiêu chí rủi ro, false nếu không
    async fn opportunity_meets_criteria(&self, 
        opportunity: &MevOpportunity, 
        max_risk_score: u8
    ) -> Result<bool>;
    
    /// Đánh giá rủi ro của phương pháp thực thi
    /// 
    /// Đánh giá mức độ rủi ro của một phương pháp thực thi MEV cụ thể,
    /// như flash loan, multi-hop swap, v.v.
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain
    /// * `method` - Phương pháp thực thi cần đánh giá
    /// 
    /// # Returns
    /// Điểm rủi ro (0-100) của phương pháp thực thi
    async fn evaluate_execution_method_risk(&self, 
        chain_id: u32,
        method: MevExecutionMethod
    ) -> Result<u8>;
    
    /// Thích nghi ngưỡng rủi ro dựa trên biến động thị trường
    /// 
    /// Điều chỉnh ngưỡng rủi ro dựa trên mức độ biến động hiện tại của thị trường.
    /// Thị trường biến động cao thường cần ngưỡng rủi ro nghiêm ngặt hơn.
    /// 
    /// # Tham số
    /// * `base_threshold` - Ngưỡng rủi ro cơ bản
    /// * `market_volatility` - Mức độ biến động thị trường (0.0-1.0)
    /// 
    /// # Returns
    /// Ngưỡng rủi ro đã được điều chỉnh
    async fn adapt_risk_threshold(&self, 
        base_threshold: u8, 
        market_volatility: f64
    ) -> Result<u8>;
}

/// MEV Opportunity Provider
///
/// Trait để truyền các cơ hội MEV từ analys module đến mev_logic.
/// Định nghĩa giao diện chuẩn để quản lý và cung cấp các cơ hội MEV
/// được phát hiện qua việc phân tích mempool và blockchain.
///
/// Các provider này chịu trách nhiệm:
/// - Lưu trữ và quản lý các cơ hội MEV được phát hiện
/// - Lọc và phân loại các cơ hội theo loại và điều kiện
/// - Theo dõi trạng thái của các cơ hội MEV
/// - Thông báo cho các module khác khi có cơ hội mới
#[async_trait]
pub trait MevOpportunityProvider: Send + Sync + 'static {
    /// Lấy tất cả các cơ hội MEV hiện có
    /// 
    /// Trả về danh sách tất cả các cơ hội MEV hiện tại và hợp lệ
    /// trên một blockchain cụ thể.
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain cần truy vấn
    /// 
    /// # Returns
    /// Danh sách các cơ hội MEV
    async fn get_all_opportunities(&self, chain_id: u32) -> Result<Vec<MevOpportunity>>;
    
    /// Lấy các cơ hội MEV theo loại
    /// 
    /// Trả về danh sách các cơ hội MEV thuộc loại cụ thể
    /// trên một blockchain cụ thể.
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain cần truy vấn
    /// * `opportunity_type` - Loại cơ hội cần lọc
    /// 
    /// # Returns
    /// Danh sách các cơ hội MEV thuộc loại được chỉ định
    async fn get_opportunities_by_type(&self, 
        chain_id: u32, 
        opportunity_type: MevOpportunityType
    ) -> Result<Vec<MevOpportunity>>;
    
    /// Đăng ký nhận thông báo cho cơ hội mới
    /// 
    /// Thiết lập một callback để nhận thông báo khi có cơ hội MEV mới
    /// được phát hiện.
    /// 
    /// # Tham số
    /// * `callback` - Hàm callback được gọi khi có cơ hội mới
    /// 
    /// # Returns
    /// ID đăng ký để có thể hủy sau này
    async fn register_opportunity_listener(&self, 
        callback: Arc<dyn Fn(MevOpportunity) -> Result<()> + Send + Sync + 'static>
    ) -> Result<String>;
    
    /// Hủy đăng ký listener
    /// 
    /// Hủy đăng ký callback đã thiết lập trước đó.
    /// 
    /// # Tham số
    /// * `listener_id` - ID đăng ký cần hủy
    async fn unregister_listener(&self, listener_id: &str) -> Result<()>;
    
    /// Cập nhật trạng thái cơ hội
    /// 
    /// Cập nhật trạng thái thực thi của một cơ hội MEV.
    /// Dùng để đánh dấu cơ hội đã được thực thi hoặc hết hạn.
    /// 
    /// # Tham số
    /// * `opportunity_id` - ID của cơ hội cần cập nhật
    /// * `executed` - true nếu cơ hội đã được thực thi, false nếu chưa
    /// * `tx_hash` - Hash giao dịch nếu đã thực thi
    async fn update_opportunity_status(&self, 
        opportunity_id: &str, 
        executed: bool, 
        tx_hash: Option<String>
    ) -> Result<()>;
    
    /// Lọc cơ hội theo điều kiện profit, risk, và loại
    /// 
    /// Trả về các cơ hội MEV phù hợp với các điều kiện lọc cụ thể.
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain cần truy vấn
    /// * `min_profit_usd` - Lợi nhuận tối thiểu (USD)
    /// * `max_risk` - Điểm rủi ro tối đa
    /// * `types` - Danh sách các loại cơ hội cần lọc (None = tất cả)
    /// 
    /// # Returns
    /// Danh sách các cơ hội MEV thỏa mãn điều kiện lọc
    async fn filter_opportunities(&self, 
        chain_id: u32,
        min_profit_usd: f64,
        max_risk: u8,
        types: Option<Vec<MevOpportunityType>>
    ) -> Result<Vec<MevOpportunity>>;
}

/// Base trait for trade coordinator that doesn't contain async methods
/// This trait is compatible with dyn dispatch and defines the core types
pub trait TradeCoordinatorBase: Send + Sync + 'static {
    /// Get executor type by ID
    fn get_executor_type(&self, executor_id: &str) -> Option<ExecutorType>;
    
    /// Check if an executor is registered
    fn is_executor_registered(&self, executor_id: &str) -> bool;
    
    /// Get opportunity by ID
    fn get_opportunity_by_id(&self, opportunity_id: &str) -> Option<SharedOpportunity>;
    
    /// Check if an opportunity is reserved
    fn is_opportunity_reserved(&self, opportunity_id: &str) -> bool;
    
    /// Get reservation for an opportunity
    fn get_reservation(&self, opportunity_id: &str) -> Option<OpportunityReservation>;
    
    /// Check if an executor has permission to release an opportunity
    fn can_release_opportunity(&self, opportunity_id: &str, executor_id: &str) -> bool;
}

/// Async methods for TradeCoordinator
/// This trait contains all async methods that were in the original TradeCoordinator
#[async_trait]
pub trait TradeCoordinatorAsync: TradeCoordinatorBase {
    /// Register a trade executor
    /// 
    /// Đăng ký một executor với coordinator để tham gia vào hệ thống chia sẻ cơ hội.
    /// Mỗi executor cần đăng ký để nhận được cơ hội và chia sẻ cơ hội với các executor khác.
    /// 
    /// # Tham số
    /// * `executor_id` - ID duy nhất định danh executor
    /// * `executor_type` - Loại executor (SmartTrade, MevBot, ManualTrade, Custom)
    /// 
    /// # Returns
    /// Ok(()) nếu đăng ký thành công, Error nếu không thành công
    async fn register_executor(&self, executor_id: &str, executor_type: ExecutorType) -> Result<()>;
    
    /// Unregister a trade executor
    /// 
    /// Hủy đăng ký một executor khỏi coordinator, dọn dẹp tất cả tài nguyên
    /// và subscription liên quan đến executor này.
    /// 
    /// # Tham số
    /// * `executor_id` - ID của executor cần hủy đăng ký
    /// 
    /// # Returns
    /// Ok(()) nếu hủy đăng ký thành công, Error nếu không thành công
    async fn unregister_executor(&self, executor_id: &str) -> Result<()>;
    
    /// Share a new trading opportunity with other executors
    /// 
    /// Chia sẻ một cơ hội giao dịch mới được phát hiện để các executor khác có thể
    /// đánh giá và thực thi nếu phù hợp. Cơ hội được lưu trữ và broadcast
    /// đến tất cả các executor đã đăng ký.
    /// 
    /// # Tham số
    /// * `from_executor` - ID của executor chia sẻ cơ hội
    /// * `opportunity` - Thông tin chi tiết về cơ hội giao dịch
    /// 
    /// # Returns
    /// Ok(()) nếu chia sẻ thành công, Error nếu không thành công
    async fn share_opportunity(&self, 
        from_executor: &str,
        opportunity: SharedOpportunity,
    ) -> Result<()>;
    
    /// Subscribe to opportunity updates
    /// 
    /// Đăng ký nhận thông báo khi có cơ hội mới được chia sẻ. Callback
    /// sẽ được gọi mỗi khi có cơ hội mới phù hợp với criteria.
    /// 
    /// # Tham số
    /// * `executor_id` - ID của executor đăng ký
    /// * `callback` - Hàm callback được gọi khi có cơ hội mới, nhận SharedOpportunity làm tham số
    /// 
    /// # Returns
    /// ID đăng ký để có thể hủy đăng ký sau này
    async fn subscribe_to_opportunities(&self, 
        executor_id: &str,
        callback: Arc<dyn Fn(SharedOpportunity) -> Result<()> + Send + Sync + 'static>,
    ) -> Result<String>;
    
    /// Unsubscribe from opportunity updates
    /// 
    /// Hủy đăng ký nhận thông báo về cơ hội mới, dựa trên subscription_id
    /// đã nhận được khi đăng ký trước đó.
    /// 
    /// # Tham số
    /// * `subscription_id` - ID đăng ký cần hủy
    /// 
    /// # Returns
    /// Ok(()) nếu hủy đăng ký thành công, Error nếu không thành công
    async fn unsubscribe_from_opportunities(&self, subscription_id: &str) -> Result<()>;
    
    /// Reserve an opportunity for an executor
    /// 
    /// Đặt trước một cơ hội để executor có thể xử lý mà không bị 
    /// executor khác cạnh tranh. Sử dụng mức độ ưu tiên để xác định
    /// quyền đặt trước trong trường hợp xảy ra xung đột.
    /// 
    /// # Tham số
    /// * `opportunity_id` - ID của cơ hội cần đặt trước
    /// * `executor_id` - ID của executor đặt trước
    /// * `priority` - Mức độ ưu tiên của việc đặt trước (Low, Medium, High, Critical)
    /// 
    /// # Returns
    /// true nếu đặt trước thành công, false nếu không thể đặt trước (đã có người khác)
    async fn reserve_opportunity(&self, 
        opportunity_id: &str, 
        executor_id: &str,
        priority: OpportunityPriority,
    ) -> Result<bool>;
    
    /// Release a previously reserved opportunity
    /// 
    /// Giải phóng một cơ hội đã đặt trước, cho phép các executor khác
    /// có thể sử dụng. Chỉ executor đã đặt trước mới có thể giải phóng.
    /// 
    /// # Tham số
    /// * `opportunity_id` - ID của cơ hội cần giải phóng
    /// * `executor_id` - ID của executor yêu cầu giải phóng
    /// 
    /// # Returns
    /// Ok(()) nếu giải phóng thành công, Error nếu không thành công
    async fn release_opportunity(&self, opportunity_id: &str, executor_id: &str) -> Result<()>;
    
    /// Get all available opportunities
    /// 
    /// Lấy danh sách tất cả các cơ hội hiện có, bao gồm cả những cơ hội
    /// đã được đặt trước và chưa được đặt trước.
    /// 
    /// # Returns
    /// Danh sách các cơ hội giao dịch
    async fn get_all_opportunities(&self) -> Result<Vec<SharedOpportunity>>;
    
    /// Get statistics about opportunity sharing and usage
    /// 
    /// Lấy thống kê về việc chia sẻ và sử dụng cơ hội, bao gồm
    /// số lượng cơ hội theo loại, nguồn, tỷ lệ thành công, v.v.
    /// 
    /// # Returns
    /// Đối tượng SharingStatistics chứa thông tin thống kê
    async fn get_sharing_statistics(&self) -> Result<SharingStatistics>;
    
    /// Update global optimization parameters based on market conditions
    /// 
    /// Cập nhật các tham số tối ưu hóa toàn cục dựa trên điều kiện thị trường.
    /// Các tham số này được chia sẻ giữa các executor để đảm bảo 
    /// chiến lược giao dịch nhất quán trong toàn hệ thống.
    /// 
    /// # Tham số
    /// * `parameters` - Bảng ánh xạ tên tham số đến giá trị
    /// 
    /// # Returns
    /// Ok(()) nếu cập nhật thành công, Error nếu không thành công
    async fn update_global_parameters(&self, parameters: HashMap<String, f64>) -> Result<()>;
    
    /// Get current global parameters
    /// 
    /// Lấy giá trị hiện tại của các tham số tối ưu hóa toàn cục.
    /// Các executor có thể sử dụng các tham số này để điều chỉnh
    /// chiến lược giao dịch theo điều kiện thị trường hiện tại.
    /// 
    /// # Returns
    /// Bảng ánh xạ tên tham số đến giá trị
    async fn get_global_parameters(&self) -> Result<HashMap<String, f64>>;
}

/// Complete TradeCoordinator trait combines the base and async traits
/// This trait should be used for implementation
#[async_trait]
pub trait TradeCoordinator: TradeCoordinatorBase + TradeCoordinatorAsync {
}

// Blanket implementation for any type that implements both base traits
impl<T: TradeCoordinatorBase + TradeCoordinatorAsync> TradeCoordinator for T {}

/// Enum cho loại executor
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

/// Opportunity to be shared between executors
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

/// Type of opportunity being shared
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