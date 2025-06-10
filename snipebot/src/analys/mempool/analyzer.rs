use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use crate::analys::mempool::types::*;
use crate::analys::mempool::detection;
use crate::analys::mempool::filter;
use crate::analys::mempool::priority;
use super::utils;

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
        if !alerts.is_empty() {
            let mut all_alerts = self.alerts.write().await;
            all_alerts.extend(alerts);
        }
        
        // Cập nhật gas price trung bình
        if tx_count > 0 {
            let new_avg_gas = sum_gas_price / tx_count as f64;
            let mut avg_gas = self.average_gas_price.write().await;
            
            if *avg_gas == 0.0 {
                *avg_gas = new_avg_gas;
            } else {
                // Cập nhật trung bình theo công thức EMA (Exponential Moving Average)
                const ALPHA: f64 = 0.1; // Hệ số làm mịn
                *avg_gas = (1.0 - ALPHA) * *avg_gas + ALPHA * new_avg_gas;
            }
        }
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
                    detected_at: transaction.detected_at,
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
    
    /// Tạo cảnh báo whale mà không cần lock alerts
    fn create_whale_alert_without_lock(&self, transaction: &MempoolTransaction) -> MempoolAlert {
        MempoolAlert {
            alert_type: MempoolAlertType::WhaleTransaction,
            severity: AlertSeverity::Medium,
            detected_at: transaction.detected_at,
            description: format!(
                "Whale transaction detected: {} ETH from {} to {}",
                transaction.value,
                transaction.from_address,
                transaction.to_address.clone().unwrap_or_default()
            ),
            related_transactions: vec![transaction.tx_hash.clone()],
            related_token: None,
            related_addresses: vec![
                transaction.from_address.clone(),
                transaction.to_address.clone().unwrap_or_default(),
            ],
        }
    }
    
    /// Phát hiện pattern đáng ngờ mà không lock alert
    async fn detect_suspicious_pattern(&self, transaction: &MempoolTransaction) -> Option<SuspiciousPattern> {
        // Lấy tất cả giao dịch đang pending
        let pending_txs = self.pending_transactions.read().await;
        
        // Tìm front-running
        match detection::detect_front_running(
            transaction,
            &pending_txs
        ).await {
            Ok(Some(pattern)) => return Some(pattern),
            _ => {}
        }
        
        // Tìm sandwich attack
        match detection::detect_sandwich_attack(
            transaction,
            &pending_txs
        ).await {
            Ok(Some(pattern)) => return Some(pattern),
            _ => {}
        }
        
        None
    }
    
    /// Tạo cảnh báo pattern đáng ngờ mà không cần lock alerts
    fn create_suspicious_pattern_alert_without_lock(&self, transaction: &MempoolTransaction, pattern: SuspiciousPattern) -> MempoolAlert {
        // Xác định severity của alert
        let severity = match &pattern {
            SuspiciousPattern::FrontRunning => AlertSeverity::High,
            SuspiciousPattern::SandwichAttack => AlertSeverity::High,
            SuspiciousPattern::MevBot => AlertSeverity::Medium,
            _ => AlertSeverity::Low,
        };
        
        // Tạo mô tả chi tiết
        let description = match &pattern {
            SuspiciousPattern::FrontRunning => 
                format!("Front-running detected: Transaction {} may be front-running another transaction", 
                        transaction.tx_hash),
            SuspiciousPattern::SandwichAttack => 
                format!("Sandwich attack detected: Transaction {} may be part of a sandwich attack", 
                        transaction.tx_hash),
            SuspiciousPattern::MevBot => 
                format!("MEV bot detected: Address {} appears to be an MEV bot", 
                        transaction.from_address),
            SuspiciousPattern::SuddenLiquidityRemoval => 
                format!("Sudden liquidity removal detected from address {}", 
                        transaction.from_address),
            _ => format!("Suspicious pattern detected: {:?}", pattern),
        };
        
        MempoolAlert {
            alert_type: MempoolAlertType::SuspiciousTransaction(pattern),
            severity,
            detected_at: transaction.detected_at,
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

    /// Create new token info object from transaction data
    fn create_new_token_info(&self, transaction: &MempoolTransaction) -> NewTokenInfo {
        // Lấy các thông tin từ transaction
        let creator_address = transaction.from_address.clone();
        let created_at = transaction.detected_at;
        
        // Tạo token info với các thông tin có sẵn
        NewTokenInfo {
            token_address: transaction.to_address.clone().unwrap_or_default(),
            chain_id: transaction.chain_id,
            creation_tx: transaction.tx_hash.clone(),
            creator_address,
            detected_at: created_at,
            liquidity_added: false,
            liquidity_tx: None,
            liquidity_value: None,
            dex_type: None,
        }
    }

    /// Create base token info for newly discovered token
    fn create_base_token_info(&self, token_address: String, chain_id: u32) -> TokenInfo {
        TokenInfo {
            address: token_address,
            chain_id,
            created_at: 0, // Unknown creation time
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