//! Định nghĩa các kiểu dữ liệu cho phân tích mempool

use std::cmp::Ordering;
use std::collections::HashSet;
use std::time::SystemTime;
use std::collections::HashMap;

// === Các hằng số và ngưỡng phân tích ===

/// Giao dịch lớn - Threshold cho giao dịch với giá trị lớn
pub const LARGE_TRANSACTION_ETH: f64 = 5.0; // >5 ETH là giao dịch lớn
/// Giao dịch rất lớn - Threshold cho giao dịch với giá trị rất lớn
pub const VERY_LARGE_TRANSACTION_ETH: f64 = 20.0; // >20 ETH là giao dịch rất lớn
/// Giao dịch whale - Threshold cho giao dịch với giá trị cực lớn
pub const WHALE_TRANSACTION_ETH: f64 = 50.0; // >50 ETH là giao dịch whale

/// Thời gian tối đa theo dõi giao dịch pending (giây)
pub const MAX_PENDING_TX_AGE_SEC: u64 = 600; // Theo dõi giao dịch trong 10 phút
/// Tần suất làm mới mempool (mili giây)
pub const MEMPOOL_REFRESH_INTERVAL_MS: u64 = 3000; // Làm mới mempool 3 giây/lần
/// Tần suất dọn dẹp các giao dịch cũ (giây)
pub const TRANSACTION_CLEANUP_INTERVAL_SEC: u64 = 30; // Dọn dẹp giao dịch cũ 30 giây/lần

/// Ngưỡng độ tin cậy tối thiểu để coi một mẫu là đáng ngờ
pub const MIN_PATTERN_CONFIDENCE: f64 = 0.6; // 60% độ tin cậy nhận dạng mẫu
/// Cửa sổ thời gian để phát hiện front-running (mili giây)
pub const FRONT_RUNNING_TIME_WINDOW_MS: u64 = 500; // Cửa sổ thời gian 500ms để phát hiện front-running
/// Cửa sổ thời gian tối đa để phát hiện sandwich attack (mili giây)
pub const MAX_SANDWICH_TIME_WINDOW_MS: u64 = 1000; // Cửa sổ thời gian 1000ms để phát hiện sandwich attack

// === Các kiểu dữ liệu phân tích mempool ===

/// Thông tin giao dịch trong mempool
#[derive(Debug, Clone)]
pub struct MempoolTransaction {
    /// Hash giao dịch
    pub tx_hash: String,
    /// Địa chỉ người gửi
    pub from_address: String,
    /// Địa chỉ người nhận
    pub to_address: Option<String>,
    /// Giá trị giao dịch (native token - ETH, BNB, v.v.)
    pub value: f64,
    /// Giá trị giao dịch (ETH)
    pub value_eth: f64,
    /// Giá trị giao dịch (USD)
    pub value_usd: f64,
    /// Gas price (gwei)
    pub gas_price: f64,
    /// Gas limit
    pub gas_limit: u64,
    /// Dữ liệu giao dịch (hex)
    pub input_data: String,
    /// Thời gian phát hiện giao dịch
    pub detected_at: u64,
    /// Nonce giao dịch
    pub nonce: u64,
    /// Function signature (4 bytes đầu tiên của input data)
    pub function_signature: Option<String>,
    /// Loại giao dịch (transfer, swap, add_liquidity, v.v.)
    pub transaction_type: TransactionType,
    /// Mức độ ưu tiên (dựa trên gas price)
    pub priority: TransactionPriority,
    /// Cờ đánh dấu giao dịch đáng ngờ
    pub is_suspicious: bool,
    /// Mô tả nếu giao dịch đáng ngờ
    pub suspicious_reason: Option<String>,
    /// Optional from token info (for swaps)
    pub from_token: Option<TokenInfo>,
    /// Optional to token info (for swaps)
    pub to_token: Option<TokenInfo>,
    /// Pool address
    pub pool_address: String,
    /// Timestamp
    pub timestamp: u64,
}

/// Loại giao dịch được phát hiện
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TransactionType {
    /// Chuyển token native (ETH, BNB, v.v.)
    NativeTransfer,
    /// Chuyển token ERC20
    TokenTransfer,
    /// Tạo token mới
    TokenCreation,
    /// Thêm thanh khoản
    AddLiquidity,
    /// Rút thanh khoản
    RemoveLiquidity,
    /// Swap token
    Swap,
    /// Mint NFT
    NftMint,
    /// Tương tác với contract
    ContractInteraction,
    /// Triển khai contract mới
    ContractDeployment,
    /// Không xác định được
    Unknown,
}

/// Mức độ ưu tiên của giao dịch dựa trên gas price
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TransactionPriority {
    /// Ưu tiên rất cao (>95% gas price trung bình)
    VeryHigh,
    /// Ưu tiên cao (>50% gas price trung bình)
    High,
    /// Ưu tiên trung bình (gas price trung bình)
    Medium,
    /// Ưu tiên thấp (<50% gas price trung bình)
    Low,
}

impl PartialOrd for TransactionPriority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TransactionPriority {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (TransactionPriority::VeryHigh, TransactionPriority::VeryHigh) => Ordering::Equal,
            (TransactionPriority::VeryHigh, _) => Ordering::Greater,
            (TransactionPriority::High, TransactionPriority::VeryHigh) => Ordering::Less,
            (TransactionPriority::High, TransactionPriority::High) => Ordering::Equal,
            (TransactionPriority::High, _) => Ordering::Greater,
            (TransactionPriority::Medium, TransactionPriority::VeryHigh | TransactionPriority::High) => Ordering::Less,
            (TransactionPriority::Medium, TransactionPriority::Medium) => Ordering::Equal,
            (TransactionPriority::Medium, TransactionPriority::Low) => Ordering::Greater,
            (TransactionPriority::Low, TransactionPriority::Low) => Ordering::Equal,
            (TransactionPriority::Low, _) => Ordering::Less,
        }
    }
}

/// Thông tin token phát hiện từ mempool
#[derive(Debug, Clone)]
pub struct NewTokenInfo {
    /// Địa chỉ contract token
    pub token_address: String,
    /// Chain ID của blockchain
    pub chain_id: u32,
    /// Giao dịch tạo token
    pub creation_tx: String,
    /// Người tạo token
    pub creator_address: String,
    /// Thời gian phát hiện
    pub detected_at: u64,
    /// Đã thêm thanh khoản chưa
    pub liquidity_added: bool,
    /// Giao dịch thêm thanh khoản (nếu có)
    pub liquidity_tx: Option<String>,
    /// Giá trị thanh khoản (USD)
    pub liquidity_value: Option<f64>,
    /// Loại DEX được sử dụng (Uniswap, PancakeSwap, v.v.)
    pub dex_type: Option<String>,
}

/// Mẫu giao dịch đáng ngờ được phát hiện
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuspiciousPattern {
    /// Front-running (giao dịch trước giao dịch lớn với gas price cao hơn)
    FrontRunning,
    /// Sandwich attack (kẹp giao dịch swap lớn giữa hai giao dịch nhỏ)
    SandwichAttack,
    /// MEV bot (phát hiện bot khai thác MEV)
    MevBot,
    /// Rút thanh khoản đột ngột (rug pull)
    SuddenLiquidityRemoval,
    /// Giao dịch lớn từ ví whale
    WhaleMovement,
    /// Nhiều giao dịch liên tiếp từ cùng địa chỉ
    HighFrequencyTrading,
    /// Tạo token và thêm thanh khoản cùng lúc (có thể là scam)
    TokenLiquidityPattern,
    /// Rút thanh khoản (rug pull)
    LiquidityRemoval,
    /// Sandwich attack
    Sandwich,
}

/// Metadata for suspicious pattern with additional details and confidence score
#[derive(Debug, Clone, Default)]
pub struct PatternMetadata {
    /// Confidence score (0.0-1.0) with 1.0 being highest confidence
    pub confidence: f64,
    /// Detailed description of the pattern
    pub detail: String,
    /// Related transaction hashes
    pub related_txs: Vec<String>,
    /// Additional properties as key-value pairs
    pub properties: HashMap<String, String>,
}

/// Implement methods for SuspiciousPattern
impl SuspiciousPattern {
    // Global pattern metadata storage
    thread_local! {
        static PATTERN_METADATA: std::cell::RefCell<HashMap<String, PatternMetadata>> = 
            std::cell::RefCell::new(HashMap::new());
    }
    
    /// Generate a unique identifier for this pattern instance
    fn generate_id(&self) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        
        format!("{:?}_{}", self, timestamp)
    }
    
    /// Set confidence score for this pattern (0.0-1.0)
    pub fn set_confidence(&mut self, confidence: f64) {
        let id = self.generate_id();
        Self::PATTERN_METADATA.with(|metadata| {
            let mut metadata = metadata.borrow_mut();
            let entry = metadata.entry(id).or_insert_with(PatternMetadata::default);
            entry.confidence = confidence.max(0.0).min(1.0);
        });
    }
    
    /// Get confidence score for this pattern
    pub fn confidence(&self) -> f64 {
        let id = self.generate_id();
        Self::PATTERN_METADATA.with(|metadata| {
            metadata.borrow().get(&id).map_or(0.5, |m| m.confidence)
        })
    }
    
    /// Set detailed description for this pattern
    pub fn set_detail(&mut self, detail: String) {
        let id = self.generate_id();
        Self::PATTERN_METADATA.with(|metadata| {
            let mut metadata = metadata.borrow_mut();
            let entry = metadata.entry(id).or_insert_with(PatternMetadata::default);
            entry.detail = detail;
        });
    }
    
    /// Get detailed description for this pattern
    pub fn detail(&self) -> String {
        let id = self.generate_id();
        Self::PATTERN_METADATA.with(|metadata| {
            metadata.borrow().get(&id).map_or_else(
                || format!("{:?} pattern detected", self),
                |m| m.detail.clone()
            )
        })
    }
    
    /// Add related transaction hash
    pub fn add_related_tx(&mut self, tx_hash: String) {
        let id = self.generate_id();
        Self::PATTERN_METADATA.with(|metadata| {
            let mut metadata = metadata.borrow_mut();
            let entry = metadata.entry(id).or_insert_with(PatternMetadata::default);
            entry.related_txs.push(tx_hash);
        });
    }
    
    /// Set additional property
    pub fn set_property(&mut self, key: String, value: String) {
        let id = self.generate_id();
        Self::PATTERN_METADATA.with(|metadata| {
            let mut metadata = metadata.borrow_mut();
            let entry = metadata.entry(id).or_insert_with(PatternMetadata::default);
            entry.properties.insert(key, value);
        });
    }
    
    /// Get property value
    pub fn get_property(&self, key: &str) -> Option<String> {
        let id = self.generate_id();
        Self::PATTERN_METADATA.with(|metadata| {
            metadata.borrow().get(&id).and_then(|m| 
                m.properties.get(key).cloned()
            )
        })
    }
    
    /// Add key-value metadata to this pattern
    pub fn add_metadata(&mut self, key: &str, value: &str) {
        self.set_property(key.to_string(), value.to_string());
    }
    
    /// Get all metadata for this pattern
    pub fn get_all_metadata(&self) -> HashMap<String, String> {
        Self::PATTERN_METADATA.with(|metadata| {
            let metadata = metadata.borrow();
            if let Some(pattern_metadata) = metadata.get(&self.generate_id()) {
                pattern_metadata.properties.clone()
            } else {
                HashMap::new()
            }
        })
    }
}

/// Cảnh báo từ phân tích mempool
#[derive(Debug, Clone)]
pub struct MempoolAlert {
    /// Loại cảnh báo
    pub alert_type: MempoolAlertType,
    /// Mức độ nghiêm trọng
    pub severity: AlertSeverity,
    /// Thời gian phát hiện
    pub detected_at: u64,
    /// Mô tả cảnh báo
    pub description: String,
    /// Các giao dịch liên quan
    pub related_transactions: Vec<String>,
    /// Token liên quan (nếu có)
    pub related_token: Option<String>,
    /// Giao dịch mục tiêu (nếu có)
    pub target_transaction: Option<String>,
    /// Mẫu đáng ngờ (nếu có)
    pub suspicious_pattern: Option<SuspiciousPattern>,
    /// Địa chỉ liên quan
    pub related_addresses: Vec<String>,
}

/// Loại cảnh báo mempool
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MempoolAlertType {
    /// Token mới được tạo
    NewToken,
    /// Thêm thanh khoản mới
    LiquidityAdded,
    /// Rút thanh khoản
    LiquidityRemoved,
    /// Hoạt động đáng ngờ
    SuspiciousActivity,
    /// Giao dịch đáng ngờ
    SuspiciousTransaction(SuspiciousPattern),
    /// Giao dịch whale lớn
    WhaleTransaction,
    /// Cơ hội MEV phát hiện
    MevOpportunity,
}

/// Mức độ nghiêm trọng của cảnh báo
#[derive(Debug, Clone, PartialEq, Ord, PartialOrd, Eq)]
pub enum AlertSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

/// Thông tin chuyển token
#[derive(Debug, Clone)]
pub struct TokenTransferInfo {
    /// Địa chỉ token
    pub token_address: String,
    /// Địa chỉ nhận
    pub to_address: Option<String>,
    /// Số lượng token
    pub amount: f64,
}

/// Cluster giao dịch
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TransactionCluster {
    /// Giao dịch thông thường
    Normal,
    /// Giao dịch với gas cao
    HighGas,
    /// Chuyển token giá trị lớn (whale)
    WhaleMovement,
    /// Triển khai contract
    ContractDeployment,
    /// Thay đổi thanh khoản
    LiquidityChange,
    /// Giao dịch swap
    Swap,
}

/// Dữ liệu dự đoán giao dịch
#[derive(Debug, Clone)]
pub struct TransactionPredictionData {
    /// Hash giao dịch
    pub tx_hash: String,
    /// Gas price
    pub gas_price: f64,
    /// Gas limit
    pub gas_limit: u64,
    /// Giá trị giao dịch
    pub value: f64,
    /// Thời gian phát hiện
    pub detected_at: u64,
    /// Loại giao dịch
    pub transaction_type: TransactionType,
    /// Mức độ ưu tiên
    pub priority: TransactionPriority,
    /// Địa chỉ người gửi
    pub from_address: String,
    /// Địa chỉ người nhận
    pub to_address: Option<String>,
    /// Độ dài dữ liệu đầu vào
    pub input_data_length: usize,
    /// Function signature
    pub function_signature: Option<String>,
    /// Nonce giao dịch
    pub nonce: u64,
}

/// Thông tin token
#[derive(Debug, Clone)]
pub struct TokenInfo {
    /// Địa chỉ token
    pub address: String,
    /// Symbol
    pub symbol: Option<String>,
    /// Số lượng token
    pub amount: f64,
    /// Token price in USD
    pub price: f64,
    /// Token liquidity in USD
    pub liquidity_usd: f64,
    /// Thanh khoản token trong ETH
    pub liquidity_eth: Option<f64>,
    /// 24-hour volume
    pub volume_24h: f64,
    /// Number of holders
    pub holders: u64,
    /// Last update timestamp
    pub last_updated: u64,
    /// Risk score (0-100)
    pub risk_score: f64,
    /// Warning messages
    pub warnings: Vec<String>,
    /// Buy tax percentage
    pub buy_tax: f64,
    /// Sell tax percentage
    pub sell_tax: f64,
    /// Maximum transaction amount
    pub max_tx_amount: Option<f64>,
    /// Maximum wallet amount
    pub max_wallet_amount: Option<f64>,
    /// Market capitalization
    pub market_cap: f64,
    /// 24-hour price change percentage
    pub price_change_24h: f64,
    /// 7-day price change percentage
    pub price_change_7d: f64,
    /// Whether the contract is verified
    pub contract_verified: bool,
    /// Contract creation date
    pub contract_creation_date: Option<u64>,
    /// Contract creator address
    pub contract_creator: Option<String>,
    /// Whether this is a base token (ETH, BNB, etc.)
    pub is_base_token: bool,
    /// Token creation timestamp
    pub created_at: u64,
    /// Chain ID của token
    pub chain_id: u32,
    /// Có phải honeypot không
    pub is_honeypot: bool,
    /// Có thể mint thêm token không
    pub is_mintable: bool,
    /// Có sử dụng proxy không
    pub is_proxy: bool,
    /// Có chức năng blacklist không
    pub has_blacklist: bool,
    /// Token name
    pub name: String,
    /// Token decimals
    pub decimals: u8,
    /// Token total supply
    pub total_supply: u64,
    /// Token price in USD
    pub price_usd: f64,
}

/// Các tùy chọn lọc giao dịch
#[derive(Debug, Clone)]
pub struct TransactionFilterOptions {
    /// Lọc theo loại giao dịch
    pub transaction_types: Option<HashSet<TransactionType>>,
    /// Lọc theo mức độ ưu tiên
    pub priorities: Option<HashSet<TransactionPriority>>,
    /// Giá trị giao dịch tối thiểu (ETH)
    pub min_value: Option<f64>,
    /// Giá trị giao dịch tối đa (ETH)
    pub max_value: Option<f64>,
    /// Gas price tối thiểu (Gwei)
    pub min_gas_price: Option<f64>,
    /// Gas price tối đa (Gwei)
    pub max_gas_price: Option<f64>,
    /// Lọc theo địa chỉ người gửi
    pub from_addresses: Option<Vec<String>>,
    /// Lọc theo địa chỉ người nhận
    pub to_addresses: Option<Vec<String>>,
    /// Lọc theo function signature
    pub function_signatures: Option<Vec<String>>,
    /// Thời gian bắt đầu (unix timestamp)
    pub start_time: Option<u64>,
    /// Thời gian kết thúc (unix timestamp)
    pub end_time: Option<u64>,
    /// Chỉ lấy giao dịch từ địa chỉ được theo dõi
    pub only_watched_addresses: Option<bool>,
    /// Có chứa dữ liệu input không
    pub has_input_data: Option<bool>,
    /// Có chứa nonce gap không (giao dịch có thể bị mắc kẹt)
    pub has_nonce_gap: Option<bool>,
    /// Tiêu chí sắp xếp
    pub sort_by: Option<TransactionSortCriteria>,
    /// Thứ tự sắp xếp (true = tăng dần, false = giảm dần)
    pub sort_ascending: Option<bool>,
}

/// Tiêu chí sắp xếp giao dịch
#[derive(Debug, Clone)]
pub enum TransactionSortCriteria {
    /// Thời gian phát hiện
    DetectedTime,
    /// Giá trị giao dịch
    Value,
    /// Gas price
    GasPrice,
    /// Nonce
    Nonce,
    /// Mức độ ưu tiên
    Priority,
}

/// Loại sự kiện thanh khoản
#[derive(Debug, Clone, PartialEq)]
pub enum LiquidityEventType {
    /// Thêm thanh khoản
    Added,
    /// Rút thanh khoản
    Removed,
    /// Chuyển thanh khoản
    Transferred,
    /// Thêm thanh khoản (dùng hàm addLiquidity)
    AddLiquidity,
    /// Rút thanh khoản (dùng hàm removeLiquidity)
    RemoveLiquidity,
}

/// Implement methods for detection module
pub mod detection {
    use super::*;
    
    /// Kiểm tra giao dịch có khả năng front-running không
    pub fn is_potential_front_running(tx: &MempoolTransaction) -> bool {
        // Kiểm tra các dấu hiệu front-running:
        // 1. Gas price cao hơn trung bình
        // 2. Là giao dịch swap
        // 3. Từ địa chỉ đã biết là bot hoặc có pattern đáng ngờ
        tx.gas_price > 50.0 && 
            tx.transaction_type == TransactionType::Swap &&
            tx.is_suspicious
    }
    
    /// Kiểm tra giao dịch có khả năng là sandwich attack không
    pub fn is_potential_sandwich_attack(tx: &MempoolTransaction) -> bool {
        // Kiểm tra các dấu hiệu sandwich attack:
        // 1. Gas price cao
        // 2. Là giao dịch swap
        // 3. Có các giao dịch liên quan từ cùng địa chỉ
        tx.gas_price > 60.0 && 
            tx.transaction_type == TransactionType::Swap &&
            tx.is_suspicious
    }
    
    /// Kiểm tra địa chỉ có khả năng là MEV bot không
    pub fn is_potential_mev_bot(address: &str) -> bool {
        // Kiểm tra các dấu hiệu MEV bot:
        // 1. Địa chỉ đã biết là bot
        // 2. Có pattern giao dịch đặc trưng của bot
        // Đây là stub implementation, cần thay thế bằng logic thực tế
        address.starts_with("0x") && address.ends_with("bot")
    }
}

impl MempoolTransaction {
    /// Tạo giao dịch mới
    ///
    /// # Parameters
    /// - `tx_hash`: Hash của giao dịch
    /// - `from_address`: Địa chỉ gửi
    /// - `to_address`: Địa chỉ nhận
    /// - `value`: Giá trị giao dịch (ETH/BNB)
    /// - `gas_limit`: Gas limit
    /// - `gas_price`: Gas price (Gwei)
    /// - `input_data`: Dữ liệu đầu vào
    /// - `nonce`: Nonce
    ///
    /// # Returns
    /// - `MempoolTransaction`: Giao dịch mới
    pub fn new(
        tx_hash: String,
        from_address: String,
        to_address: Option<String>,
        value: f64,
        gas_limit: u64,
        gas_price: f64,
        input_data: String,
        nonce: u64,
    ) -> Self {
        // Thời điểm hiện tại
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Function signature từ input data
        let function_signature = if input_data.len() >= 10 {
            Some(input_data[0..10].to_string())
        } else {
            None
        };
        
        // Xác định loại giao dịch
        let transaction_type = if to_address.is_none() {
            TransactionType::ContractDeployment
        } else if input_data.len() <= 2 {
            TransactionType::NativeTransfer
        } else if function_signature.as_ref().map_or(false, |s| s == "0xa9059cbb" || s == "0x23b872dd") {
            TransactionType::TokenTransfer
        } else if function_signature.as_ref().map_or(false, |s| s.contains("swap") || s.starts_with("0x38ed1739")) {
            TransactionType::Swap
        } else {
            TransactionType::ContractInteraction
        };
        
        // Mức độ ưu tiên
        let priority = TransactionPriority::Medium;
        
        MempoolTransaction {
            tx_hash,
            from_address,
            to_address,
            value,
            value_eth: 0.0,
            value_usd: 0.0,
            gas_price,
            gas_limit,
            input_data,
            nonce,
            transaction_type,
            priority,
            detected_at: now,
            function_signature,
            is_suspicious: false,
            suspicious_reason: None,
            from_token: None,
            to_token: None,
            pool_address: String::new(),
            timestamp: 0,
        }
    }
    
    /// Đánh dấu giao dịch là đáng ngờ
    ///
    /// # Parameters
    /// - `reason`: Lý do đánh dấu
    pub fn mark_as_suspicious(&mut self, reason: &str) {
        self.is_suspicious = true;
        self.suspicious_reason = Some(reason.to_string());
    }
}

/// MempoolAnalyzer
pub struct MempoolAnalyzer {
    // ... existing code ...
}

impl MempoolAnalyzer {
    /// Thêm alert mới
    pub fn add_alert(&mut self, alert: MempoolAlert) {
        // ... existing code ...
    }
} 