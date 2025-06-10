//! Định nghĩa các kiểu dữ liệu chính sử dụng trong module smart_trade

use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use anyhow::Result;
use chrono::Utc;

// Internal Imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::TradeType;

/// Trạng thái của một giao dịch
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeStatus {
    /// Giao dịch đã được tạo nhưng chưa thực thi
    Created,
    /// Đã phê duyệt token (nếu cần)
    Approved,
    /// Đang thực thi giao dịch mua
    Buying,
    /// Đã mua thành công, đang theo dõi
    Bought,
    /// Đang thực thi giao dịch bán
    Selling,
    /// Đã bán thành công
    Sold,
    /// Giao dịch thất bại
    Failed,
    /// Đã hủy bởi người dùng
    Cancelled,
    /// Đang chờ xác nhận
    Pending,
    /// Đã xác nhận
    Confirmed,
}

/// Kết quả của một giao dịch
#[derive(Debug, Clone)]
pub struct TradeResult {
    /// Hash của giao dịch
    pub tx_hash: Option<String>,
    /// Trạng thái thành công/thất bại
    pub success: bool,
    /// Lỗi nếu có
    pub error: Option<String>,
    /// Giá thực thi
    pub execution_price: Option<f64>,
    /// Số lượng token nhận được
    pub token_amount: Option<f64>,
    /// Chi phí gas
    pub gas_cost: Option<f64>,
    /// Thời gian thực thi (ms)
    pub execution_time_ms: u64,
    /// Thông tin bổ sung
    pub additional_info: HashMap<String, String>,
}

/// Thông tin theo dõi giao dịch
#[derive(Debug, Clone)]
pub struct TradeTracker {
    /// ID giao dịch độc nhất
    pub trade_id: String,
    /// Tham số giao dịch
    pub params: crate::types::TradeParams,
    /// Trạng thái hiện tại
    pub status: TradeStatus,
    /// Thời gian tạo
    pub created_at: i64,
    /// Thời gian cập nhật gần nhất
    pub updated_at: i64,
    /// Giá mua vào
    pub buy_price: Option<f64>,
    /// Giá bán ra
    pub sell_price: Option<f64>,
    /// Hash của giao dịch mua
    pub buy_tx_hash: Option<String>,
    /// Hash của giao dịch bán
    pub sell_tx_hash: Option<String>,
    /// Số tiền đầu tư
    pub investment_amount: f64,
    /// Số lượng token đã mua
    pub token_amount: Option<f64>,
    /// Lý do bán (nếu đã bán)
    pub exit_reason: Option<String>,
    /// Giá cao nhất đạt được
    pub highest_price: f64,
    /// Chiến lược giao dịch
    pub strategy: Option<String>,
    /// Giá stop-loss
    pub stop_loss: Option<f64>,
    /// Giá take-profit
    pub take_profit: Option<f64>,
    /// Trailing stop loss (%)
    pub trailing_stop_percentage: Option<f64>,
}

/// Chiến lược giao dịch
#[derive(Debug, Clone)]
pub enum TradeStrategy {
    /// Giao dịch thủ công: Chỉ mua bán theo lệnh trực tiếp của người dùng,
    /// không có chức năng tự động bán.
    Manual,

    /// Giao dịch nhanh: Mua khi phát hiện lệnh lớn trong mempool, bán nhanh khi 
    /// đạt lợi nhuận mục tiêu (mặc định 5%) hoặc sau thời gian ngắn (5 phút).
    /// Thích hợp cho các cơ hội thoáng qua, ít rủi ro.
    Quick,

    /// Giao dịch thông minh: Sử dụng Trailing Stop Loss (TSL), có thời gian giữ 
    /// lâu hơn và chiến lược thoát vị thế tinh vi hơn. Thích hợp cho các token
    /// có tiềm năng tăng trưởng dài hơn.
    Smart,
}

/// Cấu trúc chứa các ngưỡng phân tích rủi ro thị trường
/// 
/// Sử dụng cho phân tích rủi ro động thay vì dùng các giá trị cứng cố định
#[derive(Debug, Clone)]
pub struct MarketRiskThresholds {
    /// Ngưỡng thanh khoản tối thiểu (USD)
    pub min_liquidity_usd: f64,
    
    /// Ngưỡng thanh khoản thấp (USD)
    pub low_liquidity_usd: f64,
    
    /// Ngưỡng thanh khoản trung bình (USD)
    pub medium_liquidity_usd: f64,
    
    /// Điểm phạt cho thanh khoản thấp
    pub low_liquidity_penalty: u8,
    
    /// Tỷ lệ mua/bán thấp
    pub low_buy_sell_ratio: f64,
    
    /// Tỷ lệ mua/bán trung bình
    pub medium_buy_sell_ratio: f64,
    
    /// Tỷ lệ mua/bán cao
    pub high_buy_sell_ratio: f64,
    
    /// Tỷ lệ mua/bán tối đa (dùng khi không có lệnh bán)
    pub max_buy_sell_ratio: f64,
    
    /// Điểm phạt cho tỷ lệ mua/bán cao
    pub high_buy_sell_penalty: u8,
    
    /// Phần trăm thay đổi giá thấp
    pub low_price_change_pct: f64,
    
    /// Phần trăm thay đổi giá trung bình
    pub medium_price_change_pct: f64,
    
    /// Phần trăm thay đổi giá cao
    pub high_price_change_pct: f64,
    
    /// Điểm phạt cho biến động giá cao
    pub high_volatility_penalty: u8,
    
    /// Phần trăm tập trung token thấp
    pub low_concentration_pct: f64,
    
    /// Phần trăm tập trung token trung bình
    pub medium_concentration_pct: f64,
    
    /// Phần trăm tập trung token cao
    pub high_concentration_pct: f64,
    
    /// Điểm phạt cho tập trung token cao
    pub high_concentration_penalty: u8,
}

/// Cấu hình SmartTrade
#[derive(Debug, Clone)]
pub struct SmartTradeConfig {
    /// Bật/tắt module
    pub enabled: bool,
    /// Bật/tắt giao dịch tự động
    pub auto_trade: bool,
    /// Số tiền giao dịch tối thiểu (USD)
    pub min_trade_amount: f64,
    /// Số tiền giao dịch tối đa (USD)
    pub max_trade_amount: f64,
    /// Tỷ lệ vốn cho mỗi giao dịch (%)
    pub capital_per_trade_ratio: f64,
    /// Số lượng giao dịch tối đa cùng lúc
    pub max_concurrent_trades: u32,
    /// Danh sách token được phép giao dịch (whitelist), nếu trống = tất cả token
    pub token_whitelist: HashSet<String>,
    /// Danh sách token cấm giao dịch (blacklist), luôn được áp dụng
    pub token_blacklist: HashSet<String>,
    /// Chiến lược giao dịch mặc định
    pub default_strategy: TradeStrategy,
    /// Danh sách DEX được phép giao dịch
    pub allowed_dexes: HashSet<String>,
    /// Phần trăm risk factor cho position sizing
    pub risk_factor_percent: f64,
    /// Bật position sizing động
    pub dynamic_position_sizing: bool,
    /// Hệ số biến động thị trường theo chain
    pub market_volatility_factor: HashMap<u32, f64>,
    /// Ngưỡng giao dịch tối thiểu theo chain
    pub min_trade_amount_map: HashMap<u32, f64>,
    /// Ngưỡng giao dịch tối đa theo chain
    pub max_trade_amount_map: HashMap<u32, f64>,
    /// Ngưỡng rủi ro thị trường mặc định
    pub default_market_risk_thresholds: MarketRiskThresholds,
    /// Ngưỡng rủi ro thị trường theo chain
    pub market_risk_thresholds: HashMap<u32, MarketRiskThresholds>,
    /// Hệ số điều chỉnh rủi ro thị trường theo chain
    pub market_risk_factor: HashMap<u32, f64>,
    /// Tần suất kiểm tra giá (giây)
    pub price_check_interval_seconds: u64,
    /// Khoảng thời gian giữa các lần cập nhật vị thế (giây)
    pub position_update_interval_seconds: u64,
    /// Khoảng thời gian giữa các lần kiểm tra cơ hội giao dịch (giây)
    pub opportunity_check_interval_seconds: u64,
    /// Mức độ tin cậy tối thiểu cho độ ưu tiên thấp
    pub min_confidence_for_low_priority: u8,
    /// Mức độ tin cậy tối thiểu cho độ ưu tiên trung bình
    pub min_confidence_for_medium_priority: u8,
    /// Số lần thử lại tối đa khi gặp lỗi mạng
    pub max_retries: u8,
    /// Thời gian chờ ban đầu giữa các lần thử lại (ms)
    pub retry_delay_ms: u64,
    /// Thời gian chờ tối đa giữa các lần thử lại (ms)
    pub max_retry_delay_ms: u64,
    /// Tần suất kiểm tra health check cho các kết nối blockchain (giây)
    pub health_check_interval_seconds: u64,
    /// Timeout cho các cuộc gọi blockchain (ms)
    pub blockchain_call_timeout_ms: u64,
    /// Thời gian giữa các lần tự động kết nối lại khi mất kết nối (giây)
    pub reconnect_interval_seconds: u64,
    /// Thời gian chờ tối đa trước khi hủy giao dịch do lỗi mạng (giây)
    pub network_failure_trade_timeout_seconds: u64,
    /// Danh sách token theo dõi theo chain
    pub watched_tokens: HashMap<u32, HashSet<String>>,
    /// Slippage mặc định (%)
    pub default_slippage: f64,
    /// Deadline mặc định (phút)
    pub default_deadline_minutes: u64,
    /// Giới hạn gas mặc định
    pub default_gas_limit: u64,
    /// Chiến lược trailing stop loss mặc định (%)
    pub default_trailing_stop_percentage: f64,
    /// Chiến lược take profit mặc định (%)
    pub default_take_profit_percentage: f64,
    /// Chiến lược stop loss mặc định (%)
    pub default_stop_loss_percentage: f64,
    /// Tỷ lệ đầu tư trên vốn khả dụng (0.0-1.0), ví dụ: 0.1 = 10% vốn
    pub capital_per_trade: f64,
    /// Điểm rủi ro tối đa cho phép (0-100), token có điểm rủi ro cao hơn sẽ bị bỏ qua
    pub max_risk_score: f64,
    /// Khoảng thời gian kiểm tra giá (ms)
    pub price_check_interval_ms: u64,
    /// Khoảng thời gian kiểm tra sức khỏe mạng (ms)
    pub health_check_interval_ms: u64,
    /// Khoảng thời gian kiểm tra thị trường (ms)
    pub market_monitor_interval_ms: u64,
    /// Chiến lược retry
    pub retry_strategy: RetryStrategy,
    /// Cài đặt cảnh báo
    pub alert_settings: AlertSettings,
    /// Danh sách token chính theo chain
    pub default_base_token: HashMap<u32, String>,
    /// Chặn token chưa được xác minh hợp đồng
    pub block_unverified_contracts: bool,
    /// Chặn token có rủi ro cao
    pub block_high_risk_tokens: bool,
    /// Chặn token có rủi ro trung bình
    pub block_medium_risk_tokens: bool,
    /// Mức độ tin cậy tối thiểu cho độ ưu tiên cao
    pub min_confidence_for_high_priority: f64,
    /// Lợi nhuận tối thiểu để kích hoạt trailing stop
    pub min_profit_for_trailing_stop: f64,
}

impl Default for SmartTradeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_trade: false,
            min_trade_amount: 0.01,
            max_trade_amount: 0.1,
            capital_per_trade_ratio: 0.05,
            max_concurrent_trades: 5,
            token_whitelist: HashSet::new(),
            token_blacklist: HashSet::new(),
            default_strategy: TradeStrategy::Smart,
            max_risk_score: 70.0,
            allowed_dexes: ["uniswap", "pancakeswap", "sushiswap", "spookyswap"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            risk_factor_percent: 2.0,
            dynamic_position_sizing: true,
            market_volatility_factor: HashMap::new(),
            min_trade_amount_map: HashMap::new(),
            max_trade_amount_map: HashMap::new(),
            default_market_risk_thresholds: MarketRiskThresholds::default(),
            market_risk_thresholds: HashMap::new(),
            market_risk_factor: HashMap::new(),
            price_check_interval_seconds: 5,
            position_update_interval_seconds: 60,
            opportunity_check_interval_seconds: 300,
            min_confidence_for_low_priority: 50,
            min_confidence_for_medium_priority: 70,
            max_retries: 3,
            retry_delay_ms: 500,
            max_retry_delay_ms: 10000,
            health_check_interval_seconds: 60,
            blockchain_call_timeout_ms: 15000,
            reconnect_interval_seconds: 30,
            network_failure_trade_timeout_seconds: 300,
            watched_tokens: HashMap::new(),
            default_slippage: 0.005,
            default_deadline_minutes: 5,
            default_gas_limit: 1000000,
            default_trailing_stop_percentage: 0.05,
            default_take_profit_percentage: 0.1,
            default_stop_loss_percentage: 0.05,
            capital_per_trade: 0.1,
            price_check_interval_ms: 5000,
            health_check_interval_ms: 10000,
            market_monitor_interval_ms: 300000,
            retry_strategy: RetryStrategy::default(),
            alert_settings: AlertSettings::default(),
            default_base_token: HashMap::new(),
            block_unverified_contracts: true,
            block_high_risk_tokens: true,
            block_medium_risk_tokens: true,
            min_confidence_for_high_priority: 0.9,
            min_profit_for_trailing_stop: 0.05,
        }
    }
}

// Re-export types from common module that are used frequently here for convenience
pub use crate::tradelogic::common::types::TraderBehaviorType;
pub use crate::tradelogic::common::types::TraderExpertiseLevel;

/// Quản lý nonce cho các giao dịch blockchain
///
/// Đảm bảo các giao dịch liên tiếp sử dụng nonce hợp lệ và không bị trùng lặp
/// hoặc gây ra nonce gap.
#[derive(Debug, Clone)]
pub struct NonceManager {
    /// Mapping từ (chain_id, wallet_address) đến nonce hiện tại
    pub current_nonces: HashMap<(u32, String), u64>,
    
    /// Mapping từ (chain_id, wallet_address) đến thời gian cập nhật nonce cuối cùng
    pub last_update_time: HashMap<(u32, String), u64>,
    
    /// Thời gian hết hạn cache nonce (giây)
    pub cache_expiry_seconds: u64,
    
    /// Mapping từ (chain_id, wallet_address) đến các nonce đang chờ xử lý (pending)
    pub pending_nonces: HashMap<(u32, String), HashSet<u64>>,
    
    /// Mapping từ (chain_id, wallet_address, nonce) đến tx_hash
    pub nonce_to_tx_hash: HashMap<(u32, String, u64), String>,
}

impl NonceManager {
    /// Tạo NonceManager mới với cấu hình mặc định
    pub fn new() -> Self {
        Self {
            current_nonces: HashMap::new(),
            last_update_time: HashMap::new(),
            cache_expiry_seconds: 60, // Cache nonce trong 60 giây
            pending_nonces: HashMap::new(),
            nonce_to_tx_hash: HashMap::new(),
        }
    }
    
    /// Lấy nonce tiếp theo cho transaction. Sẽ tự động đồng bộ nếu cache đã hết hạn.
    pub async fn get_next_nonce(&mut self, chain_id: u32, wallet_address: &str, adapter: &Arc<EvmAdapter>) -> anyhow::Result<u64> {
        let key = (chain_id, wallet_address.to_string());
        let now = Utc::now().timestamp() as u64;
        
        // Kiểm tra cache
        if let Some(last_update) = self.last_update_time.get(&key) {
            if now - last_update < self.cache_expiry_seconds {
                if let Some(current_nonce) = self.current_nonces.get(&key) {
                    // Tìm nonce tiếp theo chưa sử dụng
                    let mut next_nonce = *current_nonce;
                    let pending = self.pending_nonces.entry(key.clone()).or_insert_with(HashSet::new);
                    
                    // Tìm nonce chưa được sử dụng
                    while pending.contains(&next_nonce) {
                        next_nonce += 1;
                    }
                    
                    // Đánh dấu nonce này là đang sử dụng
                    pending.insert(next_nonce);
                    
                    debug!("Using cached nonce {} for wallet {} on chain {}", next_nonce, wallet_address, chain_id);
                    return Ok(next_nonce);
                }
            }
        }
        
        // Nếu cache không có hoặc đã hết hạn, lấy nonce từ blockchain
        match adapter.get_next_nonce(wallet_address).await {
            Ok(nonce) => {
                debug!("Got fresh nonce {} from blockchain for wallet {} on chain {}", nonce, wallet_address, chain_id);
                
                // Cập nhật cache
                self.current_nonces.insert(key.clone(), nonce);
                self.last_update_time.insert(key.clone(), now);
                
                // Đánh dấu nonce này là đang sử dụng
                let pending = self.pending_nonces.entry(key.clone()).or_insert_with(HashSet::new);
                pending.insert(nonce);
                
                Ok(nonce)
            },
            Err(e) => {
                error!("Failed to get nonce for wallet {} on chain {}: {}", wallet_address, chain_id, e);
                Err(anyhow!("Failed to get nonce: {}", e))
            }
        }
    }
    
    /// Đánh dấu transaction đã được gửi với nonce tương ứng
    pub fn mark_transaction_sent(&mut self, chain_id: u32, wallet_address: &str, nonce: u64, tx_hash: &str) {
        let key = (chain_id, wallet_address.to_string());
        
        // Lưu tx_hash cho nonce này
        self.nonce_to_tx_hash.insert((chain_id, wallet_address.to_string(), nonce), tx_hash.to_string());
        
        // Cập nhật nonce hiện tại nếu nonce mới lớn hơn
        if let Some(current_nonce) = self.current_nonces.get(&key) {
            if nonce >= *current_nonce {
                self.current_nonces.insert(key.clone(), nonce + 1);
            }
        } else {
            self.current_nonces.insert(key.clone(), nonce + 1);
        }
        
        // Cập nhật thời gian
        self.last_update_time.insert(key.clone(), Utc::now().timestamp() as u64);
    }
    
    /// Đánh dấu transaction đã hoàn tất với nonce tương ứng
    pub fn mark_transaction_completed(&mut self, chain_id: u32, wallet_address: &str, nonce: u64) {
        let key = (chain_id, wallet_address.to_string());
        
        // Xóa khỏi danh sách pending
        if let Some(pending) = self.pending_nonces.get_mut(&key) {
            pending.remove(&nonce);
        }
    }
    
    /// Đồng bộ nonce với blockchain
    pub async fn sync_nonce(&mut self, chain_id: u32, wallet_address: &str, adapter: &Arc<EvmAdapter>) -> anyhow::Result<u64> {
        let key = (chain_id, wallet_address.to_string());
        let nonce = adapter.get_next_nonce(wallet_address).await?;
        self.current_nonces.insert(key.clone(), nonce);
        self.last_update_time.insert(key, Utc::now().timestamp() as u64);
        Ok(nonce)
    }
    
    /// Kiểm tra xem transaction với nonce đã được xác nhận chưa
    pub async fn check_transaction_confirmed(&self, chain_id: u32, wallet_address: &str, nonce: u64, adapter: &Arc<EvmAdapter>) -> anyhow::Result<bool> {
        let key = (chain_id, wallet_address.to_string(), nonce);
        
        if let Some(tx_hash) = self.nonce_to_tx_hash.get(&key) {
            let confirmed = is_transaction_confirmed(tx_hash, adapter).await?;
            Ok(confirmed)
        } else {
            // Nếu không có tx_hash, kiểm tra bằng cách so sánh nonce hiện tại
            let current_nonce = adapter.get_next_nonce(wallet_address).await?;
            Ok(current_nonce > nonce) // Nếu nonce hiện tại > nonce cũ, giao dịch đã được xác nhận
        }
    }
    
    /// Xử lý transaction bị "mắc kẹt" (stuck)
    pub async fn handle_stuck_transaction(&mut self, chain_id: u32, wallet_address: &str, nonce: u64, adapter: &Arc<EvmAdapter>) -> anyhow::Result<Option<String>> {
        // Kiểm tra xem giao dịch có trong danh sách pending không
        let key = (chain_id, wallet_address.to_string());
        let is_pending = self.pending_nonces.get(&key).map_or(false, |set| set.contains(&nonce));
        
        if !is_pending {
            return Ok(None); // Không phải giao dịch đang chờ xử lý
        }
        
        // Lấy tx_hash gốc
        let original_tx_hash = match self.nonce_to_tx_hash.get(&(chain_id, wallet_address.to_string(), nonce)) {
            Some(hash) => hash.clone(),
            None => return Err(anyhow!("No transaction hash found for nonce {}", nonce)),
        };
        
        // Kiểm tra trạng thái giao dịch
        let status = adapter.get_transaction_status(&original_tx_hash).await?;
        
        // Nếu đã xác nhận, không cần thay thế
        if matches!(status, TransactionStatus::Confirmed(_)) {
            self.mark_transaction_completed(chain_id, wallet_address, nonce);
            return Ok(None);
        }
        
        // Nếu đã chờ quá lâu, thử thay thế bằng giao dịch mới với gas price cao hơn
        if let TransactionStatus::Pending(pending_info) = &status {
            if pending_info.elapsed_seconds > 300 { // > 5 phút
                debug!("Transaction {} with nonce {} stuck for {} seconds, attempting replace-by-fee", 
                    original_tx_hash, nonce, pending_info.elapsed_seconds);
                
                // Lấy thông tin giao dịch gốc
                let tx_data = adapter.get_transaction_data(&original_tx_hash).await?;
                
                // Tính gas price mới cao hơn 20%
                let new_gas_price = (tx_data.gas_price as f64 * 1.2) as u64;
                
                // Gửi giao dịch thay thế
                let new_tx_hash = adapter.replace_transaction(
                    wallet_address,
                    nonce,
                    &tx_data.to,
                    tx_data.value,
                    &tx_data.data,
                    new_gas_price,
                ).await?;
                
                // Cập nhật tx_hash mới
                self.nonce_to_tx_hash.insert((chain_id, wallet_address.to_string(), nonce), new_tx_hash.clone());
                
                info!("Replaced stuck transaction {} with new transaction {} (nonce: {}, higher gas price: {})", 
                    original_tx_hash, new_tx_hash, nonce, new_gas_price);
                
                return Ok(Some(new_tx_hash));
            }
        }
        
        // Giao dịch vẫn đang chờ nhưng chưa cần thay thế
        Ok(None)
    }
    
    /// Kiểm tra và cập nhật trạng thái của tất cả các pending transaction
    pub async fn check_pending_transactions(&mut self, adapter: &Arc<EvmAdapter>, chain_id: u32, wallet_address: &str) -> anyhow::Result<()> {
        let key = (chain_id, wallet_address.to_string());
        
        // Lấy danh sách các nonce đang chờ xử lý
        let pending_nonces = match self.pending_nonces.get(&key) {
            Some(nonces) => nonces.clone(),
            None => return Ok(()),
        };
        
        for nonce in pending_nonces {
            // Kiểm tra xem giao dịch đã được xác nhận chưa
            match self.check_transaction_confirmed(chain_id, wallet_address, nonce, adapter).await {
                Ok(confirmed) => {
                    if confirmed {
                        // Nếu đã xác nhận, đánh dấu hoàn thành
                        self.mark_transaction_completed(chain_id, wallet_address, nonce);
                        debug!("Transaction with nonce {} for wallet {} on chain {} confirmed", nonce, wallet_address, chain_id);
                    } else {
                        // Nếu chưa xác nhận, thử xử lý giao dịch bị mắc kẹt
                        if let Ok(Some(new_tx_hash)) = self.handle_stuck_transaction(chain_id, wallet_address, nonce, adapter).await {
                            info!("Replaced stuck transaction with nonce {} with new tx: {}", nonce, new_tx_hash);
                        }
                    }
                },
                Err(e) => {
                    warn!("Failed to check transaction status for nonce {}: {}", nonce, e);
                }
            }
        }
        
        Ok(())
    }
}

/// Kiểm tra trạng thái giao dịch
pub async fn check_transaction_status(
    tx_hash: &str,
    adapter: &Arc<EvmAdapter>
) -> Result<TransactionStatus, anyhow::Error> {
    // Lấy trạng thái giao dịch từ blockchain
    let status = adapter.get_transaction_status(tx_hash).await?;
    
    // Ghi log trạng thái
    match &status {
        TransactionStatus::Pending(_) => {
            info!("Transaction {} is still pending", tx_hash);
        },
        TransactionStatus::Confirmed(_) => {
            info!("Transaction {} has been confirmed", tx_hash);
        },
        TransactionStatus::Failed(reason) => {
            warn!("Transaction {} failed: {}", tx_hash, reason);
        }
    }
    
    Ok(status)
}

/// Kiểm tra xem giao dịch đã được xác nhận chưa
pub async fn is_transaction_confirmed(
    tx_hash: &str,
    adapter: &Arc<EvmAdapter>
) -> Result<bool, anyhow::Error> {
    let status = check_transaction_status(tx_hash, adapter).await?;
    Ok(status == TransactionStatus::Confirmed)
}

/// Kiểm tra xem giao dịch đã thất bại chưa
pub async fn is_transaction_failed(
    tx_hash: &str, 
    adapter: &Arc<EvmAdapter>,
    timeout_seconds: u64
) -> Result<bool, anyhow::Error> {
    let status = check_transaction_status(tx_hash, adapter).await?;
    
    match status {
        TransactionStatus::Failed => Ok(true),
        TransactionStatus::Confirmed => Ok(false),
        TransactionStatus::Pending => {
            // Kiểm tra nếu giao dịch đã chờ quá lâu
            if let TransactionStatus::Pending(pending_info) = status {
                if pending_info.waiting_seconds > timeout_seconds {
                    return Ok(true); // Coi như thất bại nếu chờ quá lâu
                }
            }
            
            Ok(false)
        }
    }
}
