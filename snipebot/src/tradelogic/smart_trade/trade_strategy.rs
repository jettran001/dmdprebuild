/// Trade strategy implementation for SmartTradeExecutor
///
/// This module contains advanced trading strategies like dynamic trailing stop,
/// dynamic take profit/stop loss, and automatic reaction to market conditions.

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use chrono::Utc;
use tracing::{debug, error, info, warn};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::risk_analyzer::TradeRiskAnalysis;
use crate::analys::token_status::{TokenStatus, TokenSafety};
use crate::analys::mempool::MempoolAnalyzer;

// Module imports
use super::types::{TradeTracker, TradeStrategy};
use super::constants::*;
use super::executor::SmartTradeExecutor;

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
        if let Some(chain_data) = self.market_data.get(&chain_id) {
            return chain_data.get(token_address);
        }
        None
    }
    
    /// Tính toán phần trăm TSL cho một giao dịch dựa trên dữ liệu thị trường
    pub fn calculate_tsl_percentage(&mut self, trade: &TradeTracker) -> f64 {
        if let Some(market_data) = self.get_market_data(trade.chain_id, &trade.token_address) {
            // Reset adaptive TSL
            self.adaptive_tsl = AdaptiveTSL::new();
            
            // Tính thời gian giữ hiện tại
            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_secs();
            let hold_time = current_time - trade.entry_time;
            
            // Tính lợi nhuận hiện tại
            let current_profit = if trade.entry_price > 0.0 {
                (trade.highest_price - trade.entry_price) / trade.entry_price * 100.0
            } else {
                0.0
            };
            
            // Điều chỉnh TSL dựa trên các yếu tố thị trường
            self.adaptive_tsl.adjust_for_volatility(market_data.volatility_24h);
            self.adaptive_tsl.adjust_for_market_trend(&market_data.market_trend);
            self.adaptive_tsl.adjust_for_hold_time(hold_time, market_data.avg_trade_duration);
            self.adaptive_tsl.adjust_for_profit(current_profit);
            
            // Tính toán TSL cuối cùng
            return self.adaptive_tsl.calculate_final_percentage();
        }
        
        // Nếu không có dữ liệu thị trường, trả về giá trị mặc định
        SMART_TRADE_TSL_PERCENT
    }
    
    /// Tính toán giá kích hoạt stop loss dựa trên giá cao nhất và phần trăm TSL
    pub fn calculate_stop_price(&self, highest_price: f64, tsl_percentage: f64) -> f64 {
        highest_price * (1.0 - tsl_percentage / 100.0)
    }
    
    /// Xác định thời điểm tối ưu để bán dựa trên dữ liệu thị trường và lợi nhuận hiện tại
    pub fn should_take_profit(&self, trade: &TradeTracker, current_price: f64) -> bool {
        if let Some(market_data) = self.get_market_data(trade.chain_id, &trade.token_address) {
            // Tính lợi nhuận hiện tại
            let current_profit = if trade.entry_price > 0.0 {
                (current_price - trade.entry_price) / trade.entry_price * 100.0
            } else {
                0.0
            };
            
            // Tính thời gian giữ hiện tại
            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_secs();
            let hold_time = current_time - trade.entry_time;
            
            // Các quyết định dựa trên xu hướng thị trường và thời gian giữ
            match market_data.market_trend {
                MarketTrend::Bullish => {
                    // Thị trường tăng, giữ lâu hơn nếu lợi nhuận tốt
                    if current_profit >= trade.take_profit_percent * 1.5 {
                        return true;
                    }
                    
                    // Nếu giữ đủ lâu và có lời
                    if hold_time >= SMART_TRADE_MAX_HOLD_TIME && current_profit > 0.0 {
                        return true;
                    }
                },
                MarketTrend::Bearish => {
                    // Thị trường giảm, bán sớm hơn nếu có lời
                    if current_profit >= trade.take_profit_percent * 0.8 {
                        return true;
                    }
                    
                    // Nếu giữ quá nửa thời gian tối đa và có lời
                    if hold_time >= SMART_TRADE_MAX_HOLD_TIME / 2 && current_profit > 0.0 {
                        return true;
                    }
                },
                MarketTrend::Sideways => {
                    // Thị trường đi ngang, bán khi đạt mục tiêu
                    if current_profit >= trade.take_profit_percent {
                        return true;
                    }
                }
            }
            
            // Bán nếu đã giữ quá thời gian tối đa
            if hold_time >= SMART_TRADE_MAX_HOLD_TIME {
                return true;
            }
        } else {
            // Nếu không có dữ liệu thị trường, áp dụng chiến lược mặc định
            // Tính lợi nhuận hiện tại
            let current_profit = if trade.entry_price > 0.0 {
                (current_price - trade.entry_price) / trade.entry_price * 100.0
            } else {
                0.0
            };
            
            // Tính thời gian giữ hiện tại
            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_secs();
            let hold_time = current_time - trade.entry_time;
            
            // Bán nếu đạt mục tiêu lợi nhuận
            if current_profit >= trade.take_profit_percent {
                return true;
            }
            
            // Bán nếu đã giữ quá thời gian tối đa
            if hold_time >= SMART_TRADE_MAX_HOLD_TIME {
                return true;
            }
        }
        
        false
    }
    
    /// Xác định thời điểm tối ưu để cắt lỗ dựa trên dữ liệu thị trường và lỗ hiện tại
    pub fn should_stop_loss(&self, trade: &TradeTracker, current_price: f64) -> bool {
        // Tính lỗ hiện tại
        let current_loss = if trade.entry_price > 0.0 {
            (trade.entry_price - current_price) / trade.entry_price * 100.0
        } else {
            0.0
        };
        
        // Nếu lỗ vượt ngưỡng cắt lỗ
        if current_loss >= trade.stop_loss_percent {
            return true;
        }
        
        false
    }
    
    /// Kiểm tra nếu giá hiện tại kích hoạt trailing stop loss
    pub fn is_tsl_triggered(&self, trade: &TradeTracker, current_price: f64, tsl_percentage: f64) -> bool {
        if trade.highest_price > 0.0 {
            let stop_price = self.calculate_stop_price(trade.highest_price, tsl_percentage);
            return current_price <= stop_price;
        }
        
        false
    }
    
    /// Xác định chiến lược giao dịch tốt nhất dựa trên phân tích token và thị trường
    pub fn determine_best_strategy(&self, token_address: &str, chain_id: u32) -> TradeStrategy {
        if let Some(market_data) = self.get_market_data(chain_id, token_address) {
            match market_data.market_trend {
                MarketTrend::Bullish => {
                    // Thị trường tăng, ưu tiên chiến lược thông minh
                    TradeStrategy::Smart
                },
                MarketTrend::Bearish => {
                    // Thị trường giảm, ưu tiên chiến lược nhanh
                    TradeStrategy::Quick
                },
                MarketTrend::Sideways => {
                    // Thị trường đi ngang, tùy thuộc vào biến động
                    if market_data.volatility_24h > 10.0 {
                        // Biến động cao, sử dụng chiến lược thông minh
                        TradeStrategy::Smart
                    } else {
                        // Biến động thấp, sử dụng chiến lược nhanh
                        TradeStrategy::Quick
                    }
                }
            }
        } else {
            // Nếu không có dữ liệu thị trường, sử dụng chiến lược mặc định
            TradeStrategy::Smart
        }
    }
}

impl SmartTradeExecutor {
    /// Xác định chiến lược giao dịch phù hợp
    pub(crate) fn determine_trading_strategy(&self, token_status: &TokenStatus, trade_risk: &TradeRiskAnalysis) -> TradeStrategy {
        // Nếu token an toàn và rủi ro thấp, dùng Smart
        if token_status.evaluate_safety() == TokenSafety::Safe && trade_risk.risk_score < 30.0 {
            TradeStrategy::Smart
        } else {
            // Nếu token vừa phải hoặc rủi ro trung bình, dùng Quick
            TradeStrategy::Quick
        }
    }
    
    /// Tạo trailing stop động dựa trên volatility
    /// 
    /// Điều chỉnh trailing stop theo độ biến động của giá
    /// 
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `base_tsl_percent`: Phần trăm TSL cơ bản
    /// - `adapter`: EVM adapter
    /// 
    /// # Returns
    /// - `f64`: Phần trăm TSL đã điều chỉnh
    pub async fn dynamic_trailing_stop(&self, chain_id: u32, token_address: &str, base_tsl_percent: f64, adapter: &Arc<EvmAdapter>) -> f64 {
        // Lấy dữ liệu giá theo thời gian
        let price_data = match adapter.get_price_history(token_address, 24).await {
            Ok(data) => data,
            Err(_) => {
                warn!("Không thể lấy lịch sử giá cho token {}, sử dụng TSL cơ bản", token_address);
                return base_tsl_percent;
            }
        };
        
        if price_data.len() < 10 {
            warn!("Không đủ dữ liệu giá để tính toán ATR và Bollinger Bands cho token {}", token_address);
            return base_tsl_percent;
        }
        
        // Tính toán ATR (Average True Range) - Đo độ biến động
        let mut true_ranges = Vec::new();
        for i in 1..price_data.len() {
            let high = price_data[i].high;
            let low = price_data[i].low;
            let prev_close = price_data[i-1].close;
            
            // True Range = max(high - low, |high - prev_close|, |low - prev_close|)
            let tr = (high - low).max((high - prev_close).abs()).max((low - prev_close).abs());
            true_ranges.push(tr);
        }
        
        // Tính ATR (trung bình 14 chu kỳ)
        let period = 14.min(true_ranges.len());
        let atr: f64 = true_ranges.iter().take(period).sum::<f64>() / period as f64;
        
        // Tính độ lệch chuẩn cho Bollinger Bands
        let closes: Vec<f64> = price_data.iter().map(|candle| candle.close).collect();
        let mean_price: f64 = closes.iter().sum::<f64>() / closes.len() as f64;
        
        let sum_squared_diff: f64 = closes.iter()
            .map(|price| (price - mean_price).powi(2))
            .sum::<f64>();
        
        let std_dev = (sum_squared_diff / closes.len() as f64).sqrt();
        
        // Phân tích volume 
        let volumes: Vec<f64> = price_data.iter().map(|candle| candle.volume).collect();
        let mean_volume: f64 = volumes.iter().sum::<f64>() / volumes.len() as f64;
        let recent_volume = volumes.last().unwrap_or(&mean_volume);
        let volume_ratio = recent_volume / mean_volume;
        
        // Giá gần đây
        let recent_prices: Vec<f64> = closes.iter().rev().take(5).cloned().collect();
        let is_uptrend = recent_prices.windows(2).all(|w| w[0] >= w[1]);
        let is_downtrend = recent_prices.windows(2).all(|w| w[0] <= w[1]);
        
        // Điều chỉnh TSL dựa trên phân tích
        let mut adjusted_tsl = base_tsl_percent;
        
        // 1. Điều chỉnh theo ATR
        let price_volatility_ratio = atr / mean_price;
        
        if price_volatility_ratio > 0.05 {
            // Biến động cao, tăng TSL
            adjusted_tsl += base_tsl_percent * (price_volatility_ratio * 10.0);
            debug!("Biến động cao (ATR/Price = {}), tăng TSL", price_volatility_ratio);
        } else if price_volatility_ratio < 0.01 {
            // Biến động thấp, giảm TSL
            adjusted_tsl -= base_tsl_percent * 0.3;
            debug!("Biến động thấp (ATR/Price = {}), giảm TSL", price_volatility_ratio);
        }
        
        // 2. Điều chỉnh theo Bollinger Bands (độ lệch chuẩn)
        let bollinger_ratio = std_dev / mean_price;
        
        if bollinger_ratio > 0.1 {
            // Khoảng Bollinger rộng, tăng TSL
            adjusted_tsl += base_tsl_percent * 0.5;
            debug!("Bollinger rộng (Std/Price = {}), tăng TSL", bollinger_ratio);
        }
        
        // 3. Điều chỉnh theo volume
        if volume_ratio > 2.0 {
            // Volume cao bất thường, tăng TSL để bảo vệ
            adjusted_tsl += base_tsl_percent * 0.7;
            debug!("Volume cao ({}x trung bình), tăng TSL", volume_ratio);
        }
        
        // 4. Điều chỉnh theo xu hướng
        if is_downtrend {
            // Đang giảm giá, tăng TSL để bán sớm
            adjusted_tsl -= base_tsl_percent * 0.3;
            debug!("Đang downtrend, giảm TSL để bán sớm");
        } else if is_uptrend {
            // Đang tăng giá, giảm TSL để tận dụng đà tăng
            adjusted_tsl += base_tsl_percent * 0.3;
            debug!("Đang uptrend, tăng TSL để tận dụng đà tăng");
        }
        
        // Giới hạn giá trị TSL
        let min_tsl = base_tsl_percent * 0.5;
        let max_tsl = base_tsl_percent * 3.0;
        
        let adjusted_tsl = adjusted_tsl.max(min_tsl).min(max_tsl);
        
        info!(
            "Token {} - TSL động: cơ bản={:.2}%, điều chỉnh={:.2}%, ATR={:.6}, Vol={:.2}x",
            token_address, base_tsl_percent, adjusted_tsl, atr, volume_ratio
        );
        
        adjusted_tsl
    }

    /// Điều chỉnh take profit và stop loss tự động
    ///
    /// Phương thức này sẽ điều chỉnh TP/SL dựa trên:
    /// - Xu hướng thị trường
    /// - Biến động giá gần đây
    /// - Volume giao dịch
    /// - Sự kiện on-chain (lệnh lớn, whale movement)
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `base_tp_percent`: % TP cơ bản
    /// - `base_sl_percent`: % SL cơ bản
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - (take_profit_percent, stop_loss_percent) - Cặp giá trị TP/SL đã điều chỉnh
    pub async fn dynamic_tp_sl(&self, chain_id: u32, token_address: &str, base_tp_percent: f64, base_sl_percent: f64, adapter: &Arc<EvmAdapter>) -> (f64, f64) {
        // Lấy dữ liệu giá 24h
        let price_data = match adapter.get_price_history(token_address, 24).await {
            Ok(data) => data,
            Err(_) => {
                warn!("Không thể lấy lịch sử giá cho token {}, sử dụng TP/SL cơ bản", token_address);
                return (base_tp_percent, base_sl_percent);
            }
        };
        
        if price_data.len() < 10 {
            warn!("Không đủ dữ liệu giá để điều chỉnh TP/SL cho token {}", token_address);
            return (base_tp_percent, base_sl_percent);
        }
        
        // Kiểm tra token đã vào sổ đen bởi các API bên ngoài không
        let external_blacklisted = match self.fetch_external_token_data(token_address, &format!("{}", chain_id)).await {
            Some(data) => data.get("is_blacklisted").map_or(false, |val| val == "true"),
            None => false,
        };
        
        // Tính các chỉ số thị trường
        let closes: Vec<f64> = price_data.iter().map(|candle| candle.close).collect();
        let highs: Vec<f64> = price_data.iter().map(|candle| candle.high).collect();
        let lows: Vec<f64> = price_data.iter().map(|candle| candle.low).collect();
        let volumes: Vec<f64> = price_data.iter().map(|candle| candle.volume).collect();
        
        // Tính giá trung bình và biến động
        let avg_price: f64 = closes.iter().sum::<f64>() / closes.len() as f64;
        let current_price = *closes.last().unwrap_or(&avg_price);
        
        // Tính ATR (Average True Range)
        let mut true_ranges = Vec::new();
        for i in 1..price_data.len() {
            let high = price_data[i].high;
            let low = price_data[i].low;
            let prev_close = price_data[i-1].close;
            
            let tr = (high - low).max((high - prev_close).abs()).max((low - prev_close).abs());
            true_ranges.push(tr);
        }
        
        let atr_period = 14.min(true_ranges.len());
        let atr: f64 = true_ranges.iter().take(atr_period).sum::<f64>() / atr_period as f64;
        
        // Tính biến động giá 24h
        let highest_price = highs.iter().fold(0.0, |a, &b| a.max(b));
        let lowest_price = lows.iter().fold(f64::MAX, |a, &b| a.min(b));
        let price_range_percent = (highest_price - lowest_price) / lowest_price * 100.0;
        
        // Phân tích xu hướng
        let recent_prices: Vec<f64> = closes.iter().rev().take(10).cloned().collect();
        let is_uptrend = recent_prices.windows(2).filter(|w| w[0] > w[1]).count() > 7;
        let is_downtrend = recent_prices.windows(2).filter(|w| w[0] < w[1]).count() > 7;
        
        // Phân tích volume
        let recent_volumes: Vec<f64> = volumes.iter().rev().take(5).cloned().collect();
        let avg_recent_volume: f64 = recent_volumes.iter().sum::<f64>() / recent_volumes.len() as f64;
        let avg_volume: f64 = volumes.iter().sum::<f64>() / volumes.len() as f64;
        let volume_ratio = avg_recent_volume / avg_volume;
        
        // Kiểm tra mempool để phát hiện swap lớn
        let have_large_pending_sell = match self.mempool_analyzers.get(&chain_id) {
            Some(analyzer) => {
                analyzer.detect_large_pending_sells(token_address, 5.0).await
            },
            None => false,
        };
        
        // Lấy dữ liệu on-chain (ví whale)
        let (whale_activity, _) = self.whale_tracker(chain_id, token_address, WHALE_THRESHOLD_PERCENT, adapter).await;
        
        // Kiểm tra trạng thái token
        let (has_risk, _, _) = self.comprehensive_analysis(chain_id, token_address, adapter).await;
        
        // Điều chỉnh TP/SL dựa trên phân tích
        let mut adjusted_tp = base_tp_percent;
        let mut adjusted_sl = base_sl_percent;
        
        // 1. Kiểm tra các yếu tố rủi ro cao, nếu có thì giảm TP và tăng SL (bán sớm)
        if external_blacklisted || have_large_pending_sell || whale_activity || has_risk {
            adjusted_tp *= 0.7; // Giảm mục tiêu lợi nhuận
            adjusted_sl *= 0.6; // Giảm ngưỡng chịu lỗ (bán sớm hơn)
            
            debug!(
                "Token {} - Phát hiện rủi ro: blacklisted={}, large_sells={}, whale_activity={}, token_risk={}",
                token_address, external_blacklisted, have_large_pending_sell, whale_activity, has_risk
            );
        }
        
        // 2. Điều chỉnh theo ATR (biến động)
        let atr_percent = atr / avg_price * 100.0;
        
        if atr_percent > 5.0 {
            // Biến động lớn, tăng TP/SL theo tỷ lệ
            let volatility_factor = (atr_percent / 5.0).min(3.0);
            adjusted_tp *= volatility_factor;
            adjusted_sl *= 0.9; // Giảm SL khi biến động cao
            
            debug!("Token {} - Biến động cao: ATR={}%, điều chỉnh TP x{}", 
                   token_address, atr_percent, volatility_factor);
        }
        
        // 3. Điều chỉnh theo xu hướng thị trường
        if is_uptrend {
            // Xu hướng tăng, tăng TP và giảm SL
            adjusted_tp *= 1.3;
            adjusted_sl *= 0.9;
            debug!("Token {} - Xu hướng tăng, tăng TP", token_address);
        } else if is_downtrend {
            // Xu hướng giảm, giảm TP và tăng SL
            adjusted_tp *= 0.7;
            adjusted_sl *= 0.7; // Bán sớm khi giảm
            debug!("Token {} - Xu hướng giảm, giảm TP & SL", token_address);
        }
        
        // 4. Điều chỉnh theo volume
        if volume_ratio > 2.0 {
            // Volume tăng mạnh, có thể là pump
            adjusted_tp *= 1.2; // Tăng TP để tận dụng
            debug!("Token {} - Volume tăng mạnh: {}x trung bình", token_address, volume_ratio);
        } else if volume_ratio < 0.5 {
            // Volume thấp, không nhiều người quan tâm
            adjusted_tp *= 0.8;
            adjusted_sl *= 0.8; // Bảo thủ hơn
            debug!("Token {} - Volume thấp: {}x trung bình", token_address, volume_ratio);
        }
        
        // 5. Điều chỉnh dựa trên biến động giá 24h
        if price_range_percent > 50.0 {
            // Giá dao động mạnh, tăng TP (cơ hội lớn hơn)
            adjusted_tp *= 1.5;
            debug!("Token {} - Biến động giá lớn: {}% trong 24h", token_address, price_range_percent);
        }
        
        // Giới hạn kết quả
        let min_tp = base_tp_percent * 0.5;
        let max_tp = base_tp_percent * 3.0;
        let min_sl = base_sl_percent * 0.5;
        let max_sl = base_sl_percent * 1.5;
        
        adjusted_tp = adjusted_tp.max(min_tp).min(max_tp);
        adjusted_sl = adjusted_sl.max(min_sl).min(max_sl);
        
        info!(
            "Token {} - TP/SL động: TP={:.2}% -> {:.2}%, SL={:.2}% -> {:.2}%", 
            token_address, base_tp_percent, adjusted_tp, base_sl_percent, adjusted_sl
        );
        
        (adjusted_tp, adjusted_sl)
    }
    
    /// Auto-sell khi phát hiện bất thường
    /// 
    /// # Parameters
    /// - `trade`: Thông tin trade đang theo dõi
    /// - `adapter`: EVM adapter
    /// 
    /// # Returns
    /// - `bool`: True nếu phát hiện bất thường và tiến hành bán
    /// - `String`: Lý do bán (nếu có)
    pub async fn auto_sell_on_alert(&self, trade: &TradeTracker, adapter: &Arc<EvmAdapter>) -> (bool, String) {
        let chain_id = trade.chain_id;
        let token_address = &trade.token_address;
        
        let mut should_sell = false;
        let mut reason = String::new();
        
        // Kiểm tra tax thay đổi
        let (tax_changed, buy_tax, sell_tax) = self.detect_dynamic_tax(chain_id, token_address, adapter).await;
        if tax_changed && sell_tax > MAX_SAFE_SELL_TAX {
            should_sell = true;
            reason = format!("Phát hiện sell tax tăng đột biến: {}%", sell_tax);
        }
        
        // Kiểm tra liquidity
        let (liquidity_risk, liquidity_reason) = self.detect_liquidity_risk(chain_id, token_address, adapter).await;
        if liquidity_risk {
            should_sell = true;
            reason = format!("Rủi ro thanh khoản: {}", liquidity_reason);
        }
        
        // Kiểm tra owner privilege
        let (has_dangerous_privilege, privileges) = self.detect_owner_privilege(chain_id, token_address, adapter).await;
        if has_dangerous_privilege && !privileges.is_empty() {
            // Chỉ bán nếu phát hiện các privilege nguy hiểm nhất định
            for privilege in &privileges {
                if privilege.contains("mint") || privilege.contains("pause") || privilege.contains("blacklist") {
                    should_sell = true;
                    reason = format!("Phát hiện owner sử dụng quyền nguy hiểm: {}", privilege);
                    break;
                }
            }
        }
        
        // Kiểm tra blacklist
        let is_blacklisted = self.detect_blacklist(chain_id, token_address, adapter).await;
        if is_blacklisted {
            should_sell = true;
            reason = "Địa chỉ ví đã bị blacklist hoặc có nguy cơ bị blacklist".to_string();
        }
        
        // Nếu cần bán, gọi hàm bán token
        if should_sell {
            self.log_trade_decision(
                &trade.trade_id, 
                token_address, 
                super::types::TradeStatus::Pending, 
                &format!("Auto sell triggered: {}", reason),
                chain_id
            ).await;
            
            // Lấy giá hiện tại
            if let Ok(current_price) = adapter.get_token_price(token_address).await {
                // Thực hiện bán token
                self.sell_token(trade.clone(), reason.clone(), current_price).await;
            }
            
            return (true, reason);
        }
        
        (false, "No alerts detected".to_string())
    }
    
    /// Theo dõi các ví lớn của token
    /// 
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `whale_threshold`: Ngưỡng xác định whale (%)
    /// - `adapter`: EVM adapter
    /// 
    /// # Returns
    /// - `bool`: True nếu phát hiện whale bán
    /// - `String`: Chi tiết về hoạt động của whale
    pub async fn whale_tracker(&self, chain_id: u32, token_address: &str, whale_threshold: f64, adapter: &Arc<EvmAdapter>) -> (bool, String) {
        let mut whale_selling = false;
        let mut details = String::new();
        
        // Lấy mempool analyzer
        if let Some(mempool) = self.mempool_analyzers.get(&chain_id) {
            // Lấy tổng supply của token
            if let Ok(total_supply) = adapter.get_token_total_supply(token_address).await {
                // Lấy các giao dịch gần đây nhất từ mempool
                if let Ok(transactions) = mempool.get_token_transactions(token_address, 20).await {
                    // Theo dõi các ví lớn bán token
                    for tx in transactions {
                        // Nếu là giao dịch bán
                        if tx.is_sell && tx.token_address == token_address {
                            // Lấy số dư của địa chỉ
                            if let Ok(balance) = adapter.get_token_balance(token_address, &tx.sender).await {
                                // Tính phần trăm so với tổng supply
                                let balance_percent = (balance / total_supply) * 100.0;
                                
                                // Nếu là whale (sở hữu >3% total supply)
                                if balance_percent > whale_threshold {
                                    whale_selling = true;
                                    details = format!(
                                        "Phát hiện whale {} bán {} token ({}% của total supply)",
                                        tx.sender, tx.token_amount, balance_percent
                                    );
                                    
                                    self.log_trade_decision(
                                        "N/A", 
                                        token_address, 
                                        super::types::TradeStatus::Pending, 
                                        &format!("Whale activity detected: {}", details),
                                        chain_id
                                    ).await;
                                    
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
        
        (whale_selling, details)
    }
    
    /// Kiểm tra các bất thường của token đang trade và tự động bán nếu cần
    pub async fn monitor_token_safety(&self) {
        // Clone các giao dịch đang mở để xử lý bên ngoài lock
        let active_trades: Vec<TradeTracker> = {
            let trades = self.active_trades.read().await;
            trades.clone()
        };
        
        // Kiểm tra từng giao dịch đang mở
        for trade in active_trades {
            if trade.status != super::types::TradeStatus::Open {
                continue;
            }
            
            if let Some(adapter) = self.evm_adapters.get(&trade.chain_id) {
                // 1. Kiểm tra honeypot
                let is_honeypot = self.detect_honeypot(trade.chain_id, &trade.token_address, adapter).await;
                if is_honeypot {
                    self.auto_sell_on_alert(&trade, adapter).await;
                    continue;
                }
                
                // 2. Kiểm tra tax
                let (is_dynamic_tax, buy_tax, sell_tax) = self.detect_dynamic_tax(trade.chain_id, &trade.token_address, adapter).await;
                if is_dynamic_tax {
                    self.auto_sell_on_alert(&trade, adapter).await;
                    continue;
                }
                
                // 3. Kiểm tra thanh khoản
                let (liquidity_risk, details) = self.detect_liquidity_risk(trade.chain_id, &trade.token_address, adapter).await;
                if liquidity_risk {
                    self.auto_sell_on_alert(&trade, adapter).await;
                    continue;
                }
                
                // 4. Kiểm tra quyền owner
                let (has_dangerous_privileges, privileges) = self.detect_owner_privilege(trade.chain_id, &trade.token_address, adapter).await;
                if has_dangerous_privileges {
                    self.auto_sell_on_alert(&trade, adapter).await;
                    continue;
                }
                
                // 5. Kiểm tra blacklist
                let has_blacklist = self.detect_blacklist(trade.chain_id, &trade.token_address, adapter).await;
                if has_blacklist {
                    self.auto_sell_on_alert(&trade, adapter).await;
                    continue;
                }
            }
        }
    }
}
