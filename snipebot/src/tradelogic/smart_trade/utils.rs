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

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::token_status::TokenSafety;
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};

// Module imports
use super::types::{TradeTracker, TradeStatus, TradeResult, TradeStrategy};
use super::constants::*;
use super::executor::SmartTradeExecutor;
use crate::tradelogic::common::utils::{
    calculate_profit,
    wait_for_transaction,
    convert_to_token_units,
    current_time_seconds
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
    
    /// Tạo ID giao dịch từ các tham số
    ///
    /// # Parameters
    /// - `token_address`: Địa chỉ token
    /// - `chain_id`: ID của blockchain
    /// - `timestamp`: Thời gian giao dịch
    ///
    /// # Returns
    /// - `String`: ID giao dịch
    pub fn generate_trade_id(&self, token_address: &str, chain_id: u32, timestamp: i64) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        token_address.hash(&mut hasher);
        chain_id.hash(&mut hasher);
        timestamp.hash(&mut hasher);
        
        let hash = hasher.finish();
        format!("{}_{:x}", chain_id, hash)
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
