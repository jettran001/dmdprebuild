use std::sync::Arc;
use tokio::sync::Mutex;

/// Service quản lý hàng đợi giao dịch và xử lý concurrency
/// 
/// Sử dụng Redis queue kết hợp với tokio::sync::Mutex để tránh double-sign 
/// và race-condition khi nhiều người cùng yêu cầu ký transaction.
pub struct QueueManagementService {
    /// Redis queue cho các transaction đang chờ xử lý
    pub redis_queue: RedisQueueManager,
    
    /// Global transaction checker ngăn chặn double-sign
    pub tx_checker: Arc<Mutex<GlobalTxChecker>>,
    
    /// Quản lý nonce để tránh race-condition 
    pub nonce_manager: Arc<Mutex<NonceManager>>,
    
    /// Phân loại và ưu tiên transaction
    pub tx_prioritization: TransactionPrioritization,
}

impl QueueManagementService {
    /// Tạo mới service quản lý hàng đợi
    pub fn new(redis_url: &str) -> Self {
        Self {
            redis_queue: RedisQueueManager::new(redis_url),
            tx_checker: Arc::new(Mutex::new(GlobalTxChecker::new())),
            nonce_manager: Arc::new(Mutex::new(NonceManager::new())),
            tx_prioritization: TransactionPrioritization::new(),
        }
    }
    
    /// Thêm transaction vào hàng đợi xử lý
    pub async fn enqueue_transaction(&self, tx: Transaction) -> Result<String, QueueError> {
        // Kiểm tra trùng lặp transaction
        {
            let mut checker = self.tx_checker.lock().await;
            if checker.is_duplicate(&tx) {
                return Err(QueueError::DuplicateTransaction);
            }
            checker.mark_transaction(&tx);
        }
        
        // Quản lý nonce để tránh race-condition
        {
            let mut nonce_mgr = self.nonce_manager.lock().await;
            nonce_mgr.reserve_nonce(&tx).await?;
        }
        
        // Phân loại độ ưu tiên
        let priority = self.tx_prioritization.get_priority(&tx);
        
        // Thêm vào Redis queue
        let tx_id = self.redis_queue.push_transaction(tx, priority).await?;
        
        Ok(tx_id)
    }
    
    /// Lấy transaction tiếp theo từ hàng đợi
    pub async fn dequeue_transaction(&self) -> Result<Option<Transaction>, QueueError> {
        self.redis_queue.pop_transaction().await
    }
    
    /// Hoàn thành xử lý transaction và cập nhật trạng thái
    pub async fn complete_transaction(&self, tx_id: &str, success: bool) -> Result<(), QueueError> {
        let result = self.redis_queue.mark_complete(tx_id, success).await?;
        
        if let Some(tx) = result {
            // Giải phóng nonce nếu transaction thất bại
            if !success {
                let mut nonce_mgr = self.nonce_manager.lock().await;
                nonce_mgr.release_nonce(&tx).await?;
            }
            
            // Xóa khỏi transaction checker
            let mut checker = self.tx_checker.lock().await;
            checker.remove_transaction(&tx);
        }
        
        Ok(())
    }
}

/// Quản lý Redis queue cho transaction
pub struct RedisQueueManager {
    _redis_url: String,
}

impl RedisQueueManager {
    /// Tạo mới Redis queue manager
    pub fn new(redis_url: &str) -> Self {
        Self {
            _redis_url: redis_url.to_string(),
        }
    }
    
    /// Thêm transaction vào queue
    pub async fn push_transaction(&self, _tx: Transaction, _priority: u8) -> Result<String, QueueError> {
        // TODO: Triển khai kết nối Redis và push transaction
        Ok("tx_id_placeholder".to_string())
    }
    
    /// Lấy transaction tiếp theo từ queue
    pub async fn pop_transaction(&self) -> Result<Option<Transaction>, QueueError> {
        // TODO: Triển khai kết nối Redis và pop transaction
        Ok(None)
    }
    
    /// Đánh dấu transaction đã hoàn thành
    pub async fn mark_complete(&self, _tx_id: &str, _success: bool) -> Result<Option<Transaction>, QueueError> {
        // TODO: Triển khai cập nhật trạng thái transaction
        Ok(None)
    }
}

/// Kiểm tra trùng lặp transaction
pub struct GlobalTxChecker {
    // Giữ danh sách các transaction hash đang xử lý
    pending_txs: Vec<String>,
}

impl Default for GlobalTxChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalTxChecker {
    /// Tạo mới global transaction checker
    pub fn new() -> Self {
        Self {
            pending_txs: Vec::new(),
        }
    }
    
    /// Kiểm tra transaction có trùng lặp không
    pub fn is_duplicate(&self, tx: &Transaction) -> bool {
        self.pending_txs.contains(&tx.hash)
    }
    
    /// Đánh dấu transaction đang xử lý
    pub fn mark_transaction(&mut self, tx: &Transaction) {
        self.pending_txs.push(tx.hash.clone());
    }
    
    /// Xóa transaction khỏi danh sách đang xử lý
    pub fn remove_transaction(&mut self, tx: &Transaction) {
        if let Some(pos) = self.pending_txs.iter().position(|x| *x == tx.hash) {
            self.pending_txs.remove(pos);
        }
    }
}

/// Quản lý nonce cho transaction để tránh race-condition
pub struct NonceManager {
    // Lưu các nonce đang được sử dụng cho từng địa chỉ
    address_nonces: std::collections::HashMap<String, Vec<u64>>,
}

impl Default for NonceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl NonceManager {
    /// Tạo mới nonce manager
    pub fn new() -> Self {
        Self {
            address_nonces: std::collections::HashMap::new(),
        }
    }
    
    /// Kiểm tra và đặt chỗ nonce cho transaction
    pub async fn reserve_nonce(&mut self, tx: &Transaction) -> Result<(), QueueError> {
        let nonces = self.address_nonces.entry(tx.from_address.clone()).or_default();
        
        if nonces.contains(&tx.nonce) {
            return Err(QueueError::NonceAlreadyUsed);
        }
        
        nonces.push(tx.nonce);
        Ok(())
    }
    
    /// Giải phóng nonce đã đặt chỗ
    pub async fn release_nonce(&mut self, tx: &Transaction) -> Result<(), QueueError> {
        if let Some(nonces) = self.address_nonces.get_mut(&tx.from_address) {
            if let Some(pos) = nonces.iter().position(|&x| x == tx.nonce) {
                nonces.remove(pos);
            }
        }
        
        Ok(())
    }
}

/// Phân loại và ưu tiên transaction
pub struct TransactionPrioritization {
    // TODO: Thêm thông tin cấu hình cho ưu tiên
}

impl Default for TransactionPrioritization {
    fn default() -> Self {
        Self::new()
    }
}

impl TransactionPrioritization {
    /// Tạo mới service phân loại ưu tiên
    pub fn new() -> Self {
        Self {}
    }
    
    /// Xác định mức độ ưu tiên cho transaction
    pub fn get_priority(&self, _tx: &Transaction) -> u8 {
        // TODO: Triển khai logic phân loại
        // - Dựa vào gas price, loại giao dịch, giá trị, v.v.
        1
    }
}

/// Transaction struct
pub struct Transaction {
    /// Transaction hash
    pub hash: String,
    
    /// Địa chỉ gửi
    pub from_address: String,
    
    /// Địa chỉ nhận
    pub to: String,
    
    /// Nonce
    pub nonce: u64,
    
    /// Gas price
    pub gas_price: u64,
    
    /// Gas limit
    pub gas_limit: u64,
    
    /// Giá trị chuyển
    pub value: u64,
    
    /// Data
    pub data: Vec<u8>,
}

/// Lỗi quản lý queue
#[derive(Debug)]
pub enum QueueError {
    /// Transaction trùng lặp
    DuplicateTransaction,
    
    /// Nonce đã được sử dụng
    NonceAlreadyUsed,
    
    /// Lỗi Redis
    RedisError(String),
    
    /// Lỗi khác
    Other(String),
}

impl From<String> for QueueError {
    fn from(error: String) -> Self {
        QueueError::Other(error)
    }
} 