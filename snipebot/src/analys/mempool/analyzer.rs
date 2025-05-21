use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use crate::analys::mempool::types::*;
use crate::analys::mempool::detection;
use crate::analys::mempool::utils;

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
        
        // Add Liquidity
        signatures.insert(
            "0xe8e33700".to_string(),
            ("addLiquidity(address,address,uint256,uint256,uint256,uint256,address,uint256)".to_string(), 
             TransactionType::AddLiquidity),
        );
        
        // Add Liquidity ETH
        signatures.insert(
            "0xf305d719".to_string(),
            ("addLiquidityETH(address,uint256,uint256,uint256,address,uint256)".to_string(), 
             TransactionType::AddLiquidity),
        );
        
        // Remove Liquidity
        signatures.insert(
            "0xbaa2abde".to_string(),
            ("removeLiquidity(address,address,uint256,uint256,uint256,address,uint256)".to_string(), 
             TransactionType::RemoveLiquidity),
        );
        
        // Remove Liquidity ETH
        signatures.insert(
            "0x02751cec".to_string(),
            ("removeLiquidityETH(address,uint256,uint256,uint256,address,uint256)".to_string(), 
             TransactionType::RemoveLiquidity),
        );
        
        // Swap Exact Tokens For Tokens
        signatures.insert(
            "0x38ed1739".to_string(),
            ("swapExactTokensForTokens(uint256,uint256,address[],address,uint256)".to_string(), 
             TransactionType::Swap),
        );
        
        // Swap Exact ETH For Tokens
        signatures.insert(
            "0x7ff36ab5".to_string(),
            ("swapExactETHForTokens(uint256,address[],address,uint256)".to_string(), 
             TransactionType::Swap),
        );
        
        // Swap Exact Tokens For ETH
        signatures.insert(
            "0x18cbafe5".to_string(),
            ("swapExactTokensForETH(uint256,uint256,address[],address,uint256)".to_string(), 
             TransactionType::Swap),
        );
        
        // Mint NFT
        signatures.insert(
            "0x6a627842".to_string(),
            ("mint(address)".to_string(), TransactionType::NftMint),
        );
        
        signatures.insert(
            "0x40c10f19".to_string(),
            ("mint(address,uint256)".to_string(), TransactionType::NftMint),
        );
        
        signatures
    }

    /// Thêm giao dịch mới vào danh sách theo dõi
    pub async fn add_transaction(&self, transaction: MempoolTransaction) {
        // Phân tích giao dịch trước khi thêm
        self.analyze_transaction(&transaction).await;
        
        // Cập nhật gas price trung bình
        self.update_average_gas_price(&transaction).await;
        
        // Thêm vào danh sách pending
        let mut pending_txs = self.pending_transactions.write().await;
        pending_txs.insert(transaction.tx_hash.clone(), transaction);
    }
    
    /// Cập nhật gas price trung bình
    async fn update_average_gas_price(&self, transaction: &MempoolTransaction) {
        let mut avg_gas = self.average_gas_price.write().await;
        
        if *avg_gas == 0.0 {
            *avg_gas = transaction.gas_price;
        } else {
            // Cập nhật trung bình theo công thức EMA (Exponential Moving Average)
            const ALPHA: f64 = 0.1; // Hệ số làm mịn
            *avg_gas = (1.0 - ALPHA) * *avg_gas + ALPHA * transaction.gas_price;
        }
    }
    
    /// Phân tích giao dịch
    async fn analyze_transaction(&self, transaction: &MempoolTransaction) {
        // Phân tích theo loại giao dịch
        match transaction.transaction_type {
            TransactionType::TokenCreation => {
                self.process_potential_token_creation(transaction).await;
            },
            TransactionType::AddLiquidity => {
                self.process_liquidity_event(transaction, true).await;
            },
            TransactionType::RemoveLiquidity => {
                self.process_liquidity_event(transaction, false).await;
            },
            _ => {}
        }
        
        // Phát hiện và cảnh báo các giao dịch whale
        if transaction.value >= WHALE_TRANSACTION_ETH {
            self.create_whale_alert(transaction).await;
        }
        
        // Phát hiện các pattern đáng ngờ
        self.detect_suspicious_patterns(transaction).await;
    }
    
    /// Phát hiện loại giao dịch từ input data
    fn detect_transaction_type(&self, transaction: &MempoolTransaction) -> TransactionType {
        // Nếu không có input data, đây là chuyển native token
        if transaction.input_data == "0x" || transaction.input_data.len() <= 10 {
            return TransactionType::NativeTransfer;
        }
        
        // Lấy function signature (4 bytes đầu tiên của input data)
        let signature = &transaction.input_data[..10].to_lowercase();
        
        // Kiểm tra trong danh sách signature đã biết
        if let Some((_, tx_type)) = self.function_signatures.get(signature) {
            return tx_type.clone();
        }
        
        // Heuristics để phát hiện loại giao dịch
        if transaction.to_address.is_none() {
            // Không có địa chỉ nhận => triển khai contract
            return TransactionType::ContractDeployment;
        }
        
        // Mặc định là tương tác với contract
        TransactionType::ContractInteraction
    }
    
    /// Xử lý giao dịch có thể là tạo token mới
    async fn process_potential_token_creation(&self, transaction: &MempoolTransaction) {
        // Lấy địa chỉ contract mới tạo (nếu có)
        let contract_address = match transaction.to_address {
            None => {
                // Tạm tính địa chỉ contract dựa trên from và nonce
                let creator = utils::normalize_address(&transaction.from_address);
                let nonce = transaction.nonce;
                format!("0x{:x}", nonce) // Đây chỉ là placeholder, cần tính chính xác hơn
            },
            Some(_) => return, // Có địa chỉ nhận => không phải tạo contract mới
        };
        
        // Thêm vào danh sách token mới
        let new_token = NewTokenInfo {
            token_address: contract_address.clone(),
            chain_id: self.chain_id,
            creation_tx: transaction.tx_hash.clone(),
            creator_address: transaction.from_address.clone(),
            detected_at: transaction.detected_at,
            liquidity_added: false,
            liquidity_tx: None,
            liquidity_value: None,
            dex_type: None,
        };
        
        let mut new_tokens = self.new_tokens.write().await;
        new_tokens.insert(contract_address.clone(), new_token);
        
        // Tạo cảnh báo token mới
        let alert = MempoolAlert {
            alert_type: MempoolAlertType::NewToken,
            severity: AlertSeverity::Info,
            detected_at: transaction.detected_at,
            description: format!("Phát hiện token mới được tạo: {}", contract_address),
            related_transactions: vec![transaction.tx_hash.clone()],
            related_token: Some(contract_address),
            related_addresses: vec![transaction.from_address.clone()],
        };
        
        let mut alerts = self.alerts.write().await;
        alerts.push(alert);
    }
    
    /// Xử lý sự kiện thêm/rút thanh khoản
    async fn process_liquidity_event(&self, transaction: &MempoolTransaction, is_adding: bool) {
        // Trích xuất thông tin token từ input data
        let token_address = match self.extract_token_from_liquidity_event(transaction) {
            Some(addr) => addr,
            None => return,
        };
        
        // Cập nhật token nếu đã trong danh sách theo dõi
        let mut new_tokens = self.new_tokens.write().await;
        if let Some(token) = new_tokens.get_mut(&token_address) {
            if is_adding && !token.liquidity_added {
                token.liquidity_added = true;
                token.liquidity_tx = Some(transaction.tx_hash.clone());
                // TODO: Tính liquidity_value từ input data
                token.liquidity_value = Some(transaction.value * 2.0); // Giá trị ước tính
                
                // Xác định DEX từ địa chỉ nhận
                if let Some(to) = &transaction.to_address {
                    token.dex_type = Some(self.detect_dex_from_router(to));
                }
            }
        }
        
        // Tạo cảnh báo sự kiện thanh khoản
        let alert_type = if is_adding {
            MempoolAlertType::LiquidityAdded
        } else {
            MempoolAlertType::LiquidityRemoved
        };
        
        let description = if is_adding {
            format!("Thêm thanh khoản cho token: {}", token_address)
        } else {
            format!("Rút thanh khoản từ token: {}", token_address)
        };
        
        let severity = if is_adding {
            AlertSeverity::Info
        } else {
            AlertSeverity::Medium // Rút thanh khoản có thể là dấu hiệu rug pull
        };
        
        let alert = MempoolAlert {
            alert_type,
            severity,
            detected_at: transaction.detected_at,
            description,
            related_transactions: vec![transaction.tx_hash.clone()],
            related_token: Some(token_address),
            related_addresses: vec![transaction.from_address.clone()],
        };
        
        let mut alerts = self.alerts.write().await;
        alerts.push(alert);
    }
    
    /// Tạo cảnh báo giao dịch whale
    async fn create_whale_alert(&self, transaction: &MempoolTransaction) {
        let alert = MempoolAlert {
            alert_type: MempoolAlertType::WhaleTransaction,
            severity: AlertSeverity::Medium,
            detected_at: transaction.detected_at,
            description: format!(
                "Giao dịch whale phát hiện: {} ETH/BNB từ {}",
                transaction.value, transaction.from_address
            ),
            related_transactions: vec![transaction.tx_hash.clone()],
            related_token: None,
            related_addresses: vec![transaction.from_address.clone()],
        };
        
        let mut alerts = self.alerts.write().await;
        alerts.push(alert);
        
        // Tự động thêm địa chỉ whale vào danh sách theo dõi
        self.add_watched_address(&transaction.from_address).await;
    }
    
    /// Phát hiện các pattern đáng ngờ
    async fn detect_suspicious_patterns(&self, transaction: &MempoolTransaction) {
        let pending_txs = self.pending_transactions.read().await;
        
        // Phát hiện front-running
        if let Some(pattern) = detection::detect_front_running(transaction, &pending_txs, FRONT_RUNNING_TIME_WINDOW_MS).await {
            self.create_suspicious_pattern_alert(transaction, pattern).await;
        }
        
        // Phát hiện sandwich attack
        if let Some(pattern) = detection::detect_sandwich_attack(transaction, &pending_txs, MAX_SANDWICH_TIME_WINDOW_MS).await {
            self.create_suspicious_pattern_alert(transaction, pattern).await;
        }
        
        // Phát hiện high frequency trading
        if let Some(pattern) = detection::detect_high_frequency_trading(transaction, &pending_txs).await {
            self.create_suspicious_pattern_alert(transaction, pattern).await;
        }
    }
    
    /// Tạo cảnh báo cho pattern đáng ngờ
    async fn create_suspicious_pattern_alert(&self, transaction: &MempoolTransaction, pattern: SuspiciousPattern) {
        let (description, severity) = match pattern {
            SuspiciousPattern::FrontRunning => (
                format!(
                    "Phát hiện front-running: {} với gas price cao {}",
                    transaction.tx_hash, transaction.gas_price
                ),
                AlertSeverity::High
            ),
            SuspiciousPattern::SandwichAttack => (
                format!(
                    "Phát hiện sandwich attack: {} đang kẹp một giao dịch swap lớn",
                    transaction.tx_hash
                ),
                AlertSeverity::High
            ),
            SuspiciousPattern::MevBot => (
                format!(
                    "Phát hiện MEV bot: {} từ địa chỉ {}",
                    transaction.tx_hash, transaction.from_address
                ),
                AlertSeverity::Medium
            ),
            SuspiciousPattern::SuddenLiquidityRemoval => (
                format!(
                    "CẢNH BÁO: Rút thanh khoản đột ngột trong {}",
                    transaction.tx_hash
                ),
                AlertSeverity::Critical
            ),
            SuspiciousPattern::WhaleMovement => (
                format!(
                    "Whale đang di chuyển tiền: {} ETH/BNB từ {}",
                    transaction.value, transaction.from_address
                ),
                AlertSeverity::Medium
            ),
            SuspiciousPattern::HighFrequencyTrading => (
                format!(
                    "Giao dịch tần suất cao từ {}",
                    transaction.from_address
                ),
                AlertSeverity::Low
            ),
            SuspiciousPattern::TokenLiquidityPattern => (
                format!(
                    "Token + LP được tạo cùng lúc: {} (có thể là scam)",
                    transaction.tx_hash
                ),
                AlertSeverity::High
            ),
        };
        
        let alert = MempoolAlert {
            alert_type: MempoolAlertType::SuspiciousTransaction(pattern),
            severity,
            detected_at: transaction.detected_at,
            description,
            related_transactions: vec![transaction.tx_hash.clone()],
            related_token: None, // TODO: Xác định token liên quan nếu có
            related_addresses: vec![transaction.from_address.clone()],
        };
        
        let mut alerts = self.alerts.write().await;
        alerts.push(alert);
    }
    
    /// Xóa các giao dịch cũ khỏi mempool
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
    
    /// Lấy danh sách cảnh báo hiện tại
    pub async fn get_alerts(&self, min_severity: Option<AlertSeverity>) -> Vec<MempoolAlert> {
        let alerts = self.alerts.read().await;
        
        match min_severity {
            Some(min_sev) => alerts.iter()
                .filter(|alert| alert.severity >= min_sev)
                .cloned()
                .collect(),
            None => alerts.clone(),
        }
    }
    
    /// Lấy danh sách token mới phát hiện
    pub async fn get_new_tokens(&self) -> Vec<NewTokenInfo> {
        let new_tokens = self.new_tokens.read().await;
        new_tokens.values().cloned().collect()
    }
    
    /// Lấy số lượng giao dịch đang theo dõi
    pub async fn get_pending_transaction_count(&self) -> usize {
        let pending_txs = self.pending_transactions.read().await;
        pending_txs.len()
    }
    
    /// Thêm địa chỉ vào danh sách theo dõi
    pub async fn add_watched_address(&self, address: &str) {
        let normalized = utils::normalize_address(address);
        let mut watched = self.watched_addresses.write().await;
        watched.insert(normalized);
    }
    
    /// Xóa địa chỉ khỏi danh sách theo dõi
    pub async fn remove_watched_address(&self, address: &str) {
        let normalized = utils::normalize_address(address);
        let mut watched = self.watched_addresses.write().await;
        watched.remove(&normalized);
    }
    
    /// Kiểm tra địa chỉ có trong danh sách theo dõi không
    pub async fn is_watched_address(&self, address: &str) -> bool {
        let normalized = utils::normalize_address(address);
        let watched = self.watched_addresses.read().await;
        watched.contains(&normalized)
    }
    
    /// Lấy danh sách giao dịch đang theo dõi
    pub async fn get_pending_transactions(&self, limit: usize) -> Vec<MempoolTransaction> {
        let pending_txs = self.pending_transactions.read().await;
        
        pending_txs.values()
            .take(limit)
            .cloned()
            .collect()
    }
    
    /// Trích xuất thông tin token từ giao dịch add/remove liquidity
    fn extract_token_from_liquidity_event(&self, transaction: &MempoolTransaction) -> Option<String> {
        // TODO: Implement token extraction from input data
        transaction.to_address.clone()
    }
    
    /// Phát hiện DEX từ router address
    fn detect_dex_from_router(&self, router_address: &str) -> String {
        let router = router_address.to_lowercase();
        
        // Các router phổ biến
        match router.as_str() {
            "0x10ed43c718714eb63d5aa57b78b54704e256024e" => "PancakeSwap".to_string(),
            "0x7a250d5630b4cf539739df2c5dacb4c659f2488d" => "Uniswap".to_string(),
            "0x1b02da8cb0d097eb8d57a175b88c7d8b47997506" => "SushiSwap".to_string(),
            _ => "Unknown DEX".to_string(),
        }
    }
    
    /// Lấy gas price trung bình hiện tại
    pub async fn get_average_gas_price(&self) -> f64 {
        let avg_gas = self.average_gas_price.read().await;
        *avg_gas
    }
} 