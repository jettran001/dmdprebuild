/// Position Manager - Quản lý vị thế giao dịch
///
/// Module này chứa logic quản lý vị thế giao dịch, trailing stop loss, take profit,
/// và các chiến lược quản lý vị thế khác.
use chrono::Utc;
use tracing::{debug, error, info};
use anyhow::{Result, anyhow};

// Internal imports
use crate::types::{TradeParams, TradeType};

// Module imports
use crate::tradelogic::smart_trade::executor::core::SmartTradeExecutor;
use super::types::{TradeResult, TradeStatus, TradeTracker};
use super::trade_handler::execute_sell;

/// Cập nhật tất cả các vị thế đang mở
pub async fn update_positions(executor: &SmartTradeExecutor) -> Result<()> {
    let active_trades = executor.active_trades.read().await.clone();
    
    // Chỉ xử lý các trade đang trong trạng thái monitoring
    let monitoring_trades: Vec<_> = active_trades.iter()
        .filter(|t| t.status == TradeStatus::Monitoring)
        .collect();
    
    if monitoring_trades.is_empty() {
        return Ok(());
    }
    
    info!("Updating {} active positions", monitoring_trades.len());
    
    for trade in monitoring_trades {
        // Tạo bản sao để cập nhật
        let mut trade_clone = trade.clone();
        
        // Kiểm tra giá hiện tại và cập nhật trạng thái
        match check_position_status(executor, &mut trade_clone).await {
            Ok(action) => match action {
                PositionAction::Hold => {
                    // Tiếp tục giữ vị thế, không làm gì
                    debug!("Holding position for trade {}", trade.trade_id);
                },
                PositionAction::TakeProfit => {
                    info!("Taking profit for trade {}", trade.trade_id);
                    if let Err(e) = take_profit(executor, &mut trade_clone).await {
                        error!("Failed to take profit for trade {}: {}", trade.trade_id, e);
                    }
                },
                PositionAction::StopLoss => {
                    info!("Executing stop loss for trade {}", trade.trade_id);
                    if let Err(e) = stop_loss(executor, &mut trade_clone).await {
                        error!("Failed to execute stop loss for trade {}: {}", trade.trade_id, e);
                    }
                },
                PositionAction::TrailingStop => {
                    info!("Executing trailing stop for trade {}", trade.trade_id);
                    if let Err(e) = trailing_stop(executor, &mut trade_clone).await {
                        error!("Failed to execute trailing stop for trade {}: {}", trade.trade_id, e);
                    }
                },
            },
            Err(e) => {
                error!("Failed to check position status for trade {}: {}", trade.trade_id, e);
            }
        }
        
        // Cập nhật trade trong active_trades nếu đã thay đổi
        if trade_clone.status != trade.status || trade_clone.updated_at != trade.updated_at {
            let mut active_trades = executor.active_trades.write().await;
            
            // Tìm vị trí của trade cũ
            if let Some(index) = active_trades.iter().position(|t| t.trade_id == trade.trade_id) {
                // Thay thế bằng phiên bản mới
                active_trades[index] = trade_clone;
            }
        }
    }
    
    Ok(())
}

/// Các hành động có thể thực hiện cho vị thế
#[derive(Debug, Clone, PartialEq)]
enum PositionAction {
    /// Giữ vị thế, không làm gì
    Hold,
    
    /// Đóng vị thế với lợi nhuận
    TakeProfit,
    
    /// Đóng vị thế với khoản lỗ
    StopLoss,
    
    /// Đóng vị thế do đạt trailing stop
    TrailingStop,
}

/// Kiểm tra trạng thái của vị thế
async fn check_position_status(
    executor: &SmartTradeExecutor,
    trade: &mut TradeTracker,
) -> Result<PositionAction> {
    // Lấy adapter cho chain
    let adapter = match executor.evm_adapters.get(&trade.params.chain_id()) {
        Some(adapter) => adapter,
        None => return Err(anyhow!("No adapter found for chain ID {}", trade.params.chain_id())),
    };
    
    // Lấy giá hiện tại
    let current_price = adapter.get_token_price(&trade.params.token_pair.token_address).await?;
    
    // Cập nhật thời gian
    trade.updated_at = Utc::now().timestamp() as u64;
    
    // Lấy giá ban đầu
    let initial_price = match trade.initial_price {
        Some(price) => price,
        None => {
            // Nếu chưa có giá ban đầu, cập nhật
            trade.initial_price = Some(current_price);
            current_price
        }
    };
    
    // Tính lợi nhuận/lỗ hiện tại (%)
    let profit_percentage = ((current_price - initial_price) / initial_price) * 100.0;
    
    // Lấy cấu hình
    let config = executor.config.read().await;
    
    // Kiểm tra trailing stop
    let mut highest_price = trade.metadata.get("highest_price")
        .and_then(|p| p.parse::<f64>().ok())
        .unwrap_or(initial_price);
    
    // Cập nhật highest_price nếu giá hiện tại cao hơn
    if current_price > highest_price {
        highest_price = current_price;
        trade.metadata.insert("highest_price".to_string(), highest_price.to_string());
    }
    
    // Tính trailing stop (%)
    let trailing_stop_percentage = config.default_trailing_stop_percentage;
    let trailing_stop_price = highest_price * (1.0 - trailing_stop_percentage / 100.0);
    
    // Log thông tin
    debug!(
        "Position status for trade {}: price={}, initial={}, profit={}%, highest={}, trailing_stop_price={}",
        trade.trade_id, current_price, initial_price, profit_percentage, highest_price, trailing_stop_price
    );
    
    // Kiểm tra điều kiện take profit
    if let Some(target_price) = trade.target_price {
        if current_price >= target_price {
            return Ok(PositionAction::TakeProfit);
        }
    }
    
    // Kiểm tra điều kiện stop loss
    if let Some(stop_loss) = trade.stop_loss {
        if current_price <= stop_loss {
            return Ok(PositionAction::StopLoss);
        }
    }
    
    // Kiểm tra trailing stop
    if current_price <= trailing_stop_price && highest_price > initial_price {
        // Chỉ kích hoạt trailing stop nếu đã có lợi nhuận trước đó
        let profit_from_highest = ((current_price - initial_price) / initial_price) * 100.0;
        
        if profit_from_highest > config.min_profit_for_trailing_stop {
            return Ok(PositionAction::TrailingStop);
        }
    }
    
    // Mặc định giữ vị thế
    Ok(PositionAction::Hold)
}

/// Đóng vị thế với lợi nhuận
async fn take_profit(
    executor: &SmartTradeExecutor,
    trade: &mut TradeTracker,
) -> Result<TradeResult> {
    // Cập nhật metadata
    trade.metadata.insert("close_reason".to_string(), "take_profit".to_string());
    
    // Tạo trade params mới cho lệnh bán
    let mut sell_params = trade.params.clone();
    sell_params.trade_type = TradeType::Sell;
    
    // Sử dụng trade_handler để thực hiện bán
    let result = execute_sell(executor, trade).await?;
    
    // Thêm vào lịch sử giao dịch
    if result.status == TradeStatus::Completed {
        let mut history = executor.trade_history.write().await;
        history.push(result.clone());
        
        // Xóa khỏi active trades
        let mut active_trades = executor.active_trades.write().await;
        active_trades.retain(|t| t.trade_id != trade.trade_id);
    }
    
    Ok(result)
}

/// Đóng vị thế khi kích hoạt stop loss
async fn stop_loss(
    executor: &SmartTradeExecutor,
    trade: &mut TradeTracker,
) -> Result<TradeResult> {
    // Cập nhật metadata
    trade.metadata.insert("close_reason".to_string(), "stop_loss".to_string());
    
    // Tạo trade params mới cho lệnh bán
    let mut sell_params = trade.params.clone();
    sell_params.trade_type = TradeType::Sell;
    
    // Sử dụng trade_handler để thực hiện bán
    let result = execute_sell(executor, trade).await?;
    
    // Thêm vào lịch sử giao dịch
    if result.status == TradeStatus::Completed {
        let mut history = executor.trade_history.write().await;
        history.push(result.clone());
        
        // Xóa khỏi active trades
        let mut active_trades = executor.active_trades.write().await;
        active_trades.retain(|t| t.trade_id != trade.trade_id);
    }
    
    Ok(result)
}

/// Đóng vị thế khi kích hoạt trailing stop
async fn trailing_stop(
    executor: &SmartTradeExecutor,
    trade: &mut TradeTracker,
) -> Result<TradeResult> {
    // Cập nhật metadata
    trade.metadata.insert("close_reason".to_string(), "trailing_stop".to_string());
    
    // Tạo trade params mới cho lệnh bán
    let mut sell_params = trade.params.clone();
    sell_params.trade_type = TradeType::Sell;
    
    // Sử dụng trade_handler để thực hiện bán
    let result = execute_sell(executor, trade).await?;
    
    // Thêm vào lịch sử giao dịch
    if result.status == TradeStatus::Completed {
        let mut history = executor.trade_history.write().await;
        history.push(result.clone());
        
        // Xóa khỏi active trades
        let mut active_trades = executor.active_trades.write().await;
        active_trades.retain(|t| t.trade_id != trade.trade_id);
    }
    
    Ok(result)
}

/// Đóng vị thế thủ công theo yêu cầu người dùng
pub async fn close_position(
    executor: &SmartTradeExecutor,
    trade_id: &str,
) -> Result<TradeResult> {
    // Tìm trade trong active trades
    let mut active_trades = executor.active_trades.write().await;
    let trade_index = active_trades.iter().position(|t| t.trade_id == trade_id);
    
    match trade_index {
        Some(index) => {
            // Lấy và xóa trade từ active trades
            let mut trade = active_trades.remove(index);
            
            // Cập nhật metadata
            trade.metadata.insert("close_reason".to_string(), "manual_close".to_string());
            
            // Tạo trade params mới cho lệnh bán
            let mut sell_params = trade.params.clone();
            sell_params.trade_type = TradeType::Sell;
            
            // Sử dụng trade_handler để thực hiện bán
            let result = execute_sell(executor, &mut trade).await?;
            
            // Thêm vào lịch sử giao dịch
            if result.status == TradeStatus::Completed {
                let mut history = executor.trade_history.write().await;
                history.push(result.clone());
            } else {
                // Nếu thất bại, thêm lại vào active trades
                active_trades.push(trade);
            }
            
            Ok(result)
        },
        None => {
            Err(anyhow!("Trade with ID {} not found", trade_id))
        }
    }
}

/// Tạo vị thế DCA (Dollar Cost Averaging)
pub async fn create_dca_position(
    executor: &SmartTradeExecutor,
    params: &TradeParams,
    total_entries: u32,
    entry_interval_seconds: u64,
) -> Result<TradeTracker> {
    // Tạo trade tracker chính
    let trade_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().timestamp() as u64;
    
    // Tạo trade tracker với chiến lược DCA
    let mut tracker = super::trade_handler::create_trade_tracker(
        params,
        &trade_id,
        now,
        super::types::TradeStrategy::DCA,
    );
    
    // Thêm thông tin DCA vào metadata
    tracker.metadata.insert("dca_total_entries".to_string(), total_entries.to_string());
    tracker.metadata.insert("dca_entry_interval".to_string(), entry_interval_seconds.to_string());
    tracker.metadata.insert("dca_completed_entries".to_string(), "0".to_string());
    
    // Lưu vào active trades
    let mut active_trades = executor.active_trades.write().await;
    active_trades.push(tracker.clone());
    
    // Trả về tracker cho lệnh DCA
    Ok(tracker)
}

/// Thực hiện một lần DCA
pub async fn execute_dca_entry(
    executor: &SmartTradeExecutor,
    dca_trade_id: &str,
) -> Result<TradeResult> {
    // Tìm trade trong active trades
    let mut active_trades = executor.active_trades.write().await;
    let trade_index = active_trades.iter().position(|t| t.trade_id == dca_trade_id);
    
    match trade_index {
        Some(index) => {
            // Lấy trade từ active trades
            let mut dca_trade = active_trades[index].clone();
            
            // Kiểm tra xem có phải chiến lược DCA không
            if dca_trade.strategy != super::types::TradeStrategy::DCA {
                return Err(anyhow!("Trade is not a DCA strategy"));
            }
            
            // Đọc thông tin DCA
            let completed_entries = dca_trade.metadata.get("dca_completed_entries")
                .and_then(|e| e.parse::<u32>().ok())
                .unwrap_or(0);
            
            let total_entries = dca_trade.metadata.get("dca_total_entries")
                .and_then(|e| e.parse::<u32>().ok())
                .unwrap_or(0);
            
            // Kiểm tra xem đã hoàn thành tất cả entry chưa
            if completed_entries >= total_entries {
                return Err(anyhow!("DCA strategy already completed all entries"));
            }
            
            // Tạo params cho entry mới
            let entry_params = dca_trade.params.clone();
            
            // Tạo trade tracker mới cho entry này
            let entry_trade_id = format!("{}_entry_{}", dca_trade_id, completed_entries + 1);
            let now = Utc::now().timestamp() as u64;
            
            let mut entry_tracker = super::trade_handler::create_trade_tracker(
                &entry_params,
                &entry_trade_id,
                now,
                super::types::TradeStrategy::SingleTrade,
            );
            
            // Thêm metadata
            entry_tracker.metadata.insert("parent_dca_id".to_string(), dca_trade_id.to_string());
            entry_tracker.metadata.insert("dca_entry_number".to_string(), (completed_entries + 1).to_string());
            
            // Thực hiện entry
            let result = execute_sell(executor, &mut entry_tracker).await?;
            
            // Cập nhật completed entries
            dca_trade.metadata.insert("dca_completed_entries".to_string(), (completed_entries + 1).to_string());
            
            // Thêm kết quả vào sub_trades
            dca_trade.sub_trades.push(result.clone());
            
            // Cập nhật DCA trade trong active_trades
            active_trades[index] = dca_trade;
            
            // Trả về kết quả
            Ok(result)
        },
        None => {
            Err(anyhow!("DCA trade with ID {} not found", dca_trade_id))
        }
    }
} 