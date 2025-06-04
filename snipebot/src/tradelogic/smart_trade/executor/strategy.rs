/// Strategy - Chiến lược giao dịch
///
/// Module này chứa các chiến lược giao dịch khác nhau như DCA, grid trading, TWAP,
/// và các chiến lược tùy chỉnh khác.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{Utc, Duration};
use tracing::{debug, error, info, warn};
use anyhow::{Result, Context, anyhow, bail};

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::{TokenPair, TradeParams, TradeType};
use crate::tradelogic::traits::OpportunityPriority;
use crate::tradelogic::common::ResourcePriority;
use crate::tradelogic::common::get_resource_manager;

// Module imports
use crate::tradelogic::smart_trade::executor::core::SmartTradeExecutor;
use super::types::{TradeResult, TradeStatus, TradeTracker, TradeStrategy, TaskHandle};
use super::trade_handler::{create_trade_tracker, execute_buy, execute_sell};
use super::position_manager::create_dca_position;

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
        match super::position_manager::execute_dca_entry(&executor_clone, &dca_id).await {
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
            match super::position_manager::execute_dca_entry(&executor_clone, &dca_id).await {
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
        let adapter = match executor.evm_adapters.get(&grid_trade.params.chain_id) {
            Some(adapter) => adapter,
            None => {
                error!("No adapter found for chain ID {}", grid_trade.params.chain_id);
                // Đợi một lúc trước khi kiểm tra lại
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                continue;
            }
        };
        
        // Sử dụng resource manager để lấy giá token, đảm bảo không quá tải hệ thống
        let token_price = match resource_manager
            .with_blockchain_limits_async(
                grid_trade.params.chain_id as u32,
                ResourcePriority::Medium,
                "grid_strategy_get_price",
                || adapter.get_token_price(&grid_trade.params.token_pair.token_address),
            ).await {
            Ok(price) => price,
            Err(e) => {
                warn!("Failed to get token price for grid strategy: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(config.price_check_interval_seconds)).await;
                continue;
            }
        };
        
        // Kiểm tra xem giá hiện tại nằm trong grid nào
        for (i, &grid_price) in grid_prices.iter().enumerate() {
            // Kiểm tra xem grid này đã thực thi chưa
            let grid_key = format!("grid_{}_executed", i);
            
            // Lấy thông tin grid từ metadata
            let grid_executed = {
                let active_trades = executor.active_trades.read().await;
                if let Some(trade) = active_trades.iter().find(|t| t.trade_id == trade_id) {
                    trade.metadata.contains_key(&grid_key)
                } else {
                    // Trade không còn tồn tại
                    return Ok(());
                }
            };
            
            if grid_executed {
                continue;
            }
            
            // Nếu giá hiện tại gần với giá grid (trong khoảng 0.5%)
            if (token_price - grid_price).abs() / grid_price < 0.005 {
                debug!("Price {} close to grid {} at {}, executing grid", token_price, i, grid_price);
                
                // Tạo trade mới cho grid này
                let mut grid_params = grid_trade.params.clone();
                grid_params.amount = amount_per_grid;
                
                // Nếu giá đang tăng thì bán, nếu giá đang giảm thì mua
                grid_params.trade_type = if i > 0 && token_price > grid_prices[i - 1] {
                    TradeType::Sell
                } else {
                    TradeType::Buy
                };
                
                // Tạo trade tracker mới cho grid này
                let grid_trade_id = format!("{}_grid_{}", trade_id, i);
                let now = Utc::now().timestamp() as u64;
                
                let mut grid_tracker = create_trade_tracker(
                    &grid_params,
                    &grid_trade_id,
                    now,
                    TradeStrategy::SingleTrade,
                );
                
                // Thêm metadata
                grid_tracker.metadata.insert("parent_grid_id".to_string(), trade_id.to_string());
                grid_tracker.metadata.insert("grid_number".to_string(), i.to_string());
                grid_tracker.metadata.insert("grid_price".to_string(), grid_price.to_string());
                
                // Thực hiện grid trade
                let result = match grid_params.trade_type {
                    TradeType::Buy => execute_buy(executor, &mut grid_tracker).await,
                    TradeType::Sell => execute_sell(executor, &mut grid_tracker).await,
                    _ => Err(anyhow!("Unsupported trade type for grid strategy")),
                };
                
                // Cập nhật trạng thái grid
                let mut active_trades = executor.active_trades.write().await;
                if let Some(index) = active_trades.iter().position(|t| t.trade_id == trade_id) {
                    let mut updated_trade = active_trades[index].clone();
                    updated_trade.metadata.insert(grid_key, "true".to_string());
                    
                    // Thêm kết quả vào sub_trades nếu thành công
                    if let Ok(trade_result) = result {
                        updated_trade.sub_trades.push(trade_result);
                    }
                    
                    active_trades[index] = updated_trade;
                }
            }
        }
        
        // Kiểm tra xem tất cả các grid đã được thực thi chưa
        let all_executed = {
            let active_trades = executor.active_trades.read().await;
            if let Some(trade) = active_trades.iter().find(|t| t.trade_id == trade_id) {
                (0..grid_prices.len()).all(|i| {
                    trade.metadata.contains_key(&format!("grid_{}_executed", i))
                })
            } else {
                false
            }
        };
        
        // Nếu tất cả grid đã thực thi, kết thúc chiến lược
        if all_executed {
            info!("All grids for trade {} have been executed, completing strategy", trade_id);
            
            // Cập nhật trạng thái
            let mut active_trades = executor.active_trades.write().await;
            if let Some(index) = active_trades.iter().position(|t| t.trade_id == trade_id) {
                let mut updated_trade = active_trades[index].clone();
                updated_trade.status = TradeStatus::Completed;
                updated_trade.metadata.insert("strategy_completed".to_string(), "true".to_string());
                
                // Chuyển từ active trades sang history
                let trade_result = TradeResult {
                    trade_id: updated_trade.trade_id.clone(),
                    params: updated_trade.params.clone(),
                    status: TradeStatus::Completed,
                    created_at: updated_trade.created_at,
                    completed_at: Some(Utc::now().timestamp() as u64),
                    tx_hash: None,
                    actual_amount: None,
                    actual_price: None,
                    fee: None,
                    profit_loss: None,
                    error: None,
                    explorer_url: None,
                };
                
                // Lưu vào history
                let mut history = executor.trade_history.write().await;
                history.push(trade_result);
                
                active_trades[index] = updated_trade;
            }
            
            break;
        }
        
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
    
    // Lưu vào active trades
    let mut active_trades = executor.active_trades.write().await;
    
    // Khởi động vòng lặp TWAP
    let task_id = uuid::Uuid::new_v4().to_string();
    let executor_clone = executor.clone();
    let params_clone = params.clone();
    let trade_id_clone = trade_id.clone();
    
    let handle = tokio::spawn(async move {
        let amount_per_part = params_clone.amount / (total_parts as f64);
        
        // Theo dõi giá ban đầu
        let adapter = match executor_clone.evm_adapters.get(&params_clone.chain_id) {
            Some(adapter) => adapter,
            None => {
                error!("No adapter found for chain ID {}", params_clone.chain_id);
                return;
            }
        };
        
        // Sử dụng resource manager để lấy giá ban đầu
        let resource_manager = get_resource_manager();
        let initial_price = match resource_manager
            .with_blockchain_limits_async(
                params_clone.chain_id,
                ResourcePriority::Medium,
                "twap_get_initial_price",
                || adapter.get_token_price(&params_clone.token_pair.token_address),
            ).await {
            Ok(price) => price,
            Err(e) => {
                error!("Failed to get initial token price for TWAP: {}", e);
                return;
            }
        };
        
        // Thực hiện từng phần TWAP
        for i in 0..total_parts {
            if i > 0 {
                // Chờ đến thời gian tiếp theo
                tokio::time::sleep(tokio::time::Duration::from_secs(interval_seconds)).await;
            }
            
            // Kiểm tra nếu executor đã dừng
            if !*executor_clone.running.read().await {
                debug!("Executor stopped, cancelling TWAP execution for trade {}", trade_id_clone);
                break;
            }
            
            // Kiểm tra xem TWAP còn trong active trades không
            let active_trades = executor_clone.active_trades.read().await;
            if !active_trades.iter().any(|t| t.trade_id == trade_id_clone) {
                debug!("TWAP position {} no longer active, stopping schedule", trade_id_clone);
                break;
            }
            
            // Lấy giá hiện tại
            let current_price = match resource_manager
                .with_blockchain_limits_async(
                    params_clone.chain_id,
                    ResourcePriority::Medium,
                    "twap_get_current_price",
                    || adapter.get_token_price(&params_clone.token_pair.token_address),
                ).await {
                Ok(price) => price,
                Err(e) => {
                    error!("Failed to get current token price for TWAP part {}: {}", i, e);
                    continue;
                }
            };
            
            // Kiểm tra slippage
            let price_change = ((current_price - initial_price) / initial_price).abs() * 100.0;
            if price_change > max_slippage_percent {
                warn!(
                    "TWAP part {} skipped due to excessive price change: {:.2}% (max: {:.2}%)",
                    i, price_change, max_slippage_percent
                );
                continue;
            }
            
            // Tạo params cho phần này
            let mut part_params = params_clone.clone();
            part_params.amount = amount_per_part;
            
            // Tạo trade tracker mới cho phần này
            let part_trade_id = format!("{}_part_{}", trade_id_clone, i);
            let now = Utc::now().timestamp() as u64;
            
            let mut part_tracker = create_trade_tracker(
                &part_params,
                &part_trade_id,
                now,
                TradeStrategy::SingleTrade,
            );
            
            // Thêm metadata
            part_tracker.metadata.insert("parent_twap_id".to_string(), trade_id_clone.to_string());
            part_tracker.metadata.insert("twap_part_number".to_string(), i.to_string());
            
            // Thực hiện phần TWAP
            let result = match part_params.trade_type {
                TradeType::Buy => execute_buy(&executor_clone, &mut part_tracker).await,
                TradeType::Sell => execute_sell(&executor_clone, &mut part_tracker).await,
                _ => {
                    error!("Unsupported trade type for TWAP strategy");
                    continue;
                }
            };
            
            // Cập nhật trạng thái TWAP
            let mut active_trades = executor_clone.active_trades.write().await;
            if let Some(index) = active_trades.iter().position(|t| t.trade_id == trade_id_clone) {
                let mut updated_trade = active_trades[index].clone();
                
                // Cập nhật số phần đã hoàn thành
                let completed_parts = updated_trade.metadata.get("twap_completed_parts")
                    .and_then(|p| p.parse::<u32>().ok())
                    .unwrap_or(0);
                    
                updated_trade.metadata.insert("twap_completed_parts".to_string(), (completed_parts + 1).to_string());
                
                // Thêm kết quả vào sub_trades nếu thành công
                if let Ok(trade_result) = result {
                    updated_trade.sub_trades.push(trade_result);
                }
                
                active_trades[index] = updated_trade;
            }
        }
        
        // Cập nhật trạng thái TWAP khi hoàn thành
        let mut active_trades = executor_clone.active_trades.write().await;
        if let Some(index) = active_trades.iter().position(|t| t.trade_id == trade_id_clone) {
            let mut updated_trade = active_trades[index].clone();
            updated_trade.status = TradeStatus::Completed;
            
            // Tính trung bình giá tất cả các phần
            let mut total_amount = 0.0;
            let mut total_value = 0.0;
            
            for sub_trade in &updated_trade.sub_trades {
                if let (Some(amount), Some(price)) = (sub_trade.actual_amount, sub_trade.actual_price) {
                    total_amount += amount;
                    total_value += amount * price;
                }
            }
            
            let avg_price = if total_amount > 0.0 {
                total_value / total_amount
            } else {
                0.0
            };
            
            updated_trade.metadata.insert("twap_average_price".to_string(), avg_price.to_string());
            
            // Chuyển từ active trades sang history
            let mut history = executor_clone.trade_history.write().await;
            
            // Tạo kết quả tổng hợp
            let result = TradeResult {
                trade_id: updated_trade.trade_id.clone(),
                params: updated_trade.params.clone(),
                status: TradeStatus::Completed,
                created_at: updated_trade.created_at,
                completed_at: Some(Utc::now().timestamp() as u64),
                tx_hash: None, // Không có tx hash cho toàn bộ TWAP
                actual_amount: Some(total_amount),
                actual_price: Some(avg_price),
                fee: None,
                profit_loss: None,
                error: None,
                explorer_url: None,
            };
            
            history.push(result);
            
            // Xóa khỏi active trades
            active_trades.remove(index);
        }
    });
    
    // Lưu task handle vào tracker
    tracker.task_handle = Some(TaskHandle {
        join_handle: handle,
        task_id,
        description: format!("TWAP strategy execution for trade {}", trade_id),
    });
    
    // Thêm vào active trades
    active_trades.push(tracker.clone());
    
    Ok(tracker)
} 