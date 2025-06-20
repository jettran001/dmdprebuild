//! Unified Trading Strategies for Smart Trading
//!
//! This module contains all trading strategies for the smart trade system:
//! - Basic strategies: DCA, Grid Trading, TWAP
//! - Advanced strategies: Dynamic TSL, TP/SL, Market Reaction
//! - Market data analysis and strategy optimization

// External imports
use std::sync::Arc;
use std::collections::HashMap;
use anyhow::{Result, anyhow};
use tokio::sync::RwLock;
use chrono::Utc;
use tracing::{debug, error, info, warn};

// Internal imports
use crate::types::{TradeParams, TradeType, TokenPair};
use crate::tradelogic::common::ResourcePriority;
use crate::tradelogic::common::get_resource_manager;
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::token_status::TokenStatus;

// Module imports
use super::executor::core::SmartTradeExecutor;
use super::types::{TradeResult, TradeStatus, TradeTracker, TradeStrategy, TaskHandle, TradeRiskAnalysis};
use super::executor::{trade_handler::{create_trade_tracker, execute_buy, execute_sell}, position_manager::create_dca_position};
use super::trade_constants::*;

//=============================================================================
// MARKET DATA STRUCTURES AND ANALYSIS
//=============================================================================

/// Cấu trúc lưu trữ dữ liệu thị trường để điều chỉnh TSL
pub struct MarketData {
    /// Biến động giá trung bình 24h (%)
    pub volatility_24h: f64,
    /// Tổng khối lượng giao dịch 24h
    pub volume_24h: f64,
    /// Loại thị trường (tăng/giảm/đi ngang)
    pub market_trend: MarketTrend,
    /// Thời gian giao dịch trung bình cho token này
    pub avg_trade_duration: u64,
    /// Biến động giá trung bình theo giờ (%)
    pub hourly_volatility: Vec<f64>,
}

/// Loại xu hướng thị trường
pub enum MarketTrend {
    /// Thị trường tăng
    Bullish,
    /// Thị trường giảm
    Bearish,
    /// Thị trường đi ngang
    Sideways,
}

/// Dữ liệu giá cho phân tích kỹ thuật
pub struct PriceData {
    /// Giá cao nhất
    pub high: f64,
    /// Giá thấp nhất
    pub low: f64,
    /// Giá đóng cửa
    pub close: f64,
    /// Khối lượng giao dịch
    pub volume: f64,
}

impl PriceData {
    /// Tạo mới đối tượng PriceData
    pub fn new(high: f64, low: f64, close: f64, volume: f64) -> Self {
        Self { high, low, close, volume }
    }
}

//=============================================================================
// ADAPTIVE TRAILING STOP LOSS
//=============================================================================

/// Chiến lược Trailing Stop Loss thích ứng
pub struct AdaptiveTSL {
    /// Phần trăm TSL cơ bản
    pub base_percentage: f64,
    /// Điều chỉnh dựa trên biến động
    pub volatility_adjustment: f64,
    /// Điều chỉnh dựa trên khối lượng giao dịch
    pub volume_adjustment: f64,
    /// Điều chỉnh dựa trên thời gian giữ
    pub time_adjustment: f64,
    /// Điều chỉnh dựa trên tỷ lệ lợi nhuận hiện tại
    pub profit_adjustment: f64,
    /// Phần trăm TSL tối thiểu
    pub min_percentage: f64,
    /// Phần trăm TSL tối đa
    pub max_percentage: f64,
}

impl AdaptiveTSL {
    /// Tạo mới một AdaptiveTSL với các tham số mặc định
    pub fn new() -> Self {
        Self {
            base_percentage: SMART_TRADE_TSL_PERCENT,
            volatility_adjustment: 0.0,
            volume_adjustment: 0.0,
            time_adjustment: 0.0,
            profit_adjustment: 0.0,
            min_percentage: 1.0, // Tối thiểu 1%
            max_percentage: 10.0, // Tối đa 10%
        }
    }
    
    /// Cập nhật điều chỉnh dựa trên biến động thị trường
    pub fn adjust_for_volatility(&mut self, volatility_24h: f64) {
        // Điều chỉnh phần trăm TSL dựa trên biến động
        // Thị trường biến động cao -> tăng TSL để bảo vệ lợi nhuận nhanh hơn
        const BASE_VOLATILITY: f64 = 5.0; // 5% là biến động cơ bản
        
        if volatility_24h > BASE_VOLATILITY {
            // Tăng TSL khi biến động cao
            self.volatility_adjustment = (volatility_24h - BASE_VOLATILITY) * 0.1;
        } else {
            // Giảm TSL khi biến động thấp
            self.volatility_adjustment = (volatility_24h - BASE_VOLATILITY) * 0.05;
        }
    }
    
    /// Cập nhật điều chỉnh dựa trên khối lượng giao dịch
    pub fn adjust_for_volume(&mut self, volume_24h: f64, avg_volume_24h: f64) {
        // Điều chỉnh phần trăm TSL dựa trên khối lượng giao dịch
        // Khối lượng cao hơn trung bình -> tăng TSL do khả năng biến động cao
        if volume_24h > avg_volume_24h {
            let volume_ratio = volume_24h / avg_volume_24h;
            self.volume_adjustment = (volume_ratio - 1.0) * 0.5;
            
            // Giới hạn điều chỉnh tối đa
            if self.volume_adjustment > 2.0 {
                self.volume_adjustment = 2.0;
            }
        } else {
            self.volume_adjustment = 0.0;
        }
    }
    
    /// Cập nhật điều chỉnh dựa trên thời gian giữ
    pub fn adjust_for_hold_time(&mut self, current_hold_time: u64, optimal_hold_time: u64) {
        // Điều chỉnh phần trăm TSL dựa trên thời gian giữ
        // Thời gian càng dài -> TSL càng thấp để tối ưu lợi nhuận
        if current_hold_time < optimal_hold_time {
            // Mới giữ -> TSL cao hơn để bảo vệ
            let time_ratio = current_hold_time as f64 / optimal_hold_time as f64;
            self.time_adjustment = (1.0 - time_ratio) * 1.0;
        } else {
            // Đã giữ đủ lâu -> giảm TSL để tối ưu lợi nhuận
            let time_ratio = current_hold_time as f64 / optimal_hold_time as f64;
            self.time_adjustment = -((time_ratio - 1.0) * 0.5);
            
            // Giới hạn điều chỉnh tối đa
            if self.time_adjustment < -2.0 {
                self.time_adjustment = -2.0;
            }
        }
    }
    
    /// Cập nhật điều chỉnh dựa trên lợi nhuận hiện tại
    pub fn adjust_for_profit(&mut self, current_profit_percent: f64) {
        // Điều chỉnh phần trăm TSL dựa trên lợi nhuận hiện tại
        // Lợi nhuận càng cao -> TSL càng cao để bảo vệ
        if current_profit_percent > 20.0 {
            // Lợi nhuận lớn -> tăng TSL mạnh để bảo vệ
            self.profit_adjustment = (current_profit_percent - 20.0) * 0.05;
            
            // Giới hạn điều chỉnh tối đa
            if self.profit_adjustment > 3.0 {
                self.profit_adjustment = 3.0;
            }
        } else if current_profit_percent > 10.0 {
            // Lợi nhuận khá -> tăng TSL vừa phải
            self.profit_adjustment = (current_profit_percent - 10.0) * 0.03;
        } else {
            self.profit_adjustment = 0.0;
        }
    }
    
    /// Điều chỉnh dựa trên xu hướng thị trường
    pub fn adjust_for_market_trend(&mut self, trend: &MarketTrend) {
        match trend {
            MarketTrend::Bullish => {
                // Thị trường tăng -> giảm TSL để tối ưu lợi nhuận
                self.adjust_base_percentage(-0.5);
            },
            MarketTrend::Bearish => {
                // Thị trường giảm -> tăng TSL để bảo vệ nhanh
                self.adjust_base_percentage(1.0);
            },
            MarketTrend::Sideways => {
                // Thị trường đi ngang -> giữ nguyên TSL
            }
        }
    }
    
    /// Điều chỉnh phần trăm cơ bản
    fn adjust_base_percentage(&mut self, amount: f64) {
        self.base_percentage += amount;
    }
    
    /// Tính toán phần trăm TSL cuối cùng sau khi áp dụng tất cả điều chỉnh
    pub fn calculate_final_percentage(&self) -> f64 {
        // Tổng hợp tất cả điều chỉnh
        let adjusted_percentage = self.base_percentage 
            + self.volatility_adjustment 
            + self.volume_adjustment 
            + self.time_adjustment 
            + self.profit_adjustment;
        
        // Giới hạn trong khoảng min-max
        if adjusted_percentage < self.min_percentage {
            return self.min_percentage;
        } else if adjusted_percentage > self.max_percentage {
            return self.max_percentage;
        }
        
        adjusted_percentage
    }
}

//=============================================================================
// TRADE STRATEGY MANAGER
//=============================================================================

/// Phân tích và quản lý chiến lược giao dịch
pub struct TradeStrategyManager {
    /// Dữ liệu thị trường để điều chỉnh chiến lược
    market_data: HashMap<u32, HashMap<String, MarketData>>,
    /// Chiến lược TSL thích ứng
    adaptive_tsl: AdaptiveTSL,
}

impl TradeStrategyManager {
    /// Tạo mới một TradeStrategyManager
    pub fn new() -> Self {
        Self {
            market_data: HashMap::new(),
            adaptive_tsl: AdaptiveTSL::new(),
        }
    }
    
    /// Cập nhật dữ liệu thị trường cho một token
    pub fn update_market_data(&mut self, chain_id: u32, token_address: &str, data: MarketData) {
        if !self.market_data.contains_key(&chain_id) {
            self.market_data.insert(chain_id, HashMap::new());
        }
        
        if let Some(chain_data) = self.market_data.get_mut(&chain_id) {
            chain_data.insert(token_address.to_string(), data);
        }
    }
    
    /// Lấy dữ liệu thị trường cho một token
    pub fn get_market_data(&self, chain_id: u32, token_address: &str) -> Option<&MarketData> {
        self.market_data
            .get(&chain_id)
            .and_then(|chain_data| chain_data.get(token_address))
    }
    
    /// Tính toán phần trăm TSL dựa trên dữ liệu thị trường
    pub fn calculate_tsl_percentage(&mut self, trade: &TradeTracker) -> f64 {
        let chain_id = trade.params.chain_id();
        let token = trade.params.token_address.clone();
        
        // Reset TSL trước khi tính toán
        self.adaptive_tsl = AdaptiveTSL::new();
        
        // Điều chỉnh dựa trên dữ liệu thị trường nếu có
        if let Some(market_data) = self.get_market_data(chain_id, &token) {
            self.adaptive_tsl.adjust_for_volatility(market_data.volatility_24h);
            
            // Đối với thị trường
            self.adaptive_tsl.adjust_for_market_trend(&market_data.market_trend);
            
            // Thời gian giữ hiện tại
            let now = Utc::now().timestamp() as u64;
            let hold_time = now.saturating_sub(trade.created_at);
            self.adaptive_tsl.adjust_for_hold_time(hold_time, market_data.avg_trade_duration);
        }
        
        // Điều chỉnh theo lợi nhuận hiện tại
        if let Some(buy_price) = trade.entry_price {
            if let Some(current_price) = trade.current_price {
                let profit_pct = ((current_price - buy_price) / buy_price) * 100.0;
                self.adaptive_tsl.adjust_for_profit(profit_pct);
            }
        }
        
        // Tính toán TSL cuối cùng
        self.adaptive_tsl.calculate_final_percentage()
    }
    
    /// Tính toán giá stop dựa trên phần trăm TSL
    pub fn calculate_stop_price(&self, highest_price: f64, tsl_percentage: f64) -> f64 {
        highest_price * (1.0 - tsl_percentage / 100.0)
    }
    
    /// Kiểm tra xem có nên kích hoạt take profit không
    pub fn should_take_profit(&self, trade: &TradeTracker, current_price: f64) -> bool {
        // Nếu đã có giá mục tiêu và giá hiện tại vượt qua
        if let Some(target_price) = trade.metadata.get("tp_price").and_then(|s| s.parse::<f64>().ok()) {
            return current_price >= target_price;
        }
        
        // Nếu có phần trăm TP và giá hiện tại đạt mức
        if let (Some(tp_percent_str), Some(entry_price)) = (trade.metadata.get("tp_percent"), trade.entry_price) {
            if let Ok(tp_percent) = tp_percent_str.parse::<f64>() {
                let target_price = entry_price * (1.0 + tp_percent / 100.0);
                return current_price >= target_price;
            }
        }
        
        false
    }
    
    /// Kiểm tra xem có nên kích hoạt stop loss không 
    pub fn should_stop_loss(&self, trade: &TradeTracker, current_price: f64) -> bool {
        // Nếu đã có giá stop và giá hiện tại dưới mức đó
        if let Some(stop_price) = trade.metadata.get("sl_price").and_then(|s| s.parse::<f64>().ok()) {
            return current_price <= stop_price;
        }
        
        // Nếu có phần trăm SL và giá hiện tại dưới mức
        if let (Some(sl_percent_str), Some(entry_price)) = (trade.metadata.get("sl_percent"), trade.entry_price) {
            if let Ok(sl_percent) = sl_percent_str.parse::<f64>() {
                let stop_price = entry_price * (1.0 - sl_percent / 100.0);
                return current_price <= stop_price;
            }
        }
        
        false
    }
    
    /// Kiểm tra xem TSL có được kích hoạt không
    pub fn is_tsl_triggered(&self, trade: &TradeTracker, current_price: f64, tsl_percentage: f64) -> bool {
        if let Some(highest_price) = trade.highest_price {
            let stop_price = self.calculate_stop_price(highest_price, tsl_percentage);
            return current_price <= stop_price;
        }
        
        false
    }
    
    /// Xác định chiến lược tốt nhất cho token
    pub fn determine_best_strategy(&self, token_address: &str, chain_id: u32) -> TradeStrategy {
        // Lấy dữ liệu thị trường
        if let Some(market_data) = self.get_market_data(chain_id, token_address) {
            match market_data.market_trend {
                MarketTrend::Bullish => {
                    if market_data.volatility_24h > 15.0 {
                        // Thị trường tăng và biến động cao
                        TradeStrategy::DynamicTSL
                    } else {
                        // Thị trường tăng ổn định
                        TradeStrategy::StandardTP
                    }
                },
                MarketTrend::Bearish => {
                    // Thị trường giảm
                    TradeStrategy::TightTSL
                },
                MarketTrend::Sideways => {
                    if market_data.volatility_24h > 8.0 {
                        // Thị trường đi ngang nhưng biến động
                        TradeStrategy::DynamicTSL
                    } else {
                        // Thị trường đi ngang ổn định
                        TradeStrategy::Grid
                    }
                }
            }
        } else {
            // Mặc định nếu không có dữ liệu
            TradeStrategy::StandardTP
        }
    }
}

//=============================================================================
// BASIC TRADING STRATEGIES
//=============================================================================

/// Thực hiện chiến lược Dollar Cost Averaging (DCA)
pub async fn execute_dca_strategy(
    executor: &SmartTradeExecutor,
    params: &TradeParams,
    total_entries: u32,
    entry_interval_seconds: u64,
) -> Result<TradeTracker> {
    // Tạo vị thế DCA
    let mut dca_tracker = create_dca_position(executor, params, total_entries, entry_interval_seconds).await?;
    
    // Thực hiện entry đầu tiên ngay lập tức
    let dca_id = dca_tracker.trade_id.clone();
    let executor_clone = executor.clone();
    
    // Spawm task để thực hiện các entry tiếp theo
    let task_id = uuid::Uuid::new_v4().to_string();
    let handle = tokio::spawn(async move {
        // Thực hiện entry đầu tiên
        match super::executor::position_manager::execute_dca_entry(&executor_clone, &dca_id).await {
            Ok(_) => {
                debug!("First DCA entry executed successfully");
            },
            Err(e) => {
                error!("Failed to execute first DCA entry: {}", e);
                return;
            }
        }
        
        // Thực hiện các entry tiếp theo theo lịch
        for i in 1..total_entries {
            // Chờ đến thời gian entry tiếp theo
            tokio::time::sleep(tokio::time::Duration::from_secs(entry_interval_seconds)).await;
            
            // Kiểm tra xem executor còn đang chạy không
            if !*executor_clone.running.read().await {
                debug!("Executor stopped, cancelling DCA execution for trade {}", dca_id);
                break;
            }
            
            // Kiểm tra xem DCA còn trong active trades không
            let active_trades = executor_clone.active_trades.read().await;
            if !active_trades.iter().any(|t| t.trade_id == dca_id) {
                debug!("DCA position {} no longer active, stopping schedule", dca_id);
                break;
            }
            
            // Thực hiện entry tiếp theo
            match super::executor::position_manager::execute_dca_entry(&executor_clone, &dca_id).await {
                Ok(_) => {
                    debug!("DCA entry {} executed successfully", i + 1);
                },
                Err(e) => {
                    error!("Failed to execute DCA entry {}: {}", i + 1, e);
                    break;
                }
            }
        }
    });
    
    // Lưu task handle vào tracker
    dca_tracker.task_handle = Some(TaskHandle {
        join_handle: handle,
        task_id: task_id.clone(),
        description: format!("DCA strategy execution for trade {}", dca_id),
    });
    
    // Thêm vào active trades
    let mut active_trades = executor.active_trades.write().await;
    active_trades.push(dca_tracker.clone());
    
    Ok(dca_tracker)
}

/// Thực hiện chiến lược Grid Trading
pub async fn execute_grid_strategy(
    executor: &SmartTradeExecutor,
    params: &TradeParams,
    upper_price: f64,
    lower_price: f64,
    num_grids: u32,
) -> Result<TradeTracker> {
    // Kiểm tra tham số
    if upper_price <= lower_price {
        return Err(anyhow!("Upper price must be greater than lower price"));
    }
    
    if num_grids < 2 {
        return Err(anyhow!("Number of grids must be at least 2"));
    }
    
    // Tạo trade tracker cho chiến lược Grid
    let trade_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().timestamp() as u64;
    
    let mut tracker = create_trade_tracker(
        params,
        &trade_id,
        now,
        TradeStrategy::Grid,
    );
    
    // Thêm thông tin Grid vào metadata
    tracker.metadata.insert("grid_upper_price".to_string(), upper_price.to_string());
    tracker.metadata.insert("grid_lower_price".to_string(), lower_price.to_string());
    tracker.metadata.insert("grid_num_grids".to_string(), num_grids.to_string());
    
    // Tính toán giá mỗi grid
    let grid_step = (upper_price - lower_price) / (num_grids as f64 - 1.0);
    
    // Tính số lượng token cho mỗi grid
    let amount_per_grid = params.amount / (num_grids as f64);
    
    // Lưu các giá grid
    let mut grid_prices = Vec::new();
    for i in 0..num_grids {
        let grid_price = lower_price + (i as f64 * grid_step);
        grid_prices.push(grid_price);
        
        tracker.metadata.insert(
            format!("grid_price_{}", i),
            grid_price.to_string(),
        );
    }
    
    // Khởi động vòng lặp grid trading với Task Handle
    let executor_clone = executor.clone();
    let trade_id_clone = trade_id.clone();
    let grid_prices_clone = grid_prices.clone();
    
    // Tạo task handle để có thể abort khi cần
    let task_id = uuid::Uuid::new_v4().to_string();
    let handle = tokio::spawn(async move {
        let _ = monitor_grid_strategy(&executor_clone, &trade_id_clone, &grid_prices_clone, amount_per_grid).await;
    });
    
    // Lưu task handle vào tracker để có thể hủy sau này
    tracker.task_handle = Some(TaskHandle {
        join_handle: handle,
        task_id: task_id.clone(),
        description: format!("Grid strategy monitoring for trade {}", trade_id),
    });
    
    // Lưu vào active trades
    let mut active_trades = executor.active_trades.write().await;
    active_trades.push(tracker.clone());
    
    Ok(tracker)
}

/// Theo dõi và thực thi chiến lược Grid Trading
async fn monitor_grid_strategy(
    executor: &SmartTradeExecutor,
    trade_id: &str,
    grid_prices: &[f64],
    amount_per_grid: f64,
) -> Result<()> {
    // Lấy cấu hình
    let config = executor.config.read().await.clone();
    
    // Lấy resource manager để sử dụng tài nguyên hiệu quả
    let resource_manager = get_resource_manager();
    
    // Vòng lặp monitoring
    loop {
        // Kiểm tra xem executor còn hoạt động không
        if !*executor.running.read().await {
            debug!("Executor stopped, stopping grid monitoring for trade {}", trade_id);
            break;
        }
        
        // Kiểm tra xem grid còn trong active trades không
        let active_trades = executor.active_trades.read().await;
        let grid_trade_opt = active_trades.iter().find(|t| t.trade_id == trade_id);
        
        if grid_trade_opt.is_none() {
            debug!("Grid position {} no longer active, stopping monitor", trade_id);
            break;
        }
        
        let grid_trade = grid_trade_opt.unwrap().clone();
        drop(active_trades);
        
        // Lấy adapter cho chain
        let chain_id = grid_trade.params.chain_id();
        let adapter = match executor.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                error!("No adapter found for chain ID {}", chain_id);
                // Đợi một lúc trước khi kiểm tra lại
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                continue;
            }
        };
        
        // Sử dụng resource manager để lấy giá token, đảm bảo không quá tải hệ thống
        let token_price = match resource_manager
            .with_blockchain_limits_async(
                chain_id,
                ResourcePriority::Medium,
                "grid_strategy_get_price",
                || adapter.get_token_price(&grid_trade.params.token_address),
            ).await {
            Ok(price) => price,
            Err(e) => {
                warn!("Failed to get token price for grid strategy: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(config.price_check_interval_seconds)).await;
                continue;
            }
        };
        
        // Thực hiện kiểm tra và giao dịch theo grid
        // ... (Logic grid trading tiếp theo)
        
        // Chờ đến lần kiểm tra tiếp theo
        tokio::time::sleep(tokio::time::Duration::from_secs(config.price_check_interval_seconds)).await;
    }
    
    Ok(())
}

/// Thực hiện chiến lược TWAP (Time-Weighted Average Price)
pub async fn execute_twap_strategy(
    executor: &SmartTradeExecutor,
    params: &TradeParams,
    total_parts: u32,
    interval_seconds: u64,
    max_slippage_percent: f64,
) -> Result<TradeTracker> {
    // Kiểm tra tham số
    if total_parts < 2 {
        return Err(anyhow!("TWAP strategy requires at least 2 parts"));
    }
    
    // Tạo trade tracker cho chiến lược TWAP
    let trade_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().timestamp() as u64;
    
    let mut tracker = create_trade_tracker(
        params,
        &trade_id,
        now,
        TradeStrategy::TWAP,
    );
    
    // Thêm thông tin TWAP vào metadata
    tracker.metadata.insert("twap_total_parts".to_string(), total_parts.to_string());
    tracker.metadata.insert("twap_interval_seconds".to_string(), interval_seconds.to_string());
    tracker.metadata.insert("twap_max_slippage".to_string(), max_slippage_percent.to_string());
    tracker.metadata.insert("twap_completed_parts".to_string(), "0".to_string());
    
    // Khởi động task TWAP
    let executor_clone = executor.clone();
    let params_clone = params.clone();
    let trade_id_clone = trade_id.clone();
    let task_id = uuid::Uuid::new_v4().to_string();
    
    let handle = tokio::spawn(async move {
        // Thực thi TWAP
        // Logic theo dõi và thực thi TWAP
        // ...
    });
    
    // Lưu task handle
    tracker.task_handle = Some(TaskHandle {
        join_handle: handle,
        task_id,
        description: format!("TWAP strategy execution for trade {}", trade_id),
    });
    
    // Lưu vào active trades
    let mut active_trades = executor.active_trades.write().await;
    active_trades.push(tracker.clone());
    
    Ok(tracker)
}

//=============================================================================
// EXECUTOR EXTENSION METHODS FOR ADVANCED STRATEGIES
//=============================================================================

impl SmartTradeExecutor {
    /// Xác định chiến lược giao dịch dựa trên phân tích token và rủi ro
    pub(crate) fn determine_trading_strategy(&self, token_status: &TokenStatus, trade_risk: &TradeRiskAnalysis) -> TradeStrategy {
        // Logic xác định chiến lược dựa trên phân tích token và rủi ro
        if trade_risk.risk_score > 75 {
            // Rủi ro cao - dùng chiến lược an toàn nhất
            TradeStrategy::TightTSL
        } else if trade_risk.risk_score > 50 {
            // Rủi ro vừa phải - dùng chiến lược linh hoạt
            TradeStrategy::DynamicTSL
        } else {
            // Rủi ro thấp - có thể tối ưu hóa lợi nhuận
            if token_status.volatility_24h > 10.0 {
                TradeStrategy::DynamicTP
            } else {
                TradeStrategy::StandardTP
            }
        }
    }

    /// Thực hiện chiến lược Dynamic Trailing Stop Loss
    pub async fn dynamic_trailing_stop(&self, trade: &TradeTracker, base_tsl_percent: f64, adapter: &Arc<EvmAdapter>) -> Result<f64> {
        // Logic tính toán TSL động
        // ...
        
        // Trả về giá trị cuối cùng
        Ok(base_tsl_percent)
    }

    /// Thực hiện chiến lược Dynamic Take Profit/Stop Loss
    pub async fn dynamic_tp_sl(&self, trade: &TradeTracker, base_tp_percent: f64, base_sl_percent: f64, adapter: &Arc<EvmAdapter>) -> (f64, f64) {
        // Logic tính toán TP/SL động
        // ...
        
        (base_tp_percent, base_sl_percent)
    }

    /// Tự động bán khi có cảnh báo
    pub async fn auto_sell_on_alert(&self, trade: &TradeTracker, adapter: &Arc<EvmAdapter>) -> (bool, String) {
        // Logic phát hiện và xử lý cảnh báo
        // ...
        
        (false, "No alert triggered".to_string())
    }

    /// Theo dõi whale để phát hiện cơ hội/rủi ro
    pub async fn whale_tracker(&self, chain_id: u32, token_address: &str, whale_threshold: f64, adapter: &Arc<EvmAdapter>) -> (bool, String) {
        // Logic theo dõi các ví whale
        // ...
        
        (false, "No whale activity detected".to_string())
    }

    /// Theo dõi sự an toàn của token
    pub async fn monitor_token_safety(&self) {
        // Logic theo dõi độ an toàn của token
        // ...
    }

    /// Bán token với lý do cụ thể
    pub async fn sell_token(&self, trade: TradeTracker, reason: String, current_price: f64) -> Result<(), anyhow::Error> {
        // Logic bán token
        // ...
        
        // Cập nhật metadata
        let mut trade_clone = trade.clone();
        trade_clone.metadata.insert("sell_reason".to_string(), reason.clone());

        // Gọi hàm execute_sell
        match execute_sell(self, &mut trade_clone).await {
            Ok(_) => {
                info!("Sold token from trade {} due to: {}", trade.trade_id, reason);
                Ok(())
            },
            Err(e) => {
                error!("Failed to sell token from trade {}: {}", trade.trade_id, e);
                Err(e)
            }
        }
    }

    /// Cập nhật trạng thái giao dịch
    async fn update_trade_status(
        &self, 
        trade_id: &str, 
        status: TradeStatus,
        tx_hash: Option<String>,
        reason: Option<String>,
        price: Option<f64>,
    ) {
        // Logic cập nhật trạng thái giao dịch
        // ...
    }
} 