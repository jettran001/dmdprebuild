/// Phân tích giao dịch trong mempool để phát hiện cơ hội và rủi ro.
///
/// Module này theo dõi và phân tích các giao dịch chưa được xác nhận (pending)
/// trong mempool của blockchain để:
/// - Phát hiện sớm các token mới được tạo
/// - Phát hiện giao dịch thêm thanh khoản lớn
/// - Theo dõi và tận dụng cơ hội MEV
/// - Phát hiện giao dịch đáng ngờ từ ví lớn (whale)
/// - Cảnh báo rủi ro giao dịch dựa trên pattern
///
/// # Flow
/// `blockchain` -> `snipebot/mempool_analyzer` -> `tradelogic`

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use sha2::{Sha256, Digest};

// === Các hằng số và ngưỡng phân tích ===

// Các ngưỡng giá trị giao dịch
const LARGE_TRANSACTION_ETH: f64 = 5.0; // >5 ETH là giao dịch lớn
const VERY_LARGE_TRANSACTION_ETH: f64 = 20.0; // >20 ETH là giao dịch rất lớn
const WHALE_TRANSACTION_ETH: f64 = 50.0; // >50 ETH là giao dịch whale

// Ngưỡng thời gian theo dõi
const MAX_PENDING_TX_AGE_SECONDS: u64 = 600; // Theo dõi giao dịch trong 10 phút
const MEMPOOL_REFRESH_INTERVAL_MS: u64 = 3000; // Làm mới mempool 3 giây/lần
const TRANSACTION_CLEANUP_INTERVAL_SEC: u64 = 30; // Dọn dẹp giao dịch cũ 30 giây/lần

// Ngưỡng pattern detection
const MIN_PATTERN_CONFIDENCE: f64 = 0.6; // 60% độ tin cậy nhận dạng mẫu
const FRONT_RUNNING_TIME_WINDOW_MS: u64 = 500; // Cửa sổ thời gian 500ms để phát hiện front-running
const MAX_SANDWICH_TIME_WINDOW_MS: u64 = 1000; // Cửa sổ thời gian 1000ms để phát hiện sandwich attack

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
}

/// Loại giao dịch được phát hiện
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq, Ord, PartialOrd, Eq)]
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
#[derive(Debug, Clone, PartialEq)]
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
    /// Địa chỉ liên quan
    pub related_addresses: Vec<String>,
}

/// Loại cảnh báo mempool
#[derive(Debug, Clone, PartialEq)]
pub enum MempoolAlertType {
    /// Token mới được tạo
    NewToken,
    /// Thêm thanh khoản mới
    LiquidityAdded,
    /// Rút thanh khoản
    LiquidityRemoved,
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

/// Phân tích và theo dõi mempool
pub struct MempoolAnalyzer {
    /// Giao dịch đang chờ xử lý
    pending_transactions: RwLock<HashMap<String, MempoolTransaction>>,
    /// Token mới phát hiện
    new_tokens: RwLock<HashMap<String, NewTokenInfo>>,
    /// Cảnh báo phát hiện được
    alerts: RwLock<Vec<MempoolAlert>>,
    /// Thời gian cập nhật mempool cuối cùng
    last_update: RwLock<Instant>,
    /// Gas price trung bình hiện tại (gwei)
    average_gas_price: RwLock<f64>,
    /// Các địa chỉ quan trọng cần theo dõi (whale, dev, v.v.)
    watched_addresses: RwLock<HashSet<String>>,
    /// Các function signature phổ biến để phân loại giao dịch
    function_signatures: HashMap<String, (String, TransactionType)>,
    /// Chain ID đang phân tích
    chain_id: u32,
}

impl MempoolAnalyzer {
    /// Tạo mới một phân tích mempool cho chain cụ thể
    pub fn new(chain_id: u32) -> Self {
        Self {
            pending_transactions: RwLock::new(HashMap::new()),
            new_tokens: RwLock::new(HashMap::new()),
            alerts: RwLock::new(Vec::new()),
            last_update: RwLock::new(Instant::now()),
            average_gas_price: RwLock::new(0.0),
            watched_addresses: RwLock::new(HashSet::new()),
            function_signatures: Self::initialize_function_signatures(),
            chain_id,
        }
    }

    /// Khởi tạo bảng function signature phổ biến
    fn initialize_function_signatures() -> HashMap<String, (String, TransactionType)> {
        let mut signatures = HashMap::new();
        
        // Transfer ERC20
        signatures.insert(
            "0xa9059cbb".to_string(),
            ("transfer(address,uint256)".to_string(), TransactionType::TokenTransfer),
        );
        
        // TransferFrom ERC20
        signatures.insert(
            "0x23b872dd".to_string(),
            ("transferFrom(address,address,uint256)".to_string(), TransactionType::TokenTransfer),
        );
        
        // Approve ERC20
        signatures.insert(
            "0x095ea7b3".to_string(),
            ("approve(address,uint256)".to_string(), TransactionType::ContractInteraction),
        );
        
        // AddLiquidity Uniswap V2
        signatures.insert(
            "0xe8e33700".to_string(),
            ("addLiquidity(address,address,uint256,uint256,uint256,uint256,address,uint256)".to_string(), 
             TransactionType::AddLiquidity),
        );
        
        // AddLiquidityETH Uniswap V2
        signatures.insert(
            "0xf305d719".to_string(),
            ("addLiquidityETH(address,uint256,uint256,uint256,address,uint256)".to_string(), 
             TransactionType::AddLiquidity),
        );
        
        // RemoveLiquidity Uniswap V2
        signatures.insert(
            "0xbaa2abde".to_string(),
            ("removeLiquidity(address,address,uint256,uint256,uint256,address,uint256)".to_string(), 
             TransactionType::RemoveLiquidity),
        );
        
        // RemoveLiquidityETH Uniswap V2
        signatures.insert(
            "0x02751cec".to_string(),
            ("removeLiquidityETH(address,uint256,uint256,uint256,address,uint256)".to_string(), 
             TransactionType::RemoveLiquidity),
        );
        
        // SwapExactTokensForTokens Uniswap V2
        signatures.insert(
            "0x38ed1739".to_string(),
            ("swapExactTokensForTokens(uint256,uint256,address[],address,uint256)".to_string(), 
             TransactionType::Swap),
        );
        
        // SwapExactETHForTokens Uniswap V2
        signatures.insert(
            "0x7ff36ab5".to_string(),
            ("swapExactETHForTokens(uint256,address[],address,uint256)".to_string(), 
             TransactionType::Swap),
        );
        
        // SwapExactTokensForETH Uniswap V2
        signatures.insert(
            "0x18cbafe5".to_string(),
            ("swapExactTokensForETH(uint256,uint256,address[],address,uint256)".to_string(), 
             TransactionType::Swap),
        );
        
        signatures
    }

    /// Thêm một giao dịch mới vào mempool analyzer
    pub async fn add_transaction(&self, transaction: MempoolTransaction) {
        let mut pending_txs = self.pending_transactions.write().await;
        
        // Thêm giao dịch vào danh sách theo dõi
        pending_txs.insert(transaction.tx_hash.clone(), transaction.clone());
        
        // Cập nhật gas price trung bình
        self.update_average_gas_price(&transaction).await;
        
        // Phân tích giao dịch để tìm token mới hoặc giao dịch đáng ngờ
        self.analyze_transaction(&transaction).await;
        
        // Cập nhật thời gian làm mới mempool
        *self.last_update.write().await = Instant::now();
    }

    /// Cập nhật gas price trung bình
    async fn update_average_gas_price(&self, transaction: &MempoolTransaction) {
        let mut avg_gas = self.average_gas_price.write().await;
        if *avg_gas == 0.0 {
            *avg_gas = transaction.gas_price;
        } else {
            // Simple exponential moving average
            *avg_gas = *avg_gas * 0.95 + transaction.gas_price * 0.05;
        }
    }

    /// Phân tích giao dịch để phát hiện pattern, token mới và cảnh báo
    async fn analyze_transaction(&self, transaction: &MempoolTransaction) {
        // Phân tích input_data để phát hiện loại giao dịch
        let tx_type = self.detect_transaction_type(transaction);
        
        // Kiểm tra nếu là giao dịch tạo token mới
        if tx_type == TransactionType::ContractDeployment {
            self.process_potential_token_creation(transaction).await;
        }
        
        // Kiểm tra nếu là giao dịch thêm/rút thanh khoản
        if tx_type == TransactionType::AddLiquidity {
            self.process_liquidity_event(transaction, true).await;
        } else if tx_type == TransactionType::RemoveLiquidity {
            self.process_liquidity_event(transaction, false).await;
        }
        
        // Kiểm tra giao dịch lớn từ whale
        if transaction.value > WHALE_TRANSACTION_ETH {
            self.create_whale_alert(transaction).await;
        }
        
        // Phát hiện mẫu giao dịch đáng ngờ
        self.detect_suspicious_patterns(transaction).await;
    }

    /// Phát hiện loại giao dịch dựa trên input data
    fn detect_transaction_type(&self, transaction: &MempoolTransaction) -> TransactionType {
        // Nếu không có input data hoặc input data quá ngắn (chỉ có giao dịch ETH đơn thuần)
        if transaction.input_data.len() <= 2 || transaction.input_data == "0x" {
            return TransactionType::NativeTransfer;
        }
        
        // Kiểm tra function signature (4 bytes đầu)
        if transaction.input_data.len() >= 10 {
            let signature = &transaction.input_data[0..10];
            if let Some((_, tx_type)) = self.function_signatures.get(signature) {
                return tx_type.clone();
            }
        }
        
        // Kiểm tra nếu là contract deployment
        if transaction.to_address.is_none() && transaction.input_data.len() > 10 {
            return TransactionType::ContractDeployment;
        }
        
        // Mặc định là tương tác với contract không xác định
        TransactionType::ContractInteraction
    }

    /// Xử lý giao dịch có thể là tạo token mới
    async fn process_potential_token_creation(&self, transaction: &MempoolTransaction) {
        // Thực tế cần phân tích bytecode để xác định đây có phải là token hay không
        // Đây là phiên bản đơn giản, trong thực tế cần kiểm tra kĩ hơn
        
        // Tạo thông tin token mới
        let new_token = NewTokenInfo {
            token_address: format!("[predicted_address_for_{}_nonce_{}]", transaction.from_address, transaction.nonce),
            chain_id: self.chain_id,
            creation_tx: transaction.tx_hash.clone(),
            creator_address: transaction.from_address.clone(),
            detected_at: transaction.detected_at,
            liquidity_added: false,
            liquidity_tx: None,
            liquidity_value: None,
            dex_type: None,
        };
        
        // Lưu thông tin token mới
        let mut new_tokens = self.new_tokens.write().await;
        new_tokens.insert(new_token.token_address.clone(), new_token.clone());
        
        // Tạo cảnh báo token mới
        let alert = MempoolAlert {
            alert_type: MempoolAlertType::NewToken,
            severity: AlertSeverity::Info,
            detected_at: transaction.detected_at,
            description: format!("New token created by {}", transaction.from_address),
            related_transactions: vec![transaction.tx_hash.clone()],
            related_token: Some(new_token.token_address.clone()),
            related_addresses: vec![transaction.from_address.clone()],
        };
        
        // Lưu cảnh báo
        let mut alerts = self.alerts.write().await;
        alerts.push(alert);
    }

    /// Xử lý sự kiện thanh khoản (thêm/rút)
    async fn process_liquidity_event(&self, transaction: &MempoolTransaction, is_adding: bool) {
        // Trong thực tế, cần giải mã input_data để lấy thông tin chi tiết về token và giá trị
        // Đây là phiên bản đơn giản
        
        let alert_type = if is_adding {
            MempoolAlertType::LiquidityAdded
        } else {
            MempoolAlertType::LiquidityRemoved
        };
        
        let severity = if is_adding {
            AlertSeverity::Info
        } else {
            // Rút thanh khoản có thể là dấu hiệu của rug pull
            AlertSeverity::Medium
        };
        
        let description = if is_adding {
            format!("Liquidity added by {}", transaction.from_address)
        } else {
            format!("Liquidity removed by {}", transaction.from_address)
        };
        
        // Tạo cảnh báo sự kiện thanh khoản
        let alert = MempoolAlert {
            alert_type,
            severity,
            detected_at: transaction.detected_at,
            description,
            related_transactions: vec![transaction.tx_hash.clone()],
            related_token: None, // Cần giải mã input_data để biết token nào
            related_addresses: vec![transaction.from_address.clone()],
        };
        
        // Lưu cảnh báo
        let mut alerts = self.alerts.write().await;
        alerts.push(alert);
        
        // TODO: Cập nhật thông tin token nếu có trong danh sách token mới
    }

    /// Tạo cảnh báo giao dịch whale
    async fn create_whale_alert(&self, transaction: &MempoolTransaction) {
        let alert = MempoolAlert {
            alert_type: MempoolAlertType::WhaleTransaction,
            severity: AlertSeverity::Medium,
            detected_at: transaction.detected_at,
            description: format!(
                "Whale transaction detected: {} ETH from {}",
                transaction.value, transaction.from_address
            ),
            related_transactions: vec![transaction.tx_hash.clone()],
            related_token: None,
            related_addresses: vec![transaction.from_address.clone()],
        };
        
        // Lưu cảnh báo
        let mut alerts = self.alerts.write().await;
        alerts.push(alert);
        
        // Thêm địa chỉ vào danh sách theo dõi nếu chưa có
        let mut watched = self.watched_addresses.write().await;
        watched.insert(transaction.from_address.clone());
    }

    /// Phát hiện các mẫu giao dịch đáng ngờ
    async fn detect_suspicious_patterns(&self, transaction: &MempoolTransaction) {
        // Lấy toàn bộ giao dịch đang chờ để phân tích pattern
        let pending_txs = self.pending_transactions.read().await;
        
        // Phát hiện front-running
        if let Some(pattern) = self.detect_front_running(transaction, &pending_txs).await {
            self.create_suspicious_pattern_alert(transaction, pattern).await;
        }
        
        // Phát hiện sandwich attack
        if let Some(pattern) = self.detect_sandwich_attack(transaction, &pending_txs).await {
            self.create_suspicious_pattern_alert(transaction, pattern).await;
        }
        
        // Phát hiện giao dịch tần suất cao
        if let Some(pattern) = self.detect_high_frequency_trading(transaction, &pending_txs).await {
            self.create_suspicious_pattern_alert(transaction, pattern).await;
        }
    }

    /// Phát hiện front-running
    async fn detect_front_running(
        &self,
        transaction: &MempoolTransaction,
        pending_txs: &HashMap<String, MempoolTransaction>,
    ) -> Option<SuspiciousPattern> {
        // Trong front-running, một giao dịch với gas price cao hơn được đưa vào
        // ngay trước một giao dịch lớn để tận dụng thông tin
        
        // Kiểm tra nếu đây là giao dịch swap với gas price cao
        if transaction.transaction_type == TransactionType::Swap 
            && transaction.priority == TransactionPriority::VeryHigh {
            
            // Tìm các giao dịch swap khác cùng token trong cửa sổ thời gian
            for (_, other_tx) in pending_txs.iter() {
                if other_tx.tx_hash != transaction.tx_hash
                    && other_tx.transaction_type == TransactionType::Swap
                    && other_tx.value > transaction.value
                    && transaction.gas_price > other_tx.gas_price
                    && (transaction.detected_at - other_tx.detected_at) < FRONT_RUNNING_TIME_WINDOW_MS
                {
                    return Some(SuspiciousPattern::FrontRunning);
                }
            }
        }
        
        None
    }

    /// Phát hiện sandwich attack
    async fn detect_sandwich_attack(
        &self,
        transaction: &MempoolTransaction,
        pending_txs: &HashMap<String, MempoolTransaction>,
    ) -> Option<SuspiciousPattern> {
        // Trong sandwich attack, hai giao dịch nhỏ kẹp một giao dịch lớn để tận dụng slippage
        
        // Kiểm tra nếu có giao dịch swap lớn
        if transaction.transaction_type == TransactionType::Swap && transaction.value > LARGE_TRANSACTION_ETH {
            // Tìm các giao dịch swap nhỏ hơn với cùng token và gas price cao hơn
            let mut potential_sandwich = false;
            
            for (_, other_tx) in pending_txs.iter() {
                if other_tx.tx_hash != transaction.tx_hash
                    && other_tx.transaction_type == TransactionType::Swap
                    && other_tx.value < transaction.value
                    && other_tx.gas_price > transaction.gas_price
                    && (other_tx.detected_at - transaction.detected_at).abs() < MAX_SANDWICH_TIME_WINDOW_MS as i64
                {
                    potential_sandwich = true;
                    break;
                }
            }
            
            if potential_sandwich {
                return Some(SuspiciousPattern::SandwichAttack);
            }
        }
        
        None
    }

    /// Phát hiện giao dịch tần suất cao
    async fn detect_high_frequency_trading(
        &self,
        transaction: &MempoolTransaction,
        pending_txs: &HashMap<String, MempoolTransaction>,
    ) -> Option<SuspiciousPattern> {
        // Đếm số giao dịch từ cùng địa chỉ trong khoảng thời gian gần đây
        let mut tx_count = 0;
        
        for (_, other_tx) in pending_txs.iter() {
            if other_tx.from_address == transaction.from_address
                && (transaction.detected_at - other_tx.detected_at) < 10000 // 10 giây
            {
                tx_count += 1;
                
                // Nếu có nhiều hơn 5 giao dịch trong 10 giây từ cùng địa chỉ
                if tx_count >= 5 {
                    return Some(SuspiciousPattern::HighFrequencyTrading);
                }
            }
        }
        
        None
    }

    /// Tạo cảnh báo mẫu giao dịch đáng ngờ
    async fn create_suspicious_pattern_alert(&self, transaction: &MempoolTransaction, pattern: SuspiciousPattern) {
        let (description, severity) = match pattern {
            SuspiciousPattern::FrontRunning => (
                format!("Potential front-running detected from {}", transaction.from_address),
                AlertSeverity::High,
            ),
            SuspiciousPattern::SandwichAttack => (
                format!("Potential sandwich attack detected around transaction {}", transaction.tx_hash),
                AlertSeverity::High,
            ),
            SuspiciousPattern::MevBot => (
                format!("MEV bot activity detected from {}", transaction.from_address),
                AlertSeverity::Medium,
            ),
            SuspiciousPattern::SuddenLiquidityRemoval => (
                format!("Sudden liquidity removal detected by {}", transaction.from_address),
                AlertSeverity::Critical,
            ),
            SuspiciousPattern::WhaleMovement => (
                format!("Whale movement detected: {} ETH", transaction.value),
                AlertSeverity::Medium,
            ),
            SuspiciousPattern::HighFrequencyTrading => (
                format!("High frequency trading detected from {}", transaction.from_address),
                AlertSeverity::Low,
            ),
            SuspiciousPattern::TokenLiquidityPattern => (
                format!("Suspicious token creation and liquidity pattern from {}", transaction.from_address),
                AlertSeverity::High,
            ),
        };
        
        let alert = MempoolAlert {
            alert_type: MempoolAlertType::SuspiciousTransaction(pattern),
            severity,
            detected_at: transaction.detected_at,
            description,
            related_transactions: vec![transaction.tx_hash.clone()],
            related_token: None,
            related_addresses: vec![transaction.from_address.clone()],
        };
        
        // Lưu cảnh báo
        let mut alerts = self.alerts.write().await;
        alerts.push(alert);
    }

    /// Làm sạch các giao dịch cũ khỏi mempool
    pub async fn cleanup_old_transactions(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();
        
        let mut pending_txs = self.pending_transactions.write().await;
        
        // Xóa các giao dịch quá cũ
        pending_txs.retain(|_, tx| {
            now - tx.detected_at < MAX_PENDING_TX_AGE_SECONDS
        });
    }

    /// Lấy các cảnh báo mới từ mempool
    pub async fn get_alerts(&self, min_severity: Option<AlertSeverity>) -> Vec<MempoolAlert> {
        let alerts = self.alerts.read().await;
        
        if let Some(severity) = min_severity {
            alerts.iter()
                .filter(|alert| alert.severity >= severity)
                .cloned()
                .collect()
        } else {
            alerts.clone()
        }
    }

    /// Lấy các token mới phát hiện được
    pub async fn get_new_tokens(&self) -> Vec<NewTokenInfo> {
        let new_tokens = self.new_tokens.read().await;
        new_tokens.values().cloned().collect()
    }

    /// Lấy số lượng giao dịch đang theo dõi
    pub async fn get_pending_transaction_count(&self) -> usize {
        let pending_txs = self.pending_transactions.read().await;
        pending_txs.len()
    }

    /// Thêm địa chỉ vào danh sách theo dõi đặc biệt
    pub async fn add_watched_address(&self, address: &str) {
        let mut watched = self.watched_addresses.write().await;
        watched.insert(address.to_string());
    }

    /// Xóa địa chỉ khỏi danh sách theo dõi đặc biệt
    pub async fn remove_watched_address(&self, address: &str) {
        let mut watched = self.watched_addresses.write().await;
        watched.remove(address);
    }

    /// Kiểm tra xem một địa chỉ có trong danh sách theo dõi không
    pub async fn is_watched_address(&self, address: &str) -> bool {
        let watched = self.watched_addresses.read().await;
        watched.contains(address)
    }

    /// Lấy danh sách giao dịch đang chờ, giới hạn theo số lượng
    /// 
    /// # Parameters
    /// * `limit` - Số lượng giao dịch tối đa muốn lấy
    /// 
    /// # Returns
    /// Danh sách giao dịch đang chờ
    pub async fn get_pending_transactions(&self, limit: usize) -> Vec<MempoolTransaction> {
        // Chuyển sang phương thức mới với tùy chọn mặc định
        self.get_filtered_transactions(TransactionFilterOptions::default(), limit).await
    }

    /// Lấy danh sách giao dịch đang chờ với các tùy chọn lọc nâng cao
    /// 
    /// # Parameters
    /// * `filter_options` - Các tùy chọn lọc
    /// * `limit` - Số lượng giao dịch tối đa muốn lấy
    /// 
    /// # Returns
    /// Danh sách giao dịch đã lọc
    pub async fn get_filtered_transactions(
        &self,
        filter_options: TransactionFilterOptions,
        limit: usize,
    ) -> Vec<MempoolTransaction> {
        let pending_txs = self.pending_transactions.read().await;
        
        // Lấy danh sách giao dịch đang chờ
        let filtered_txs: Vec<MempoolTransaction> = pending_txs
            .values()
            .filter(|tx| {
                // Lọc theo loại giao dịch
                if let Some(types) = &filter_options.transaction_types {
                    if !types.contains(&tx.transaction_type) {
                        return false;
                    }
                }
                
                // Lọc theo mức độ ưu tiên
                if let Some(priorities) = &filter_options.priorities {
                    if !priorities.contains(&tx.priority) {
                        return false;
                    }
                }
                
                // Lọc theo giá trị giao dịch
                if let Some(min_value) = filter_options.min_value {
                    if tx.value < min_value {
                        return false;
                    }
                }
                
                if let Some(max_value) = filter_options.max_value {
                    if tx.value > max_value {
                        return false;
                    }
                }
                
                // Lọc theo gas price
                if let Some(min_gas_price) = filter_options.min_gas_price {
                    if tx.gas_price < min_gas_price {
                        return false;
                    }
                }
                
                if let Some(max_gas_price) = filter_options.max_gas_price {
                    if tx.gas_price > max_gas_price {
                        return false;
                    }
                }
                
                // Lọc theo địa chỉ người gửi
                if let Some(from_addresses) = &filter_options.from_addresses {
                    if !from_addresses.contains(&tx.from_address) {
                        return false;
                    }
                }
                
                // Lọc theo địa chỉ người nhận
                if let Some(to_addresses) = &filter_options.to_addresses {
                    if let Some(to_address) = &tx.to_address {
                        if !to_addresses.contains(to_address) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                
                // Lọc theo function signature
                if let Some(signatures) = &filter_options.function_signatures {
                    if let Some(signature) = &tx.function_signature {
                        if !signatures.contains(signature) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                
                // Lọc theo thời gian
                if let Some(start_time) = filter_options.start_time {
                    if tx.detected_at < start_time {
                        return false;
                    }
                }
                
                if let Some(end_time) = filter_options.end_time {
                    if tx.detected_at > end_time {
                        return false;
                    }
                }
                
                // Lọc theo địa chỉ được theo dõi
                if let Some(true) = filter_options.only_watched_addresses {
                    let watched_addresses = self.watched_addresses.blocking_read();
                    if !watched_addresses.contains(&tx.from_address) {
                        if let Some(to_address) = &tx.to_address {
                            if !watched_addresses.contains(to_address) {
                                return false;
                            }
                        } else {
                            return false;
                        }
                    }
                }
                
                // Lọc theo có input data hay không
                if let Some(has_input_data) = filter_options.has_input_data {
                    let has_data = !tx.input_data.is_empty() && tx.input_data != "0x";
                    if has_data != has_input_data {
                        return false;
                    }
                }
                
                // Mặc định: giữ lại giao dịch
                true
            })
            .cloned()
            .collect();
        
        // Sắp xếp kết quả nếu được yêu cầu
        let mut sorted_txs = if let Some(sort_criteria) = &filter_options.sort_by {
            let mut sorted = filtered_txs;
            let is_ascending = filter_options.sort_ascending.unwrap_or(false);
            
            match sort_criteria {
                TransactionSortCriteria::DetectedTime => {
                    if is_ascending {
                        sorted.sort_by_key(|tx| tx.detected_at);
                    } else {
                        sorted.sort_by_key(|tx| std::cmp::Reverse(tx.detected_at));
                    }
                },
                TransactionSortCriteria::Value => {
                    if is_ascending {
                        sorted.sort_by(|a, b| a.value.partial_cmp(&b.value).unwrap_or(std::cmp::Ordering::Equal));
                    } else {
                        sorted.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal));
                    }
                },
                TransactionSortCriteria::GasPrice => {
                    if is_ascending {
                        sorted.sort_by(|a, b| a.gas_price.partial_cmp(&b.gas_price).unwrap_or(std::cmp::Ordering::Equal));
                    } else {
                        sorted.sort_by(|a, b| b.gas_price.partial_cmp(&a.gas_price).unwrap_or(std::cmp::Ordering::Equal));
                    }
                },
                TransactionSortCriteria::Nonce => {
                    if is_ascending {
                        sorted.sort_by_key(|tx| tx.nonce);
                    } else {
                        sorted.sort_by_key(|tx| std::cmp::Reverse(tx.nonce));
                    }
                },
                TransactionSortCriteria::Priority => {
                    if is_ascending {
                        sorted.sort_by_key(|tx| tx.priority.clone());
                    } else {
                        sorted.sort_by_key(|tx| std::cmp::Reverse(tx.priority.clone()));
                    }
                },
            }
            
            sorted
        } else {
            // Mặc định: sắp xếp theo giá gas giảm dần
            let mut sorted = filtered_txs;
            sorted.sort_by(|a, b| b.gas_price.partial_cmp(&a.gas_price).unwrap_or(std::cmp::Ordering::Equal));
            sorted
        };
        
        // Xử lý lọc theo nonce gap
        if let Some(true) = filter_options.has_nonce_gap {
            // Nhóm giao dịch theo người gửi
            let mut txs_by_sender: HashMap<String, Vec<&MempoolTransaction>> = HashMap::new();
            for tx in &sorted_txs {
                txs_by_sender.entry(tx.from_address.clone())
                    .or_default()
                    .push(tx);
            }
            
            // Tìm các giao dịch có nonce gap
            let mut txs_with_nonce_gap = Vec::new();
            
            for (_, txs) in txs_by_sender {
                if txs.len() < 2 {
                    continue;
                }
                
                // Sắp xếp theo nonce
                let mut sorted_by_nonce = txs.clone();
                sorted_by_nonce.sort_by_key(|tx| tx.nonce);
                
                // Tìm các nonce gap
                let mut has_gap = false;
                let mut prev_nonce = sorted_by_nonce[0].nonce;
                
                for tx in sorted_by_nonce.iter().skip(1) {
                    if tx.nonce > prev_nonce + 1 {
                        has_gap = true;
                        break;
                    }
                    prev_nonce = tx.nonce;
                }
                
                if has_gap {
                    for tx in sorted_by_nonce {
                        txs_with_nonce_gap.push(tx.clone());
                    }
                }
            }
            
            sorted_txs = txs_with_nonce_gap;
        }
        
        // Giới hạn số lượng kết quả
        sorted_txs.into_iter().take(limit).collect()
    }

    /// Tìm giao dịch hoặc nhóm giao dịch có khả năng là Sandwich Attack
    /// 
    /// # Parameters
    /// * `min_value_eth` - Giá trị tối thiểu của giao dịch "victim" (ở giữa sandwich)
    /// * `time_window_ms` - Cửa sổ thời gian để phát hiện sandwich (ms)
    /// * `limit` - Số lượng kết quả tối đa
    /// 
    /// # Returns
    /// Danh sách các nhóm giao dịch sandwich
    pub async fn find_sandwich_attacks(
        &self,
        min_value_eth: f64,
        time_window_ms: u64,
        limit: usize,
    ) -> Vec<Vec<MempoolTransaction>> {
        // Lấy tất cả giao dịch swap
        let filter_options = TransactionFilterOptions {
            transaction_types: Some(vec![TransactionType::Swap]),
            sort_by: Some(TransactionSortCriteria::DetectedTime),
            sort_ascending: Some(true),
            ..Default::default()
        };
        
        let swap_txs = self.get_filtered_transactions(filter_options, 1000).await;
        
        // Nhóm các giao dịch theo token liên quan (thông qua function signature và to_address)
        let mut potential_sandwiches = Vec::new();
        
        for (i, tx) in swap_txs.iter().enumerate() {
            // Nếu tx có giá trị lớn hơn ngưỡng, có thể là "victim" (giao dịch ở giữa sandwich)
            if tx.value >= min_value_eth {
                // Tìm các giao dịch trước và sau trong cửa sổ thời gian
                let before_txs: Vec<&MempoolTransaction> = swap_txs[0..i].iter()
                    .filter(|t| {
                        // Cùng token/contract và thời gian gần nhau
                        t.to_address == tx.to_address &&
                        tx.detected_at - t.detected_at <= time_window_ms &&
                        // Gas price cao hơn
                        t.gas_price > tx.gas_price
                    })
                    .collect();
                
                let after_txs: Vec<&MempoolTransaction> = swap_txs[i+1..].iter()
                    .filter(|t| {
                        // Cùng token/contract và thời gian gần nhau
                        t.to_address == tx.to_address &&
                        t.detected_at - tx.detected_at <= time_window_ms &&
                        // Gas price cao hơn
                        t.gas_price > tx.gas_price
                    })
                    .collect();
                
                // Nếu có ít nhất một giao dịch trước và một giao dịch sau, có thể là sandwich
                if !before_txs.is_empty() && !after_txs.is_empty() {
                    // Ưu tiên giao dịch gần nhất theo thời gian
                    let before = before_txs.iter().max_by_key(|t| t.detected_at).unwrap();
                    let after = after_txs.iter().min_by_key(|t| t.detected_at).unwrap();
                    
                    // Kiểm tra thêm: cùng người gửi?
                    if before.from_address == after.from_address {
                        let sandwich = vec![(*before).clone(), tx.clone(), (*after).clone()];
                        potential_sandwiches.push(sandwich);
                    }
                }
            }
        }
        
        // Giới hạn số lượng kết quả
        potential_sandwiches.into_iter().take(limit).collect()
    }

    /// Tìm giao dịch hoặc nhóm giao dịch có khả năng là Front-running
    /// 
    /// # Parameters
    /// * `min_value_eth` - Giá trị tối thiểu của giao dịch "victim"
    /// * `time_window_ms` - Cửa sổ thời gian để phát hiện front-running (ms)
    /// * `limit` - Số lượng kết quả tối đa
    /// 
    /// # Returns
    /// Danh sách các cặp giao dịch front-running (frontrunner, victim)
    pub async fn find_frontrunning_transactions(
        &self,
        min_value_eth: f64,
        time_window_ms: u64,
        limit: usize,
    ) -> Vec<(MempoolTransaction, MempoolTransaction)> {
        // Lấy tất cả giao dịch swap và liquidity
        let filter_options = TransactionFilterOptions {
            transaction_types: Some(vec![
                TransactionType::Swap,
                TransactionType::AddLiquidity,
                TransactionType::RemoveLiquidity
            ]),
            sort_by: Some(TransactionSortCriteria::DetectedTime),
            sort_ascending: Some(true),
            ..Default::default()
        };
        
        let txs = self.get_filtered_transactions(filter_options, 1000).await;
        
        // Tìm các cặp giao dịch tiềm năng front-running
        let mut potential_frontrunning = Vec::new();
        
        for (i, victim) in txs.iter().enumerate() {
            // Nếu giao dịch có giá trị lớn, nó có thể là mục tiêu của front-running
            if victim.value >= min_value_eth {
                // Tìm các giao dịch trước đó có thể là front-runner
                for j in 0..i {
                    let frontrunner = &txs[j];
                    
                    // Kiểm tra điều kiện front-running:
                    // 1. Cùng token/contract
                    // 2. Trong cửa sổ thời gian
                    // 3. Gas price cao hơn
                    // 4. Không cùng người gửi
                    if frontrunner.to_address == victim.to_address &&
                       victim.detected_at - frontrunner.detected_at <= time_window_ms &&
                       frontrunner.gas_price > victim.gas_price &&
                       frontrunner.from_address != victim.from_address {
                        potential_frontrunning.push((frontrunner.clone(), victim.clone()));
                    }
                }
            }
        }
        
        // Giới hạn số lượng kết quả
        potential_frontrunning.into_iter().take(limit).collect()
    }

    /// Tìm các giao dịch có khả năng từ các bot MEV
    /// 
    /// # Parameters
    /// * `high_frequency_threshold` - Số lượng giao dịch tối thiểu trong khoảng thời gian ngắn để được coi là bot
    /// * `time_window_sec` - Cửa sổ thời gian để phát hiện hoạt động tần suất cao (giây)
    /// * `limit` - Số lượng địa chỉ bot tối đa trả về
    /// 
    /// # Returns
    /// Danh sách địa chỉ đáng ngờ là bot và số lượng giao dịch của chúng
    pub async fn find_mev_bots(
        &self,
        high_frequency_threshold: usize,
        time_window_sec: u64,
        limit: usize,
    ) -> Vec<(String, usize)> {
        let bursts = self.detect_transaction_bursts(time_window_sec, high_frequency_threshold).await;
        
        // Đếm số lượng giao dịch cho mỗi địa chỉ phát hiện
        let mut bot_frequency = HashMap::new();
        
        for (address, transactions) in bursts {
            *bot_frequency.entry(address).or_insert(0) += transactions.len();
        }
        
        // Chuyển thành vector và sắp xếp theo số lượng giao dịch
        let mut bot_list: Vec<(String, usize)> = bot_frequency.into_iter().collect();
        bot_list.sort_by(|a, b| b.1.cmp(&a.1));
        
        // Giới hạn số lượng kết quả
        bot_list.into_iter().take(limit).collect()
    }

    /// Phân tích thời gian sống của giao dịch và ảnh hưởng của nó đến mempool
    pub async fn analyze_transaction_lifetime(&self) -> HashMap<TransactionPriority, (f64, u64)> {
        let pending_txs = self.pending_transactions.read().await;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        
        let mut result = HashMap::new();
        let mut count = HashMap::new();
        
        for tx in pending_txs.values() {
            if now > tx.detected_at {
                let lifetime = now - tx.detected_at;
                let entry = result.entry(tx.priority.clone()).or_insert((0.0, 0));
                entry.0 += lifetime as f64;
                entry.1 += 1;
                *count.entry(tx.priority.clone()).or_insert(0) += 1;
            }
        }
        
        // Tính trung bình
        for (priority, (total_time, total_count)) in &mut result {
            if *total_count > 0 {
                *total_time /= *total_count as f64;
            }
        }
        
        result
    }

    /// Phân tích phân phối gas price để dự đoán độ tắc nghẽn mạng
    pub async fn analyze_gas_price_distribution(&self) -> (f64, f64, f64, f64) {
        let pending_txs = self.pending_transactions.read().await;
        let mut gas_prices: Vec<f64> = pending_txs.values()
            .map(|tx| tx.gas_price)
            .collect();
        
        if gas_prices.is_empty() {
            return (0.0, 0.0, 0.0, 0.0);
        }
        
        gas_prices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        
        let len = gas_prices.len();
        let min = gas_prices[0];
        let max = gas_prices[len - 1];
        let median = if len % 2 == 0 {
            (gas_prices[len / 2 - 1] + gas_prices[len / 2]) / 2.0
        } else {
            gas_prices[len / 2]
        };
        
        // Tính standard deviation để đánh giá sự phân tán
        let mean = gas_prices.iter().sum::<f64>() / len as f64;
        let variance = gas_prices.iter()
            .map(|x| (*x - mean).powi(2))
            .sum::<f64>() / len as f64;
        let std_dev = variance.sqrt();
        
        (min, median, max, std_dev)
    }

    /// Nhóm các giao dịch theo người gửi để phát hiện mẫu hành vi
    pub async fn group_transactions_by_sender(&self) -> HashMap<String, Vec<MempoolTransaction>> {
        let pending_txs = self.pending_transactions.read().await;
        let mut result = HashMap::new();
        
        for tx in pending_txs.values() {
            result.entry(tx.from_address.clone())
                .or_insert_with(Vec::new)
                .push(tx.clone());
        }
        
        // Sắp xếp theo nonce để xác định thứ tự giao dịch
        for txs in result.values_mut() {
            txs.sort_by_key(|tx| tx.nonce);
        }
        
        result
    }

    /// Phân tích blockspace utilization để dự đoán khi nào giao dịch được xác nhận
    pub async fn analyze_blockspace_utilization(&self) -> (f64, f64) {
        let pending_txs = self.pending_transactions.read().await;
        let total_gas = pending_txs.values()
            .map(|tx| tx.gas_limit)
            .sum::<u64>();
        
        // Ước tính gas limit trung bình của block (thay đổi tùy chain)
        let avg_block_gas_limit = match self.chain_id {
            1 => 15_000_000, // Ethereum
            56 => 30_000_000, // BSC
            137 => 20_000_000, // Polygon
            _ => 15_000_000, // Mặc định
        };
        
        // Ước tính số block cần để xử lý hết mempool
        let estimated_blocks = (total_gas as f64) / (avg_block_gas_limit as f64);
        
        // Ước tính thời gian chờ trung bình (giây)
        let avg_block_time = match self.chain_id {
            1 => 12.0, // Ethereum
            56 => 3.0, // BSC
            137 => 2.0, // Polygon
            _ => 12.0, // Mặc định
        };
        
        let estimated_wait_time = estimated_blocks * avg_block_time;
        
        (estimated_blocks, estimated_wait_time)
    }

    /// Phát hiện các giao dịch nonce gap (bị mắc kẹt do nonce không liên tục)
    pub async fn detect_nonce_gaps(&self) -> HashMap<String, Vec<u64>> {
        let txs_by_sender = self.group_transactions_by_sender().await;
        let mut nonce_gaps = HashMap::new();
        
        for (sender, txs) in txs_by_sender {
            if txs.len() < 2 {
                continue;
            }
            
            let mut gaps = Vec::new();
            let mut prev_nonce = txs[0].nonce;
            
            for tx in txs.iter().skip(1) {
                if tx.nonce > prev_nonce + 1 {
                    // Có khoảng trống nonce
                    for nonce in (prev_nonce + 1)..tx.nonce {
                        gaps.push(nonce);
                    }
                }
                prev_nonce = tx.nonce;
            }
            
            if !gaps.is_empty() {
                nonce_gaps.insert(sender, gaps);
            }
        }
        
        nonce_gaps
    }

    /// Phát hiện các giao dịch replace-by-fee (thay thế bằng phí cao hơn)
    pub async fn detect_replace_by_fee_transactions(&self) -> Vec<(MempoolTransaction, MempoolTransaction)> {
        let txs_by_sender = self.group_transactions_by_sender().await;
        let mut replacements = Vec::new();
        
        for txs in txs_by_sender.values() {
            // Tạo một HashMap để lưu trữ giao dịch mới nhất cho mỗi nonce
            let mut nonce_map = HashMap::new();
            
            // Trước tiên, sắp xếp các giao dịch theo thời gian phát hiện
            let mut sorted_txs = txs.clone();
            sorted_txs.sort_by_key(|tx| tx.detected_at);
            
            for tx in sorted_txs {
                if let Some(prev_tx) = nonce_map.get(&tx.nonce) {
                    // Cùng nonce, kiểm tra xem gas price có cao hơn không
                    if tx.gas_price > prev_tx.gas_price {
                        replacements.push((prev_tx.clone(), tx.clone()));
                        nonce_map.insert(tx.nonce, tx);
                    }
                } else {
                    nonce_map.insert(tx.nonce, tx);
                }
            }
        }
        
        replacements
    }

    /// Phát hiện bursts của giao dịch (đột biến số lượng giao dịch trong khoảng thời gian ngắn)
    pub async fn detect_transaction_bursts(&self, time_window_sec: u64, threshold: usize) -> Vec<(String, Vec<MempoolTransaction>)> {
        let txs_by_sender = self.group_transactions_by_sender().await;
        let mut bursts = Vec::new();
        
        for (sender, txs) in txs_by_sender {
            if txs.len() < threshold {
                continue;
            }
            
            // Sắp xếp theo thời gian phát hiện
            let mut sorted_txs = txs.clone();
            sorted_txs.sort_by_key(|tx| tx.detected_at);
            
            let mut current_burst = Vec::new();
            let mut burst_start_time = 0;
            
            for tx in sorted_txs {
                if current_burst.is_empty() {
                    current_burst.push(tx.clone());
                    burst_start_time = tx.detected_at;
                } else if tx.detected_at <= burst_start_time + time_window_sec {
                    current_burst.push(tx.clone());
                } else {
                    // Kiểm tra xem burst hiện tại có đạt ngưỡng không
                    if current_burst.len() >= threshold {
                        bursts.push((sender.clone(), current_burst));
                    }
                    
                    // Bắt đầu burst mới
                    current_burst = vec![tx.clone()];
                    burst_start_time = tx.detected_at;
                }
            }
            
            // Kiểm tra burst cuối cùng
            if current_burst.len() >= threshold {
                bursts.push((sender.clone(), current_burst));
            }
        }
        
        bursts
    }

    /// Phân tích lịch sử gas price theo thời gian để dự đoán xu hướng
    pub async fn analyze_gas_price_trend(&self, time_window_sec: u64) -> Vec<(u64, f64)> {
        let pending_txs = self.pending_transactions.read().await;
        
        // Chuyển thành vector để dễ sắp xếp
        let mut txs: Vec<&MempoolTransaction> = pending_txs.values().collect();
        txs.sort_by_key(|tx| tx.detected_at);
        
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let start_time = now.saturating_sub(time_window_sec);
        
        // Nhóm gas price theo khoảng thời gian (mỗi phút)
        let mut gas_by_time = HashMap::new();
        let interval = 60; // 60 giây
        
        for tx in txs {
            if tx.detected_at >= start_time {
                let time_bucket = (tx.detected_at - start_time) / interval * interval + start_time;
                let entry = gas_by_time.entry(time_bucket).or_insert(Vec::new());
                entry.push(tx.gas_price);
            }
        }
        
        // Tính giá trị trung bình cho mỗi khoảng thời gian
        let mut result = Vec::new();
        for (time, gas_prices) in gas_by_time {
            if !gas_prices.is_empty() {
                let avg_gas = gas_prices.iter().sum::<f64>() / gas_prices.len() as f64;
                result.push((time, avg_gas));
            }
        }
        
        // Sắp xếp theo thời gian
        result.sort_by_key(|(time, _)| *time);
        
        result
    }

    /// Phát hiện các giao dịch liên quan đến địa chỉ tương tự (có thể là bot MEV/arbitrage)
    pub async fn detect_related_address_transactions(&self, similarity_threshold: f64) -> HashMap<String, HashSet<String>> {
        let txs_by_sender = self.group_transactions_by_sender().await;
        let mut related_addresses = HashMap::new();
        
        // Tìm các giao dịch có cùng recipient hoặc pattern tương tự
        for (sender1, txs1) in &txs_by_sender {
            let mut similar_addresses = HashSet::new();
            
            // Lấy danh sách người nhận từ giao dịch của sender1
            let recipients1: HashSet<String> = txs1.iter()
                .filter_map(|tx| tx.to_address.clone())
                .collect();
            
            for (sender2, txs2) in &txs_by_sender {
                if sender1 == sender2 {
                    continue;
                }
                
                // Lấy danh sách người nhận từ giao dịch của sender2
                let recipients2: HashSet<String> = txs2.iter()
                    .filter_map(|tx| tx.to_address.clone())
                    .collect();
                
                // Tính độ giống nhau giữa hai tập hợp
                if !recipients1.is_empty() && !recipients2.is_empty() {
                    let intersection = recipients1.intersection(&recipients2).count();
                    let union = recipients1.union(&recipients2).count();
                    let similarity = intersection as f64 / union as f64;
                    
                    if similarity >= similarity_threshold {
                        similar_addresses.insert(sender2.clone());
                    }
                }
            }
            
            if !similar_addresses.is_empty() {
                related_addresses.insert(sender1.clone(), similar_addresses);
            }
        }
        
        related_addresses
    }

    /// Phân tích token flow từ các giao dịch mempool để dự đoán xu hướng thị trường
    pub async fn analyze_token_flow(&self) -> HashMap<String, (f64, f64)> {
        let pending_txs = self.pending_transactions.read().await;
        let mut token_flow = HashMap::new();
        
        for tx in pending_txs.values() {
            // Chỉ xem xét giao dịch token
            if tx.transaction_type == TransactionType::TokenTransfer {
                // Phân tích input data để lấy thông tin token và số lượng
                if let Some(token_info) = self.extract_token_transfer_info(tx) {
                    let entry = token_flow.entry(token_info.token_address).or_insert((0.0, 0.0));
                    
                    // Cập nhật inflow/outflow dựa trên danh sách địa chỉ quan trọng
                    let is_watched_sender = self.is_watched_address(&tx.from_address).await;
                    let is_watched_recipient = if let Some(to) = &token_info.to_address {
                        self.is_watched_address(to).await
                    } else {
                        false
                    };
                    
                    if is_watched_sender && !is_watched_recipient {
                        // Outflow: từ địa chỉ quan trọng ra ngoài
                        entry.1 += token_info.amount;
                    } else if !is_watched_sender && is_watched_recipient {
                        // Inflow: từ bên ngoài đến địa chỉ quan trọng
                        entry.0 += token_info.amount;
                    }
                }
            }
        }
        
        token_flow
    }

    /// Trích xuất thông tin chuyển token từ input data
    fn extract_token_transfer_info(&self, tx: &MempoolTransaction) -> Option<TokenTransferInfo> {
        // Kiểm tra function signature cho transfer
        if let Some(signature) = &tx.function_signature {
            if signature == "0xa9059cbb" {
                // transfer(address,uint256)
                if tx.input_data.len() >= 138 {
                    // Parsing:
                    // 0xa9059cbb: 10 ký tự (function signature)
                    // address: 64 ký tự (32 bytes)
                    // amount: 64 ký tự (32 bytes)
                    let to_address = format!("0x{}", &tx.input_data[34..74]);
                    
                    // Parse số lượng token
                    if let Ok(amount_hex) = hex::decode(&tx.input_data[74..138]) {
                        // Chuyển đổi sang big integer
                        let mut amount_bytes = [0u8; 32];
                        for (i, byte) in amount_hex.iter().enumerate() {
                            if i < 32 {
                                amount_bytes[i] = *byte;
                            }
                        }
                        
                        // Chuyển đổi sang f64 (đơn giản hóa)
                        let amount = u128::from_be_bytes(amount_bytes[16..32].try_into().unwrap_or([0u8; 16])) as f64;
                        
                        return Some(TokenTransferInfo {
                            token_address: tx.to_address.clone().unwrap_or_default(),
                            to_address: Some(to_address),
                            amount,
                        });
                    }
                }
            } else if signature == "0x23b872dd" {
                // transferFrom(address,address,uint256)
                if tx.input_data.len() >= 202 {
                    // Parsing:
                    // 0x23b872dd: 10 ký tự (function signature)
                    // from_address: 64 ký tự (32 bytes)
                    // to_address: 64 ký tự (32 bytes)
                    // amount: 64 ký tự (32 bytes)
                    let from_address = format!("0x{}", &tx.input_data[34..74]);
                    let to_address = format!("0x{}", &tx.input_data[98..138]);
                    
                    // Parse số lượng token
                    if let Ok(amount_hex) = hex::decode(&tx.input_data[138..202]) {
                        // Chuyển đổi sang big integer
                        let mut amount_bytes = [0u8; 32];
                        for (i, byte) in amount_hex.iter().enumerate() {
                            if i < 32 {
                                amount_bytes[i] = *byte;
                            }
                        }
                        
                        // Chuyển đổi sang f64 (đơn giản hóa)
                        let amount = u128::from_be_bytes(amount_bytes[16..32].try_into().unwrap_or([0u8; 16])) as f64;
                        
                        return Some(TokenTransferInfo {
                            token_address: tx.to_address.clone().unwrap_or_default(),
                            to_address: Some(to_address),
                            amount,
                        });
                    }
                }
            }
        }
        
        None
    }

    /// Phân cụm giao dịch để phát hiện hành vi bất thường
    pub async fn cluster_transactions(&self) -> HashMap<TransactionCluster, Vec<MempoolTransaction>> {
        let pending_txs = self.pending_transactions.read().await;
        let mut clusters = HashMap::new();
        
        for tx in pending_txs.values() {
            let cluster = self.determine_transaction_cluster(tx);
            clusters.entry(cluster)
                .or_insert_with(Vec::new)
                .push(tx.clone());
        }
        
        clusters
    }

    /// Xác định cụm cho giao dịch
    fn determine_transaction_cluster(&self, tx: &MempoolTransaction) -> TransactionCluster {
        // Mặc định cluster
        let mut cluster = TransactionCluster::Normal;
        
        // Xác định dựa trên gas price
        if tx.gas_price > 3.0 * tx.gas_price {
            cluster = TransactionCluster::HighGas;
        }
        
        // Nếu là giao dịch token với giá trị lớn
        if tx.transaction_type == TransactionType::TokenTransfer && tx.value > WHALE_TRANSACTION_ETH {
            cluster = TransactionCluster::WhaleMovement;
        }
        
        // Nếu có liên quan đến contract
        match tx.transaction_type {
            TransactionType::ContractDeployment => cluster = TransactionCluster::ContractDeployment,
            TransactionType::AddLiquidity | TransactionType::RemoveLiquidity => 
                cluster = TransactionCluster::LiquidityChange,
            TransactionType::Swap => cluster = TransactionCluster::Swap,
            _ => {}
        }
        
        cluster
    }

    /// Phát hiện độ tương quan giữa các giao dịch để dự đoán xu hướng hệ sinh thái
    pub async fn detect_transaction_correlations(&self) -> Vec<(String, String, f64)> {
        let txs_by_sender = self.group_transactions_by_sender().await;
        let mut correlations = Vec::new();
        
        // Tạo danh sách các cặp contract được tương tác nhiều nhất
        let mut contract_pairs = HashMap::new();
        
        for txs in txs_by_sender.values() {
            // Theo dõi các contract được tương tác trong cùng một phiên
            let mut session_contracts = HashSet::new();
            
            for tx in txs {
                if let Some(to_address) = &tx.to_address {
                    if !to_address.is_empty() {
                        session_contracts.insert(to_address.clone());
                    }
                }
            }
            
            // Tạo tất cả các cặp có thể từ các contract trong phiên
            let contracts: Vec<String> = session_contracts.into_iter().collect();
            for i in 0..contracts.len() {
                for j in i+1..contracts.len() {
                    let pair = if contracts[i] < contracts[j] {
                        (contracts[i].clone(), contracts[j].clone())
                    } else {
                        (contracts[j].clone(), contracts[i].clone())
                    };
                    
                    *contract_pairs.entry(pair).or_insert(0) += 1;
                }
            }
        }
        
        // Tính độ tương quan dựa trên số lần xuất hiện cùng nhau
        let total_senders = txs_by_sender.len() as f64;
        if total_senders > 0.0 {
            for ((contract1, contract2), count) in contract_pairs {
                let correlation = count as f64 / total_senders;
                if correlation > 0.1 { // Chỉ lấy các cặp có tương quan > 10%
                    correlations.push((contract1, contract2, correlation));
                }
            }
        }
        
        // Sắp xếp theo độ tương quan giảm dần
        correlations.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        
        correlations
    }

    /// Tính tỷ lệ chấp nhận/từ chối giao dịch theo thời gian 
    pub async fn calculate_transaction_success_rate(&self) -> f64 {
        // Giả lập thông tin này vì chúng ta không theo dõi những giao dịch nào bị từ chối
        // Trong thực tế, cần theo dõi cả giao dịch chấp nhận và từ chối
        
        let pending_txs = self.pending_transactions.read().await;
        let total_txs = pending_txs.len();
        
        if total_txs == 0 {
            return 1.0; // 100% nếu không có giao dịch
        }
        
        // Ước tính tỷ lệ thành công dựa trên gas price
        let avg_gas_price = *self.average_gas_price.read().await;
        if avg_gas_price == 0.0 {
            return 0.5; // 50% nếu không có thông tin gas price
        }
        
        let mut likely_success = 0;
        for tx in pending_txs.values() {
            if tx.gas_price >= avg_gas_price * 0.8 { // Gas price ít nhất 80% của trung bình
                likely_success += 1;
            }
        }
        
        likely_success as f64 / total_txs as f64
    }

    /// Cung cấp dữ liệu giao dịch cho hệ thống dự đoán
    pub async fn export_transaction_data_for_prediction(&self) -> Vec<TransactionPredictionData> {
        let pending_txs = self.pending_transactions.read().await;
        let mut prediction_data = Vec::new();
        
        for tx in pending_txs.values() {
            // Convert to prediction-friendly data format
            let data = TransactionPredictionData {
                tx_hash: tx.tx_hash.clone(),
                gas_price: tx.gas_price,
                gas_limit: tx.gas_limit,
                value: tx.value,
                detected_at: tx.detected_at,
                transaction_type: tx.transaction_type.clone(),
                priority: tx.priority.clone(),
                from_address: tx.from_address.clone(),
                to_address: tx.to_address.clone(),
                input_data_length: tx.input_data.len(),
                function_signature: tx.function_signature.clone(),
                nonce: tx.nonce,
            };
            
            prediction_data.push(data);
        }
        
        prediction_data
    }

    /// Tìm các giao dịch có khả năng sẽ thất bại trong mempool
    /// 
    /// # Parameters
    /// * `include_nonce_gaps` - Có bao gồm giao dịch bị mắc kẹt do nonce gap
    /// * `include_low_gas` - Có bao gồm giao dịch có gas thấp hơn ngưỡng hiện tại
    /// * `include_long_pending` - Có bao gồm giao dịch chờ quá lâu
    /// * `min_wait_time_sec` - Thời gian chờ tối thiểu để coi là đã chờ quá lâu (giây)
    /// * `limit` - Số lượng kết quả tối đa
    /// 
    /// # Returns
    /// Danh sách các giao dịch có khả năng thất bại cùng lý do
    pub async fn find_potential_failing_transactions(
        &self,
        include_nonce_gaps: bool,
        include_low_gas: bool,
        include_long_pending: bool,
        min_wait_time_sec: u64,
        limit: usize,
    ) -> Vec<(MempoolTransaction, String)> {
        let mut failing_txs = Vec::new();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        
        // Phát hiện nonce gaps
        if include_nonce_gaps {
            let nonce_gaps = self.detect_nonce_gaps().await;
            
            for (address, gaps) in nonce_gaps {
                // Tìm tất cả giao dịch từ địa chỉ này
                let filter_options = TransactionFilterOptions {
                    from_addresses: Some(vec![address.clone()]),
                    sort_by: Some(TransactionSortCriteria::Nonce),
                    sort_ascending: Some(true),
                    ..Default::default()
                };
                
                let txs_from_address = self.get_filtered_transactions(filter_options, 100).await;
                
                // Phát hiện giao dịch bị kẹt sau nonce gap
                let gap_nonces: HashSet<u64> = gaps.into_iter().collect();
                
                for tx in txs_from_address {
                    // Nếu có nonce thấp hơn nonce gap, có thể bị kẹt do giao dịch khác
                    if gap_nonces.iter().any(|&gap_nonce| gap_nonce < tx.nonce) {
                        failing_txs.push((tx, "Nonce gap: Giao dịch có thể bị mắc kẹt do thiếu giao dịch với nonce thấp hơn".to_string()));
                    }
                }
            }
        }
        
        // Phát hiện gas price thấp
        if include_low_gas {
            let avg_gas_price = *self.average_gas_price.read().await;
            let min_viable_gas = avg_gas_price * 0.8; // 80% của giá gas trung bình
            
            let filter_options = TransactionFilterOptions {
                max_gas_price: Some(min_viable_gas),
                sort_by: Some(TransactionSortCriteria::GasPrice),
                sort_ascending: Some(true),
                ..Default::default()
            };
            
            let low_gas_txs = self.get_filtered_transactions(filter_options, 100).await;
            
            for tx in low_gas_txs {
                let ratio = tx.gas_price / avg_gas_price;
                let reason = format!(
                    "Gas price thấp: {:.1} gwei ({:.1}% của mức trung bình {:.1} gwei)", 
                    tx.gas_price, 
                    ratio * 100.0,
                    avg_gas_price
                );
                failing_txs.push((tx, reason));
            }
        }
        
        // Phát hiện giao dịch chờ quá lâu
        if include_long_pending {
            let filter_options = TransactionFilterOptions {
                sort_by: Some(TransactionSortCriteria::DetectedTime),
                sort_ascending: Some(true),
                ..Default::default()
            };
            
            let all_txs = self.get_filtered_transactions(filter_options, 1000).await;
            
            for tx in all_txs {
                let wait_time = now - tx.detected_at;
                if wait_time > min_wait_time_sec {
                    let wait_time_min = wait_time as f64 / 60.0;
                    let reason = format!(
                        "Chờ quá lâu: {:.1} phút", 
                        wait_time_min
                    );
                    failing_txs.push((tx, reason));
                }
            }
        }
        
        // Loại bỏ các bản ghi trùng lặp (giữ lý do chi tiết nhất)
        let mut unique_txs = HashMap::new();
        
        for (tx, reason) in failing_txs {
            let entry = unique_txs.entry(tx.tx_hash.clone()).or_insert((tx.clone(), String::new()));
            
            // Nếu lý do mới dài hơn hoặc chưa có lý do, cập nhật lý do
            if entry.1.is_empty() || reason.len() > entry.1.len() {
                *entry = (tx, reason);
            }
        }
        
        // Chuyển về Vec và giới hạn số lượng kết quả
        let mut result: Vec<(MempoolTransaction, String)> = unique_txs.into_values().collect();
        
        // Sắp xếp theo thời gian phát hiện (cũ -> mới)
        result.sort_by_key(|(tx, _)| tx.detected_at);
        
        result.into_iter().take(limit).collect()
    }

    /// Ước tính xác suất thành công của giao dịch
    /// 
    /// # Parameters
    /// * `transaction` - Giao dịch cần ước tính
    /// * `include_details` - Có bao gồm chi tiết lý do hay không
    /// 
    /// # Returns
    /// Xác suất thành công (0.0-1.0) và lý do (nếu include_details = true)
    pub async fn estimate_transaction_success_probability(
        &self,
        transaction: &MempoolTransaction,
        include_details: bool,
    ) -> (f64, Option<String>) {
        let mut probability = 1.0;
        let mut reasons = Vec::new();
        
        // 1. Kiểm tra gas price so với trung bình
        let avg_gas_price = *self.average_gas_price.read().await;
        let gas_ratio = transaction.gas_price / avg_gas_price;
        
        if gas_ratio < 0.5 {
            probability *= 0.2; // Cơ hội rất thấp nếu gas < 50% trung bình
            if include_details {
                reasons.push(format!("Gas price thấp ({:.1}% của trung bình)", gas_ratio * 100.0));
            }
        } else if gas_ratio < 0.8 {
            probability *= 0.7; // Cơ hội trung bình nếu gas < 80% trung bình
            if include_details {
                reasons.push(format!("Gas price hơi thấp ({:.1}% của trung bình)", gas_ratio * 100.0));
            }
        }
        
        // 2. Kiểm tra nonce gap
        let sender_txs = self.group_transactions_by_sender().await;
        if let Some(txs) = sender_txs.get(&transaction.from_address) {
            // Tìm nonce nhỏ nhất trong danh sách
            let min_nonce = txs.iter().map(|tx| tx.nonce).min().unwrap_or(0);
            
            // Nếu nonce của giao dịch lớn hơn nonce nhỏ nhất + 1, có thể có nonce gap
            if transaction.nonce > min_nonce + 1 {
                // Kiểm tra xem có đủ các giao dịch giữa min_nonce và nonce hiện tại không
                let mut has_all_nonces = true;
                for n in min_nonce + 1..transaction.nonce {
                    if !txs.iter().any(|tx| tx.nonce == n) {
                        has_all_nonces = false;
                        break;
                    }
                }
                
                if !has_all_nonces {
                    probability *= 0.3; // Khả năng thành công thấp nếu có nonce gap
                    if include_details {
                        reasons.push("Có nonce gap: thiếu giao dịch với nonce thấp hơn".to_string());
                    }
                }
            }
        }
        
        // 3. Kiểm tra thời gian chờ
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let wait_time = now - transaction.detected_at;
        
        if wait_time > 3600 {
            probability *= 0.5; // Giảm 50% xác suất nếu đã chờ > 1 giờ
            if include_details {
                reasons.push(format!("Đã chờ quá lâu ({:.1} phút)", wait_time as f64 / 60.0));
            }
        } else if wait_time > 1800 {
            probability *= 0.8; // Giảm 20% xác suất nếu đã chờ > 30 phút
            if include_details {
                reasons.push(format!("Đã chờ khá lâu ({:.1} phút)", wait_time as f64 / 60.0));
            }
        }
        
        // 4. Kiểm tra block space utilization
        let (_, estimated_wait_time) = self.analyze_blockspace_utilization().await;
        
        if estimated_wait_time > 300.0 {
            probability *= 0.9; // Giảm 10% xác suất nếu mạng đang nghẽn (> 5 phút)
            if include_details {
                reasons.push(format!("Mạng đang nghẽn (ước tính {:.1} phút chờ)", estimated_wait_time / 60.0));
            }
        }
        
        // Thông tin chi tiết
        let details = if include_details && !reasons.is_empty() {
            Some(reasons.join(", "))
        } else {
            None
        };
        
        (probability, details)
    }
}

/// Thông tin chuyển token được trích xuất từ giao dịch
#[derive(Debug, Clone)]
pub struct TokenTransferInfo {
    /// Địa chỉ token
    pub token_address: String,
    /// Địa chỉ nhận
    pub to_address: Option<String>,
    /// Số lượng token
    pub amount: f64,
}

/// Loại cụm giao dịch
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

/// Dữ liệu giao dịch cho hệ thống dự đoán
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
}

/// Hàm trợ giúp: Tạo bảng hash từ các khối dữ liệu
pub fn hash_data(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    format!("{:x}", result)
}

/// Làm sạch địa chỉ
pub fn normalize_address(address: &str) -> String {
    if address.len() >= 2 && &address[0..2] == "0x" {
        address.to_lowercase()
    } else {
        format!("0x{}", address.to_lowercase())
    }
}

/// Chuyển đổi bội số gas thành mô tả
pub fn gas_multiplier_to_string(multiplier: f64) -> String {
    if multiplier >= 2.0 {
        "Rất cao".to_string()
    } else if multiplier >= 1.5 {
        "Cao".to_string()
    } else if multiplier >= 1.0 {
        "Trung bình".to_string()
    } else if multiplier >= 0.8 {
        "Thấp".to_string()
    } else {
        "Rất thấp".to_string()
    }
}

/// Ước tính thời gian xác nhận
pub fn estimate_confirmation_time(tx_priority: &TransactionPriority, chain_id: u32) -> String {
    let base_time = match chain_id {
        1 => 15.0,  // ETH
        56 => 5.0,  // BSC
        137 => 5.0, // Polygon
        _ => 15.0,  // Mặc định (giây)
    };
    
    let multiplier = match tx_priority {
        TransactionPriority::VeryHigh => 1.0,
        TransactionPriority::High => 1.5,
        TransactionPriority::Medium => 2.0,
        TransactionPriority::Low => 3.0,
    };
    
    let estimated_time = base_time * multiplier;
    
    if estimated_time < 60.0 {
        format!("{:.0} giây", estimated_time)
    } else if estimated_time < 3600.0 {
        format!("{:.1} phút", estimated_time / 60.0)
    } else {
        format!("{:.1} giờ", estimated_time / 3600.0)
    }
}

/// Các tùy chọn lọc cho giao dịch trong mempool
#[derive(Debug, Clone, Default)]
pub struct TransactionFilterOptions {
    /// Lọc theo loại giao dịch
    pub transaction_types: Option<Vec<TransactionType>>,
    /// Lọc theo mức độ ưu tiên
    pub priorities: Option<Vec<TransactionPriority>>,
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
#[derive(Debug, Clone, PartialEq)]
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
