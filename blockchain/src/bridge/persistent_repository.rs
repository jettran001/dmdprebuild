//! Module cung cấp persistent storage cho các giao dịch bridge

use crate::bridge::transaction::{BridgeTransaction, BridgeTransactionRepository, BridgeTransactionStatus};
use crate::bridge::error::BridgeError;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use log::{debug, error, info, warn};
use serde_json::{Map, Value};
use tokio::sync::mpsc;
use anyhow::{Result, Context, anyhow};

/// Repository lưu trữ giao dịch bridge vào file JSON
pub struct JsonBridgeTransactionRepository {
    /// Đường dẫn tới file lưu trữ
    file_path: PathBuf,
    /// Cache trong bộ nhớ để truy cập nhanh
    cache: RwLock<HashMap<String, BridgeTransaction>>,
    /// Kênh để gửi yêu cầu lưu trữ bất đồng bộ
    save_tx: Option<mpsc::Sender<SaveOperation>>,
}

/// Enum chỉ định loại thao tác lưu trữ
enum SaveOperation {
    /// Lưu một giao dịch
    SaveTransaction(BridgeTransaction),
    /// Xóa một giao dịch
    DeleteTransaction(String),
    /// Xóa nhiều giao dịch
    BulkDelete(Vec<String>),
    /// Đánh dấu cần đồng bộ ngay
    Flush,
}

impl JsonBridgeTransactionRepository {
    /// Tạo mới repository
    pub fn new(file_path: impl AsRef<Path>) -> Result<Self> {
        let file_path = file_path.as_ref().to_path_buf();
        let cache = RwLock::new(HashMap::new());
        
        // Đảm bảo thư mục cha tồn tại
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).context("Không thể tạo thư mục cha")?;
        }
        
        // Tạo file nếu chưa tồn tại
        if !file_path.exists() {
            File::create(&file_path).context("Không thể tạo file lưu trữ")?;
            fs::write(&file_path, "{}").context("Không thể ghi file lưu trữ ban đầu")?;
        } else {
            // Đọc dữ liệu hiện có
            let data = fs::read_to_string(&file_path).context("Không thể đọc file lưu trữ")?;
            if !data.trim().is_empty() {
                let transactions: HashMap<String, BridgeTransaction> = serde_json::from_str(&data)
                    .context("Không thể parse dữ liệu JSON từ file lưu trữ")?;
                cache.write()
                    .map_err(|e| anyhow!("Không thể lấy write lock cho cache: {}", e))?
                    .clone_from(&transactions);
            }
        }
        
        Ok(Self {
            file_path,
            cache,
            save_tx: None,
        })
    }
    
    /// Khởi động worker để lưu trữ bất đồng bộ
    pub fn start_async_worker(mut self) -> Self {
        let (tx, mut rx) = mpsc::channel::<SaveOperation>(100);
        self.save_tx = Some(tx);
        
        let file_path = self.file_path.clone();
        let cache = Arc::new(self.cache.clone());
        
        tokio::spawn(async move {
            let mut need_flush = false;
            
            while let Some(op) = rx.recv().await {
                match op {
                    SaveOperation::SaveTransaction(transaction) => {
                        let mut cache_guard = cache.write()
                            .map_err(|e| {
                                error!("Không thể lấy write lock khi lưu transaction: {}", e);
                                // Vẫn tiếp tục xử lý mà không panic
                            }).unwrap_or_else(|_| {
                                // Cố gắng lấy lock mới nếu lock bị poison
                                match cache.try_write() {
                                    Ok(guard) => guard,
                                    Err(e) => {
                                        error!("Không thể khôi phục sau poison lock: {}", e);
                                        // Tạo guard trống để tiếp tục hoạt động
                                        let new_cache = Arc::new(RwLock::new(HashMap::new()));
                                        match new_cache.write() {
                                            Ok(guard) => guard,
                                            Err(_) => panic!("Không thể tạo cache mới sau khi lock bị poison")
                                        }
                                    }
                                }
                            });
                        cache_guard.insert(transaction.id.clone(), transaction);
                        need_flush = true;
                    },
                    SaveOperation::DeleteTransaction(id) => {
                        let mut cache_guard = match cache.write() {
                            Ok(guard) => guard,
                            Err(e) => {
                                error!("Không thể lấy write lock khi xóa transaction: {}", e);
                                return;
                            }
                        };
                        cache_guard.remove(&id);
                        need_flush = true;
                    },
                    SaveOperation::BulkDelete(ids) => {
                        let mut cache_guard = match cache.write() {
                            Ok(guard) => guard,
                            Err(e) => {
                                error!("Không thể lấy write lock khi xóa nhiều transaction: {}", e);
                                return;
                            }
                        };
                        for id in ids {
                            cache_guard.remove(&id);
                        }
                        need_flush = true;
                    },
                    SaveOperation::Flush => {
                        need_flush = true;
                    }
                }
                
                if need_flush {
                    let cache_guard = cache.read()
                        .map_err(|e| {
                            error!("Không thể lấy read lock khi flush cache: {}", e);
                            return;
                        }).unwrap();
                    // Tránh ghi vào file quá thường xuyên
                    if let Err(e) = Self::write_to_file(&file_path, &cache_guard) {
                        error!("Không thể ghi dữ liệu vào file: {}", e);
                    }
                    need_flush = false;
                }
            }
        });
        
        self
    }
    
    /// Ghi dữ liệu vào file JSON
    fn write_to_file(file_path: &Path, transactions: &HashMap<String, BridgeTransaction>) -> Result<()> {
        let json = serde_json::to_string_pretty(&transactions)
            .context("Không thể chuyển dữ liệu thành JSON")?;
        
        // Đảm bảo thư mục cha tồn tại
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .context(format!("Không thể tạo thư mục cha: {:?}", parent))?;
        }
        
        // Kiểm tra quyền ghi vào thư mục cha
        if let Some(parent) = file_path.parent() {
            // Kiểm tra thư mục có tồn tại và có quyền ghi không
            let metadata = fs::metadata(parent)
                .context(format!("Không thể truy cập metadata của thư mục: {:?}", parent))?;
                
            if !metadata.is_dir() {
                return Err(anyhow!("Đường dẫn cha không phải là thư mục: {:?}", parent));
            }
            
            // Kiểm tra không gian đĩa (rất cơ bản)
            // Chỉ cảnh báo, không dừng chức năng
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                
                let avail_blocks = metadata.blocks();
                if avail_blocks < 1000 {  // Giới hạn tùy ý
                    warn!("Không gian đĩa thấp: {} blocks, có thể gây ra lỗi khi ghi file", avail_blocks);
                }
            }
        }
        
        // Tạo đường dẫn tới file tạm
        let temp_path = file_path.with_extension("tmp");
        
        // Xử lý backup file hiện tại nếu có
        if file_path.exists() {
            let backup_path = file_path.with_extension("bak");
            
            // Nếu tìm thấy file hiện tại, sao lưu nó
            if let Err(e) = fs::copy(file_path, &backup_path) {
                warn!("Không thể tạo backup file: {}", e);
                // Tiếp tục mà không dừng lại vì đây chỉ là bước bảo đảm
            }
        }
        
        // Ghi dữ liệu mới vào file tạm
        let file_result = File::create(&temp_path);
        let mut file = match file_result {
            Ok(file) => file,
            Err(e) => {
                match e.kind() {
                    io::ErrorKind::PermissionDenied => {
                        return Err(anyhow!("Không đủ quyền để tạo file: {:?} - {}", temp_path, e));
                    },
                    io::ErrorKind::OutOfMemory | io::ErrorKind::StorageFull => {
                        return Err(anyhow!("Không gian đĩa đầy hoặc hết bộ nhớ: {}", e));
                    },
                    _ => {
                        return Err(anyhow!("Lỗi không xác định khi tạo file tạm: {:?} - {}", temp_path, e));
                    }
                }
            }
        };
        
        // Ghi dữ liệu vào file
        match file.write_all(json.as_bytes()) {
            Ok(_) => {
                // Đảm bảo dữ liệu đã được ghi xuống đĩa
                if let Err(e) = file.sync_all() {
                    return Err(anyhow!("Không thể đồng bộ dữ liệu xuống đĩa: {}", e));
                }
                // Đóng file để giải phóng handle
                drop(file);
            },
            Err(e) => {
                // Xử lý lỗi khi ghi
                match e.kind() {
                    io::ErrorKind::StorageFull => {
                        return Err(anyhow!("Hết không gian đĩa khi ghi dữ liệu: {}", e));
                    },
                    io::ErrorKind::WriteZero => {
                        return Err(anyhow!("Không thể ghi dữ liệu: Đĩa đã đầy hoặc hạn ngạch bị vượt quá"));
                    },
                    _ => {
                        return Err(anyhow!("Lỗi không xác định khi ghi dữ liệu: {}", e));
                    }
                }
            }
        }
        
        // Atomically đổi tên file tạm thành file chính để đảm bảo tính nguyên vẹn
        if let Err(e) = fs::rename(&temp_path, file_path) {
            // Nếu đổi tên thất bại, thử sao chép
            error!("Không thể đổi tên file tạm, thử sao chép thay thế: {}", e);
            
            match fs::copy(&temp_path, file_path) {
                Ok(_) => {
                    // Xóa file tạm sau khi sao chép thành công
                    if let Err(e) = fs::remove_file(&temp_path) {
                        warn!("Không thể xóa file tạm sau khi sao chép: {}", e);
                        // Không dừng xử lý vì đã sao chép thành công
                    }
                },
                Err(e) => {
                    let err_msg = format!("Không thể sao chép file tạm: {}", e);
                    error!("{}", err_msg);
                    
                    // Khôi phục từ backup nếu có
                    let backup_path = file_path.with_extension("bak");
                    if backup_path.exists() {
                        info!("Thử khôi phục từ file backup");
                        if let Err(restore_err) = fs::copy(&backup_path, file_path) {
                            error!("Không thể khôi phục từ backup: {}", restore_err);
                        } else {
                            info!("Đã khôi phục thành công từ backup");
                        }
                    }
                    
                    return Err(anyhow!(err_msg));
                }
            }
        }
        
        // Xóa file backup nếu tất cả thành công
        let backup_path = file_path.with_extension("bak");
        if backup_path.exists() {
            if let Err(e) = fs::remove_file(&backup_path) {
                warn!("Không thể xóa file backup: {}", e);
                // Không dừng xử lý vì đây không phải lỗi nghiêm trọng
            }
        }
        
        debug!("Đã ghi dữ liệu vào file thành công: {:?}", file_path);
        Ok(())
    }
    
    /// Đồng bộ dữ liệu từ bộ nhớ ra đĩa
    pub fn flush(&self) -> Result<()> {
        // Nếu đang chạy bất đồng bộ, gửi tín hiệu đồng bộ
        if let Some(tx) = &self.save_tx {
            let _ = tx.try_send(SaveOperation::Flush);
            return Ok(());
        }
        
        // Nếu không, đồng bộ ngay lập tức
        let cache_guard = self.cache.read()
            .map_err(|e| {
                error!("Không thể lấy read lock khi flush cache: {}", e);
                return Ok(());
            })?;
        Self::write_to_file(&self.file_path, &cache_guard)
    }
}

impl BridgeTransactionRepository for JsonBridgeTransactionRepository {
    fn save(&self, transaction: &BridgeTransaction) -> Result<(), String> {
        // Nếu đang chạy bất đồng bộ, gửi tín hiệu lưu
        if let Some(tx) = &self.save_tx {
            if let Err(e) = tx.try_send(SaveOperation::SaveTransaction(transaction.clone())) {
                return Err(format!("Không thể gửi yêu cầu lưu giao dịch: {}", e));
            }
            return Ok(());
        }
        
        // Nếu không, lưu ngay lập tức
        let mut cache_guard = self.cache.write()
            .map_err(|e| format!("Không thể lấy write lock cho cache: {}", e))?;
        cache_guard.insert(transaction.id.clone(), transaction.clone());
        
        match Self::write_to_file(&self.file_path, &cache_guard) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Không thể lưu giao dịch: {}", e)),
        }
    }
    
    /// Triển khai lưu hàng loạt giao dịch
    fn save_batch(&self, transactions: &[BridgeTransaction]) -> Result<(), String> {
        if transactions.is_empty() {
            return Ok(());
        }
        
        // Nếu đang chạy bất đồng bộ, gửi từng giao dịch vào hàng đợi
        if let Some(tx) = &self.save_tx {
            for transaction in transactions {
                if let Err(e) = tx.try_send(SaveOperation::SaveTransaction(transaction.clone())) {
                    return Err(format!("Không thể gửi yêu cầu lưu giao dịch hàng loạt: {}", e));
                }
            }
            return Ok(());
        }
        
        // Nếu không, lưu tất cả vào cache và ghi vào file một lần
        let mut cache_guard = self.cache.write()
            .map_err(|e| format!("Không thể lấy write lock cho cache: {}", e))?;
        
        // Đo thời gian để ghi log hiệu suất
        let start_time = std::time::Instant::now();
        let count = transactions.len();
        
        for transaction in transactions {
            cache_guard.insert(transaction.id.clone(), transaction.clone());
        }
        
        match Self::write_to_file(&self.file_path, &cache_guard) {
            Ok(_) => {
                let elapsed = start_time.elapsed();
                debug!("Đã lưu {} giao dịch trong {} ms", count, elapsed.as_millis());
                Ok(())
            },
            Err(e) => {
                error!("Không thể lưu hàng loạt giao dịch: {} - cố gắng lưu từng giao dịch", e);
                
                // Nếu lưu hàng loạt thất bại, thử lưu từng giao dịch một
                drop(cache_guard); // Giải phóng khóa
                
                for transaction in transactions {
                    if let Err(e) = self.save(transaction) {
                        error!("Không thể lưu giao dịch {}: {}", transaction.id, e);
                    }
                }
                
                Err(format!("Lưu hàng loạt thất bại: {}", e))
            },
        }
    }
    
    fn find_by_id(&self, id: &str) -> Result<Option<BridgeTransaction>, String> {
        let cache_guard = self.cache.read()
            .map_err(|e| format!("Không thể lấy read lock cho cache: {}", e))?;
        Ok(cache_guard.get(id).cloned())
    }
    
    fn find_by_source_tx_id(&self, tx_id: &str) -> Result<Option<BridgeTransaction>, String> {
        let cache_guard = self.cache.read()
            .map_err(|e| format!("Không thể lấy read lock cho cache: {}", e))?;
        Ok(cache_guard.values()
            .find(|tx| tx.source_tx_id.as_ref().map_or(false, |id| id == tx_id))
            .cloned())
    }
    
    fn find_by_target_tx_id(&self, tx_id: &str) -> Result<Option<BridgeTransaction>, String> {
        let cache_guard = self.cache.read()
            .map_err(|e| format!("Không thể lấy read lock cho cache: {}", e))?;
        Ok(cache_guard.values()
            .find(|tx| tx.target_tx_id.as_ref().map_or(false, |id| id == tx_id))
            .cloned())
    }
    
    fn find_by_source_address(&self, address: &str) -> Result<Vec<BridgeTransaction>, String> {
        let cache_guard = self.cache.read()
            .map_err(|e| format!("Không thể lấy read lock cho cache: {}", e))?;
        Ok(cache_guard.values()
            .filter(|tx| tx.source_address == address)
            .cloned()
            .collect())
    }
    
    fn find_by_target_address(&self, address: &str) -> Result<Vec<BridgeTransaction>, String> {
        let cache_guard = self.cache.read()
            .map_err(|e| format!("Không thể lấy read lock cho cache: {}", e))?;
        Ok(cache_guard.values()
            .filter(|tx| tx.target_address == address)
            .cloned()
            .collect())
    }
    
    fn find_by_status(&self, status: &BridgeTransactionStatus) -> Result<Vec<BridgeTransaction>, String> {
        let cache_guard = self.cache.read()
            .map_err(|e| format!("Không thể lấy read lock cho cache: {}", e))?;
        Ok(cache_guard.values()
            .filter(|tx| &tx.status == status)
            .cloned()
            .collect())
    }
    
    fn delete_by_id(&self, id: &str) -> Result<bool, String> {
        // Nếu đang chạy bất đồng bộ, gửi tín hiệu xóa
        if let Some(tx) = &self.save_tx {
            if let Err(e) = tx.try_send(SaveOperation::DeleteTransaction(id.to_string())) {
                return Err(format!("Không thể gửi yêu cầu xóa giao dịch: {}", e));
            }
            return Ok(true);
        }
        
        // Nếu không, xóa ngay lập tức
        let mut cache_guard = self.cache.write()
            .map_err(|e| format!("Không thể lấy write lock cho cache: {}", e))?;
        let existed = cache_guard.remove(id).is_some();
        
        if existed {
            match Self::write_to_file(&self.file_path, &cache_guard) {
                Ok(_) => Ok(true),
                Err(e) => Err(format!("Không thể lưu sau khi xóa giao dịch: {}", e)),
            }
        } else {
            Ok(false)
        }
    }
    
    fn delete_older_than(&self, timestamp: u64) -> Result<usize, String> {
        let cache_guard = self.cache.read()
            .map_err(|e| format!("Không thể lấy read lock cho cache: {}", e))?;
        let ids_to_delete: Vec<String> = cache_guard.values()
            .filter(|tx| tx.created_at < timestamp)
            .map(|tx| tx.id.clone())
            .collect();
        
        let count = ids_to_delete.len();
        
        // Nếu đang chạy bất đồng bộ, gửi tín hiệu xóa hàng loạt
        if let Some(tx) = &self.save_tx {
            if let Err(e) = tx.try_send(SaveOperation::BulkDelete(ids_to_delete)) {
                return Err(format!("Không thể gửi yêu cầu xóa hàng loạt: {}", e));
            }
            return Ok(count);
        }
        
        // Nếu không, xóa ngay lập tức
        let mut cache_guard = self.cache.write()
            .map_err(|e| format!("Không thể lấy write lock cho cache: {}", e))?;
        for id in &ids_to_delete {
            cache_guard.remove(id);
        }
        
        match Self::write_to_file(&self.file_path, &cache_guard) {
            Ok(_) => Ok(count),
            Err(e) => Err(format!("Không thể lưu sau khi xóa giao dịch: {}", e)),
        }
    }
    
    fn limit_transaction_count(&self, max_count: usize) -> Result<usize, String> {
        let mut cache_guard = self.cache.write()
            .map_err(|e| format!("Không thể lấy write lock cho cache: {}", e))?;
        
        // Sắp xếp giao dịch theo thời gian tạo (mới nhất trước)
        let mut transactions: Vec<_> = cache_guard.values().cloned().collect();
        transactions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        
        // Nếu số lượng giao dịch vượt quá max_count, xóa các giao dịch cũ
        if transactions.len() > max_count {
            let ids_to_keep: Vec<_> = transactions.iter()
                .take(max_count)
                .map(|tx| tx.id.clone())
                .collect();
            
            let new_cache: HashMap<_, _> = transactions.into_iter()
                .filter(|tx| ids_to_keep.contains(&tx.id))
                .map(|tx| (tx.id.clone(), tx))
                .collect();
            
            let removed_count = cache_guard.len() - new_cache.len();
            *cache_guard = new_cache;
            
            match Self::write_to_file(&self.file_path, &cache_guard) {
                Ok(_) => Ok(removed_count),
                Err(e) => Err(format!("Không thể lưu sau khi giới hạn giao dịch: {}", e)),
            }
        } else {
            Ok(0)
        }
    }
    
    fn get_all_transactions(&self) -> Result<Vec<BridgeTransaction>, String> {
        let cache_guard = self.cache.read()
            .map_err(|e| format!("Không thể lấy read lock cho cache: {}", e))?;
        Ok(cache_guard.values().cloned().collect())
    }
    
    fn find_by_hub_tx_id(&self, tx_id: &str) -> Result<Option<BridgeTransaction>, String> {
        let cache_guard = self.cache.read()
            .map_err(|e| format!("Không thể lấy read lock cho cache: {}", e))?;
        Ok(cache_guard.values()
            .find(|tx| tx.hub_tx_id.as_ref().map_or(false, |id| id == tx_id))
            .cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smartcontracts::dmd_token::DmdChain;
    use tempfile::tempdir;
    use std::time::{SystemTime, UNIX_EPOCH};
    
    #[test]
    fn test_save_and_find_transaction() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("transactions.json");
        
        let repo = JsonBridgeTransactionRepository::new(&file_path).unwrap();
        
        // Tạo giao dịch test
        let tx = BridgeTransaction::new(
            "test-id".to_string(),
            "source-address".to_string(),
            "target-address".to_string(),
            DmdChain::Ethereum,
            DmdChain::BinanceSmartChain,
            "1000".to_string(),
            None,
            "10".to_string(),
        );
        
        // Lưu giao dịch
        repo.save(&tx).unwrap();
        
        // Tìm kiếm giao dịch
        let found_tx = repo.find_by_id("test-id").unwrap().unwrap();
        
        // Kiểm tra kết quả
        assert_eq!(found_tx.id, "test-id");
        assert_eq!(found_tx.source_address, "source-address");
        assert_eq!(found_tx.target_address, "target-address");
    }
    
    #[test]
    fn test_delete_transaction() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("transactions.json");
        
        let repo = JsonBridgeTransactionRepository::new(&file_path).unwrap();
        
        // Tạo và lưu giao dịch
        let tx = BridgeTransaction::new(
            "test-id".to_string(),
            "source-address".to_string(),
            "target-address".to_string(),
            DmdChain::Ethereum,
            DmdChain::BinanceSmartChain,
            "1000".to_string(),
            None,
            "10".to_string(),
        );
        
        repo.save(&tx).unwrap();
        
        // Xóa giao dịch
        let deleted = repo.delete_by_id("test-id").unwrap();
        assert!(deleted);
        
        // Kiểm tra xem giao dịch đã bị xóa chưa
        let found_tx = repo.find_by_id("test-id").unwrap();
        assert!(found_tx.is_none());
    }
    
    #[test]
    fn test_delete_older_than() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("transactions.json");
        
        let repo = JsonBridgeTransactionRepository::new(&file_path).unwrap();
        
        // Lấy thời gian hiện tại
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Tạo giao dịch cũ (giả lập thời gian tạo là 1 giờ trước)
        let mut old_tx = BridgeTransaction::new(
            "old-tx".to_string(),
            "source-address".to_string(),
            "target-address".to_string(),
            DmdChain::Ethereum,
            DmdChain::BinanceSmartChain,
            "1000".to_string(),
            None,
            "10".to_string(),
        );
        old_tx.created_at = now - 3600; // 1 giờ trước
        
        // Tạo giao dịch mới
        let new_tx = BridgeTransaction::new(
            "new-tx".to_string(),
            "source-address".to_string(),
            "target-address".to_string(),
            DmdChain::Ethereum,
            DmdChain::BinanceSmartChain,
            "1000".to_string(),
            None,
            "10".to_string(),
        );
        
        // Lưu cả hai giao dịch
        repo.save(&old_tx).unwrap();
        repo.save(&new_tx).unwrap();
        
        // Xóa giao dịch cũ hơn 30 phút
        let deleted_count = repo.delete_older_than(now - 1800).unwrap(); // 30 phút trước
        assert_eq!(deleted_count, 1);
        
        // Kiểm tra xem giao dịch cũ đã bị xóa chưa và giao dịch mới vẫn còn
        let found_old_tx = repo.find_by_id("old-tx").unwrap();
        let found_new_tx = repo.find_by_id("new-tx").unwrap();
        
        assert!(found_old_tx.is_none());
        assert!(found_new_tx.is_some());
    }
} 