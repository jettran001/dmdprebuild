use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use crate::analys::mempool::types::*;
use crate::analys::mempool::detection;
use crate::analys::mempool::filter;
use crate::analys::mempool::priority;
use super::utils;
use crate::types::Transaction;
use crate::tradelogic::mev_logic::types::{TraderBehaviorType, TraderExpertiseLevel};

/// Thời gian tối đa lưu trữ giao dịch trong mempool (giây)
const MAX_PENDING_TX_AGE_SECONDS: u64 = 3600; // 1 giờ

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
    /// Cache cho token price để giảm số lượng truy vấn
    token_price_cache: RwLock<HashMap<String, (f64, Instant)>>,
    /// Thời gian cache token price (seconds)
    token_price_cache_ttl: u64,
    /// Thời gian tối đa lưu trữ giao dịch (seconds)
    transaction_ttl: u64,
    /// Số lượng cảnh báo tối đa lưu trữ
    max_alerts: usize,
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
            token_price_cache: RwLock::new(HashMap::new()),
            token_price_cache_ttl: 60, // 60 giây mặc định
            transaction_ttl: 300, // 5 phút mặc định
            max_alerts: 1000, // Giới hạn 1000 cảnh báo
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

    /// Thêm nhiều giao dịch mới vào danh sách theo dõi (batch processing)
    pub async fn add_transactions(&self, transactions: Vec<MempoolTransaction>) {
        if transactions.is_empty() {
            return;
        }
        
        // Cleanup dữ liệu cũ trước khi thêm mới
        self.cleanup_old_data().await;
        
        // Batch processing cho các phân tích transaction
        let mut alerts = Vec::new();
        let mut new_tokens = HashMap::new();
        let mut sum_gas_price = 0.0;
        let mut tx_count = 0;
        
        // Phân tích từng giao dịch và thu thập kết quả
        for transaction in &transactions {
            // Phân tích theo loại giao dịch
            match transaction.transaction_type {
                TransactionType::TokenCreation => {
                    if let Some(token_info) = self.analyze_potential_token_creation(transaction).await {
                        new_tokens.insert(token_info.token_address.clone(), token_info);
                    }
                },
                TransactionType::AddLiquidity => {
                    if let Some(alert) = self.analyze_liquidity_event(transaction, true).await {
                        alerts.push(alert);
                    }
                },
                TransactionType::RemoveLiquidity => {
                    if let Some(alert) = self.analyze_liquidity_event(transaction, false).await {
                        alerts.push(alert);
                    }
                },
                _ => {}
            }
            
            // Phát hiện và cảnh báo các giao dịch whale
            if transaction.value >= WHALE_TRANSACTION_ETH {
                alerts.push(self.create_whale_alert_without_lock(transaction));
            }
            
            // Phát hiện các pattern đáng ngờ
            if let Some(pattern) = self.detect_suspicious_pattern(transaction).await {
                alerts.push(self.create_suspicious_pattern_alert_without_lock(transaction, pattern));
            }
            
            // Thu thập gas price cho tính trung bình
            sum_gas_price += transaction.gas_price;
            tx_count += 1;
        }
        
        // Cập nhật transaction pool với một write lock duy nhất
        {
            let mut pending_txs = self.pending_transactions.write().await;
            for transaction in transactions {
                pending_txs.insert(transaction.tx_hash.clone(), transaction);
            }
        }
        
        // Cập nhật new tokens với một write lock duy nhất
        if !new_tokens.is_empty() {
            let mut tokens = self.new_tokens.write().await;
            tokens.extend(new_tokens);
        }
        
        // Cập nhật alerts với một write lock duy nhất
        for alert in alerts {
            self.add_alert(alert).await;
        }
        
        // Cập nhật gas price trung bình
        if tx_count > 0 {
            let mut avg_gas = self.average_gas_price.write().await;
            *avg_gas = sum_gas_price / tx_count as f64;
        }
        
        // Cập nhật thời gian cập nhật cuối
        *self.last_update.write().await = Instant::now();
    }
    
    /// Thêm giao dịch mới vào danh sách theo dõi
    pub async fn add_transaction(&self, transaction: MempoolTransaction) {
        // Sử dụng phương thức batch cho một giao dịch đơn lẻ
        self.add_transactions(vec![transaction]).await;
    }
    
    /// Phân tích token creation mà không ghi vào new_tokens
    async fn analyze_potential_token_creation(&self, transaction: &MempoolTransaction) -> Option<NewTokenInfo> {
        // Kiểm tra nếu đây là giao dịch tạo contract
        if transaction.to_address.is_none() {
            // Trong giao dịch contract creation, không có to_address
            // và contract_address chỉ có sau khi giao dịch được confirmed
            // Nhưng vì chúng ta đang xử lý mempool, ta có thể không có địa chỉ contract
            
            // Thông tin token mới
            let token_info = NewTokenInfo {
                chain_id: self.chain_id,
                token_address: "pending_contract".to_string(), // Địa chỉ sẽ chỉ có khi confirmed
                creator_address: transaction.from_address.clone(),
                creation_tx: transaction.tx_hash.clone(),
                detected_at: transaction.detected_at,
                liquidity_added: false,
                liquidity_tx: None,
                liquidity_value: None,
                dex_type: None,
            };
            
            return Some(token_info);
        }
        
        None
    }
    
    /// Phân tích token creation mà không ghi vào new_tokens
    async fn analyze_liquidity_event(&self, transaction: &MempoolTransaction, _is_adding: bool) -> Option<MempoolAlert> {
        let token_address = match self.extract_token_from_liquidity_event(transaction) {
            Some(addr) => addr,
            None => return None,
        };
        
        // Lấy giá token từ cache hoặc API
        let _token_price = self.get_token_price_with_cache(&token_address).await.unwrap_or(0.0);
        
        // Check for liquidity removal
        if transaction.transaction_type == TransactionType::RemoveLiquidity {
            if let Some(token_address) = transaction.to_token.as_ref().map(|t| t.address.clone()) {
                let token_price = transaction.to_token.as_ref().map(|t| t.price_usd).unwrap_or(0.0);
                
                // Alert về việc rút thanh khoản đột ngột
                return Some(MempoolAlert {
                    alert_type: MempoolAlertType::LiquidityRemoved,
                    severity: AlertSeverity::Medium,
                    detected_at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_else(|_| {
                            warn!("Failed to get system time, using default value");
                            std::time::Duration::from_secs(0)
                        })
                        .as_secs(),
                    description: format!(
                        "Large liquidity removal detected for token {} with value approximately ${:.2}",
                        token_address, transaction.value * token_price
                    ),
                    related_transactions: vec![transaction.tx_hash.clone()],
                    related_token: Some(token_address),
                    related_addresses: vec![transaction.from_address.clone()],
                });
            }
        }
        
        None
    }
    
    /// Thêm cảnh báo mới vào danh sách
    async fn add_alert(&self, alert: MempoolAlert) {
        let mut alerts = self.alerts.write().await;
        alerts.push(alert);
        
        // Giới hạn số lượng cảnh báo
        if alerts.len() > self.max_alerts {
            alerts.remove(0); // Xóa cảnh báo cũ nhất
        }
    }
    
    /// Phát hiện các pattern đáng ngờ
    async fn detect_suspicious_pattern(&self, transaction: &MempoolTransaction) -> Option<SuspiciousPattern> {
        // Phát hiện front-running
        if detection::is_potential_front_running(transaction) {
            return Some(SuspiciousPattern::FrontRunning);
        }
        
        // Phát hiện sandwich attack
        if detection::is_potential_sandwich_attack(transaction) {
            return Some(SuspiciousPattern::SandwichAttack);
        }
        
        // Phát hiện MEV bot
        if detection::is_potential_mev_bot(&transaction.from_address) {
            return Some(SuspiciousPattern::MevBot);
        }
        
        // Phát hiện rút thanh khoản đột ngột
        if transaction.transaction_type == TransactionType::RemoveLiquidity && 
           transaction.value > LARGE_TRANSACTION_ETH {
            return Some(SuspiciousPattern::SuddenLiquidityRemoval);
        }
        
        None
    }
    
    /// Tạo cảnh báo cho giao dịch đáng ngờ
    fn create_suspicious_pattern_alert_without_lock(&self, transaction: &MempoolTransaction, pattern: SuspiciousPattern) -> MempoolAlert {
        let severity = match pattern {
            SuspiciousPattern::FrontRunning => AlertSeverity::Medium,
            SuspiciousPattern::SandwichAttack => AlertSeverity::High,
            SuspiciousPattern::MevBot => AlertSeverity::Low,
            SuspiciousPattern::SuddenLiquidityRemoval => AlertSeverity::High,
            SuspiciousPattern::WhaleMovement => AlertSeverity::Medium,
            SuspiciousPattern::HighFrequencyTrading => AlertSeverity::Low,
            SuspiciousPattern::TokenLiquidityPattern => AlertSeverity::Medium,
        };
        
        let description = match pattern {
            SuspiciousPattern::FrontRunning => format!(
                "Phát hiện giao dịch front-running từ địa chỉ {} với gas price {} gwei",
                transaction.from_address, transaction.gas_price
            ),
            SuspiciousPattern::SandwichAttack => format!(
                "Phát hiện sandwich attack từ địa chỉ {} với gas price {} gwei",
                transaction.from_address, transaction.gas_price
            ),
            SuspiciousPattern::MevBot => format!(
                "Phát hiện MEV bot tại địa chỉ {}", 
                transaction.from_address
            ),
            SuspiciousPattern::SuddenLiquidityRemoval => format!(
                "Phát hiện rút thanh khoản đột ngột trị giá {} ETH từ địa chỉ {}",
                transaction.value, transaction.from_address
            ),
            SuspiciousPattern::WhaleMovement => format!(
                "Phát hiện giao dịch whale từ địa chỉ {} với giá trị {} ETH",
                transaction.from_address, transaction.value
            ),
            SuspiciousPattern::HighFrequencyTrading => format!(
                "Phát hiện giao dịch tần suất cao từ địa chỉ {}",
                transaction.from_address
            ),
            SuspiciousPattern::TokenLiquidityPattern => format!(
                "Phát hiện mẫu tạo token và thêm thanh khoản đáng ngờ từ địa chỉ {}",
                transaction.from_address
            ),
        };
        
        MempoolAlert {
            alert_type: MempoolAlertType::SuspiciousTransaction(pattern),
            severity,
            detected_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| {
                    warn!("Failed to get system time, using default value");
                    std::time::Duration::from_secs(0)
                })
                .as_secs(),
            description,
            related_transactions: vec![transaction.tx_hash.clone()],
            related_token: transaction.to_token.as_ref().map(|t| t.address.clone()),
            related_addresses: vec![transaction.from_address.clone()],
        }
    }
    
    /// Tạo cảnh báo cho giao dịch whale
    fn create_whale_alert_without_lock(&self, transaction: &MempoolTransaction) -> MempoolAlert {
        let severity = if transaction.value >= VERY_LARGE_TRANSACTION_ETH {
            AlertSeverity::High
        } else {
            AlertSeverity::Medium
        };
        
        let description = format!(
            "Giao dịch whale phát hiện: {} ETH từ địa chỉ {}",
            transaction.value, transaction.from_address
        );
        
        MempoolAlert {
            alert_type: MempoolAlertType::WhaleTransaction,
            severity,
            detected_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| {
                    warn!("Failed to get system time, using default value");
                    std::time::Duration::from_secs(0)
                })
                .as_secs(),
            description,
            related_transactions: vec![transaction.tx_hash.clone()],
            related_token: None,
            related_addresses: vec![transaction.from_address.clone()],
        }
    }
    
    /// Lấy giá token từ cache hoặc API
    async fn get_token_price_with_cache(&self, token_address: &str) -> Option<f64> {
        // Kiểm tra cache trước
        {
            let cache = self.token_price_cache.read().await;
            if let Some((price, timestamp)) = cache.get(token_address) {
                // Nếu cache còn hạn, trả về giá đã cache
                if timestamp.elapsed().as_secs() < self.token_price_cache_ttl {
                    return Some(*price);
                }
            }
        }
        
        // Nếu không có trong cache hoặc cache hết hạn, truy vấn API
        if let Some(price) = self.get_token_price(token_address).await {
            // Cập nhật cache
            let mut cache = self.token_price_cache.write().await;
            cache.insert(token_address.to_string(), (price, Instant::now()));
            return Some(price);
        }
        
        None
    }

    /// Xóa các giao dịch cũ khỏi mempool và làm sạch cache
    pub async fn cleanup_old_data(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();
        
        // Xóa các giao dịch cũ
        {
            let mut pending_txs = self.pending_transactions.write().await;
            pending_txs.retain(|_, tx| now - tx.detected_at < MAX_PENDING_TX_AGE_SECONDS);
        }
        
        // Làm sạch token price cache hết hạn
        {
            let mut cache = self.token_price_cache.write().await;
            cache.retain(|_, (_, timestamp)| timestamp.elapsed().as_secs() < self.token_price_cache_ttl * 2);
        }
    }

    /// Lấy tất cả giao dịch đang chờ xử lý
    pub async fn get_all_pending_transactions(&self) -> Vec<MempoolTransaction> {
        let pending_txs = self.pending_transactions.read().await;
        pending_txs.values().cloned().collect()
    }
    
    /// Đặt TTL cho cache giá token
    pub fn set_token_price_cache_ttl(&mut self, ttl_seconds: u64) {
        self.token_price_cache_ttl = ttl_seconds;
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
    
    /// Lấy gas price trung bình hiện tại
    pub async fn get_average_gas_price(&self) -> f64 {
        let avg_gas = self.average_gas_price.read().await;
        *avg_gas
    }

    /// Thêm phương thức giả lập để lấy giá token cho mục đích ước tính trong process_liquidity_event
    async fn get_token_price(&self, _token_address: &str) -> Option<f64> {
        // Trong triển khai thực tế, sẽ truy vấn API hoặc oracle để lấy giá token
        // Đây chỉ là phương thức giả lập
        None
    }

    /// Tạo thông tin token mới
    fn create_base_token_info(&self, token_address: String, chain_id: u32) -> TokenInfo {
        TokenInfo {
            address: token_address,
            symbol: None,
            amount: 0.0,
            price: 0.0,
            liquidity_usd: 0.0,
            volume_24h: 0.0,
            holders: 0,
            last_updated: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| {
                    warn!("Failed to get system time, using default value");
                    std::time::Duration::from_secs(0)
                })
                .as_secs(),
            risk_score: 50.0, // Điểm rủi ro mặc định: 50/100
            warnings: Vec::new(),
            buy_tax: 0.0,
            sell_tax: 0.0,
            max_tx_amount: None,
            max_wallet_amount: None,
            market_cap: 0.0,
            price_change_24h: 0.0,
            price_change_7d: 0.0,
            contract_verified: false,
            contract_creation_date: None,
            contract_creator: None,
            is_base_token: false,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| {
                    warn!("Failed to get system time, using default value");
                    std::time::Duration::from_secs(0)
                })
                .as_secs(),
            chain_id,
            is_honeypot: false,
            is_mintable: false,
            is_proxy: false,
            has_blacklist: false,
            name: "Unknown Token".to_string(),
            decimals: 18,
            total_supply: 0,
            price_usd: 0.0,
        }
    }

    /// Lấy danh sách cảnh báo từ mempool
    ///
    /// # Arguments
    /// * `min_severity` - Filter các cảnh báo theo mức độ nghiêm trọng
    ///
    /// # Returns
    /// * `Vec<MempoolAlert>` - Danh sách các cảnh báo
    pub async fn get_alerts(&self, min_severity: Option<AlertSeverity>) -> Vec<MempoolAlert> {
        let alerts = self.alerts.read().await;
        
        if let Some(severity) = min_severity {
            // Filter alerts theo mức độ nghiêm trọng
            alerts.iter()
                .filter(|alert| alert.severity >= severity)
                .cloned()
                .collect()
        } else {
            // Return all alerts
            alerts.clone()
        }
    }

    /// Kiểm tra một giao dịch có trong mempool hay không
    ///
    /// # Arguments
    /// * `tx_hash` - Hash của giao dịch cần kiểm tra
    ///
    /// # Returns
    /// * `Result<bool>` - true nếu giao dịch đang trong mempool, false nếu không
    pub async fn is_transaction_in_mempool(&self, tx_hash: &str) -> Result<bool, anyhow::Error> {
        // Check if transaction exists in mempool
        let transactions = self.pending_transactions.read().await;
        let result = transactions.iter().any(|tx| tx.hash == tx_hash);
        
        Ok(result)
    }

    /// Phát hiện mẫu sandwich từ tập các swap
    pub fn detect_sandwich_pattern(&self, swaps: &[&MempoolTransaction]) -> bool {
        // Cần ít nhất 3 swap để tạo thành sandwich
        if swaps.len() < 3 {
            return false;
        }
        
        // Kiểm tra các mẫu sandwich
        // Mẫu điển hình: Swap A->B, sau đó B->A (với giá trị lớn), rồi lại A->B
        
        for i in 0..swaps.len() - 2 {
            let first = swaps[i];
            let second = swaps[i + 1];
            let third = swaps[i + 2];
            
            // Kiểm tra nếu cùng token nhưng chiều ngược nhau
            if let (Some(first_from), Some(first_to)) = (&first.from_token, &first.to_token) {
                if let (Some(second_from), Some(second_to)) = (&second.from_token, &second.to_token) {
                    if let (Some(third_from), Some(third_to)) = (&third.from_token, &third.to_token) {
                        // Kiểm tra mẫu: A->B, B->A, A->B
                        if first_from.address == second_to.address && 
                           first_to.address == second_from.address &&
                           third_from.address == first_from.address &&
                           third_to.address == first_to.address {
                            // Và kiểm tra giá trị swap giữa lớn hơn
                            if second.value_usd > first.value_usd && second.value_usd > third.value_usd {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        
        false
    }

    /// Phát hiện mẫu front-running từ tập các swap
    pub fn detect_front_running_pattern(&self, swaps: &[&MempoolTransaction]) -> bool {
        // Cần ít nhất 2 swap để tạo thành front-running
        if swaps.len() < 2 {
            return false;
        }
        
        // Sắp xếp theo gas price, xử lý NaN một cách an toàn
        let mut sorted_swaps = swaps.to_vec();
        sorted_swaps.sort_by(|a, b| {
            // Lấy gas price, mặc định là 0.0 nếu None
            let a_gas = a.gas_price;
            let b_gas = b.gas_price;
            
            // Xử lý NaN một cách đặc biệt để tránh bug khi so sánh
            match (a_gas.is_nan(), b_gas.is_nan()) {
                (true, true) => {
                    tracing::warn!("NaN gas price cho cả hai giao dịch: {} và {}, coi là bằng nhau", 
                        a.tx_hash, b.tx_hash);
                    std::cmp::Ordering::Equal
                },
                (true, false) => {
                    tracing::warn!("NaN gas price cho giao dịch: {}, đặt vào cuối danh sách", a.tx_hash);
                    std::cmp::Ordering::Less
                },
                (false, true) => {
                    tracing::warn!("NaN gas price cho giao dịch: {}, đặt vào cuối danh sách", b.tx_hash);
                    std::cmp::Ordering::Greater
                },
                (false, false) => {
                    // So sánh bình thường khi cả hai giá trị đều hợp lệ
                    b_gas.partial_cmp(&a_gas).unwrap_or_else(|| {
                        tracing::error!(
                            "Lỗi không mong đợi khi so sánh gas price: {} ({:.2}) và {} ({:.2})",
                            a.tx_hash, a_gas, b.tx_hash, b_gas
                        );
                        std::cmp::Ordering::Equal
                    })
                }
            }
        });
        
        // Ghi log số lượng giao dịch đã sắp xếp
        tracing::debug!("Đã sắp xếp {} giao dịch swap theo gas price để phân tích front-running", sorted_swaps.len());
        
        // Lọc trước các giao dịch có gas price không hợp lệ
        let valid_sorted_swaps: Vec<&MempoolTransaction> = sorted_swaps.into_iter()
            .filter(|tx| {
                let gas_price = tx.gas_price;
                if gas_price.is_nan() || gas_price <= 0.0 {
                    tracing::warn!("Bỏ qua giao dịch với gas price không hợp lệ: {} ({:?})", tx.tx_hash, gas_price);
                    false
                } else {
                    true
                }
            })
            .collect();
        
        // Kiểm tra các swap cùng token nhưng khác gas price đáng kể
        for i in 0..valid_sorted_swaps.len().saturating_sub(1) {
            let high_gas = valid_sorted_swaps[i];
            let low_gas = valid_sorted_swaps[i + 1];
            
            // Lấy gas price, đã được đảm bảo hợp lệ từ bước lọc trước
            let high_gas_price = high_gas.gas_price;
            let low_gas_price = low_gas.gas_price;
            
            // Kiểm tra nếu cùng token pair nhưng gas price chênh lệch lớn
            if let (Some(high_from), Some(high_to)) = (&high_gas.from_token, &high_gas.to_token) {
                if let (Some(low_from), Some(low_to)) = (&low_gas.from_token, &low_gas.to_token) {
                    // Kiểm tra nếu cùng token pair (cùng chiều hoặc ngược chiều)
                    let same_tokens = (high_from.address == low_from.address && high_to.address == low_to.address) ||
                                     (high_from.address == low_to.address && high_to.address == low_from.address);
                    
                    if same_tokens {
                        // Tính tỷ lệ an toàn tránh phép chia cho 0
                        let ratio = if low_gas_price >= 0.000001 { // Ngưỡng nhỏ nhất để tránh chia cho gần 0
                            high_gas_price / low_gas_price
                        } else {
                            // Nếu giá trị quá nhỏ, coi như tỷ lệ rất cao
                            1000.0
                        };
                        
                        // Nếu gas price cao hơn đáng kể
                        if ratio > 1.5 {
                            tracing::info!(
                                "Phát hiện mẫu front-running: gas price cao/thấp = {:.2}x ({:.2} vs {:.2} gwei), tokens: {} -> {}",
                                ratio, high_gas_price, low_gas_price, 
                                high_from.symbol.as_deref().unwrap_or("unknown"),
                                high_to.symbol.as_deref().unwrap_or("unknown")
                            );
                            return true;
                        }
                    }
                }
            }
        }
        
        false
    }

    /// Phát hiện mẫu thanh khoản bất thường từ tập các sự kiện thanh khoản
    pub fn detect_abnormal_liquidity_pattern(&self, events: &[&MempoolTransaction]) -> bool {
        // Cần ít nhất 2 event để phát hiện mẫu
        if events.len() < 2 {
            return false;
        }
        
        // Nhóm theo token
        let mut token_to_events: HashMap<String, Vec<&MempoolTransaction>> = HashMap::new();
        
        for event in events {
            if let Some(to_token) = &event.to_token {
                token_to_events
                    .entry(to_token.address.clone())
                    .or_insert_with(Vec::new)
                    .push(event);
            }
        }
        
        // Kiểm tra từng token
        for (_, token_events) in token_to_events {
            if token_events.len() < 2 {
                continue;
            }
            
            // Sắp xếp theo thời gian
            let mut sorted_events = token_events;
            sorted_events.sort_by_key(|&tx| tx.timestamp);
            
            for i in 0..sorted_events.len() - 1 {
                let current = sorted_events[i];
                let next = sorted_events[i + 1];
                
                // Phát hiện thêm rồi rút nhanh chóng (5 phút)
                if current.transaction_type == TransactionType::AddLiquidity &&
                   next.transaction_type == TransactionType::RemoveLiquidity &&
                   (next.timestamp - current.timestamp) < 300 { // 5 phút
                    return true;
                }
            }
        }
        
        false
    }

    /// Phân tích tần suất giao dịch và phát hiện hành vi từ lịch sử giao dịch
    pub fn analyze_transaction_frequency(&self, transactions: &[MempoolTransaction], period_hours: f64) -> f64 {
        if transactions.is_empty() || period_hours <= 0.0 {
            return 0.0;
        }
        
        // Số lượng giao dịch / số giờ = tần suất
        transactions.len() as f64 / period_hours
    }

    /// Phân tích thời gian hoạt động chính từ lịch sử giao dịch
    pub fn analyze_active_hours(&self, transactions: &[MempoolTransaction]) -> Vec<u8> {
        if transactions.is_empty() {
            return Vec::new();
        }
        
        // Đếm giao dịch theo giờ trong ngày
        let mut hour_counts: HashMap<u8, i32> = HashMap::new();
        
        for tx in transactions {
            let timestamp = tx.timestamp;
            // Xử lý an toàn timestamp, tránh unwrap có thể gây panic
            let datetime = match chrono::DateTime::from_timestamp(timestamp as i64, 0) {
                Some(dt) => dt,
                None => {
                    tracing::warn!("Không thể chuyển đổi timestamp {} thành DateTime cho giao dịch {}, bỏ qua", 
                                   timestamp, tx.tx_hash);
                    continue; // Bỏ qua giao dịch này
                }
            };
            
            let hour = datetime.hour() as u8;
            *hour_counts.entry(hour).or_insert(0) += 1;
        }
        
        // Sắp xếp và lấy 5 giờ hoạt động nhiều nhất
        let mut active_hours: Vec<(u8, i32)> = hour_counts.into_iter().collect();
        active_hours.sort_by(|a, b| b.1.cmp(&a.1));
        
        active_hours.into_iter()
            .take(5) // Top 5 active hours
            .map(|(hour, _)| hour)
            .collect()
    }

    /// Tính điểm đánh giá trader dựa trên hành vi và chuyên môn
    pub fn calculate_trader_score(&self, behavior_type: &TraderBehaviorType, expertise_level: &TraderExpertiseLevel) -> f64 {
        // Base score dựa trên loại trader
        let behavior_score = match behavior_type {
            TraderBehaviorType::Arbitrageur => 80.0,
            TraderBehaviorType::MevBot => 85.0,
            TraderBehaviorType::Institutional => 70.0,
            TraderBehaviorType::MarketMaker => 75.0,
            TraderBehaviorType::Whale => 65.0,
            TraderBehaviorType::RetailTrader => 50.0,
            TraderBehaviorType::Unknown => 40.0,
        };
        
        // Điều chỉnh theo mức độ chuyên môn
        let expertise_multiplier = match expertise_level {
            TraderExpertiseLevel::Beginner => 0.7,
            TraderExpertiseLevel::Intermediate => 0.9,
            TraderExpertiseLevel::Advanced => 1.1,
            TraderExpertiseLevel::Expert => 1.3,
            TraderExpertiseLevel::Unknown => 1.0,
        };
        
        // Điểm cuối cùng, giới hạn trong khoảng 0-100
        (behavior_score * expertise_multiplier).min(100.0).max(0.0)
    }

    /// Phát hiện các mẫu đáng ngờ từ một tập hợp các giao dịch
    pub fn detect_suspicious_patterns(&self, transactions: &[MempoolTransaction]) -> Vec<SuspiciousPattern> {
        if transactions.is_empty() {
            return Vec::new();
        }
        
        let mut patterns = Vec::new();
        
        // Nhóm các giao dịch theo loại để phân tích riêng
        let swaps: Vec<&MempoolTransaction> = transactions.iter()
            .filter(|tx| tx.transaction_type == TransactionType::Swap)
            .collect();
            
        let liquidity_events: Vec<&MempoolTransaction> = transactions.iter()
            .filter(|tx| tx.transaction_type == TransactionType::AddLiquidity || 
                        tx.transaction_type == TransactionType::RemoveLiquidity)
            .collect();
        
        // Phát hiện sandwich attack
        if self.detect_sandwich_pattern(&swaps) {
            patterns.push(SuspiciousPattern::SandwichAttack);
        }
        
        // Phát hiện front-running
        if self.detect_front_running_pattern(&swaps) {
            patterns.push(SuspiciousPattern::FrontRunning);
        }
        
        // Phát hiện mẫu thanh khoản bất thường
        if self.detect_abnormal_liquidity_pattern(&liquidity_events) {
            patterns.push(SuspiciousPattern::AbnormalLiquidity);
        }
        
        // Phát hiện các giao dịch có gas price cực cao
        let high_gas_txs: Vec<&MempoolTransaction> = transactions.iter()
            .filter(|tx| {
                let gas_price = tx.gas_price;
                !gas_price.is_nan() && gas_price > 0.0 && gas_price > self.get_average_gas_price_sync() * 5.0
            })
            .collect();
            
        if !high_gas_txs.is_empty() {
            patterns.push(SuspiciousPattern::HighGasPrice);
        }
        
        patterns
    }

    /// Phát hiện cơ hội backrunning từ tập các giao dịch
    pub fn detect_backrunning_opportunity(&self, transactions: &[&MempoolTransaction]) -> bool {
        // Cần ít nhất 1 giao dịch để phân tích
        if transactions.is_empty() {
            return false;
        }
        
        // Tìm các giao dịch có ảnh hưởng lớn đến giá token
        for tx in transactions {
            // Chỉ quan tâm đến các giao dịch swap lớn hoặc thêm/rút thanh khoản lớn
            if (tx.transaction_type == TransactionType::Swap && tx.value_usd >= 10000.0) || 
               ((tx.transaction_type == TransactionType::AddLiquidity || 
                 tx.transaction_type == TransactionType::RemoveLiquidity) && 
                tx.value_usd >= 50000.0) {
                
                // Kiểm tra nếu gas price không quá cao (để có thể backrun)
                if tx.gas_price < self.get_average_gas_price_sync() * 2.0 {
                    // Kiểm tra nếu có token với thanh khoản đủ lớn để giao dịch
                    if let Some(to_token) = &tx.to_token {
                        // Nếu có thông tin thanh khoản và đủ lớn
                        if let Some(liquidity) = to_token.liquidity_usd {
                            if liquidity >= 100000.0 {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        
        false
    }

    // Helper method to get average gas price synchronously
    fn get_average_gas_price_sync(&self) -> f64 {
        // This is a simplification - in real code you'd need proper synchronization
        // But for the purpose of this merge, we'll use a default value if not available
        20.0 // Default to 20 gwei if not available synchronously
    }
}

/// Phân tích giao dịch để lấy thông tin token
pub async fn analyze_transaction(&self, tx: &Transaction) -> Result<TokenInfo, anyhow::Error> {
    // Đây là một triển khai đơn giản, cần được mở rộng dựa trên logic phân tích thực tế
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
        
    // Tạo một TokenInfo cơ bản - cần bổ sung tất cả các trường cần thiết
    let token_info = TokenInfo {
        address: tx.to.clone().unwrap_or_default(),
        chain_id: self.chain_id,
        symbol: None,
        price: 0.0,
        price_change_24h: 0.0,
        liquidity_usd: 0.0,
        volume_24h: 0.0,
        market_cap: 0.0,
        holders: 0,
        is_honeypot: None,
        is_mintable: None,
        is_proxy: None,
        has_blacklist: None,
        buy_tax: None,
        sell_tax: None,
        amount: 0.0,
        total_supply: None,
        score: None,
    };
    
    Ok(token_info)
}

// Update method to use the filter module
pub async fn get_filtered_transactions(
    &self,
    filter_options: TransactionFilterOptions,
    limit: usize,
) -> Vec<MempoolTransaction> {
    let pending_txs = self.pending_transactions.read().await;
    filter::get_filtered_transactions(&pending_txs, filter_options, limit)
}

// Update method to use the priority module
pub async fn estimate_confirmation_time(
    &self,
    transaction_priority: &TransactionPriority,
    chain_id: u32,
) -> String {
    priority::estimate_confirmation_time(transaction_priority, chain_id)
}

// Update method to use the arbitrage module
pub async fn find_mev_bots(
    &self,
    high_frequency_threshold: usize,
    time_window_sec: u64,
    limit: usize,
) -> Vec<(String, usize)> {
    let pending_txs = self.pending_transactions.read().await;
    let mut results = super::arbitrage::find_mev_bots(
        &pending_txs, 
        high_frequency_threshold, 
        time_window_sec
    );
    
    // Apply limit
    if results.len() > limit {
        results.truncate(limit);
    }
    
    results
}

// Update method to use the arbitrage module
pub async fn find_sandwich_attacks(
    &self,
    min_value_eth: f64,
    time_window_ms: u64,
    limit: usize,
) -> Vec<Vec<MempoolTransaction>> {
    let pending_txs = self.pending_transactions.read().await;
    let mut results = super::arbitrage::find_sandwich_attacks(
        &pending_txs, 
        min_value_eth, 
        time_window_ms
    );
    
    // Apply limit
    if results.len() > limit {
        results.truncate(limit);
    }
    
    results
}

// Update method to use the arbitrage module
pub async fn find_frontrunning_transactions(
    &self,
    min_value_eth: f64,
    time_window_ms: u64,
    limit: usize,
) -> Vec<(MempoolTransaction, MempoolTransaction)> {
    let pending_txs = self.pending_transactions.read().await;
    let mut results = super::arbitrage::find_frontrunning_transactions(
        &pending_txs, 
        min_value_eth, 
        time_window_ms
    );
    
    // Apply limit
    if results.len() > limit {
        results.truncate(limit);
    }
    
    results
} 