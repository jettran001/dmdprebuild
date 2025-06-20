//! Module quản lý lịch sử giao dịch với khả năng xử lý hiệu quả các danh sách giao dịch lớn.
//!
//! Module này cung cấp các chức năng:
//! - Lưu trữ và truy xuất giao dịch sử dụng phương pháp phân trang
//! - Stream dữ liệu giao dịch thay vì tải tất cả vào bộ nhớ
//! - Cache thông minh cho các giao dịch thường xuyên truy cập
//! - Tối ưu hóa hiệu năng khi xử lý lượng giao dịch lớn

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use ethers::types::{Address, Transaction, TransactionReceipt, H256, U256};
use chrono::{DateTime, Utc};
use anyhow::Context;
use tracing::{debug, error, info, warn};
use futures::{Stream, StreamExt};
use std::pin::Pin;
use uuid::Uuid;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// Internal imports
use crate::error::{WalletError, Result};
use crate::cache::{CacheSystem, CacheConfig};

/// Kích thước mặc định cho một trang giao dịch
const DEFAULT_PAGE_SIZE: usize = 50;

/// Số trang tối đa được cache
const MAX_CACHED_PAGES: usize = 5;

/// Trạng thái giao dịch
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionStatus {
    /// Đang chờ xử lý
    Pending,
    /// Đã xác nhận
    Confirmed,
    /// Thất bại
    Failed,
    /// Đã hết hạn
    Expired,
}

impl Default for TransactionStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// Thông tin chi tiết về giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionInfo {
    /// ID giao dịch
    pub id: Uuid,
    /// Hash giao dịch blockchain
    pub tx_hash: H256,
    /// Địa chỉ ví người gửi
    pub from_address: Address,
    /// Địa chỉ ví người nhận
    pub to_address: Option<Address>,
    /// Số lượng token
    pub amount: U256,
    /// Trạng thái giao dịch
    pub status: TransactionStatus,
    /// Thời điểm tạo giao dịch
    pub timestamp: DateTime<Utc>,
    /// ID người dùng
    pub user_id: String,
    /// Chain ID
    pub chain_id: u64,
    /// Block number (nếu đã xác nhận)
    pub block_number: Option<u64>,
    /// Gas sử dụng (nếu đã xác nhận)
    pub gas_used: Option<U256>,
    /// Gas price
    pub gas_price: U256,
    /// Data (cho smart contract)
    pub data: Vec<u8>,
}

/// Bộ lọc giao dịch cho phép tìm kiếm hiệu quả
#[derive(Debug, Clone, Default)]
pub struct TransactionFilter {
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
    /// Lọc theo khoảng thời gian (từ)
    pub from_date: Option<DateTime<Utc>>,
    /// Lọc theo khoảng thời gian (đến)
    pub to_date: Option<DateTime<Utc>>,
    /// Số trang (bắt đầu từ 0)
    pub page: usize,
    /// Kích thước trang
    pub page_size: usize,
}

/// Kết quả phân trang giao dịch
#[derive(Debug, Clone)]
pub struct PaginatedTransactions {
    /// Danh sách giao dịch trong trang hiện tại
    pub transactions: Vec<TransactionInfo>,
    /// Tổng số giao dịch thỏa mãn điều kiện lọc
    pub total: usize,
    /// Trang hiện tại
    pub page: usize,
    /// Kích thước trang
    pub page_size: usize,
    /// Có trang tiếp theo hay không
    pub has_next: bool,
}

/// Trait định nghĩa các phương thức truy xuất lịch sử giao dịch
#[async_trait]
pub trait TransactionHistoryProvider: Send + Sync + 'static {
    /// Thêm giao dịch mới vào lịch sử
    async fn add_transaction(&self, transaction: TransactionInfo) -> Result<()>;
    
    /// Cập nhật trạng thái giao dịch
    async fn update_transaction_status(&self, tx_hash: &H256, status: TransactionStatus) -> Result<()>;
    
    /// Lấy thông tin giao dịch theo hash
    async fn get_transaction(&self, tx_hash: &H256) -> Result<Option<TransactionInfo>>;
    
    /// Lấy danh sách giao dịch phân trang theo bộ lọc
    async fn get_transactions(&self, filter: &TransactionFilter) -> Result<PaginatedTransactions>;
    
    /// Stream giao dịch theo bộ lọc
    async fn stream_transactions<'a>(&'a self, filter: &'a TransactionFilter) 
        -> Pin<Box<dyn Stream<Item = Result<TransactionInfo>> + Send + 'a>>;
}

/// Cài đặt mặc định cho TransactionHistoryProvider sử dụng bộ nhớ với cache
pub struct InMemoryTransactionHistory {
    /// Dữ liệu giao dịch trong bộ nhớ
    transactions: Arc<RwLock<Vec<TransactionInfo>>>,
    
    /// Cache cho các trang giao dịch
    page_cache: Arc<RwLock<HashMap<String, (PaginatedTransactions, DateTime<Utc>)>>>,
    
    /// Cache cho các giao dịch riêng lẻ
    tx_cache: CacheSystem<H256, TransactionInfo>,
}

impl InMemoryTransactionHistory {
    /// Tạo mới một instance InMemoryTransactionHistory
    pub fn new() -> Self {
        let cache_config = CacheConfig {
            max_size: 1000,
            ttl_seconds: 600, // 10 phút
        };
        
        Self {
            transactions: Arc::new(RwLock::new(Vec::new())),
            page_cache: Arc::new(RwLock::new(HashMap::new())),
            tx_cache: CacheSystem::new(cache_config),
        }
    }
    
    /// Tạo khóa cache từ bộ lọc
    fn create_cache_key(filter: &TransactionFilter) -> String {
        let mut key = format!("page:{}_size:{}", filter.page, filter.page_size);
        
        if let Some(ref user_id) = filter.user_id {
            key.push_str(&format!("_user:{}", user_id));
        }
        
        if let Some(ref from) = filter.from_address {
            key.push_str(&format!("_from:{}", from));
        }
        
        if let Some(ref to) = filter.to_address {
            key.push_str(&format!("_to:{}", to));
        }
        
        if let Some(status) = filter.status {
            key.push_str(&format!("_status:{:?}", status));
        }
        
        if let Some(chain) = filter.chain_id {
            key.push_str(&format!("_chain:{}", chain));
        }
        
        key
    }
    
    /// Lọc giao dịch theo bộ lọc
    fn filter_matches(&self, tx: &TransactionInfo, filter: &TransactionFilter) -> bool {
        // Kiểm tra user ID
        if let Some(ref user_id) = filter.user_id {
            if tx.user_id != *user_id {
                return false;
            }
        }
        
        // Kiểm tra địa chỉ gửi
        if let Some(from) = filter.from_address {
            if tx.from_address != from {
                return false;
            }
        }
        
        // Kiểm tra địa chỉ nhận
        if let Some(to) = filter.to_address {
            if let Some(tx_to) = tx.to_address {
                if tx_to != to {
                    return false;
                }
            } else {
                return false;
            }
        }
        
        // Kiểm tra trạng thái
        if let Some(status) = filter.status {
            if tx.status != status {
                return false;
            }
        }
        
        // Kiểm tra chain ID
        if let Some(chain_id) = filter.chain_id {
            if tx.chain_id != chain_id {
                return false;
            }
        }
        
        // Kiểm tra khoảng thời gian
        if let Some(from_date) = filter.from_date {
            if tx.timestamp < from_date {
                return false;
            }
        }
        
        if let Some(to_date) = filter.to_date {
            if tx.timestamp > to_date {
                return false;
            }
        }
        
        true
    }
    
    /// Xóa các cache đã hết hạn
    async fn clean_expired_cache(&self) {
        let now = Utc::now();
        let mut cache = self.page_cache.write().await;
        
        cache.retain(|_, (_, timestamp)| {
            now.signed_duration_since(*timestamp).num_seconds() < 600 // 10 phút
        });
    }
}

#[async_trait]
impl TransactionHistoryProvider for InMemoryTransactionHistory {
    async fn add_transaction(&self, transaction: TransactionInfo) -> Result<()> {
        // Thêm vào cache giao dịch đơn
        self.tx_cache.insert(transaction.tx_hash, transaction.clone()).await?;
        
        // Thêm vào danh sách giao dịch
        let mut transactions = self.transactions.write().await;
        transactions.push(transaction);
        
        // Xóa cache trang để tránh dữ liệu không đồng bộ
        let mut page_cache = self.page_cache.write().await;
        page_cache.clear();
        
        Ok(())
    }
    
    async fn update_transaction_status(&self, tx_hash: &H256, status: TransactionStatus) -> Result<()> {
        // Cập nhật trong cache giao dịch đơn
        if let Some(mut tx) = self.tx_cache.get(tx_hash).await? {
            tx.status = status;
            self.tx_cache.insert(*tx_hash, tx).await?;
        }
        
        // Cập nhật trong danh sách giao dịch
        let mut transactions = self.transactions.write().await;
        for tx in transactions.iter_mut() {
            if tx.tx_hash == *tx_hash {
                tx.status = status;
                break;
            }
        }
        
        // Xóa cache trang để tránh dữ liệu không đồng bộ
        let mut page_cache = self.page_cache.write().await;
        page_cache.clear();
        
        Ok(())
    }
    
    async fn get_transaction(&self, tx_hash: &H256) -> Result<Option<TransactionInfo>> {
        // Thử lấy từ cache trước
        if let Some(tx) = self.tx_cache.get(tx_hash).await? {
            return Ok(Some(tx));
        }
        
        // Nếu không có trong cache, tìm trong danh sách giao dịch
        let transactions = self.transactions.read().await;
        for tx in transactions.iter() {
            if &tx.tx_hash == tx_hash {
                // Thêm vào cache để lần sau truy xuất nhanh hơn
                let tx_clone = tx.clone();
                self.tx_cache.insert(*tx_hash, tx_clone.clone()).await?;
                return Ok(Some(tx_clone));
            }
        }
        
        Ok(None)
    }
    
    async fn get_transactions(&self, filter: &TransactionFilter) -> Result<PaginatedTransactions> {
        // Xóa cache hết hạn
        self.clean_expired_cache().await;
        
        let page = filter.page;
        let page_size = if filter.page_size == 0 { DEFAULT_PAGE_SIZE } else { filter.page_size };
        
        // Kiểm tra trong cache trước
        let cache_key = Self::create_cache_key(filter);
        let page_cache = self.page_cache.read().await;
        
        if let Some((cached_result, _)) = page_cache.get(&cache_key) {
            return Ok(cached_result.clone());
        }
        drop(page_cache);
        
        // Nếu không có trong cache, tìm và lọc
        let transactions = self.transactions.read().await;
        
        // Lọc theo các điều kiện
        let mut filtered: Vec<TransactionInfo> = transactions
            .iter()
            .filter(|tx| self.filter_matches(tx, filter))
            .cloned()
            .collect();
        
        // Sắp xếp theo thời gian giảm dần (mới nhất đầu tiên)
        filtered.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        
        // Tính toán phân trang
        let total = filtered.len();
        let start_idx = page * page_size;
        let end_idx = (start_idx + page_size).min(total);
        
        let has_next = end_idx < total;
        let page_items = if start_idx < total {
            filtered[start_idx..end_idx].to_vec()
        } else {
            Vec::new()
        };
        
        let result = PaginatedTransactions {
            transactions: page_items,
            total,
            page,
            page_size,
            has_next,
        };
        
        // Lưu kết quả vào cache
        let mut page_cache = self.page_cache.write().await;
        
        // Kiểm tra kích thước cache và xóa một số nếu quá lớn
        if page_cache.len() >= MAX_CACHED_PAGES {
            // Xóa cache cũ nhất
            if let Some((oldest_key, _)) = page_cache
                .iter()
                .min_by_key(|(_, (_, timestamp))| timestamp) {
                let oldest_key = oldest_key.clone();
                page_cache.remove(&oldest_key);
            }
        }
        
        page_cache.insert(cache_key, (result.clone(), Utc::now()));
        
        Ok(result)
    }
    
    async fn stream_transactions<'a>(&'a self, filter: &'a TransactionFilter) 
        -> Pin<Box<dyn Stream<Item = Result<TransactionInfo>> + Send + 'a>> {
        // Tạo stream để xử lý các giao dịch một cách hiệu quả
        let transactions_arc = self.transactions.clone();
        let filter = filter.clone();
        let this = self.clone();
        
        // Sử dụng stream để xử lý từng phần thay vì tải tất cả vào bộ nhớ
        Box::pin(async_stream::stream! {
            let transactions = transactions_arc.read().await;
            let mut batch = Vec::with_capacity(50);
            
            for tx in transactions.iter() {
                if this.filter_matches(tx, &filter) {
                    batch.push(tx.clone());
                    
                    if batch.len() >= 50 {
                        for item in batch.drain(..) {
                            yield Ok(item);
                        }
                    }
                }
            }
            
            // Trả về phần còn lại
            for item in batch {
                yield Ok(item);
            }
        })
    }
}

impl Clone for InMemoryTransactionHistory {
    fn clone(&self) -> Self {
        Self {
            transactions: self.transactions.clone(),
            page_cache: self.page_cache.clone(),
            tx_cache: self.tx_cache.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    
    fn create_test_transaction(id: u64, user_id: &str, status: TransactionStatus) -> TransactionInfo {
        TransactionInfo {
            id: Uuid::new_v4(),
            tx_hash: H256::from_low_u64_be(id),
            from_address: Address::from_str("0x1111111111111111111111111111111111111111").unwrap(),
            to_address: Some(Address::from_str("0x2222222222222222222222222222222222222222").unwrap()),
            amount: U256::from(100),
            status,
            timestamp: Utc::now(),
            user_id: user_id.to_string(),
            chain_id: 1,
            block_number: Some(12345),
            gas_used: Some(U256::from(21000)),
            gas_price: U256::from(20_000_000_000u64),
            data: Vec::new(),
        }
    }
    
    #[tokio::test]
    async fn test_add_and_get_transaction() {
        let history = InMemoryTransactionHistory::new();
        let tx = create_test_transaction(1, "user1", TransactionStatus::Pending);
        
        history.add_transaction(tx.clone()).await.unwrap();
        
        let result = history.get_transaction(&tx.tx_hash).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().tx_hash, tx.tx_hash);
    }
    
    #[tokio::test]
    async fn test_update_transaction_status() {
        let history = InMemoryTransactionHistory::new();
        let tx = create_test_transaction(2, "user1", TransactionStatus::Pending);
        
        history.add_transaction(tx.clone()).await.unwrap();
        history.update_transaction_status(&tx.tx_hash, TransactionStatus::Confirmed).await.unwrap();
        
        let result = history.get_transaction(&tx.tx_hash).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().status, TransactionStatus::Confirmed);
    }
    
    #[tokio::test]
    async fn test_pagination() {
        let history = InMemoryTransactionHistory::new();
        
        // Thêm 100 giao dịch
        for i in 0..100 {
            let status = if i % 2 == 0 { TransactionStatus::Confirmed } else { TransactionStatus::Pending };
            let user_id = if i % 3 == 0 { "user1" } else { "user2" };
            let tx = create_test_transaction(i, user_id, status);
            history.add_transaction(tx).await.unwrap();
        }
        
        // Kiểm tra phân trang với kích thước trang = 10
        let filter = TransactionFilter {
            page: 0,
            page_size: 10,
            ..Default::default()
        };
        
        let result = history.get_transactions(&filter).await.unwrap();
        assert_eq!(result.transactions.len(), 10);
        assert_eq!(result.total, 100);
        assert_eq!(result.page, 0);
        assert_eq!(result.page_size, 10);
        assert!(result.has_next);
        
        // Kiểm tra trang cuối
        let filter = TransactionFilter {
            page: 9,
            page_size: 10,
            ..Default::default()
        };
        
        let result = history.get_transactions(&filter).await.unwrap();
        assert_eq!(result.transactions.len(), 10);
        assert_eq!(result.page, 9);
        assert!(!result.has_next);
        
        // Kiểm tra lọc theo user
        let filter = TransactionFilter {
            user_id: Some("user1".to_string()),
            page: 0,
            page_size: 20,
            ..Default::default()
        };
        
        let result = history.get_transactions(&filter).await.unwrap();
        assert!(result.transactions.len() <= 20); // <= 20 vì chỉ có 1/3 số giao dịch là của user1
        assert!(result.total <= 100); // <= 100 vì đã lọc
        assert!(result.transactions.iter().all(|tx| tx.user_id == "user1"));
        
        // Kiểm tra lọc theo trạng thái
        let filter = TransactionFilter {
            status: Some(TransactionStatus::Confirmed),
            page: 0,
            page_size: 30,
            ..Default::default()
        };
        
        let result = history.get_transactions(&filter).await.unwrap();
        assert!(result.transactions.len() <= 30); // <= 30 vì chỉ có 1/2 số giao dịch là Confirmed
        assert!(result.transactions.iter().all(|tx| tx.status == TransactionStatus::Confirmed));
    }
    
    #[tokio::test]
    async fn test_stream_transactions() {
        let history = InMemoryTransactionHistory::new();
        
        // Thêm 50 giao dịch
        for i in 0..50 {
            let status = if i % 2 == 0 { TransactionStatus::Confirmed } else { TransactionStatus::Pending };
            let tx = create_test_transaction(i, "user1", status);
            history.add_transaction(tx).await.unwrap();
        }
        
        // Kiểm tra stream
        let filter = TransactionFilter {
            user_id: Some("user1".to_string()),
            ..Default::default()
        };
        
        let mut stream = history.stream_transactions(&filter).await;
        let mut count = 0;
        
        while let Some(result) = stream.next().await {
            assert!(result.is_ok());
            count += 1;
        }
        
        assert_eq!(count, 50);
    }
} 