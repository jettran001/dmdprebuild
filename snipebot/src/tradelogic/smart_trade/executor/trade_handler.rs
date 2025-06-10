//! Trade Handler cho Smart Trade Executor
//!
//! Module này xử lý việc thực thi các giao dịch, bao gồm
//! kiểm tra điều kiện, khởi tạo giao dịch, và theo dõi kết quả.
use std::sync::Arc;
use std::collections::HashMap;
use anyhow::{Result, Context};
use tracing::{debug, info, error, warn};

use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::{TradeParams, TradeType};
use crate::tradelogic::traits::{SharedOpportunity};
use crate::tradelogic::common::{
    NetworkStatus, RetryStrategy
};

// Imports from other modules
use crate::tradelogic::smart_trade::executor::core::SmartTradeExecutor;
use super::types::{TradeResult, TradeStatus, TradeStrategy, TradeTracker};
use super::utils::retry_async;
use super::risk_manager::validate_token_safety;
use super::risk_manager::RiskManager;
use super::position_manager::PositionManager;

/// Thêm cấu trúc mới để quản lý việc khôi phục giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryParams {
    /// Số lần thử tối đa
    pub max_retries: u32,
    /// Thời gian chờ giữa các lần thử (ms)
    pub retry_delay_ms: u64,
    /// Các lỗi cho phép thử lại
    pub retriable_errors: Vec<String>,
    /// Các lỗi nghiêm trọng không thử lại
    pub fatal_errors: Vec<String>,
    /// Thời gian chờ tối đa trước khi hủy giao dịch (ms)
    pub timeout_ms: u64,
    /// Dữ liệu trạng thái đã lưu từ lần thử trước
    pub recovery_state: HashMap<String, String>,
}

impl Default for RecoveryParams {
    fn default() -> Self {
        Self {
            max_retries: 3,
            retry_delay_ms: 5000,
            retriable_errors: vec![
                "nonce too low".to_string(),
                "insufficient funds for gas".to_string(),
                "transaction underpriced".to_string(),
                "connection reset by peer".to_string(),
                "timeout".to_string(),
                "network error".to_string(),
            ],
            fatal_errors: vec![
                "already known".to_string(),
                "nonce too high".to_string(),
                "replacement transaction underpriced".to_string(),
            ],
            timeout_ms: 180000,
            recovery_state: HashMap::new(),
        }
    }
}

/// Tạo trade tracker từ params
pub fn create_trade_tracker(
    params: &TradeParams,
    trade_id: &str,
    timestamp: u64,
    strategy: TradeStrategy,
) -> TradeTracker {
    // Tạo TradeTracker với các thông tin recovery mặc định
    let mut tracker = TradeTracker {
        trade_id: trade_id.to_string(),
        params: params.clone(),
        status: TradeStatus::Initialized,
        created_at: timestamp,
        updated_at: timestamp,
        strategy,
        metadata: HashMap::new(),
        initial_price: None,
        target_price: None, 
        stop_loss: None,
        tx_hash: None,
        retry_count: 0,
        sub_trades: Vec::new(),
        task_handle: None,
    };
    
    // Thêm recovery params vào metadata
    tracker.metadata.insert(
        "recovery_params".to_string(),
        serde_json::to_string(&RecoveryParams::default()).unwrap_or_default()
    );
    
    tracker
}

/// Thực thi giao dịch mua token
pub async fn execute_buy(
    executor: &SmartTradeExecutor,
    trade_tracker: &mut TradeTracker,
) -> Result<TradeResult> {
    let params = &trade_tracker.params;
    
    // Lấy adapter cho chain
    let adapter = match executor.evm_adapters.get(&params.chain_id()) {
        Some(adapter) => adapter,
        None => return Err(anyhow!("No adapter found for chain ID {}", params.chain_id())),
    };
    
    // Lưu trạng thái giao dịch
    save_trade_state(executor, trade_tracker).await?;
    
    // Kiểm tra trạng thái mạng trước khi thực hiện giao dịch
    let service_id = format!("blockchain_rpc_{}", params.chain_id());
    let network_status = executor.network_manager.get_network_status(&service_id).await;
    
    if network_status == NetworkStatus::Failed {
        // Lên lịch thử lại sau thay vì thất bại ngay lập tức
        schedule_trade_retry(executor, trade_tracker, "Network unavailable").await?;
        
        return Ok(TradeResult {
            trade_id: trade_tracker.trade_id.clone(),
            params: params.clone(),
            status: TradeStatus::Scheduled,
            created_at: trade_tracker.created_at,
            completed_at: None,
            tx_hash: None,
            actual_amount: None,
            actual_price: None,
            fee: None,
            profit_loss: None,
            error: Some(format!("Network connection to chain {} is currently unavailable. Scheduled for retry.", params.chain_id())),
            explorer_url: None,
        });
    }
    
    // Kiểm tra an toàn token trước khi mua với bảo vệ mạng
    let (is_safe, issues) = executor.with_network_protection(
        params.chain_id(),
        "validate_token_safety",
        || validate_token_safety(executor, params)
    ).await?;
    
    if !is_safe {
        let issues_str = issues.join(", ");
        warn!("Token safety check failed for trade {}: {}", trade_tracker.trade_id, issues_str);
        
        // Phân loại mức độ nghiêm trọng của vấn đề
        let has_critical_issue = issues.iter().any(|issue| 
            issue.contains("honeypot") || 
            issue.contains("blacklisted") || 
            issue.contains("scam")
        );
        
        if has_critical_issue {
            // Vấn đề nghiêm trọng, hủy giao dịch
            trade_tracker.status = TradeStatus::Failed;
            save_trade_state(executor, trade_tracker).await?;
            
            return Ok(TradeResult {
                trade_id: trade_tracker.trade_id.clone(),
                params: params.clone(),
                status: TradeStatus::Failed,
                created_at: trade_tracker.created_at,
                completed_at: Some(Utc::now().timestamp() as u64),
                tx_hash: None,
                actual_amount: None,
                actual_price: None,
                fee: None,
                profit_loss: None,
                error: Some(format!("Critical token safety issue detected: {}", issues_str)),
                explorer_url: None,
            });
        } else {
            // Vấn đề không nghiêm trọng, ghi nhật ký cảnh báo và tiếp tục
            warn!("Non-critical token safety issues detected for trade {}, proceeding with caution: {}", 
                trade_tracker.trade_id, issues_str);
            
            // Thêm cảnh báo vào metadata
            trade_tracker.metadata.insert("safety_warnings".to_string(), issues_str.clone());
        }
    }
    
    // Cập nhật trạng thái
    trade_tracker.status = TradeStatus::Executing;
    save_trade_state(executor, trade_tracker).await?;
    
    // Lấy cấu hình
    let config = executor.config.read().await;
    
    // Lấy giá hiện tại với circuit breaker
    let current_price = match executor.with_network_protection(
        params.chain_id(),
        "get_token_price",
        || adapter.get_token_price(&params.token_address)
    ).await {
        Ok(price) => price,
        Err(e) => {
            warn!("Failed to get token price for trade {}: {}", trade_tracker.trade_id, e);
            
            // Lên lịch thử lại sau
            schedule_trade_retry(executor, trade_tracker, &format!("Price fetch error: {}", e)).await?;
            
            return Ok(TradeResult {
                trade_id: trade_tracker.trade_id.clone(),
                params: params.clone(),
                status: TradeStatus::Scheduled,
                created_at: trade_tracker.created_at,
                completed_at: None,
                tx_hash: None,
                actual_amount: None,
                actual_price: None,
                fee: None,
                profit_loss: None,
                error: Some(format!("Failed to get token price, scheduled for retry: {}", e)),
                explorer_url: None,
            });
        }
    };
    
    // Lưu giá ban đầu
    trade_tracker.initial_price = Some(current_price);
    save_trade_state(executor, trade_tracker).await?;
    
    // Tính toán số lượng token mua với slippage
    let slippage = params.slippage.unwrap_or(config.default_slippage);
    
    // Lấy nonce từ NonceManager với bảo vệ mạng
    let mut nonce_manager = executor.nonce_manager.write().await;
    let nonce = match executor.with_network_protection(
        params.chain_id(),
        "get_next_nonce",
        || nonce_manager.get_next_nonce(
            params.chain_id(),
            &params.wallet_address,
            adapter
        )
    ).await {
        Ok(nonce) => nonce,
        Err(e) => {
            warn!("Failed to get nonce for trade {}: {}", trade_tracker.trade_id, e);
            
            // Lên lịch thử lại sau
            schedule_trade_retry(executor, trade_tracker, &format!("Nonce error: {}", e)).await?;
            
            return Ok(TradeResult {
                trade_id: trade_tracker.trade_id.clone(),
                params: params.clone(),
                status: TradeStatus::Scheduled,
                created_at: trade_tracker.created_at,
                completed_at: None,
                tx_hash: None,
                actual_amount: None,
                actual_price: None,
                fee: None,
                profit_loss: None,
                error: Some(format!("Failed to get nonce, scheduled for retry: {}", e)),
                explorer_url: None,
            });
        }
    };
    
    debug!("Using nonce {} for buy transaction {} on chain {}", 
        nonce, trade_tracker.trade_id, params.chain_id());
    
    // Tạo retry strategy từ cấu hình
    let retry_strategy = RetryStrategy {
        max_retries: config.max_retries as usize,
        base_delay_ms: config.retry_delay_ms,
        backoff_factor: 1.5,
        max_delay_ms: config.max_retry_delay_ms,
        add_jitter: true,
    };
    
    // Thực hiện swap từ base token sang token với circuit breaker và retry
    let result = match executor.network_manager.with_circuit_breaker(
        &service_id,
        || adapter.swap_tokens_with_nonce(
            &params.base_token(),
            &params.token_address,
            params.amount,
            slippage,
            params.wallet_address.clone(),
            nonce
        ),
        Some(retry_strategy)
    ).await {
        Ok(tx) => {
            info!("Buy executed successfully for trade {}: tx hash {}", trade_tracker.trade_id, tx.tx_hash);
            
            // Đánh dấu giao dịch đã được gửi trong NonceManager
            nonce_manager.mark_transaction_sent(
                params.chain_id(),
                &params.wallet_address,
                nonce,
                &tx.tx_hash
            );
            
            // Báo cáo thành công cho network manager
            executor.network_manager.record_success(&service_id).await;
            
            // Cập nhật trade tracker
            trade_tracker.status = TradeStatus::Monitoring;
            trade_tracker.tx_hash = Some(tx.tx_hash.clone());
            
            // Thiết lập giá mục tiêu và stop loss
            if let Some(target_profit) = params.target_profit {
                trade_tracker.target_price = Some(current_price * (1.0 + target_profit / 100.0));
            }
            
            if let Some(stop_loss) = params.stop_loss {
                trade_tracker.stop_loss = Some(current_price * (1.0 - stop_loss / 100.0));
            }
            
            // Tạo kết quả giao dịch
            TradeResult {
                trade_id: trade_tracker.trade_id.clone(),
                params: params.clone(),
                status: TradeStatus::Monitoring,
                created_at: trade_tracker.created_at,
                completed_at: None,
                tx_hash: Some(tx.tx_hash),
                actual_amount: Some(tx.amount),
                actual_price: Some(current_price),
                fee: Some(tx.fee),
                profit_loss: None,
                error: None,
                explorer_url: Some(format!("{}/tx/{}", get_explorer_url(params.chain_id()), tx.tx_hash)),
            }
        },
        Err(e) => {
            error!("Failed to execute buy for trade {}: {}", trade_tracker.trade_id, e);
            
            // Báo cáo thất bại cho network manager
            executor.network_manager.record_failure(&service_id).await;
            
            // Kiểm tra nếu giao dịch đã được đưa vào mempool nhưng gọi API lỗi
            let pending_tx = nonce_manager.check_pending_transactions(
                adapter, 
                params.chain_id(), 
                &params.wallet_address
            ).await;
            
            if let Err(e) = pending_tx {
                warn!("Failed to check pending transactions: {}", e);
            }
            
            // Cập nhật trade tracker
            trade_tracker.status = TradeStatus::Failed;
            trade_tracker.retry_count += 1;
            
            // Tạo kết quả giao dịch
            TradeResult {
                trade_id: trade_tracker.trade_id.clone(),
                params: params.clone(),
                status: TradeStatus::Failed,
                created_at: trade_tracker.created_at,
                completed_at: Some(Utc::now().timestamp() as u64),
                tx_hash: None,
                actual_amount: None,
                actual_price: None,
                fee: None,
                profit_loss: None,
                error: Some(format!("Failed to execute buy: {}", e)),
                explorer_url: None,
            }
        }
    };
    
    Ok(result)
}

/// Lấy URL explorer dựa vào chain_id
fn get_explorer_url(chain_id: u32) -> String {
    match chain_id {
        1 => "https://etherscan.io".to_string(),
        56 => "https://bscscan.com".to_string(),
        137 => "https://polygonscan.com".to_string(),
        42161 => "https://arbiscan.io".to_string(),
        10 => "https://optimistic.etherscan.io".to_string(),
        43114 => "https://snowtrace.io".to_string(),
        250 => "https://ftmscan.com".to_string(),
        _ => format!("https://blockscout.com/poa/xdai/chain/{}", chain_id),
    }
}

/// Thực thi giao dịch bán token
pub async fn execute_sell(
    executor: &SmartTradeExecutor,
    trade_tracker: &mut TradeTracker,
) -> Result<TradeResult> {
    let params = &trade_tracker.params;
    
    // Lấy adapter cho chain
    let adapter = match executor.evm_adapters.get(&params.chain_id()) {
        Some(adapter) => adapter,
        None => return Err(anyhow!("No adapter found for chain ID {}", params.chain_id())),
    };
    
    // Cập nhật trạng thái
    trade_tracker.status = TradeStatus::Executing;
    
    // Lấy giá hiện tại
    let current_price = match adapter.get_token_price(&params.token_address).await {
        Ok(price) => price,
        Err(e) => {
            warn!("Failed to get token price for trade {}: {}", trade_tracker.trade_id, e);
            
            return Ok(TradeResult {
                trade_id: trade_tracker.trade_id.clone(),
                params: params.clone(),
                status: TradeStatus::Failed,
                created_at: trade_tracker.created_at,
                completed_at: Some(Utc::now().timestamp() as u64),
                tx_hash: None,
                actual_amount: None,
                actual_price: None,
                fee: None,
                profit_loss: None,
                error: Some(format!("Failed to get token price: {}", e)),
                explorer_url: None,
            });
        }
    };
    
    // Lưu giá ban đầu nếu chưa có
    if trade_tracker.initial_price.is_none() {
        trade_tracker.initial_price = Some(current_price);
    }
    
    // Tính lợi nhuận/lỗ nếu có giá ban đầu
    let profit_loss = if let Some(initial_price) = trade_tracker.initial_price {
        Some(((current_price - initial_price) / initial_price) * 100.0)
    } else {
        None
    };
    
    // Tính toán số lượng token bán với slippage
    let config = executor.config.read().await;
    let slippage = params.slippage.unwrap_or(config.default_slippage);
    
    // Lấy nonce từ NonceManager
    let mut nonce_manager = executor.nonce_manager.write().await;
    let nonce = match nonce_manager.get_next_nonce(
        params.chain_id(),
        &params.wallet_address,
        adapter
    ).await {
        Ok(nonce) => nonce,
        Err(e) => {
            warn!("Failed to get nonce for trade {}: {}", trade_tracker.trade_id, e);
            
            return Ok(TradeResult {
                trade_id: trade_tracker.trade_id.clone(),
                params: params.clone(),
                status: TradeStatus::Failed,
                created_at: trade_tracker.created_at,
                completed_at: Some(Utc::now().timestamp() as u64),
                tx_hash: None,
                actual_amount: None,
                actual_price: None,
                fee: None,
                profit_loss,
                error: Some(format!("Failed to get nonce: {}", e)),
                explorer_url: None,
            });
        }
    };
    
    debug!("Using nonce {} for sell transaction {} on chain {}", 
        nonce, trade_tracker.trade_id, params.chain_id());
    
    // Thực hiện swap từ token sang base token với nonce cụ thể
    let result = match adapter.swap_tokens_with_nonce(
        &params.token_address,
        &params.base_token(),
        params.amount,
        slippage,
        params.wallet_address.clone(),
        nonce
    ).await {
        Ok(tx) => {
            info!("Sell executed successfully for trade {}: tx hash {}", trade_tracker.trade_id, tx.tx_hash);
            
            // Đánh dấu giao dịch đã được gửi trong NonceManager
            nonce_manager.mark_transaction_sent(
                params.chain_id(),
                &params.wallet_address,
                nonce,
                &tx.tx_hash
            );
            
            // Cập nhật trade tracker
            trade_tracker.status = TradeStatus::Completed;
            trade_tracker.tx_hash = Some(tx.tx_hash.clone());
            
            // Tạo kết quả giao dịch
            TradeResult {
                trade_id: trade_tracker.trade_id.clone(),
                params: params.clone(),
                status: TradeStatus::Completed,
                created_at: trade_tracker.created_at,
                completed_at: Some(Utc::now().timestamp() as u64),
                tx_hash: Some(tx.tx_hash),
                actual_amount: Some(tx.amount),
                actual_price: Some(current_price),
                fee: Some(tx.fee),
                profit_loss,
                error: None,
                explorer_url: Some(tx.explorer_url),
            }
        },
        Err(e) => {
            warn!("Sell failed for trade {}: {}", trade_tracker.trade_id, e);
            
            // Cập nhật trade tracker
            trade_tracker.status = TradeStatus::Failed;
            
            TradeResult {
                trade_id: trade_tracker.trade_id.clone(),
                params: params.clone(),
                status: TradeStatus::Failed,
                created_at: trade_tracker.created_at,
                completed_at: Some(Utc::now().timestamp() as u64),
                tx_hash: None,
                actual_amount: None,
                actual_price: Some(current_price),
                fee: None,
                profit_loss,
                error: Some(format!("Transaction execution failed: {}", e)),
                explorer_url: None,
            }
        }
    };
    
    Ok(result)
}

/// Thực thi giao dịch từ shared opportunity
pub async fn execute_opportunity(
    executor: &SmartTradeExecutor,
    opportunity: SharedOpportunity,
) -> Result<TradeResult> {
    // Tạo trade params từ opportunity
    let params = TradeParams {
        chain_id: opportunity.chain_id,
        token_address: opportunity.token_pair.token_address,
        trade_type: opportunity.trade_type.clone(),
        amount: opportunity.recommended_amount,
        slippage: None, // Sử dụng mặc định
        deadline_minutes: None, // Sử dụng mặc định
        target_profit: None,
        stop_loss: None,
        wallet_address: "".to_string(), // Sử dụng wallet mặc định
    };
    
    // Tạo trade tracker mới
    let trade_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().timestamp() as u64;
    let mut tracker = create_trade_tracker(&params, &trade_id, now, TradeStrategy::SingleTrade);
    
    // Thêm metadata về opportunity
    tracker.metadata.insert("opportunity_id".to_string(), opportunity.opportunity_id.clone());
    tracker.metadata.insert("opportunity_source".to_string(), opportunity.source.clone());
    tracker.metadata.insert("opportunity_confidence".to_string(), format!("{}", opportunity.confidence));
    
    // Thêm vào danh sách active trades
    {
        let mut active_trades = executor.active_trades.write().await;
        active_trades.push(tracker.clone());
    }
    
    // Thực thi giao dịch dựa vào trade type
    let result = match params.trade_type {
        TradeType::Buy => execute_buy(executor, &mut tracker).await?,
        TradeType::Sell => execute_sell(executor, &mut tracker).await?,
        _ => {
            warn!("Unsupported trade type for opportunity: {:?}", params.trade_type);
            
            TradeResult {
                trade_id: trade_id.clone(),
                params: params.clone(),
                status: TradeStatus::Failed,
                created_at: now,
                completed_at: Some(Utc::now().timestamp() as u64),
                tx_hash: None,
                actual_amount: None,
                actual_price: None,
                fee: None,
                profit_loss: None,
                error: Some("Unsupported trade type".to_string()),
                explorer_url: None,
            }
        }
    };
    
    // Thêm vào trade history nếu đã hoàn thành hoặc thất bại
    if result.status == TradeStatus::Completed || result.status == TradeStatus::Failed {
        let mut history = executor.trade_history.write().await;
        history.push(result.clone());
        
        // Xóa khỏi active trades
        let mut active_trades = executor.active_trades.write().await;
        active_trades.retain(|t| t.trade_id != trade_id);
    }
    
    Ok(result)
}

/// Mô phỏng giao dịch trước khi thực thi
pub async fn simulate_trade(
    executor: &SmartTradeExecutor,
    params: &TradeParams,
) -> Result<TradeResult> {
    // Lấy adapter cho chain
    let adapter = match executor.evm_adapters.get(&params.chain_id()) {
        Some(adapter) => adapter,
        None => return Err(anyhow!("No adapter found for chain ID {}", params.chain_id())),
    };
    
    // Mô phỏng swap
    let simulation = match params.trade_type {
        TradeType::Buy => {
            adapter.simulate_swap(
                &params.base_token(),
                &params.token_address,
                params.amount,
            ).await
        },
        TradeType::Sell => {
            adapter.simulate_swap(
                &params.token_address,
                &params.base_token(),
                params.amount,
            ).await
        },
        _ => return Err(anyhow!("Unsupported trade type for simulation: {:?}", params.trade_type)),
    };
    
    match simulation {
        Ok(result) => {
            // Tạo kết quả mô phỏng
            let trade_id = uuid::Uuid::new_v4().to_string();
            let now = Utc::now().timestamp() as u64;
            
            Ok(TradeResult {
                trade_id,
                params: params.clone(),
                status: TradeStatus::Initialized,
                created_at: now,
                completed_at: None,
                tx_hash: None,
                actual_amount: Some(result.amount_out),
                actual_price: Some(result.price),
                fee: Some(result.fee),
                profit_loss: None,
                error: None,
                explorer_url: None,
            })
        },
        Err(e) => {
            warn!("Trade simulation failed: {}", e);
            
            // Tạo kết quả lỗi
            let trade_id = uuid::Uuid::new_v4().to_string();
            let now = Utc::now().timestamp() as u64;
            
            Ok(TradeResult {
                trade_id,
                params: params.clone(),
                status: TradeStatus::Failed,
                created_at: now,
                completed_at: Some(now),
                tx_hash: None,
                actual_amount: None,
                actual_price: None,
                fee: None,
                profit_loss: None,
                error: Some(format!("Simulation failed: {}", e)),
                explorer_url: None,
            })
        }
    }
}

/// Thực thi giao dịch dựa vào trade tracker
pub async fn execute_trade_from_tracker(
    executor: &SmartTradeExecutor,
    tracker: &mut TradeTracker,
) -> Result<TradeResult> {
    match tracker.params.trade_type {
        TradeType::Buy => execute_buy(executor, tracker).await,
        TradeType::Sell => execute_sell(executor, tracker).await,
        _ => Err(anyhow!("Unsupported trade type: {:?}", tracker.params.trade_type)),
    }
}

/// Lưu trạng thái giao dịch để khôi phục nếu cần
async fn save_trade_state(executor: &SmartTradeExecutor, tracker: &TradeTracker) -> Result<()> {
    // Lấy nơi lưu trữ giao dịch
    let mut trade_store = executor.trade_store.write().await;
    
    // Lưu hoặc cập nhật giao dịch trong store
    trade_store.insert(tracker.trade_id.clone(), tracker.clone());
    
    // Lưu vào bộ nhớ tạm thời để khôi phục
    if let Ok(serialized) = serde_json::to_string(tracker) {
        // Tạo thư mục nếu chưa tồn tại
        let trades_dir = "data/trades";
        std::fs::create_dir_all(trades_dir).ok();
        
        // Lưu vào file
        let file_path = format!("{}/{}.json", trades_dir, tracker.trade_id);
        std::fs::write(&file_path, serialized).ok();
    }
    
    Ok(())
}

/// Lên lịch thử lại giao dịch sau
async fn schedule_trade_retry(
    executor: &SmartTradeExecutor, 
    tracker: &mut TradeTracker, 
    error_reason: &str
) -> Result<()> {
    // Trích xuất recovery params
    let recovery_params = match tracker.metadata.get("recovery_params")
        .and_then(|json| serde_json::from_str::<RecoveryParams>(json).ok()) {
        Some(params) => params,
        None => RecoveryParams::default(),
    };
    
    // Tăng số lần thử lại
    tracker.retry_count += 1;
    
    // Kiểm tra xem có vượt quá số lần thử lại tối đa không
    if tracker.retry_count > recovery_params.max_retries {
        // Quá số lần thử lại cho phép
        tracker.status = TradeStatus::Failed;
        tracker.metadata.insert("failure_reason".to_string(), 
            format!("Exceeded maximum retry attempts ({}). Last error: {}", 
                    recovery_params.max_retries, error_reason));
        
        save_trade_state(executor, tracker).await?;
        return Ok(());
    }
    
    // Lên lịch thử lại
    tracker.status = TradeStatus::Scheduled;
    tracker.metadata.insert("next_retry_time".to_string(), 
        (Utc::now().timestamp() as u64 + recovery_params.retry_delay_ms / 1000).to_string());
    tracker.metadata.insert("last_error".to_string(), error_reason.to_string());
    
    // Lưu trạng thái
    save_trade_state(executor, tracker).await?;
    
    // Tạo task để thử lại sau một khoảng thời gian
    let trade_id = tracker.trade_id.clone();
    let delay_ms = recovery_params.retry_delay_ms;
    let executor_clone = executor.clone();
    
    tokio::spawn(async move {
        // Đợi khoảng thời gian retry_delay_ms
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        
        // Tải lại trạng thái giao dịch (có thể đã thay đổi)
        let mut trade_store = executor_clone.trade_store.write().await;
        if let Some(mut updated_tracker) = trade_store.get(&trade_id).cloned() {
            // Chỉ thử lại nếu vẫn ở trạng thái Scheduled
            if updated_tracker.status == TradeStatus::Scheduled {
                // Thử lại giao dịch
                debug!("Retrying trade {} (attempt {}/{})", 
                       trade_id, updated_tracker.retry_count, recovery_params.max_retries);
                
                drop(trade_store); // Giải phóng lock trước khi gọi execute
                
                // Thực thi lại giao dịch
                if let Err(e) = execute_trade_from_tracker(&executor_clone, &mut updated_tracker).await {
                    error!("Retry attempt failed for trade {}: {}", trade_id, e);
                }
            }
        }
    });
    
    Ok(())
}

/// Hàm khôi phục giao dịch từ trạng thái đã lưu
pub async fn recover_trades(executor: &SmartTradeExecutor) -> Result<Vec<String>> {
    info!("Recovering trades from saved state...");
    
    let trades_dir = "data/trades";
    if !std::path::Path::new(trades_dir).exists() {
        return Ok(Vec::new());
    }
    
    let mut recovered_trades = Vec::new();
    
    // Đọc các file giao dịch
    if let Ok(entries) = std::fs::read_dir(trades_dir) {
        for entry in entries.flatten() {
            if let Ok(path) = entry.path().canonicalize() {
                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                    // Đọc nội dung file
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        // Deserialize thành TradeTracker
                        if let Ok(mut tracker) = serde_json::from_str::<TradeTracker>(&content) {
                            // Kiểm tra trạng thái giao dịch
                            match tracker.status {
                                TradeStatus::Initialized | TradeStatus::Executing | TradeStatus::Scheduled => {
                                    // Giao dịch chưa hoàn thành, thêm vào store và khôi phục
                                    let trade_id = tracker.trade_id.clone();
                                    recovered_trades.push(trade_id.clone());
                                    
                                    // Thêm vào trade store
                                    let mut trade_store = executor.trade_store.write().await;
                                    trade_store.insert(trade_id, tracker.clone());
                                    drop(trade_store);
                                    
                                    // Lên lịch thử lại
                                    schedule_trade_retry(executor, &mut tracker, "Recovered from saved state").await?;
                                    
                                    info!("Recovered trade {} with status {:?}", tracker.trade_id, tracker.status);
                                },
                                _ => {
                                    // Giao dịch đã hoàn thành hoặc thất bại, chỉ thêm vào store
                                    let trade_id = tracker.trade_id.clone();
                                    let mut trade_store = executor.trade_store.write().await;
                                    trade_store.insert(trade_id, tracker);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    info!("Recovered {} pending trades", recovered_trades.len());
    
    Ok(recovered_trades)
}

/// Handler xử lý giao dịch
pub struct TradeHandler {
    /// Risk manager
    risk_manager: Arc<RiskManager>,
    
    /// Position manager
    position_manager: Arc<PositionManager>,
}

impl TradeHandler {
    /// Tạo trade handler mới
    pub fn new(
        risk_manager: Arc<RiskManager>,
        position_manager: Arc<PositionManager>,
    ) -> Self {
        Self {
            risk_manager,
            position_manager,
        }
    }
    
    /// Thực thi giao dịch mua
    pub async fn execute_buy(
        &self,
        adapter: &Arc<EvmAdapter>,
        params: &TradeParams,
        trade_id: &str,
    ) -> Result<TradeResult> {
        debug!("Thực thi giao dịch mua: chain={}, token={}", params.chain_id(), params.token_address);
        
        // Kiểm tra rủi ro và validation
        self.validate_trade(params).await?;
        
        // Kiểm tra và điều chỉnh kích thước vị thế
        let adjusted_params = self.position_manager.calculate_position_size(params).await
            .with_context(|| "Lỗi khi tính toán kích thước vị thế")?;
        
        // Thực thi và theo dõi giao dịch
        let tx_hash = self.execute_with_retry(adapter, &adjusted_params).await
            .with_context(|| "Lỗi khi thực thi giao dịch mua")?;
        
        // Theo dõi và xây dựng kết quả giao dịch
        let result = self.monitor_and_build_result(adapter, &adjusted_params, trade_id, &tx_hash).await?;
        
        info!("Giao dịch mua hoàn thành: tx_hash={}, status={:?}", tx_hash, result.status);
        Ok(result)
    }
    
    /// Thực thi giao dịch bán
    pub async fn execute_sell(
        &self,
        adapter: &Arc<EvmAdapter>,
        params: &TradeParams,
        trade_id: &str,
    ) -> Result<TradeResult> {
        debug!("Thực thi giao dịch bán: chain={}, token={}", params.chain_id(), params.token_address);
        
        // Kiểm tra rủi ro và validation
        self.validate_trade(params).await?;
        
        // Kiểm tra số dư token trước khi bán
        let balance = adapter.get_token_balance(&params.token_address).await
            .with_context(|| format!("Lỗi khi lấy số dư token: {}", params.token_address))?;
        
        if balance < params.amount {
            return Err(anyhow!("Số dư token không đủ: có {}, cần {}", balance, params.amount));
        }
        
        // Thực thi và theo dõi giao dịch
        let tx_hash = self.execute_with_retry(adapter, params).await
            .with_context(|| "Lỗi khi thực thi giao dịch bán")?;
        
        // Theo dõi và xây dựng kết quả giao dịch
        let result = self.monitor_and_build_result(adapter, params, trade_id, &tx_hash).await?;
        
        info!("Giao dịch bán hoàn thành: tx_hash={}, status={:?}", tx_hash, result.status);
        Ok(result)
    }
    
    /// Xác thực giao dịch qua risk manager
    async fn validate_trade(&self, params: &TradeParams) -> Result<()> {
        // Kiểm tra rủi ro trước khi thực thi
        let risk_check = self.risk_manager.check_trade(params).await
            .with_context(|| "Lỗi khi kiểm tra rủi ro giao dịch")?;
        
        if !risk_check.is_acceptable {
            return Err(anyhow!("Giao dịch không được chấp nhận bởi risk manager: {}", 
                             risk_check.reason.unwrap_or_default()));
        }
        
        Ok(())
    }
    
    /// Thực hiện theo dõi và xây dựng kết quả giao dịch
    async fn monitor_and_build_result(
        &self,
        adapter: &Arc<EvmAdapter>,
        params: &TradeParams,
        trade_id: &str,
        tx_hash: &str,
    ) -> Result<TradeResult> {
        // Theo dõi trạng thái giao dịch
        let tx_status = self.wait_for_transaction(adapter, tx_hash, Duration::from_secs(300)).await
            .with_context(|| format!("Lỗi khi chờ giao dịch hoàn thành: {}", tx_hash))?;
        
        // Xây dựng kết quả giao dịch
        self.build_trade_result(
            trade_id,
            params,
            tx_hash,
            tx_status,
        ).await
    }
    
    /// Thực thi giao dịch với retry logic
    async fn execute_with_retry(
        &self,
        adapter: &Arc<EvmAdapter>,
        params: &TradeParams,
    ) -> Result<String> {
        let max_retries = 3;
        let mut last_error = None;
        
        for retry in 0..max_retries {
            match adapter.execute_transaction(params).await {
                Ok(tx_hash) => return Ok(tx_hash),
                Err(e) => {
                    warn!("Lỗi khi thực thi giao dịch (lần thử {}): {}", retry + 1, e);
                    last_error = Some(e);
                    
                    if retry < max_retries - 1 {
                        let backoff = Duration::from_millis(500 * 2u64.pow(retry as u32));
                        tokio::time::sleep(backoff).await;
                    }
                }
            }
        }
        
        // Sử dụng pattern matching thay vì unwrap_or_else
        match last_error {
            Some(e) => Err(anyhow!("Lỗi sau {} lần thử: {}", max_retries, e)),
            None => Err(anyhow!("Lỗi không xác định khi thực thi giao dịch sau {} lần thử", max_retries))
        }
    }
    
    /// Chờ giao dịch hoàn thành với timeout
    async fn wait_for_transaction(
        &self,
        adapter: &Arc<EvmAdapter>,
        tx_hash: &str,
        wait_time: Duration,
    ) -> Result<TransactionStatus> {
        let wait_future = async {
            loop {
                match adapter.get_transaction_status(tx_hash).await {
                    Ok(status) => {
                        match status {
                            TransactionStatus::Success | TransactionStatus::Failed => {
                                return Ok(status);
                            },
                            TransactionStatus::Pending => {
                                // Vẫn đang chờ, tiếp tục
                                tokio::time::sleep(Duration::from_secs(5)).await;
                            }
                        }
                    },
                    Err(e) => {
                        warn!("Lỗi khi kiểm tra trạng thái giao dịch {}: {}", tx_hash, e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        };
        
        match timeout(wait_time, wait_future).await {
            Ok(result) => result,
            Err(_) => {
                warn!("Timeout khi chờ giao dịch hoàn thành: {}", tx_hash);
                Ok(TransactionStatus::Pending) // Vẫn đang chờ xử lý
            }
        }
    }
    
    /// Xây dựng kết quả giao dịch
    async fn build_trade_result(
        &self,
        trade_id: &str,
        params: &TradeParams,
        tx_hash: &str,
        tx_status: TransactionStatus,
    ) -> Result<TradeResult> {
        let status = match tx_status {
            TransactionStatus::Success => TradeStatus::Completed,
            TransactionStatus::Failed => TradeStatus::Failed,
            TransactionStatus::Pending => TradeStatus::Pending,
        };
        
        let result = TradeResult {
            trade_id: trade_id.to_string(),
            chain_id: params.chain_id(),
            token_address: params.token_address.clone(),
            amount: params.amount,
            trade_type: params.trade_type,
            tx_hash: tx_hash.to_string(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            status,
            gas_used: None, // Cần lấy từ receipt
            gas_price: None, // Cần lấy từ receipt
            value: None, // Cần lấy từ receipt
            error: None,
            additional_data: None,
        };
        
        Ok(result)
    }
} 