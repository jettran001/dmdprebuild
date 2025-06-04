/// Market Monitor - Giám sát thị trường
///
/// Module này chứa logic giám sát thị trường và giá cả,
/// vòng lặp monitor, và phát hiện cơ hội giao dịch.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;
use chrono::Utc;
use tracing::{debug, error, info, warn};
use anyhow::{Result, Context, anyhow, bail};
use uuid;
use rand::{thread_rng, Rng};

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::{TokenPair, TradeParams, TradeType};
use crate::tradelogic::traits::{
    SharedOpportunity, SharedOpportunityType, OpportunityPriority
};

// Module imports
use crate::tradelogic::smart_trade::executor::core::SmartTradeExecutor;
use super::types::{TradeStatus, TradeOpportunity};
use super::trade_handler::execute_opportunity;
use super::position_manager::update_positions;
use super::risk_manager::analyze_market_risk;
use super::utils::get_current_timestamp;

/// Vòng lặp monitor để theo dõi thị trường và cập nhật vị thế
pub async fn monitor_loop(executor: Arc<SmartTradeExecutor>) {
    info!("Starting market monitor loop");
    
    let mut last_opportunity_check = Instant::now();
    let mut last_position_update = Instant::now();
    
    loop {
        // Kiểm tra nếu đã dừng
        if !*executor.running.read().await {
            info!("Market monitor loop stopped");
            break;
        }
        
        // Đọc cấu hình
        let config = executor.config.read().await.clone();
        
        // Cập nhật vị thế đang mở
        if last_position_update.elapsed() >= Duration::from_secs(config.position_update_interval_seconds) {
            if let Err(e) = update_positions(&executor).await {
                error!("Failed to update positions: {}", e);
            }
            last_position_update = Instant::now();
        }
        
        // Kiểm tra cơ hội giao dịch và chia sẻ với coordinator
        if last_opportunity_check.elapsed() >= Duration::from_secs(config.opportunity_check_interval_seconds) {
            if let Some(coordinator) = &executor.coordinator {
                match check_opportunities(&executor).await {
                    Ok(opportunities) => {
                        for opportunity in opportunities {
                            if let Err(e) = coordinator.share_opportunity(opportunity).await {
                                error!("Failed to share opportunity: {}", e);
                            }
                        }
                    },
                    Err(e) => {
                        error!("Failed to check opportunities: {}", e);
                    }
                }
            }
            last_opportunity_check = Instant::now();
        }
        
        // Kiểm tra cơ hội từ coordinator
        if let Some(coordinator) = &executor.coordinator {
            let subscription = executor.coordinator_subscription.read().await;
            if let Some(subscription_id) = &*subscription {
                match coordinator.get_opportunities(subscription_id).await {
                    Ok(opportunities) => {
                        for opportunity in opportunities {
                            match process_opportunity(&executor, opportunity).await {
                                Ok(processed) => {
                                    if processed {
                                        debug!("Processed opportunity from coordinator");
                                    }
                                },
                                Err(e) => {
                                    error!("Failed to process opportunity: {}", e);
                                }
                            }
                        }
                    },
                    Err(e) => {
                        error!("Failed to get opportunities from coordinator: {}", e);
                    }
                }
            }
        }
        
        // Chờ một khoảng thời gian trước khi lặp lại
        sleep(Duration::from_millis(config.monitor_interval_ms)).await;
    }
}

/// Kiểm tra các cơ hội giao dịch trên thị trường
pub async fn check_opportunities(executor: &SmartTradeExecutor) -> Result<Vec<SharedOpportunity>> {
    // Kết quả
    let mut opportunities = Vec::new();
    
    // Đọc cấu hình
    let config = executor.config.read().await.clone();
    
    // Kiểm tra cho mỗi chain được hỗ trợ
    for (chain_id, adapter) in &executor.evm_adapters {
        // Lấy danh sách token được theo dõi
        let watched_tokens = config.watched_tokens.get(chain_id).cloned().unwrap_or_default();
        
        // Bỏ qua nếu không có token nào được theo dõi
        if watched_tokens.is_empty() {
            continue;
        }
        
        // Kiểm tra từng token
        for token_address in watched_tokens {
            // Kiểm tra giá hiện tại
            let current_price = match adapter.get_token_price(&token_address).await {
                Ok(price) => price,
                Err(e) => {
                    warn!("Failed to get price for token {}: {}", token_address, e);
                    continue;
                }
            };
            
            // Lấy lịch sử giá từ cache (giả định)
            let price_history = executor.analys_client.get_price_history(*chain_id, &token_address).await?;
            
            // Phân tích xu hướng giá
            let (trend, confidence) = analyze_price_trend(&price_history);
            
            // Kiểm tra nếu có cơ hội giao dịch
            if trend > 0.05 && confidence > 70 {
                // Tạo token pair
                let token_pair = TokenPair {
                    base_token_address: config.default_base_token.get(chain_id).cloned().unwrap_or_default(),
                    token_address: token_address.clone(),
                };
                
                // Kiểm tra rủi ro thị trường
                let risk_level = analyze_market_risk(executor, *chain_id, &token_address).await?;
                
                // Chỉ tiếp tục nếu rủi ro thấp
                if risk_level > 70 {
                    continue;
                }
                
                // Tạo cơ hội
                let opportunity = SharedOpportunity {
                    opportunity_id: uuid::Uuid::new_v4().to_string(),
                    chain_id: *chain_id,
                    token_pair,
                    trade_type: TradeType::Buy,
                    opportunity_type: SharedOpportunityType::TokenPump,
                    priority: if confidence > 90 {
                        OpportunityPriority::High
                    } else if confidence > 80 {
                        OpportunityPriority::Medium
                    } else {
                        OpportunityPriority::Low
                    },
                    source: "market_monitor".to_string(),
                    confidence,
                    description: format!("Price uptrend detected with {}% confidence", confidence),
                    discovered_at: Utc::now().timestamp() as u64,
                    expires_at: Some(Utc::now().timestamp() as u64 + 3600), // 1 hour expiry
                    recommended_amount: calculate_position_size(*chain_id, current_price, &config),
                    metadata: HashMap::new(),
                };
                
                opportunities.push(opportunity);
            } else if trend < -0.05 && confidence > 70 {
                // Kiểm tra nếu có token trong vị thế đang mở
                let active_trades = executor.active_trades.read().await;
                let has_position = active_trades.iter().any(|t| {
                    t.params.chain_id == *chain_id && 
                    t.params.token_pair.token_address == token_address &&
                    t.status == TradeStatus::Monitoring
                });
                
                // Nếu có vị thế mở, tạo cơ hội bán
                if has_position {
                    // Tạo token pair
                    let token_pair = TokenPair {
                        base_token_address: config.default_base_token.get(chain_id).cloned().unwrap_or_default(),
                        token_address: token_address.clone(),
                    };
                    
                    // Tạo cơ hội bán
                    let opportunity = SharedOpportunity {
                        opportunity_id: uuid::Uuid::new_v4().to_string(),
                        chain_id: *chain_id,
                        token_pair,
                        trade_type: TradeType::Sell,
                        opportunity_type: SharedOpportunityType::TokenDump,
                        priority: if confidence > 90 {
                            OpportunityPriority::High
                        } else if confidence > 80 {
                            OpportunityPriority::Medium
                        } else {
                            OpportunityPriority::Low
                        },
                        source: "market_monitor".to_string(),
                        confidence,
                        description: format!("Price downtrend detected with {}% confidence", confidence),
                        discovered_at: Utc::now().timestamp() as u64,
                        expires_at: Some(Utc::now().timestamp() as u64 + 3600), // 1 hour expiry
                        recommended_amount: 0.0, // Will use all available amount
                        metadata: HashMap::new(),
                    };
                    
                    opportunities.push(opportunity);
                }
            }
        }
    }
    
    Ok(opportunities)
}

/// Xử lý cơ hội từ coordinator
pub async fn process_opportunity(
    executor: &SmartTradeExecutor,
    opportunity: SharedOpportunity,
) -> Result<bool> {
    // Đọc cấu hình
    let config = executor.config.read().await.clone();
    
    // Kiểm tra nếu auto trade được bật
    if !config.auto_trade_enabled {
        return Ok(false);
    }
    
    // Kiểm tra cơ hội có phù hợp với executor này không
    if !should_process_opportunity(executor, &opportunity).await? {
        return Ok(false);
    }
    
    // Thực thi cơ hội
    match execute_opportunity(executor, opportunity).await {
        Ok(result) => {
            // Nếu giao dịch thành công, trả về true
            Ok(result.status == TradeStatus::Monitoring || result.status == TradeStatus::Completed)
        },
        Err(e) => {
            error!("Failed to execute opportunity: {}", e);
            Ok(false)
        }
    }
}

/// Kiểm tra xem cơ hội có nên được xử lý bởi executor này hay không
async fn should_process_opportunity(
    executor: &SmartTradeExecutor,
    opportunity: &SharedOpportunity,
) -> Result<bool> {
    // Đọc cấu hình
    let config = executor.config.read().await.clone();
    
    // Kiểm tra nếu chain được hỗ trợ
    if !executor.evm_adapters.contains_key(&opportunity.chain_id) {
        return Ok(false);
    }
    
    // Kiểm tra token pair có hợp lệ không
    if opportunity.token_pair.token_address.is_empty() || opportunity.token_pair.base_token_address.is_empty() {
        return Ok(false);
    }
    
    // Kiểm tra mức độ ưu tiên
    match opportunity.priority {
        OpportunityPriority::High => {
            // Luôn xử lý các cơ hội ưu tiên cao
            Ok(true)
        },
        OpportunityPriority::Medium => {
            // Xử lý nếu confidence đủ cao
            Ok(opportunity.confidence >= config.min_confidence_for_medium_priority)
        },
        OpportunityPriority::Low => {
            // Chỉ xử lý nếu confidence rất cao
            Ok(opportunity.confidence >= config.min_confidence_for_low_priority)
        },
    }
}

/// Phân tích xu hướng giá từ lịch sử giá
/// Trả về (trend, confidence) với trend là phần trăm thay đổi giá và confidence là mức độ tin cậy
fn analyze_price_trend(price_history: &[(u64, f64)]) -> (f64, u8) {
    // Kiểm tra nếu không có đủ dữ liệu
    if price_history.len() < 4 {
        debug!("Không đủ dữ liệu để phân tích xu hướng giá (cần ít nhất 4 điểm)");
        return (0.0, 0);
    }
    
    // Sắp xếp theo thời gian
    let mut sorted_history = price_history.to_vec();
    sorted_history.sort_by_key(|&(timestamp, _)| timestamp);
    
    // --- Phương pháp 1: Phân tích thay đổi giá tổng thể ---
    let first_price = sorted_history.first().unwrap().1;
    let last_price = sorted_history.last().unwrap().1;
    let overall_change = (last_price - first_price) / first_price;
    
    // --- Phương pháp 2: Phân tích đường xu hướng tuyến tính (Linear Regression) ---
    let (slope, r_squared) = calculate_linear_regression(&sorted_history);
    
    // --- Phương pháp 3: Phân tích di chuyển trung bình (Moving Averages) ---
    let short_ma = calculate_moving_average(&sorted_history, 3); // MA ngắn (3 điểm)
    let long_ma = calculate_moving_average(&sorted_history, sorted_history.len().min(10)); // MA dài (tối đa 10 điểm)
    
    let ma_trend = if short_ma > long_ma { 1.0 } else if short_ma < long_ma { -1.0 } else { 0.0 };
    
    // --- Phương pháp 4: Phân tích biến động giá (Volatility) ---
    let volatility = calculate_volatility(&sorted_history);
    let volatility_factor = if volatility > 0.1 { 0.7 } else { 1.0 }; // Giảm độ tin cậy nếu biến động cao
    
    // --- Tổng hợp các phương pháp để xác định xu hướng và độ tin cậy ---
    
    // Xu hướng = trung bình có trọng số của các phương pháp
    let trend = (overall_change * 0.4) + (slope * 30.0 * 0.4) + (ma_trend * 0.2);
    
    // Tính toán mức độ tin cậy dựa trên nhiều yếu tố
    let mut confidence_factors = Vec::new();
    
    // 1. Độ tin cậy từ R-squared của hồi quy tuyến tính
    confidence_factors.push(r_squared * 100.0);
    
    // 2. Độ tin cậy từ sự nhất quán của chuyển động giá
    let price_consistency = calculate_price_consistency(&sorted_history, overall_change > 0.0);
    confidence_factors.push(price_consistency * 100.0);
    
    // 3. Độ tin cậy từ số lượng dữ liệu
    let data_points_factor = (sorted_history.len() as f64 / 20.0).min(1.0) * 100.0;
    confidence_factors.push(data_points_factor);
    
    // 4. Độ tin cậy từ thời gian (ưu tiên dữ liệu gần đây)
    let recency_factor = check_data_recency(&sorted_history);
    confidence_factors.push(recency_factor * 100.0);
    
    // Tính trung bình các yếu tố độ tin cậy và áp dụng hệ số biến động
    let avg_confidence = confidence_factors.iter().sum::<f64>() / confidence_factors.len() as f64;
    let confidence = (avg_confidence * volatility_factor) as u8;
    
    debug!(
        "Phân tích xu hướng giá: overall_change={:.2}%, slope={:.4}, r_squared={:.2}, ma_trend={:.1}, volatility={:.2}, confidence={}",
        overall_change * 100.0, slope, r_squared, ma_trend, volatility, confidence
    );
    
    (trend, confidence.min(100))
}

/// Tính toán hồi quy tuyến tính (linear regression) cho dữ liệu giá
/// Trả về (hệ số góc, r-squared)
fn calculate_linear_regression(price_data: &[(u64, f64)]) -> (f64, f64) {
    if price_data.len() < 2 {
        return (0.0, 0.0);
    }
    
    // Chuyển đổi timestamps thành indexes để đơn giản hóa
    let mut indexed_data: Vec<(f64, f64)> = price_data
        .iter()
        .enumerate()
        .map(|(i, &(_, price))| (i as f64, price))
        .collect();
    
    let n = indexed_data.len() as f64;
    
    // Tính tổng x, y, x*y, x^2
    let sum_x: f64 = indexed_data.iter().map(|(x, _)| x).sum();
    let sum_y: f64 = indexed_data.iter().map(|(_, y)| y).sum();
    let sum_xy: f64 = indexed_data.iter().map(|(x, y)| x * y).sum();
    let sum_xx: f64 = indexed_data.iter().map(|(x, _)| x * x).sum();
    
    // Tính hệ số góc (slope) và giao điểm (intercept)
    let slope = (n * sum_xy - sum_x * sum_y) / (n * sum_xx - sum_x * sum_x);
    let intercept = (sum_y - slope * sum_x) / n;
    
    // Tính R-squared
    let mean_y = sum_y / n;
    let mut ss_total = 0.0;
    let mut ss_residual = 0.0;
    
    for (x, y) in indexed_data.iter() {
        let predicted_y = slope * x + intercept;
        ss_total += (y - mean_y).powi(2);
        ss_residual += (y - predicted_y).powi(2);
    }
    
    let r_squared = if ss_total > 0.0 { 1.0 - (ss_residual / ss_total) } else { 0.0 };
    
    (slope, r_squared)
}

/// Tính toán trung bình di chuyển (moving average) với window_size
fn calculate_moving_average(price_data: &[(u64, f64)], window_size: usize) -> f64 {
    if price_data.is_empty() || window_size == 0 {
        return 0.0;
    }
    
    // Lấy window_size phần tử cuối cùng (hoặc tất cả nếu không đủ)
    let actual_window = price_data.len().min(window_size);
    let start_idx = price_data.len() - actual_window;
    
    let sum: f64 = price_data[start_idx..].iter().map(|&(_, price)| price).sum();
    sum / actual_window as f64
}

/// Tính toán biến động giá (volatility) dựa trên độ lệch chuẩn tương đối
fn calculate_volatility(price_data: &[(u64, f64)]) -> f64 {
    if price_data.len() < 2 {
        return 0.0;
    }
    
    let prices: Vec<f64> = price_data.iter().map(|&(_, price)| price).collect();
    let mean = prices.iter().sum::<f64>() / prices.len() as f64;
    
    // Tính độ lệch chuẩn
    let variance = prices.iter()
        .map(|&price| (price - mean).powi(2))
        .sum::<f64>() / prices.len() as f64;
    
    let std_dev = variance.sqrt();
    
    // Biến động = độ lệch chuẩn / giá trung bình (Coefficient of Variation)
    if mean > 0.0 {
        std_dev / mean
    } else {
        0.0
    }
}

/// Tính toán mức độ nhất quán của chuyển động giá
fn calculate_price_consistency(price_data: &[(u64, f64)], is_uptrend: bool) -> f64 {
    if price_data.len() < 3 {
        return 0.5; // Mặc định 50% nếu không đủ dữ liệu
    }
    
    let mut consistent_moves = 0;
    let mut total_moves = 0;
    
    for i in 1..price_data.len() {
        let prev_price = price_data[i-1].1;
        let curr_price = price_data[i].1;
        
        let is_price_up = curr_price > prev_price;
        
        // Đếm số lần giá di chuyển nhất quán với xu hướng tổng thể
        if (is_uptrend && is_price_up) || (!is_uptrend && !is_price_up) {
            consistent_moves += 1;
        }
        
        total_moves += 1;
    }
    
    if total_moves > 0 {
        consistent_moves as f64 / total_moves as f64
    } else {
        0.5
    }
}

/// Kiểm tra tính gần đây của dữ liệu
/// Trả về hệ số từ 0.0 đến 1.0, với 1.0 là dữ liệu rất gần đây
fn check_data_recency(price_data: &[(u64, f64)]) -> f64 {
    if price_data.is_empty() {
        return 0.0;
    }
    
    // Lấy timestamp gần nhất và timestamp hiện tại
    let most_recent_ts = price_data.iter().map(|&(ts, _)| ts).max().unwrap_or(0);
    let current_ts = Utc::now().timestamp() as u64;
    
    // Tính khoảng thời gian (tính bằng giờ)
    let hours_diff = if most_recent_ts < current_ts {
        (current_ts - most_recent_ts) / 3600 // Chuyển đổi giây thành giờ
    } else {
        0
    };
    
    // Nếu dữ liệu cũ hơn 24 giờ, giảm hệ số tin cậy
    if hours_diff < 1 {
        1.0 // Dữ liệu dưới 1 giờ
    } else if hours_diff < 6 {
        0.9 // Dữ liệu dưới 6 giờ
    } else if hours_diff < 12 {
        0.8 // Dữ liệu dưới 12 giờ
    } else if hours_diff < 24 {
        0.7 // Dữ liệu dưới 24 giờ
    } else if hours_diff < 48 {
        0.5 // Dữ liệu dưới 48 giờ
    } else if hours_diff < 72 {
        0.3 // Dữ liệu dưới 72 giờ
    } else {
        0.1 // Dữ liệu quá cũ
    }
}

/// Tính toán kích thước vị thế phù hợp dựa trên cấu hình
fn calculate_position_size(chain_id: u32, current_price: f64, config: &crate::tradelogic::smart_trade::types::SmartTradeConfig) -> f64 {
    // Lấy ngân sách tối đa cho mỗi giao dịch từ mapping hoặc giá trị mặc định
    let max_budget = config.max_trade_amount_map.get(&chain_id)
        .copied()
        .unwrap_or(config.max_trade_amount);
    
    // Lấy risk_factor từ cấu hình (trước đây là hằng số cố định 0.02)
    let risk_factor = config.risk_factor_percent / 100.0;
    
    // Điều chỉnh risk_factor dựa trên thị trường hiện tại
    let adjusted_risk_factor = if config.dynamic_position_sizing {
        // Nếu bật dynamic_position_sizing, điều chỉnh risk_factor dựa trên volatility thị trường
        let market_volatility = config.market_volatility_factor.get(&chain_id).unwrap_or(&1.0);
        (risk_factor * market_volatility).clamp(0.005, 0.05) // Giới hạn từ 0.5% đến 5%
    } else {
        risk_factor
    };
    
    debug!(
        "Position sizing: chain_id={}, max_budget={}, risk_factor={:.4}, adjusted_risk={:.4}",
        chain_id, max_budget, risk_factor, adjusted_risk_factor
    );
    
    // Chiến lược position sizing đơn giản dựa trên rủi ro
    let position_size = max_budget * adjusted_risk_factor;
    
    // Giới hạn trong khoảng min và max từ mapping hoặc giá trị mặc định
    let min_trade = config.min_trade_amount_map.get(&chain_id)
        .copied()
        .unwrap_or(config.min_trade_amount);
    
    position_size.max(min_trade).min(max_budget)
} 