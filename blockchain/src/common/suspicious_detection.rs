//! Module cung cấp các hàm phát hiện giao dịch đáng ngờ cho các bridge
//! Sử dụng các thuật toán phát hiện bất thường và các luật đã định sẵn
//! Giúp giảm trùng lặp code giữa các bridge implementation

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};

use crate::bridge::error::{BridgeError, BridgeResult};
use crate::bridge::types::{BridgeTransaction, TransactionHash};
use crate::common::bridge_utils::address_validation::{self, is_address_blocked};

/// Các loại cảnh báo giao dịch đáng ngờ
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SuspiciousTransactionType {
    /// Giao dịch với số lượng lớn bất thường
    LargeAmount,
    /// Giao dịch từ địa chỉ có lịch sử đáng ngờ
    SuspiciousSource,
    /// Giao dịch đến địa chỉ có lịch sử đáng ngờ
    SuspiciousDestination,
    /// Mẫu hoạt động bất thường (nhiều giao dịch trong thời gian ngắn)
    AbnormalPattern,
    /// Giao dịch với địa chỉ đã được đánh dấu
    FlaggedAddress,
    /// Giao dịch với contract không rõ nguồn gốc
    UnknownContract,
    /// Giao dịch có chuỗi hành vi lặp lại đáng ngờ
    RepetitivePattern,
    /// Giao dịch với gas/phí cao bất thường
    HighFee,
    /// Giao dịch được thực hiện vào thời gian bất thường
    OddTimestamp,
}

/// Bản ghi giao dịch đáng ngờ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuspiciousTransactionRecord {
    /// Loại nghi ngờ
    pub suspicious_type: SuspiciousTransactionType,
    /// Giá trị giao dịch
    pub amount: String,
    /// Địa chỉ nguồn
    pub source_address: String,
    /// Địa chỉ đích
    pub destination_address: String,
    /// Chain ID nguồn
    pub source_chain: String,
    /// Chain ID đích
    pub destination_chain: String,
    /// Token được chuyển
    pub token: String,
    /// Giá trị ngưỡng kích hoạt (nếu có)
    pub threshold_value: Option<String>,
    /// Mô tả lý do khả nghi
    pub description: String,
    /// Thời gian phát hiện
    pub detected_at: DateTime<Utc>,
    /// Hash giao dịch (nếu có)
    pub transaction_hash: Option<TransactionHash>,
    /// Đã được xử lý chưa
    pub processed: bool,
    /// Người xử lý
    pub processed_by: Option<String>,
    /// Thời gian xử lý
    pub processed_at: Option<DateTime<Utc>>,
    /// Các ghi chú bổ sung
    pub notes: Option<String>,
}

impl SuspiciousTransactionRecord {
    /// Tạo ghi nhận giao dịch đáng ngờ mới
    pub fn new(
        suspicious_type: SuspiciousTransactionType,
        amount: String,
        source_address: String,
        destination_address: String,
        source_chain: String,
        destination_chain: String,
        token: String,
        threshold_value: Option<String>,
        description: String,
        transaction_hash: Option<TransactionHash>,
    ) -> Self {
        Self {
            suspicious_type,
            amount,
            source_address,
            destination_address,
            source_chain,
            destination_chain,
            token,
            threshold_value,
            description,
            detected_at: Utc::now(),
            transaction_hash,
            processed: false,
            processed_by: None,
            processed_at: None,
            notes: None,
        }
    }

    /// Đánh dấu giao dịch đã được xử lý
    pub fn mark_as_processed(&mut self, processed_by: String, notes: Option<String>) {
        self.processed = true;
        self.processed_by = Some(processed_by);
        self.processed_at = Some(Utc::now());
        self.notes = notes;
    }

    /// Chuyển đổi từ BridgeTransaction
    pub fn from_transaction(
        transaction: &BridgeTransaction,
        suspicious_type: SuspiciousTransactionType,
        threshold_value: Option<String>,
        description: String,
    ) -> Self {
        Self::new(
            suspicious_type,
            transaction.amount.to_string(),
            transaction.source_address.clone(),
            transaction.destination_address.clone(),
            transaction.source_chain.to_string(),
            transaction.destination_chain.to_string(),
            transaction.token.clone(),
            threshold_value,
            description,
            Some(transaction.id.clone()),
        )
    }
}

/// Lưu trữ và quản lý các giao dịch đáng ngờ
#[derive(Debug)]
pub struct SuspiciousTransactionManager {
    /// Danh sách giao dịch đáng ngờ
    transactions: Arc<RwLock<Vec<SuspiciousTransactionRecord>>>,
    /// Bộ theo dõi mẫu giao dịch
    pattern_tracker: PatternTracker,
    /// Ngưỡng số lượng lớn (theo token)
    large_amount_thresholds: Arc<RwLock<HashMap<String, f64>>>,
    /// Ngưỡng số lượng giao dịch trong khoảng thời gian (giây)
    frequency_threshold: (usize, u64),
    /// Địa chỉ đáng ngờ (đã biết)
    known_suspicious_addresses: Arc<RwLock<HashSet<String>>>,
    /// Hàm thông báo cho admin (nếu có)
    admin_notifier: Option<Arc<dyn Fn(&str, &SuspiciousTransactionRecord) -> BridgeResult<()> + Send + Sync>>,
}

impl SuspiciousTransactionManager {
    /// Tạo manager mới với các ngưỡng mặc định
    pub fn new() -> Self {
        let mut large_amount_thresholds = HashMap::new();
        // Các ngưỡng mặc định cho một số token phổ biến
        large_amount_thresholds.insert("ETH".to_string(), 50.0);
        large_amount_thresholds.insert("USDT".to_string(), 100000.0);
        large_amount_thresholds.insert("USDC".to_string(), 100000.0);
        large_amount_thresholds.insert("BTC".to_string(), 2.0);
        large_amount_thresholds.insert("DMD".to_string(), 10000.0);

        Self {
            transactions: Arc::new(RwLock::new(Vec::new())),
            pattern_tracker: PatternTracker::new(100), // 100 giao dịch gần nhất mỗi địa chỉ
            large_amount_thresholds: Arc::new(RwLock::new(large_amount_thresholds)),
            frequency_threshold: (10, 60), // 10 giao dịch trong 60 giây
            known_suspicious_addresses: Arc::new(RwLock::new(HashSet::new())),
            admin_notifier: None,
        }
    }

    /// Đặt hàm thông báo admin
    pub fn with_admin_notifier<F>(mut self, notifier: F) -> Self
    where
        F: Fn(&str, &SuspiciousTransactionRecord) -> BridgeResult<()> + 'static + Send + Sync,
    {
        self.admin_notifier = Some(Arc::new(notifier));
        self
    }

    /// Đặt ngưỡng số lượng lớn cho một token
    pub fn set_large_amount_threshold(&self, token: &str, amount: f64) -> BridgeResult<()> {
        let mut thresholds = self.large_amount_thresholds
            .write()
            .map_err(|_| BridgeError::ConcurrencyError("Failed to acquire lock for thresholds".into()))?;
        
        thresholds.insert(token.to_string(), amount);
        Ok(())
    }

    /// Đặt ngưỡng tần suất giao dịch
    pub fn set_frequency_threshold(&mut self, count: usize, time_window_seconds: u64) {
        self.frequency_threshold = (count, time_window_seconds);
    }

    /// Thêm địa chỉ vào danh sách đáng ngờ
    pub fn add_suspicious_address(&self, address: &str) -> BridgeResult<()> {
        // Thêm vào danh sách địa chỉ bị chặn
        let _ = address_validation::add_blocked_address(address);
        
        let mut addresses = self.known_suspicious_addresses
            .write()
            .map_err(|_| BridgeError::ConcurrencyError("Failed to acquire lock for suspicious addresses".into()))?;
        
        addresses.insert(address.to_string());
        Ok(())
    }

    /// Kiểm tra địa chỉ có đáng ngờ không
    pub fn is_suspicious_address(&self, address: &str) -> BridgeResult<bool> {
        // Kiểm tra trong danh sách bị chặn
        if is_address_blocked(address)? {
            return Ok(true);
        }
        
        let addresses = self.known_suspicious_addresses
            .read()
            .map_err(|_| BridgeError::ConcurrencyError("Failed to read suspicious addresses".into()))?;
        
        Ok(addresses.contains(address))
    }

    /// Ghi nhận giao dịch mới và kiểm tra tất cả các qui tắc
    pub fn check_transaction(&self, transaction: &BridgeTransaction) -> BridgeResult<Vec<SuspiciousTransactionRecord>> {
        let mut suspicious_records = Vec::new();
        
        // Kiểm tra các dấu hiệu đáng ngờ
        if let Some(record) = self.check_large_amount(transaction)? {
            suspicious_records.push(record.clone());
            self.notify_admin("LARGE_AMOUNT_DETECTED", &record)?;
        }
        
        if let Some(record) = self.check_suspicious_addresses(transaction)? {
            suspicious_records.push(record.clone());
            self.notify_admin("SUSPICIOUS_ADDRESS_DETECTED", &record)?;
        }
        
        if let Some(record) = self.check_transaction_frequency(transaction)? {
            suspicious_records.push(record.clone());
            self.notify_admin("HIGH_FREQUENCY_DETECTED", &record)?;
        }
        
        if let Some(record) = self.check_repetitive_patterns(transaction)? {
            suspicious_records.push(record.clone());
            self.notify_admin("REPETITIVE_PATTERN_DETECTED", &record)?;
        }
        
        // Thêm tất cả ghi nhận vào danh sách
        if !suspicious_records.is_empty() {
            let mut transactions = self.transactions
                .write()
                .map_err(|_| BridgeError::ConcurrencyError("Failed to write to transactions".into()))?;
            
            transactions.extend(suspicious_records.clone());
        }
        
        Ok(suspicious_records)
    }

    /// Kiểm tra số lượng lớn
    fn check_large_amount(&self, transaction: &BridgeTransaction) -> BridgeResult<Option<SuspiciousTransactionRecord>> {
        let thresholds = self.large_amount_thresholds
            .read()
            .map_err(|_| BridgeError::ConcurrencyError("Failed to read thresholds".into()))?;
        
        // Lấy ngưỡng cho token này, nếu không có thì dùng 1000.0 làm mặc định
        let threshold = match thresholds.get(&transaction.token) {
            Some(t) => *t,
            None => 1000.0,
        };
        
        let amount = match transaction.amount.parse::<f64>() {
            Ok(a) => a,
            Err(_) => {
                warn!("Failed to parse transaction amount: {}", transaction.amount);
                return Ok(None);
            }
        };
        
        if amount > threshold {
            let record = SuspiciousTransactionRecord::from_transaction(
                transaction,
                SuspiciousTransactionType::LargeAmount,
                Some(threshold.to_string()),
                format!("Transaction amount ({} {}) exceeds threshold of {} {}",
                       amount, transaction.token, threshold, transaction.token),
            );
            
            return Ok(Some(record));
        }
        
        Ok(None)
    }

    /// Kiểm tra địa chỉ đáng ngờ
    fn check_suspicious_addresses(&self, transaction: &BridgeTransaction) -> BridgeResult<Option<SuspiciousTransactionRecord>> {
        // Kiểm tra địa chỉ nguồn
        if self.is_suspicious_address(&transaction.source_address)? {
            let record = SuspiciousTransactionRecord::from_transaction(
                transaction,
                SuspiciousTransactionType::SuspiciousSource,
                None,
                format!("Transaction from suspicious source address: {}", transaction.source_address),
            );
            
            return Ok(Some(record));
        }
        
        // Kiểm tra địa chỉ đích
        if self.is_suspicious_address(&transaction.destination_address)? {
            let record = SuspiciousTransactionRecord::from_transaction(
                transaction,
                SuspiciousTransactionType::SuspiciousDestination,
                None,
                format!("Transaction to suspicious destination address: {}", transaction.destination_address),
            );
            
            return Ok(Some(record));
        }
        
        Ok(None)
    }

    /// Kiểm tra tần suất giao dịch
    fn check_transaction_frequency(&self, transaction: &BridgeTransaction) -> BridgeResult<Option<SuspiciousTransactionRecord>> {
        // Thêm giao dịch vào bộ theo dõi mẫu
        self.pattern_tracker.add_transaction(
            &transaction.source_address,
            transaction.source_chain.clone(),
            &Utc::now(),
        )?;
        
        // Lấy số lượng giao dịch trong khoảng thời gian
        let (count_threshold, time_window) = self.frequency_threshold;
        let count = self.pattern_tracker.get_recent_transaction_count(
            &transaction.source_address,
            transaction.source_chain.clone(),
            time_window,
        )?;
        
        if count >= count_threshold {
            let record = SuspiciousTransactionRecord::from_transaction(
                transaction,
                SuspiciousTransactionType::AbnormalPattern,
                Some(count_threshold.to_string()),
                format!("High frequency of transactions: {} in the last {} seconds (threshold: {})",
                       count, time_window, count_threshold),
            );
            
            return Ok(Some(record));
        }
        
        Ok(None)
    }

    /// Kiểm tra mẫu lặp lại đáng ngờ
    fn check_repetitive_patterns(&self, transaction: &BridgeTransaction) -> BridgeResult<Option<SuspiciousTransactionRecord>> {
        // Kiểm tra số lượng giao dịch giống nhau (cùng số lượng) từ cùng một địa chỉ
        let repetition = self.pattern_tracker.check_amount_repetition(
            &transaction.source_address,
            transaction.source_chain.clone(),
            &transaction.amount,
            3, // Ngưỡng lặp lại (3 lần trở lên được coi là đáng ngờ)
        )?;
        
        if repetition {
            let record = SuspiciousTransactionRecord::from_transaction(
                transaction,
                SuspiciousTransactionType::RepetitivePattern,
                Some("3".to_string()),
                format!(
                    "Repetitive transaction pattern detected: Same amount ({} {}) sent multiple times",
                    transaction.amount, transaction.token
                ),
            );
            
            return Ok(Some(record));
        }
        
        Ok(None)
    }

    /// Thông báo cho admin về giao dịch đáng ngờ
    fn notify_admin(&self, alert_type: &str, record: &SuspiciousTransactionRecord) -> BridgeResult<()> {
        if let Some(ref notifier) = self.admin_notifier {
            notifier(alert_type, record)?;
        }
        
        info!(
            "Suspicious transaction detected: {} - {} {} from {} to {} ({}->{})",
            alert_type,
            record.amount,
            record.token,
            record.source_address,
            record.destination_address,
            record.source_chain,
            record.destination_chain
        );
        
        Ok(())
    }

    /// Lấy tất cả giao dịch đáng ngờ chưa được xử lý
    pub fn get_unprocessed_transactions(&self) -> BridgeResult<Vec<SuspiciousTransactionRecord>> {
        let transactions = self.transactions
            .read()
            .map_err(|_| BridgeError::ConcurrencyError("Failed to read transactions".into()))?;
        
        Ok(transactions
            .iter()
            .filter(|t| !t.processed)
            .cloned()
            .collect())
    }

    /// Đánh dấu giao dịch đã được xử lý
    pub fn mark_as_processed(&self, transaction_hash: &TransactionHash, processed_by: &str, notes: Option<String>) -> BridgeResult<bool> {
        let mut transactions = self.transactions
            .write()
            .map_err(|_| BridgeError::ConcurrencyError("Failed to write to transactions".into()))?;
        
        for transaction in transactions.iter_mut() {
            if let Some(hash) = &transaction.transaction_hash {
                if hash == transaction_hash {
                    transaction.mark_as_processed(processed_by.to_string(), notes);
                    return Ok(true);
                }
            }
        }
        
        Ok(false)
    }
}

/// Theo dõi mẫu giao dịch qua thời gian
#[derive(Debug)]
struct PatternTracker {
    /// Lưu trữ thông tin giao dịch gần đây theo địa chỉ và chain
    recent_transactions: Arc<RwLock<HashMap<(String, String), VecDeque<(DateTime<Utc>, String)>>>>,
    /// Số lượng giao dịch tối đa lưu trữ cho mỗi địa chỉ
    max_transactions_per_address: usize,
}

impl PatternTracker {
    /// Tạo bộ theo dõi mẫu mới
    pub fn new(max_transactions_per_address: usize) -> Self {
        Self {
            recent_transactions: Arc::new(RwLock::new(HashMap::new())),
            max_transactions_per_address,
        }
    }

    /// Thêm giao dịch mới vào bộ theo dõi
    pub fn add_transaction(
        &self,
        address: &str,
        chain: String,
        timestamp: &DateTime<Utc>,
        amount: Option<&str>,
    ) -> BridgeResult<()> {
        let key = (address.to_string(), chain);
        let mut transactions = self.recent_transactions
            .write()
            .map_err(|_| BridgeError::ConcurrencyError("Failed to write to recent transactions".into()))?;
        
        // Lấy hoặc tạo hàng đợi cho địa chỉ này
        let queue = transactions.entry(key).or_insert_with(VecDeque::new);
        
        // Thêm giao dịch mới
        queue.push_back((timestamp.clone(), amount.unwrap_or("").to_string()));
        
        // Giữ cho hàng đợi không vượt quá kích thước tối đa
        while queue.len() > self.max_transactions_per_address {
            queue.pop_front();
        }
        
        Ok(())
    }

    /// Lấy số lượng giao dịch gần đây trong khoảng thời gian
    pub fn get_recent_transaction_count(
        &self,
        address: &str,
        chain: String,
        time_window_seconds: u64,
    ) -> BridgeResult<usize> {
        let key = (address.to_string(), chain);
        let transactions = self.recent_transactions
            .read()
            .map_err(|_| BridgeError::ConcurrencyError("Failed to read recent transactions".into()))?;
        
        let now = Utc::now();
        
        if let Some(queue) = transactions.get(&key) {
            let count = queue
                .iter()
                .filter(|(timestamp, _)| {
                    (now - *timestamp).num_seconds() <= time_window_seconds as i64
                })
                .count();
            
            return Ok(count);
        }
        
        Ok(0)
    }

    /// Kiểm tra lặp lại số lượng với ngưỡng
    pub fn check_amount_repetition(
        &self,
        address: &str,
        chain: String,
        amount: &str,
        threshold: usize,
    ) -> BridgeResult<bool> {
        let key = (address.to_string(), chain);
        let transactions = self.recent_transactions
            .read()
            .map_err(|_| BridgeError::ConcurrencyError("Failed to read recent transactions".into()))?;
        
        if let Some(queue) = transactions.get(&key) {
            let count = queue
                .iter()
                .filter(|(_, tx_amount)| tx_amount == amount)
                .count();
            
            return Ok(count >= threshold);
        }
        
        Ok(false)
    }
} 