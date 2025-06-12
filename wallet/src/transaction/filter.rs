//! Module cung cấp các bộ lọc và chức năng tìm kiếm hiệu quả cho giao dịch blockchain
//!
//! Module này cung cấp:
//! - Bộ lọc tìm kiếm đa tiêu chí hiệu quả
//! - Tối ưu hóa truy vấn với indexing
//! - Cấu trúc dữ liệu hiệu quả cho tìm kiếm giao dịch

// External imports
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use ethers::types::{Address, H256, U256};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use anyhow::Context;
use tracing::{debug, error, info, warn};

// Internal imports
use crate::error::{WalletError, Result};
use super::history::{TransactionInfo, TransactionStatus};

/// Thời gian tối đa cache kết quả tìm kiếm
const MAX_CACHE_TIME_SECONDS: i64 = 300; // 5 phút

/// Kiểu so sánh cho các bộ lọc số học
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComparisonType {
    /// Bằng
    Equal,
    /// Lớn hơn
    GreaterThan,
    /// Nhỏ hơn
    LessThan,
    /// Lớn hơn hoặc bằng
    GreaterThanOrEqual,
    /// Nhỏ hơn hoặc bằng
    LessThanOrEqual,
    /// Khác
    NotEqual,
    /// Trong khoảng
    InRange,
}

/// Bộ lọc cho các giá trị U256
#[derive(Debug, Clone)]
pub struct U256Filter {
    /// Giá trị lọc
    pub value: U256,
    /// Giá trị thứ hai (cho khoảng)
    pub second_value: Option<U256>,
    /// Loại so sánh
    pub comparison: ComparisonType,
}

/// Bộ lọc thời gian
#[derive(Debug, Clone)]
pub struct DateTimeFilter {
    /// Giá trị lọc
    pub value: DateTime<Utc>,
    /// Giá trị thứ hai (cho khoảng)
    pub second_value: Option<DateTime<Utc>>,
    /// Loại so sánh
    pub comparison: ComparisonType,
}

/// Bộ lọc nâng cao cho giao dịch
#[derive(Debug, Clone, Default)]
pub struct AdvancedTransactionFilter {
    /// Lọc theo user ID
    pub user_id: Option<String>,
    /// Lọc theo địa chỉ gửi
    pub from_address: Option<Address>,
    /// Lọc theo địa chỉ nhận
    pub to_address: Option<Address>,
    /// Lọc theo trạng thái
    pub status: Option<TransactionStatus>,
    /// Lọc theo chain ID
    pub chain_id: Option<u64>,
    /// Lọc theo giá trị giao dịch
    pub amount: Option<U256Filter>,
    /// Lọc theo khoảng thời gian
    pub timestamp: Option<DateTimeFilter>,
    /// Lọc theo block number
    pub block_number: Option<u64>,
    /// Lọc theo gas price
    pub gas_price: Option<U256Filter>,
    /// Tìm theo từ khóa
    pub keyword: Option<String>,
    /// Số trang (bắt đầu từ 0)
    pub page: usize,
    /// Kích thước trang
    pub page_size: usize,
    /// Sắp xếp theo trường
    pub sort_by: Option<SortField>,
    /// Thứ tự sắp xếp
    pub sort_direction: SortDirection,
}

/// Trường sắp xếp
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortField {
    /// Sắp xếp theo thời gian
    Timestamp,
    /// Sắp xếp theo giá trị
    Amount,
    /// Sắp xếp theo block number
    BlockNumber,
    /// Sắp xếp theo gas price
    GasPrice,
}

/// Thứ tự sắp xếp
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    /// Tăng dần
    Ascending,
    /// Giảm dần
    Descending,
}

impl Default for SortDirection {
    fn default() -> Self {
        Self::Descending
    }
}

/// Công cụ đánh chỉ mục giao dịch
pub struct TransactionIndexer {
    /// Chỉ mục theo user ID
    user_index: Arc<RwLock<HashMap<String, HashSet<H256>>>>,
    
    /// Chỉ mục theo địa chỉ gửi
    from_address_index: Arc<RwLock<HashMap<Address, HashSet<H256>>>>,
    
    /// Chỉ mục theo địa chỉ nhận
    to_address_index: Arc<RwLock<HashMap<Address, HashSet<H256>>>>,
    
    /// Chỉ mục theo trạng thái
    status_index: Arc<RwLock<HashMap<TransactionStatus, HashSet<H256>>>>,
    
    /// Chỉ mục theo chain ID
    chain_id_index: Arc<RwLock<HashMap<u64, HashSet<H256>>>>,
    
    /// Chỉ mục từ khóa (từ ghi chú hoặc metadata)
    keyword_index: Arc<RwLock<HashMap<String, HashSet<H256>>>>,
    
    /// Cache kết quả tìm kiếm
    search_cache: Arc<RwLock<HashMap<String, (Vec<H256>, DateTime<Utc>)>>>,
}

impl TransactionIndexer {
    /// Tạo mới một instance TransactionIndexer
    pub fn new() -> Self {
        Self {
            user_index: Arc::new(RwLock::new(HashMap::new())),
            from_address_index: Arc::new(RwLock::new(HashMap::new())),
            to_address_index: Arc::new(RwLock::new(HashMap::new())),
            status_index: Arc::new(RwLock::new(HashMap::new())),
            chain_id_index: Arc::new(RwLock::new(HashMap::new())),
            keyword_index: Arc::new(RwLock::new(HashMap::new())),
            search_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Thêm giao dịch vào chỉ mục
    pub async fn index_transaction(&self, transaction: &TransactionInfo) -> Result<()> {
        // Đánh chỉ mục theo user ID
        {
            let mut user_index = self.user_index.write().await;
            user_index
                .entry(transaction.user_id.clone())
                .or_insert_with(HashSet::new)
                .insert(transaction.tx_hash);
        }
        
        // Đánh chỉ mục theo địa chỉ gửi
        {
            let mut from_index = self.from_address_index.write().await;
            from_index
                .entry(transaction.from_address)
                .or_insert_with(HashSet::new)
                .insert(transaction.tx_hash);
        }
        
        // Đánh chỉ mục theo địa chỉ nhận
        if let Some(to_address) = transaction.to_address {
            let mut to_index = self.to_address_index.write().await;
            to_index
                .entry(to_address)
                .or_insert_with(HashSet::new)
                .insert(transaction.tx_hash);
        }
        
        // Đánh chỉ mục theo trạng thái
        {
            let mut status_index = self.status_index.write().await;
            status_index
                .entry(transaction.status)
                .or_insert_with(HashSet::new)
                .insert(transaction.tx_hash);
        }
        
        // Đánh chỉ mục theo chain ID
        {
            let mut chain_index = self.chain_id_index.write().await;
            chain_index
                .entry(transaction.chain_id)
                .or_insert_with(HashSet::new)
                .insert(transaction.tx_hash);
        }
        
        // Xóa cache tìm kiếm để đảm bảo dữ liệu mới được phản ánh trong kết quả
        {
            let mut cache = self.search_cache.write().await;
            cache.clear();
        }
        
        Ok(())
    }
    
    /// Tạo khóa cache từ bộ lọc
    fn create_cache_key(filter: &AdvancedTransactionFilter) -> String {
        let mut key = format!("page:{}_size:{}", filter.page, filter.page_size);
        
        if let Some(ref user_id) = filter.user_id {
            key.push_str(&format!("_user:{}", user_id));
        }
        
        if let Some(from) = filter.from_address {
            key.push_str(&format!("_from:{}", from));
        }
        
        if let Some(to) = filter.to_address {
            key.push_str(&format!("_to:{}", to));
        }
        
        if let Some(status) = filter.status {
            key.push_str(&format!("_status:{:?}", status));
        }
        
        if let Some(chain) = filter.chain_id {
            key.push_str(&format!("_chain:{}", chain));
        }
        
        if let Some(ref keyword) = filter.keyword {
            key.push_str(&format!("_keyword:{}", keyword));
        }
        
        if let Some(ref sort_by) = filter.sort_by {
            key.push_str(&format!("_sort:{:?}_{:?}", sort_by, filter.sort_direction));
        }
        
        key
    }
    
    /// Tìm giao dịch với bộ lọc nâng cao
    pub async fn find_transactions(&self, filter: &AdvancedTransactionFilter, transactions: &[TransactionInfo]) -> Vec<H256> {
        // Kiểm tra trong cache trước
        let cache_key = Self::create_cache_key(filter);
        {
            let cache = self.search_cache.read().await;
            if let Some((cached_result, timestamp)) = cache.get(&cache_key) {
                let age = Utc::now().signed_duration_since(*timestamp).num_seconds();
                if age < MAX_CACHE_TIME_SECONDS {
                    return cached_result.clone();
                }
            }
        }
        
        let mut result_set: Option<HashSet<H256>> = None;
        
        // Lọc theo user ID nếu có
        if let Some(ref user_id) = filter.user_id {
            let user_index = self.user_index.read().await;
            if let Some(user_txs) = user_index.get(user_id) {
                result_set = Some(user_txs.clone());
            } else {
                // Không có kết quả nếu không tìm thấy user
                return vec![];
            }
        }
        
        // Lọc theo địa chỉ gửi nếu có
        if let Some(from_address) = filter.from_address {
            let from_index = self.from_address_index.read().await;
            if let Some(from_txs) = from_index.get(&from_address) {
                result_set = match result_set {
                    Some(set) => Some(set.intersection(from_txs).cloned().collect()),
                    None => Some(from_txs.clone()),
                };
            } else if result_set.is_some() {
                // Không có kết quả nếu không tìm thấy địa chỉ gửi
                return vec![];
            }
        }
        
        // Lọc theo địa chỉ nhận nếu có
        if let Some(to_address) = filter.to_address {
            let to_index = self.to_address_index.read().await;
            if let Some(to_txs) = to_index.get(&to_address) {
                result_set = match result_set {
                    Some(set) => Some(set.intersection(to_txs).cloned().collect()),
                    None => Some(to_txs.clone()),
                };
            } else if result_set.is_some() {
                // Không có kết quả nếu không tìm thấy địa chỉ nhận
                return vec![];
            }
        }
        
        // Lọc theo trạng thái nếu có
        if let Some(status) = filter.status {
            let status_index = self.status_index.read().await;
            if let Some(status_txs) = status_index.get(&status) {
                result_set = match result_set {
                    Some(set) => Some(set.intersection(status_txs).cloned().collect()),
                    None => Some(status_txs.clone()),
                };
            } else if result_set.is_some() {
                // Không có kết quả nếu không tìm thấy trạng thái
                return vec![];
            }
        }
        
        // Lọc theo chain ID nếu có
        if let Some(chain_id) = filter.chain_id {
            let chain_index = self.chain_id_index.read().await;
            if let Some(chain_txs) = chain_index.get(&chain_id) {
                result_set = match result_set {
                    Some(set) => Some(set.intersection(chain_txs).cloned().collect()),
                    None => Some(chain_txs.clone()),
                };
            } else if result_set.is_some() {
                // Không có kết quả nếu không tìm thấy chain ID
                return vec![];
            }
        }
        
        // Áp dụng các bộ lọc còn lại trên các giao dịch thực tế
        let tx_hashes = match result_set {
            Some(set) => set.into_iter().collect::<Vec<_>>(),
            None => transactions.iter().map(|tx| tx.tx_hash).collect::<Vec<_>>(),
        };
        
        let filtered_transactions: Vec<&TransactionInfo> = transactions
            .iter()
            .filter(|tx| tx_hashes.contains(&tx.tx_hash))
            .filter(|tx| Self::match_advanced_filters(tx, filter))
            .collect();
        
        // Sắp xếp kết quả nếu có yêu cầu
        let mut sorted_transactions = filtered_transactions;
        if let Some(sort_field) = filter.sort_by {
            match sort_field {
                SortField::Timestamp => {
                    sorted_transactions.sort_by(|a, b| {
                        if filter.sort_direction == SortDirection::Ascending {
                            a.timestamp.cmp(&b.timestamp)
                        } else {
                            b.timestamp.cmp(&a.timestamp)
                        }
                    });
                },
                SortField::Amount => {
                    sorted_transactions.sort_by(|a, b| {
                        if filter.sort_direction == SortDirection::Ascending {
                            a.amount.cmp(&b.amount)
                        } else {
                            b.amount.cmp(&a.amount)
                        }
                    });
                },
                SortField::BlockNumber => {
                    sorted_transactions.sort_by(|a, b| {
                        match (a.block_number, b.block_number) {
                            (Some(a_block), Some(b_block)) => {
                                if filter.sort_direction == SortDirection::Ascending {
                                    a_block.cmp(&b_block)
                                } else {
                                    b_block.cmp(&a_block)
                                }
                            },
                            (Some(_), None) => std::cmp::Ordering::Less,
                            (None, Some(_)) => std::cmp::Ordering::Greater,
                            (None, None) => std::cmp::Ordering::Equal,
                        }
                    });
                },
                SortField::GasPrice => {
                    sorted_transactions.sort_by(|a, b| {
                        if filter.sort_direction == SortDirection::Ascending {
                            a.gas_price.cmp(&b.gas_price)
                        } else {
                            b.gas_price.cmp(&a.gas_price)
                        }
                    });
                },
            }
        }
        
        // Phân trang
        let page = filter.page;
        let page_size = if filter.page_size == 0 { 50 } else { filter.page_size };
        let start_idx = page * page_size;
        
        let paged_tx_hashes: Vec<H256> = sorted_transactions
            .into_iter()
            .skip(start_idx)
            .take(page_size)
            .map(|tx| tx.tx_hash)
            .collect();
        
        // Lưu kết quả vào cache
        {
            let mut cache = self.search_cache.write().await;
            cache.insert(cache_key, (paged_tx_hashes.clone(), Utc::now()));
            
            // Dọn cache cũ
            let now = Utc::now();
            cache.retain(|_, (_, timestamp)| {
                now.signed_duration_since(*timestamp).num_seconds() < MAX_CACHE_TIME_SECONDS
            });
        }
        
        paged_tx_hashes
    }
    
    /// Kiểm tra giao dịch thỏa mãn bộ lọc nâng cao hay không
    fn match_advanced_filters(tx: &TransactionInfo, filter: &AdvancedTransactionFilter) -> bool {
        // Kiểm tra lọc theo giá trị
        if let Some(ref amount_filter) = filter.amount {
            if !Self::match_u256_filter(&tx.amount, amount_filter) {
                return false;
            }
        }
        
        // Kiểm tra lọc theo thời gian
        if let Some(ref time_filter) = filter.timestamp {
            if !Self::match_datetime_filter(&tx.timestamp, time_filter) {
                return false;
            }
        }
        
        // Kiểm tra lọc theo block number
        if let Some(block_number) = filter.block_number {
            if let Some(tx_block) = tx.block_number {
                if tx_block != block_number {
                    return false;
                }
            } else {
                return false;
            }
        }
        
        // Kiểm tra lọc theo gas price
        if let Some(ref gas_filter) = filter.gas_price {
            if !Self::match_u256_filter(&tx.gas_price, gas_filter) {
                return false;
            }
        }
        
        // Kiểm tra theo từ khóa
        if let Some(ref keyword) = filter.keyword {
            let keyword_lower = keyword.to_lowercase();
            let tx_hash_str = format!("{:?}", tx.tx_hash);
            
            // Tìm trong tx_hash
            if !tx_hash_str.to_lowercase().contains(&keyword_lower) {
                // Tìm trong user_id
                if !tx.user_id.to_lowercase().contains(&keyword_lower) {
                    // Tìm trong các trường khác nếu cần
                    return false;
                }
            }
        }
        
        true
    }
    
    /// Kiểm tra giá trị U256 thỏa mãn bộ lọc hay không
    fn match_u256_filter(value: &U256, filter: &U256Filter) -> bool {
        match filter.comparison {
            ComparisonType::Equal => *value == filter.value,
            ComparisonType::GreaterThan => *value > filter.value,
            ComparisonType::LessThan => *value < filter.value,
            ComparisonType::GreaterThanOrEqual => *value >= filter.value,
            ComparisonType::LessThanOrEqual => *value <= filter.value,
            ComparisonType::NotEqual => *value != filter.value,
            ComparisonType::InRange => {
                if let Some(second_value) = filter.second_value {
                    *value >= filter.value && *value <= second_value
                } else {
                    false
                }
            }
        }
    }
    
    /// Kiểm tra thời gian thỏa mãn bộ lọc hay không
    fn match_datetime_filter(value: &DateTime<Utc>, filter: &DateTimeFilter) -> bool {
        match filter.comparison {
            ComparisonType::Equal => *value == filter.value,
            ComparisonType::GreaterThan => *value > filter.value,
            ComparisonType::LessThan => *value < filter.value,
            ComparisonType::GreaterThanOrEqual => *value >= filter.value,
            ComparisonType::LessThanOrEqual => *value <= filter.value,
            ComparisonType::NotEqual => *value != filter.value,
            ComparisonType::InRange => {
                if let Some(second_value) = filter.second_value {
                    *value >= filter.value && *value <= second_value
                } else {
                    false
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use uuid::Uuid;
    
    fn create_test_transaction(id: u64, user_id: &str, status: TransactionStatus) -> TransactionInfo {
        TransactionInfo {
            id: Uuid::new_v4(),
            tx_hash: H256::from_low_u64_be(id),
            from_address: Address::from_str("0x1111111111111111111111111111111111111111").unwrap(),
            to_address: Some(Address::from_str("0x2222222222222222222222222222222222222222").unwrap()),
            amount: U256::from(100 * id),
            status,
            timestamp: Utc::now(),
            user_id: user_id.to_string(),
            chain_id: 1,
            block_number: Some(12345 + id as u64),
            gas_used: Some(U256::from(21000)),
            gas_price: U256::from(20_000_000_000u64),
            data: Vec::new(),
        }
    }
    
    #[tokio::test]
    async fn test_indexer() {
        let indexer = TransactionIndexer::new();
        let tx = create_test_transaction(1, "user1", TransactionStatus::Confirmed);
        
        indexer.index_transaction(&tx).await.unwrap();
        
        let filter = AdvancedTransactionFilter {
            user_id: Some("user1".to_string()),
            ..Default::default()
        };
        
        let results = indexer.find_transactions(&filter, &[tx.clone()]).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], tx.tx_hash);
    }
    
    #[tokio::test]
    async fn test_multiple_filters() {
        let indexer = TransactionIndexer::new();
        let transactions = vec![
            create_test_transaction(1, "user1", TransactionStatus::Confirmed),
            create_test_transaction(2, "user1", TransactionStatus::Pending),
            create_test_transaction(3, "user2", TransactionStatus::Confirmed),
            create_test_transaction(4, "user2", TransactionStatus::Failed),
        ];
        
        for tx in &transactions {
            indexer.index_transaction(tx).await.unwrap();
        }
        
        // Lọc theo user và status
        let filter = AdvancedTransactionFilter {
            user_id: Some("user1".to_string()),
            status: Some(TransactionStatus::Confirmed),
            ..Default::default()
        };
        
        let results = indexer.find_transactions(&filter, &transactions).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], H256::from_low_u64_be(1));
        
        // Lọc theo u256 value
        let filter = AdvancedTransactionFilter {
            amount: Some(U256Filter {
                value: U256::from(200),
                second_value: None,
                comparison: ComparisonType::GreaterThanOrEqual,
            }),
            ..Default::default()
        };
        
        let results = indexer.find_transactions(&filter, &transactions).await;
        assert_eq!(results.len(), 3);
    }
    
    #[tokio::test]
    async fn test_sorting_and_paging() {
        let indexer = TransactionIndexer::new();
        let mut transactions = vec![];
        
        // Tạo 20 giao dịch với giá trị khác nhau
        for i in 1..21 {
            let status = if i % 2 == 0 { TransactionStatus::Confirmed } else { TransactionStatus::Pending };
            let tx = create_test_transaction(i, "user1", status);
            indexer.index_transaction(&tx).await.unwrap();
            transactions.push(tx);
        }
        
        // Kiểm tra sắp xếp tăng dần theo amount
        let filter = AdvancedTransactionFilter {
            user_id: Some("user1".to_string()),
            page: 0,
            page_size: 5,
            sort_by: Some(SortField::Amount),
            sort_direction: SortDirection::Ascending,
            ..Default::default()
        };
        
        let results = indexer.find_transactions(&filter, &transactions).await;
        assert_eq!(results.len(), 5);
        
        // Các giá trị nên tăng dần
        let result_transactions: Vec<&TransactionInfo> = results
            .iter()
            .filter_map(|hash| transactions.iter().find(|tx| tx.tx_hash == *hash))
            .collect();
        
        for i in 1..result_transactions.len() {
            assert!(result_transactions[i-1].amount <= result_transactions[i].amount);
        }
        
        // Kiểm tra phân trang
        let filter = AdvancedTransactionFilter {
            user_id: Some("user1".to_string()),
            page: 1, // Trang thứ 2
            page_size: 5,
            sort_by: Some(SortField::Amount),
            sort_direction: SortDirection::Ascending,
            ..Default::default()
        };
        
        let page2_results = indexer.find_transactions(&filter, &transactions).await;
        assert_eq!(page2_results.len(), 5);
        
        // Trang 2 phải khác trang 1
        assert!(results.iter().all(|hash| !page2_results.contains(hash)));
    }
} 