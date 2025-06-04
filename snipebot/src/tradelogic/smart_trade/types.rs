//! Type definitions for Smart Trading System
//!
//! This module contains all structs, enums, and type aliases needed for the smart trading functionality.
//! These types are essential for the operation of the trade logic and are used throughout the module.

// External imports
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use tokio::sync::RwLock;

// Internal imports
use crate::types::{ChainType, TokenPair, TradeParams, TradeType};
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::MempoolAnalyzer;
use crate::analys::risk_analyzer::{
    RiskAnalyzer, 
    RiskFactor, 
    TradeRecommendation, 
    TradeRiskAnalysis
};
use crate::analys::token_status::{
    AdvancedTokenAnalysis, 
    ContractInfo, 
    LiquidityEvent, 
    LiquidityEventType, 
    TokenSafety, 
    TokenStatus
};

// Import from common module
use crate::tradelogic::common::types::{TokenIssue, TraderBehaviorAnalysis, GasBehavior, TradeStatus};

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

impl Default for MarketRiskThresholds {
    fn default() -> Self {
        Self {
            min_liquidity_usd: 5000.0,        // Tối thiểu $5K
            low_liquidity_usd: 25000.0,       // Thấp: $25K
            medium_liquidity_usd: 100000.0,   // Trung bình: $100K
            low_liquidity_penalty: 30,        // 30 điểm phạt cho thanh khoản thấp
            
            low_buy_sell_ratio: 2.0,          // Tỷ lệ mua/bán thấp: 2:1
            medium_buy_sell_ratio: 3.0,       // Tỷ lệ mua/bán trung bình: 3:1
            high_buy_sell_ratio: 5.0,         // Tỷ lệ mua/bán cao: 5:1
            max_buy_sell_ratio: 10.0,         // Tỷ lệ tối đa khi không có lệnh bán
            high_buy_sell_penalty: 25,        // 25 điểm phạt cho tỷ lệ mua/bán cao
            
            low_price_change_pct: 15.0,       // Thay đổi giá thấp: 15%
            medium_price_change_pct: 30.0,    // Thay đổi giá trung bình: 30%
            high_price_change_pct: 50.0,      // Thay đổi giá cao: 50%
            high_volatility_penalty: 30,      // 30 điểm phạt cho biến động cao
            
            low_concentration_pct: 40.0,      // Tập trung thấp: top holders nắm 40%
            medium_concentration_pct: 60.0,   // Tập trung trung bình: top holders nắm 60%
            high_concentration_pct: 80.0,     // Tập trung cao: top holders nắm 80%
            high_concentration_penalty: 15,   // 15 điểm phạt cho tập trung cao
        }
    }
}

/// Loại chiến lược giao dịch
///
/// Định nghĩa các loại chiến lược giao dịch khác nhau mà SmartTradeExecutor có thể sử dụng.
/// Mỗi chiến lược có các tham số và hành vi riêng.
#[derive(Debug, Clone, PartialEq)]
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

/// Kết quả của một giao dịch đã hoàn thành
/// 
/// Chứa thông tin về kết quả của một giao dịch, bao gồm giá mua/bán, lợi nhuận,
/// thời gian, và lý do kết thúc giao dịch. Dùng để lưu lịch sử và phân tích hiệu suất.
#[derive(Debug, Clone)]
pub struct TradeResult {
    /// ID giao dịch duy nhất
    pub trade_id: String,
    
    /// Giá mua vào (theo đơn vị base token: ETH/BNB)
    pub entry_price: f64,
    
    /// Giá bán ra (theo đơn vị base token: ETH/BNB), None nếu chưa bán
    pub exit_price: Option<f64>,
    
    /// Lợi nhuận theo phần trăm, None nếu chưa bán
    pub profit_percent: Option<f64>,
    
    /// Lợi nhuận quy đổi ra USD, None nếu chưa bán
    pub profit_usd: Option<f64>,
    
    /// Thời gian mua (Unix timestamp, giây)
    pub entry_time: u64,
    
    /// Thời gian bán (Unix timestamp, giây), None nếu chưa bán
    pub exit_time: Option<u64>,
    
    /// Trạng thái hiện tại của giao dịch
    pub status: TradeStatus,
    
    /// Lý do bán/hủy giao dịch, None nếu chưa kết thúc
    pub exit_reason: Option<String>,
    
    /// Tổng chi phí gas đã sử dụng (USD)
    pub gas_cost_usd: f64,
}

/// Thông tin theo dõi giao dịch đang diễn ra
/// 
/// Đại diện cho một giao dịch đang được theo dõi và quản lý bởi SmartTradeExecutor.
/// Chứa tất cả thông tin cần thiết để thực hiện các chiến lược tự động như 
/// trailing stop loss, take profit, và stop loss.
#[derive(Debug, Clone)]
pub struct TradeTracker {
    /// ID giao dịch duy nhất, được tạo khi giao dịch bắt đầu
    pub trade_id: String,
    
    /// Địa chỉ contract của token (0x...)
    pub token_address: String,
    
    /// Chain ID của blockchain (ví dụ: 1 = Ethereum, 56 = BSC)
    pub chain_id: u32,
    
    /// Chiến lược được áp dụng cho giao dịch này
    pub strategy: TradeStrategy,
    
    /// Giá mua vào ban đầu (theo đơn vị base token: ETH/BNB)
    pub entry_price: f64,
    
    /// Số lượng token đã mua (số token thực tế)
    pub token_amount: f64,
    
    /// Số lượng base token (ETH/BNB) đã đầu tư
    pub invested_amount: f64,
    
    /// Giá cao nhất token đã đạt được từ khi mua (cho trailing stop loss)
    pub highest_price: f64,
    
    /// Thời điểm mua (Unix timestamp, giây)
    pub entry_time: u64,
    
    /// Thời điểm tối đa được phép giữ token (Unix timestamp, giây)
    pub max_hold_time: u64,
    
    /// Ngưỡng lợi nhuận mục tiêu để bán (phần trăm, ví dụ: 5.0 = 5%)
    pub take_profit_percent: f64,
    
    /// Ngưỡng lỗ tối đa chấp nhận được (phần trăm, ví dụ: 5.0 = 5%)
    pub stop_loss_percent: f64,
    
    /// Phần trăm trailing stop loss nếu có (ví dụ: 2.0 = 2%)
    pub trailing_stop_percent: Option<f64>,
    
    /// Hash giao dịch mua trên blockchain
    pub buy_tx_hash: String,
    
    /// Hash giao dịch bán trên blockchain (None nếu chưa bán)
    pub sell_tx_hash: Option<String>,
    
    /// Trạng thái hiện tại của giao dịch
    pub status: TradeStatus,
    
    /// Lý do bán/hủy nếu giao dịch đã kết thúc (None nếu chưa kết thúc)
    pub exit_reason: Option<String>,
}

/// Cấu hình cho SmartTradeExecutor
///
/// Chứa các tham số cấu hình cho module smart trade, bao gồm:
/// - Các ngưỡng giao dịch (tối thiểu, tối đa)
/// - Tỉ lệ đầu tư
/// - Chiến lược mặc định
/// - Whitelist/blacklist token
/// - v.v.
#[derive(Debug, Clone)]
pub struct SmartTradeConfig {
    /// Có bật tính năng Smart Trade hay không (true = bật)
    pub enabled: bool,
    
    /// Cho phép tự động thực hiện giao dịch (true = tự động, false = chỉ phân tích)
    pub auto_trade: bool,
    
    /// Ngưỡng tối thiểu cho mỗi giao dịch (BNB/ETH), dưới mức này sẽ không giao dịch
    pub min_trade_amount: f64,
    
    /// Ngưỡng tối đa cho mỗi giao dịch (BNB/ETH), tránh rủi ro quá lớn
    pub max_trade_amount: f64,
    
    /// Tỉ lệ đầu tư trên vốn khả dụng (0.0-1.0), ví dụ: 0.1 = 10% vốn
    pub capital_per_trade_ratio: f64,
    
    /// Số lượng giao dịch tối đa được phép thực hiện cùng lúc
    pub max_concurrent_trades: u32,
    
    /// Danh sách token được phép giao dịch (whitelist), nếu trống = tất cả token
    pub token_whitelist: HashSet<String>,
    
    /// Danh sách token cấm giao dịch (blacklist), luôn được áp dụng
    pub token_blacklist: HashSet<String>,
    
    /// Chiến lược giao dịch mặc định sẽ áp dụng
    pub default_strategy: TradeStrategy,
    
    /// Điểm rủi ro tối đa cho phép (0-100), token có điểm rủi ro cao hơn sẽ bị bỏ qua
    pub max_risk_score: f64,
    
    /// Danh sách DEX được phép giao dịch (uniswap, pancakeswap, ...)
    pub allowed_dexes: HashSet<String>,
    
    /// Phần trăm risk factor cho position sizing (thay vì hằng số 2%)
    /// Giá trị từ 0.5 đến 5.0 (tương ứng 0.5% đến 5%)
    pub risk_factor_percent: f64,
    
    /// Bật tính năng position sizing động dựa trên biến động thị trường
    pub dynamic_position_sizing: bool,
    
    /// Hệ số biến động thị trường cho mỗi chain (<1.0 = ít biến động, >1.0 = nhiều biến động)
    pub market_volatility_factor: HashMap<u32, f64>,
    
    /// Mapping từ chain_id đến ngưỡng tối thiểu cho mỗi giao dịch
    pub min_trade_amount_map: HashMap<u32, f64>,
    
    /// Mapping từ chain_id đến ngưỡng tối đa cho mỗi giao dịch
    pub max_trade_amount_map: HashMap<u32, f64>,
    
    /// Ngưỡng rủi ro thị trường mặc định
    pub default_market_risk_thresholds: MarketRiskThresholds,
    
    /// Mapping từ chain_id đến các ngưỡng rủi ro thị trường tùy chỉnh
    pub market_risk_thresholds: HashMap<u32, MarketRiskThresholds>,
    
    /// Hệ số điều chỉnh rủi ro thị trường cho mỗi chain (>1.0 = tăng điểm rủi ro, <1.0 = giảm điểm rủi ro)
    pub market_risk_factor: HashMap<u32, f64>,
    
    /// Có bật auto_trade cho mỗi chain_id
    pub auto_trade_enabled: bool,
    
    /// Tần suất kiểm tra giá (giây)
    pub price_check_interval_seconds: u64,
    
    /// Mức độ tin cậy tối thiểu để chấp nhận cơ hội ưu tiên thấp
    pub min_confidence_for_low_priority: u8,
    
    /// Mức độ tin cậy tối thiểu để chấp nhận cơ hội ưu tiên trung bình
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
            auto_trade_enabled: false,
            price_check_interval_seconds: 5,
            min_confidence_for_low_priority: 50,
            min_confidence_for_medium_priority: 70,
            max_retries: 3,
            retry_delay_ms: 500,
            max_retry_delay_ms: 10000,
            health_check_interval_seconds: 60,
            blockchain_call_timeout_ms: 15000,
            reconnect_interval_seconds: 30,
            network_failure_trade_timeout_seconds: 300,
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
    
    /// Lấy nonce tiếp theo cho wallet và chain cụ thể
    /// 
    /// Nếu nonce đã được cache và chưa hết hạn, sử dụng giá trị cache.
    /// Nếu không, truy vấn blockchain để lấy nonce hiện tại.
    pub async fn get_next_nonce(&mut self, chain_id: u32, wallet_address: &str, adapter: &Arc<EvmAdapter>) -> Result<u64> {
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
    
    /// Đánh dấu giao dịch với nonce cụ thể đã được gửi
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
    
    /// Đánh dấu giao dịch đã hoàn thành hoặc thất bại
    pub fn mark_transaction_completed(&mut self, chain_id: u32, wallet_address: &str, nonce: u64) {
        let key = (chain_id, wallet_address.to_string());
        
        // Xóa khỏi danh sách pending
        if let Some(pending) = self.pending_nonces.get_mut(&key) {
            pending.remove(&nonce);
        }
    }
    
    /// Đồng bộ lại nonce từ blockchain (dùng khi phát hiện sự không nhất quán)
    pub async fn sync_nonce(&mut self, chain_id: u32, wallet_address: &str, adapter: &Arc<EvmAdapter>) -> Result<u64> {
        let key = (chain_id, wallet_address.to_string());
        let nonce = adapter.get_next_nonce(wallet_address).await?;
        
        debug!("Syncing nonce for wallet {} on chain {}: new nonce = {}", wallet_address, chain_id, nonce);
        
        // Cập nhật cache
        self.current_nonces.insert(key.clone(), nonce);
        self.last_update_time.insert(key.clone(), Utc::now().timestamp() as u64);
        
        Ok(nonce)
    }
    
    /// Kiểm tra xem giao dịch có nonce cụ thể đã được xác nhận chưa
    pub async fn check_transaction_confirmed(&self, chain_id: u32, wallet_address: &str, nonce: u64, adapter: &Arc<EvmAdapter>) -> Result<bool> {
        // Lấy tx_hash cho nonce này
        if let Some(tx_hash) = self.nonce_to_tx_hash.get(&(chain_id, wallet_address.to_string(), nonce)) {
            match adapter.get_transaction_status(tx_hash).await {
                Ok(status) => {
                    return Ok(status.confirmed);
                },
                Err(e) => {
                    warn!("Failed to check transaction status for tx {}: {}", tx_hash, e);
                    return Err(anyhow!("Failed to check transaction status: {}", e));
                }
            }
        }
        
        // Nếu không tìm thấy tx_hash, kiểm tra trực tiếp từ blockchain
        match adapter.is_nonce_used(wallet_address, nonce).await {
            Ok(used) => Ok(used),
            Err(e) => {
                warn!("Failed to check if nonce {} is used for wallet {} on chain {}: {}", nonce, wallet_address, chain_id, e);
                Err(anyhow!("Failed to check nonce: {}", e))
            }
        }
    }
    
    /// Xử lý các giao dịch bị mắc kẹt bằng cách thay thế (replace-by-fee)
    pub async fn handle_stuck_transaction(&mut self, chain_id: u32, wallet_address: &str, nonce: u64, adapter: &Arc<EvmAdapter>) -> Result<Option<String>> {
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
        if status.confirmed {
            self.mark_transaction_completed(chain_id, wallet_address, nonce);
            return Ok(None);
        }
        
        // Nếu đã chờ quá lâu, thử thay thế bằng giao dịch mới với gas price cao hơn
        if status.pending_seconds > 300 { // > 5 phút
            debug!("Transaction {} with nonce {} stuck for {} seconds, attempting replace-by-fee", 
                original_tx_hash, nonce, status.pending_seconds);
            
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
        
        // Giao dịch vẫn đang chờ nhưng chưa cần thay thế
        Ok(None)
    }
    
    /// Kiểm tra và xử lý tất cả các giao dịch đang chờ xử lý
    pub async fn check_pending_transactions(&mut self, adapter: &Arc<EvmAdapter>, chain_id: u32, wallet_address: &str) -> Result<()> {
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
