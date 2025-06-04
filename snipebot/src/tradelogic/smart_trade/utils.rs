/// Utility implementation for SmartTradeExecutor
///
/// This module contains utility functions and helpers for the SmartTradeExecutor
/// that don't fit neatly into other categories.

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use tokio::sync::RwLock;
use anyhow::{Result, Context, anyhow};

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::token_status::TokenSafety;
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};

// Module imports
use super::types::{TradeTracker, TradeStatus, TradeResult, TradeStrategy};
use super::constants::{UNKNOWN_FAILURE_REASON, HIGH_SLIPPAGE_FORMAT, INSUFFICIENT_LIQUIDITY_FORMAT, DEFAULT_TEST_AMOUNT};
use super::executor::SmartTradeExecutor;
use crate::tradelogic::common::utils::{
    calculate_profit,
    wait_for_transaction,
    convert_to_token_units,
    current_time_seconds,
    generate_trade_id,
};

impl SmartTradeExecutor {
    /// Lấy trade từ database bằng ID
    ///
    /// # Parameters
    /// - `trade_id`: ID của giao dịch
    ///
    /// # Returns
    /// - `Option<TradeTracker>`: Thông tin giao dịch nếu tìm thấy
    pub async fn get_trade_by_id(&self, trade_id: &str) -> Option<TradeTracker> {
        // Kiểm tra trong cache trước
        {
            let active_trades = self.active_trades.read().await;
            for trade in active_trades.iter() {
                if trade.trade_id == trade_id {
                    return Some(trade.clone());
                }
            }
        }
        
        // Kiểm tra trong database nếu không tìm thấy trong cache
        if let Some(db) = &self.trade_log_db {
            if let Ok(trade) = db.get_trade_by_id(trade_id).await {
                return Some(trade);
            }
        }
        
        None
    }
    
    /// Kiểm tra nếu token là stable coin
    ///
    /// # Parameters
    /// - `token_address`: Địa chỉ token
    /// - `chain_id`: ID của blockchain
    ///
    /// # Returns
    /// - `bool`: True nếu là stable coin
    pub fn is_stable_coin(&self, token_address: &str, chain_id: u32) -> bool {
        let stable_coins = match chain_id {
            1 => vec![ // Ethereum
                "0xdac17f958d2ee523a2206206994597c13d831ec7", // USDT
                "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", // USDC
                "0x6b175474e89094c44da98b954eedeac495271d0f", // DAI
                "0x4fabb145d64652a948d72533023f6e7a623c7c53", // BUSD
            ],
            56 => vec![ // BSC
                "0x55d398326f99059ff775485246999027b3197955", // USDT (BSC)
                "0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d", // USDC (BSC)
                "0xe9e7cea3dedca5984780bafc599bd69add087d56", // BUSD
            ],
            _ => vec![],
        };
        
        let token_key = token_address.to_lowercase();
        stable_coins.contains(&token_key.as_str())
    }
    
    /// Tính toán số lượng token tối đa có thể mua với số tiền
    ///
    /// # Parameters
    /// - `amount`: Số tiền (ETH/BNB)
    /// - `token_price`: Giá token
    /// - `slippage`: Phần trăm slippage
    ///
    /// # Returns
    /// - `f64`: Số lượng token có thể mua
    pub fn calculate_max_tokens(&self, amount: f64, token_price: f64, slippage: f64) -> f64 {
        if token_price <= 0.0 {
            return 0.0;
        }
        
        // Trừ đi phí gas ước tính
        let amount_after_gas = amount - GAS_COST_ETH;
        if amount_after_gas <= 0.0 {
            return 0.0;
        }
        
        // Tính toán với slippage
        let effective_price = token_price * (1.0 + slippage / 100.0);
        amount_after_gas / effective_price
    }
    
    /// Kiểm tra xem có nên tiếp tục theo dõi token hay không
    ///
    /// # Parameters
    /// - `token_address`: Địa chỉ token
    /// - `chain_id`: ID của blockchain
    /// - `start_time`: Thời điểm bắt đầu theo dõi
    /// - `max_hold_time`: Thời gian tối đa giữ token (giây)
    ///
    /// # Returns
    /// - `bool`: True nếu nên tiếp tục theo dõi
    pub fn should_continue_monitoring(&self, token_address: &str, chain_id: u32, start_time: DateTime<Utc>, max_hold_time: u64) -> bool {
        // Kiểm tra thời gian giữ tối đa
        let current_time = Utc::now();
        let hold_duration = current_time.signed_duration_since(start_time);
        
        // Chuyển đổi sang giây
        let hold_seconds = hold_duration.num_seconds() as u64;
        
        // Nếu đã giữ quá lâu, không tiếp tục theo dõi
        if hold_seconds > max_hold_time {
            info!(
                "Dừng theo dõi token {} vì đã quá thời gian tối đa ({} giây)",
                token_address, max_hold_time
            );
            return false;
        }
        
        // Kiểm tra xem có trong blacklist không
        if self.is_in_blacklist(chain_id, token_address).await {
            info!(
                "Dừng theo dõi token {} vì đã bị đưa vào blacklist",
                token_address
            );
            return false;
        }
        
        true
    }
    
    /// Tính giá trung bình di động (Moving Average)
    ///
    /// # Parameters
    /// - `price_history`: Lịch sử giá (từ cũ đến mới)
    /// - `period`: Số điểm dữ liệu để tính trung bình
    ///
    /// # Returns
    /// - `f64`: Giá trị trung bình di động
    pub fn calculate_moving_average(&self, price_history: &[f64], period: usize) -> f64 {
        if price_history.is_empty() || period == 0 {
            return 0.0;
        }
        
        let period = period.min(price_history.len());
        let recent_prices = &price_history[price_history.len() - period..];
        
        let sum: f64 = recent_prices.iter().sum();
        sum / period as f64
    }
    
    /// Tìm giá trị cực tiểu/cực đại trong lịch sử giá
    ///
    /// # Parameters
    /// - `price_history`: Lịch sử giá (từ cũ đến mới)
    /// - `period`: Số điểm dữ liệu để xét
    ///
    /// # Returns
    /// - `(f64, f64)`: (Giá thấp nhất, giá cao nhất)
    pub fn find_price_extremes(&self, price_history: &[f64], period: usize) -> (f64, f64) {
        if price_history.is_empty() {
            return (0.0, 0.0);
        }
        
        let period = period.min(price_history.len());
        let recent_prices = &price_history[price_history.len() - period..];
        
        let min_price = recent_prices.iter()
            .min_by(|a, b| match a.partial_cmp(b) {
                Some(ordering) => ordering,
                None => {
                    tracing::warn!("NaN encountered in price comparison for min_price");
                    std::cmp::Ordering::Equal
                }
            })
            .map(|&price| price)
            .unwrap_or(0.0);
        
        let max_price = recent_prices.iter()
            .max_by(|a, b| match a.partial_cmp(b) {
                Some(ordering) => ordering,
                None => {
                    tracing::warn!("NaN encountered in price comparison for max_price");
                    std::cmp::Ordering::Equal
                }
            })
            .map(|&price| price)
            .unwrap_or(0.0);
        
        (min_price, max_price)
    }
    
    /// Tính chỉ số sức mạnh tương đối (RSI - Relative Strength Index)
    ///
    /// # Parameters
    /// - `price_history`: Lịch sử giá (từ cũ đến mới)
    /// - `period`: Số ngày cho RSI (thường là 14)
    ///
    /// # Returns
    /// - `f64`: Giá trị RSI (0-100)
    pub fn calculate_rsi(&self, price_history: &[f64], period: usize) -> f64 {
        if price_history.len() <= period {
            return 50.0; // Không đủ dữ liệu, trả về giá trị trung tính
        }
        
        let mut gains = Vec::new();
        let mut losses = Vec::new();
        
        // Tính các giá trị tăng/giảm
        for i in 1..price_history.len() {
            let change = price_history[i] - price_history[i-1];
            if change >= 0.0 {
                gains.push(change);
                losses.push(0.0);
            } else {
                gains.push(0.0);
                losses.push(change.abs());
            }
        }
        
        // Lấy dữ liệu trong period
        let recent_gains = &gains[gains.len() - period..];
        let recent_losses = &losses[losses.len() - period..];
        
        // Tính trung bình
        let avg_gain: f64 = recent_gains.iter().sum::<f64>() / period as f64;
        let avg_loss: f64 = recent_losses.iter().sum::<f64>() / period as f64;
        
        // Tính RS và RSI
        if avg_loss == 0.0 {
            return 100.0; // Tránh chia cho 0
        }
        
        let rs = avg_gain / avg_loss;
        100.0 - (100.0 / (1.0 + rs))
    }
}

/// Execute a trade with the given parameters and update the trade tracker
///
/// This function centralizes the actual trade execution logic that was previously
/// duplicated across multiple locations in the executor.rs file.
///
/// # Arguments
/// * `adapter` - Reference to the blockchain adapter for the target chain
/// * `params` - Trade parameters
/// * `trade_id` - Unique ID for the trade
/// * `tracker` - Trade tracker object to update with execution results
/// * `active_trades` - Reference to the collection of active trades
///
/// # Returns
/// * `Result<TradeResult>` - Result of the trade execution
pub async fn execute_trade_with_params(
    adapter: &Arc<EvmAdapter>,
    params: &TradeParams,
    trade_id: &str,
    tracker: TradeTracker,
    active_trades: &RwLock<Vec<TradeTracker>>,
) -> Result<TradeResult> {
    let now = Utc::now().timestamp() as u64;
    
    // Log the trade execution attempt
    info!(
        "Executing trade on chain {} for token {}, amount: {}, type: {:?}",
        params.chain_id, params.token_address, params.amount, params.trade_type
    );
    
    // Step 1: Get current token price before execution
    let current_price = match adapter.get_token_price(&params.token_address).await {
        Ok(price) => price,
        Err(e) => {
            warn!("Could not get token price: {}, using 0.0", e);
            0.0
        }
    };
    
    // Step 2: Prepare transaction parameters
    let slippage = params.slippage.unwrap_or(0.5); // Default 0.5% slippage
    let deadline = params.deadline.unwrap_or(now + 300); // Default 5 minutes
    
    let gas_price = match adapter.get_gas_price().await {
        Ok(price) => price * 1.05, // Add 5% to current gas price
        Err(_) => 0.0, // Use default gas price
    };
    
    // Step 3: Execute the transaction
    let tx_result = match params.trade_type {
        TradeType::Buy => {
            adapter.execute_buy_token(
                &params.token_address,
                &params.amount.to_string(),
                slippage,
                deadline,
                gas_price,
            ).await
        },
        TradeType::Sell => {
            adapter.execute_sell_token(
                &params.token_address,
                &params.amount.to_string(),
                slippage,
                deadline,
                gas_price,
            ).await
        },
        _ => {
            return Err(anyhow!("Unsupported trade type: {:?}", params.trade_type));
        }
    };
    
    // Step 4: Process the result
    let transaction_hash = match tx_result {
        Ok(hash) => hash,
        Err(e) => {
            error!("Trade execution failed: {}", e);
            
            // Update tracker with failed status
            let mut updated_tracker = tracker.clone();
            updated_tracker.status = TradeStatus::Failed;
            updated_tracker.updated_at = Utc::now().timestamp() as u64;
            updated_tracker.error_message = Some(format!("Execution failed: {}", e));
            
            // Update active trades
            {
                let mut trades = active_trades.write().await;
                if let Some(pos) = trades.iter().position(|t| t.id == updated_tracker.id) {
                    trades[pos] = updated_tracker;
                } else {
                    trades.push(updated_tracker);
                }
            }
            
            // Return result with failed status
            return Ok(TradeResult {
                id: trade_id.to_string(),
                chain_id: params.chain_id,
                token_address: params.token_address.clone(),
                token_name: "Unknown".to_string(),
                token_symbol: "UNK".to_string(),
                amount: params.amount,
                entry_price: 0.0,
                exit_price: None,
                current_price,
                profit_loss: 0.0,
                status: TradeStatus::Failed,
                strategy: tracker.strategy,
                created_at: now,
                updated_at: now,
                completed_at: Some(now),
                exit_reason: Some("Execution failed".to_string()),
                gas_used: 0.0,
                safety_score: 0,
                risk_factors: Vec::new(),
            });
        }
    };
    
    // Step 5: Wait for transaction to be mined
    info!("Transaction submitted: {}", transaction_hash);
    
    let tx_receipt = match adapter.wait_for_transaction(&transaction_hash, 180).await {
        Ok(receipt) => receipt,
        Err(e) => {
            warn!("Could not get transaction receipt: {}", e);
            
            // Update tracker with pending status
            let mut updated_tracker = tracker.clone();
            updated_tracker.status = TradeStatus::Pending;
            updated_tracker.updated_at = Utc::now().timestamp() as u64;
            updated_tracker.transaction_hash = Some(transaction_hash.clone());
            
            // Update active trades
            {
                let mut trades = active_trades.write().await;
                if let Some(pos) = trades.iter().position(|t| t.id == updated_tracker.id) {
                    trades[pos] = updated_tracker;
                } else {
                    trades.push(updated_tracker);
                }
            }
            
            // Return result with pending status
            return Ok(TradeResult {
                id: trade_id.to_string(),
                chain_id: params.chain_id,
                token_address: params.token_address.clone(),
                token_name: "Unknown".to_string(),
                token_symbol: "UNK".to_string(),
                amount: params.amount,
                entry_price: current_price,
                exit_price: None,
                current_price,
                profit_loss: 0.0,
                status: TradeStatus::Pending,
                strategy: tracker.strategy,
                created_at: now,
                updated_at: now,
                completed_at: None,
                exit_reason: None,
                gas_used: 0.0,
                safety_score: 0,
                risk_factors: Vec::new(),
            });
        }
    };
    
    // Step 6: Determine if transaction was successful
    let status = if tx_receipt.status {
        TradeStatus::Active
    } else {
        TradeStatus::Failed
    };
    
    // Step 7: Get updated token price after execution
    let updated_price = match adapter.get_token_price(&params.token_address).await {
        Ok(price) => price,
        Err(_) => current_price,
    };
    
    // Step 8: Update trade tracker
    let mut updated_tracker = tracker.clone();
    updated_tracker.status = status.clone();
    updated_tracker.updated_at = Utc::now().timestamp() as u64;
    updated_tracker.transaction_hash = Some(transaction_hash);
    updated_tracker.entry_price = current_price;
    updated_tracker.current_price = updated_price;
    updated_tracker.gas_used = tx_receipt.gas_used.unwrap_or(0) as f64;
    
    // Update active trades
    {
        let mut trades = active_trades.write().await;
        if let Some(pos) = trades.iter().position(|t| t.id == updated_tracker.id) {
            trades[pos] = updated_tracker;
        } else {
            trades.push(updated_tracker);
        }
    }
    
    // Step 9: Create and return trade result
    let result = TradeResult {
        id: trade_id.to_string(),
        chain_id: params.chain_id,
        token_address: params.token_address.clone(),
        token_name: "Unknown".to_string(), // Would be populated from token data
        token_symbol: "UNK".to_string(),   // Would be populated from token data
        amount: params.amount,
        entry_price: current_price,
        exit_price: None,
        current_price: updated_price,
        profit_loss: if current_price > 0.0 {
            (updated_price - current_price) / current_price * 100.0
        } else {
            0.0
        },
        status,
        strategy: tracker.strategy,
        created_at: now,
        updated_at: now,
        completed_at: if status == TradeStatus::Failed { Some(now) } else { None },
        exit_reason: if status == TradeStatus::Failed { Some("Transaction failed".to_string()) } else { None },
        gas_used: tx_receipt.gas_used.unwrap_or(0) as f64,
        safety_score: 0,
        risk_factors: Vec::new(),
    };
    
    Ok(result)
}

/// Create a trade tracker object from the given parameters
///
/// This function centralizes the creation of TradeTracker objects to avoid
/// duplicated code across different executor methods.
///
/// # Arguments
/// * `params` - Trade parameters
/// * `trade_id` - Unique ID for the trade
/// * `timestamp` - Creation timestamp
///
/// # Returns
/// * `TradeTracker` - A new trade tracker instance
pub fn create_trade_tracker(
    params: &TradeParams,
    trade_id: &str,
    timestamp: u64,
) -> TradeTracker {
    let strategy = if let Some(strategy_type) = &params.strategy {
        match strategy_type.as_str() {
            "dca" => TradeStrategy::DollarCostAverage {
                interval: params.custom_params.get("interval").map(|v| v.parse::<u64>().unwrap_or(86400)).unwrap_or(86400),
                total_periods: params.custom_params.get("total_periods").map(|v| v.parse::<u32>().unwrap_or(10)).unwrap_or(10),
                completed_periods: 0,
            },
            "grid" => TradeStrategy::Grid {
                upper_bound: params.custom_params.get("upper_bound").map(|v| v.parse::<f64>().unwrap_or(0.0)).unwrap_or(0.0),
                lower_bound: params.custom_params.get("lower_bound").map(|v| v.parse::<f64>().unwrap_or(0.0)).unwrap_or(0.0),
                grid_levels: params.custom_params.get("grid_levels").map(|v| v.parse::<u32>().unwrap_or(5)).unwrap_or(5),
            },
            "trailing_stop" => TradeStrategy::TrailingStop {
                activation_percent: params.custom_params.get("activation_percent").map(|v| v.parse::<f64>().unwrap_or(10.0)).unwrap_or(10.0),
                trail_percent: params.custom_params.get("trail_percent").map(|v| v.parse::<f64>().unwrap_or(2.0)).unwrap_or(2.0),
                highest_price: 0.0,
            },
            _ => TradeStrategy::Standard,
        }
    } else {
        TradeStrategy::Standard
    };
    
    TradeTracker {
        id: trade_id.to_string(),
        chain_id: params.chain_id,
        token_address: params.token_address.clone(),
        amount: params.amount,
        trade_type: params.trade_type.clone(),
        entry_price: 0.0,
        current_price: 0.0,
        status: TradeStatus::Pending,
        transaction_hash: None,
        error_message: None,
        gas_used: 0.0,
        strategy,
        created_at: timestamp,
        updated_at: timestamp,
        stop_loss: params.stop_loss,
        take_profit: params.take_profit,
        max_hold_time: params.max_hold_time.unwrap_or(86400 * 7), // Default 7 days
        custom_params: params.custom_params.clone().unwrap_or_default(),
    }
}

/// Utilities cho Smart Trade strategies
///
/// Module này cung cấp các hàm tiện ích để hỗ trợ phân tích và thực thi giao dịch,
/// bao gồm xử lý lỗi, thao tác với dữ liệu, và các helper functions.

/// Trả về lý do thất bại từ Option<String>, sử dụng giá trị mặc định nếu None
pub fn get_failure_reason(reason: Option<String>) -> String {
    reason.unwrap_or_else(|| UNKNOWN_FAILURE_REASON.to_string())
}

/// Định dạng thông báo lỗi slippage cao
pub fn format_slippage_error(slippage: f64) -> String {
    format!(HIGH_SLIPPAGE_FORMAT, slippage)
}

/// Định dạng thông báo lỗi thanh khoản không đủ
pub fn format_insufficient_liquidity(details: &str) -> String {
    format!(INSUFFICIENT_LIQUIDITY_FORMAT, details)
}

/// Trả về số lượng test mặc định cho các kiểm tra honeypot
pub fn get_default_test_amount() -> &'static str {
    DEFAULT_TEST_AMOUNT
}
