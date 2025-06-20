//! Module quản lý người dùng miễn phí.

// Standard library imports
use std::collections::HashMap;
use std::sync::Arc;
use std::fs::{self, File};
use std::io::{self, Write, Read};
use std::path::Path;

// External imports
use tokio::sync::RwLock;
use tracing::{info, debug, error, warn};
use chrono::{Utc, DateTime};
use serde::{Serialize, Deserialize};

// Internal imports
use super::types::*;
use crate::error::WalletError;
use crate::cache::{self, create_async_lru_cache, AsyncCache, LRUCache};
use crate::users::subscription::constants::USER_DATA_CACHE_SECONDS;

/// Cấu hình Backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    /// Đường dẫn thư mục lưu trữ backup
    pub backup_dir: String,
    /// Số lượng bản backup tối đa cần lưu giữ
    pub max_backups: usize,
    /// Khoảng thời gian giữa các lần backup tự động (phút)
    pub auto_backup_interval_minutes: u64,
    /// Nén file backup
    pub compress_backup: bool,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            backup_dir: "./backups/free_users".to_string(),
            max_backups: 5,
            auto_backup_interval_minutes: 60, // 1 giờ
            compress_backup: true,
        }
    }
}

/// Quản lý hoạt động của người dùng miễn phí.
pub struct FreeUserManager {
    /// Cache cho thông tin người dùng miễn phí
    user_data_cache: AsyncCache<String, FreeUserData, LRUCache<String, FreeUserData>>,
    /// Lưu trữ lịch sử giao dịch
    pub(crate) transaction_history: Arc<RwLock<HashMap<String, Vec<TransactionRecord>>>>,
    /// Lưu trữ lịch sử snipebot
    pub(crate) snipebot_attempts: Arc<RwLock<HashMap<String, Vec<SnipebotAttempt>>>>,
    /// Cấu hình backup
    backup_config: BackupConfig,
    /// Thời gian backup cuối cùng
    last_backup_time: Arc<RwLock<DateTime<Utc>>>,
}

impl FreeUserManager {
    /// Tạo instance FreeUserManager mới.
    pub fn new() -> Self {
        Self {
            user_data_cache: create_async_lru_cache(500, USER_DATA_CACHE_SECONDS),
            transaction_history: Arc::new(RwLock::new(HashMap::new())),
            snipebot_attempts: Arc::new(RwLock::new(HashMap::new())),
            backup_config: BackupConfig::default(),
            last_backup_time: Arc::new(RwLock::new(Utc::now())),
        }
    }
    
    /// Tạo instance FreeUserManager với cấu hình backup tùy chỉnh
    pub fn new_with_backup_config(backup_config: BackupConfig) -> Self {
        // Đảm bảo thư mục backup tồn tại
        if let Err(e) = fs::create_dir_all(&backup_config.backup_dir) {
            error!("Không thể tạo thư mục backup {}: {}", backup_config.backup_dir, e);
        }
        
        Self {
            user_data_cache: create_async_lru_cache(500, USER_DATA_CACHE_SECONDS),
            transaction_history: Arc::new(RwLock::new(HashMap::new())),
            snipebot_attempts: Arc::new(RwLock::new(HashMap::new())),
            backup_config,
            last_backup_time: Arc::new(RwLock::new(Utc::now())),
        }
    }
    
    /// Lấy thông tin người dùng từ cache hoặc database
    /// 
    /// # Arguments
    /// - `user_id`: ID người dùng
    /// 
    /// # Returns
    /// - `Ok(Some(FreeUserData))`: Nếu tìm thấy
    /// - `Ok(None)`: Nếu không tìm thấy
    /// - `Err`: Nếu có lỗi
    pub async fn get_user_data(&self, user_id: &str) -> Result<Option<FreeUserData>, WalletError> {
        // Tìm trong cache hoặc load từ database
        let user_data = cache::get_or_load_with_cache(
            &self.user_data_cache,
            user_id, 
            |id| async move {
                self.load_user_from_db(&id).await.unwrap_or(None)
            }
        ).await;
        
        // Kiểm tra nếu cần backup tự động
        self.check_auto_backup().await;
        
        Ok(user_data)
    }
    
    /// Lưu thông tin người dùng
    /// 
    /// # Arguments
    /// - `user_id`: ID người dùng
    /// - `user_data`: Thông tin người dùng
    pub async fn save_user_data(&self, user_id: &str, user_data: FreeUserData) -> Result<(), WalletError> {
        // Cập nhật cache
        self.user_data_cache.insert(user_id.to_string(), user_data.clone()).await;
        
        // TODO: Lưu vào database
        debug!("Đã lưu thông tin người dùng free {}", user_id);
        
        // Kiểm tra nếu cần backup tự động
        self.check_auto_backup().await;
        
        Ok(())
    }
    
    // Phương thức giả lập load từ database thực tế
    async fn load_user_from_db(&self, user_id: &str) -> Result<Option<FreeUserData>, WalletError> {
        // TODO: Load từ database thực tế
        info!("Tải thông tin người dùng free {} từ database", user_id);
        
        // Thử load từ backup nếu có lỗi từ database
        let backup_data = self.load_from_backup(user_id).await;
        if let Some(data) = backup_data {
            info!("Đã khôi phục dữ liệu người dùng {} từ backup", user_id);
            return Ok(Some(data));
        }
        
        // Giả lập không tìm thấy
        Ok(None)
    }
    
    /// Tạo backup dữ liệu người dùng
    /// 
    /// # Arguments
    /// - `user_id`: ID người dùng cần backup (nếu None, backup tất cả)
    /// 
    /// # Returns
    /// - `Ok(())`: Nếu backup thành công
    /// - `Err`: Nếu có lỗi
    pub async fn backup_user_data(&self, user_id: Option<&str>) -> Result<(), WalletError> {
        match user_id {
            Some(id) => {
                // Backup một người dùng cụ thể
                if let Some(user_data) = self.user_data_cache.get(id).await {
                    self.save_to_backup(id, &user_data).await?;
                    info!("Đã tạo backup cho người dùng {}", id);
                } else {
                    warn!("Không tìm thấy dữ liệu trong cache cho người dùng {}", id);
                }
            },
            None => {
                // Backup tất cả người dùng trong cache
                let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
                let backup_path = format!("{}/all_users_{}.json", self.backup_config.backup_dir, timestamp);
                
                // Lấy tất cả dữ liệu từ cache
                let keys = self.user_data_cache.keys().await;
                let mut all_data = HashMap::new();
                
                for key in keys {
                    if let Some(user_data) = self.user_data_cache.get(&key).await {
                        all_data.insert(key, user_data);
                    }
                }
                
                if all_data.is_empty() {
                    warn!("Không có dữ liệu người dùng nào trong cache để backup");
                    return Ok(());
                }
                
                // Serialize và lưu vào file
                let serialized = serde_json::to_string(&all_data)
                    .map_err(|e| WalletError::Other(format!("Lỗi serialize: {}", e)))?;
                
                let mut file = File::create(&backup_path)
                    .map_err(|e| WalletError::Other(format!("Không thể tạo file backup: {}", e)))?;
                
                file.write_all(serialized.as_bytes())
                    .map_err(|e| WalletError::Other(format!("Lỗi ghi file: {}", e)))?;
                
                info!("Đã tạo backup cho {} người dùng tại {}", all_data.len(), backup_path);
                
                // Nén file nếu được cấu hình
                if self.backup_config.compress_backup {
                    // TODO: Nén file backup
                }
                
                // Cập nhật thời gian backup cuối cùng
                if let Ok(mut last_backup) = self.last_backup_time.write() {
                    *last_backup = Utc::now();
                }
                
                // Xóa các backup cũ nếu vượt quá số lượng cho phép
                self.cleanup_old_backups().await?;
            }
        }
        
        Ok(())
    }
    
    /// Lưu dữ liệu người dùng vào backup
    async fn save_to_backup(&self, user_id: &str, user_data: &FreeUserData) -> Result<(), WalletError> {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let file_name = format!("{}/user_{}_{}.json", self.backup_config.backup_dir, user_id, timestamp);
        
        // Đảm bảo thư mục tồn tại
        let dir = Path::new(&self.backup_config.backup_dir);
        if !dir.exists() {
            fs::create_dir_all(dir)
                .map_err(|e| WalletError::Other(format!("Không thể tạo thư mục backup: {}", e)))?;
        }
        
        // Serialize và lưu vào file
        let serialized = serde_json::to_string(user_data)
            .map_err(|e| WalletError::Other(format!("Lỗi serialize: {}", e)))?;
        
        let mut file = File::create(&file_name)
            .map_err(|e| WalletError::Other(format!("Không thể tạo file backup: {}", e)))?;
        
        file.write_all(serialized.as_bytes())
            .map_err(|e| WalletError::Other(format!("Lỗi ghi file: {}", e)))?;
        
        debug!("Đã lưu backup cho người dùng {} tại {}", user_id, file_name);
        
        Ok(())
    }
    
    /// Tải dữ liệu người dùng từ backup
    async fn load_from_backup(&self, user_id: &str) -> Option<FreeUserData> {
        let dir = Path::new(&self.backup_config.backup_dir);
        if !dir.exists() {
            return None;
        }
        
        let prefix = format!("user_{}_", user_id);
        
        // Tìm file backup mới nhất cho user_id
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                error!("Không thể đọc thư mục backup: {}", e);
                return None;
            }
        };
        
        let mut newest_file = None;
        let mut newest_time = chrono::DateTime::<Utc>::MIN_UTC;
        
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if let Some(file_name) = path.file_name().and_then(|f| f.to_str()) {
                    if file_name.starts_with(&prefix) {
                        if let Ok(metadata) = fs::metadata(&path) {
                            if let Ok(modified) = metadata.modified() {
                                if let Ok(datetime) = modified.into_std().map(|t| chrono::DateTime::<Utc>::from(t)) {
                                    if datetime > newest_time {
                                        newest_time = datetime;
                                        newest_file = Some(path);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // Đọc file backup nếu tìm thấy
        if let Some(path) = newest_file {
            match File::open(&path) {
                Ok(mut file) => {
                    let mut contents = String::new();
                    if file.read_to_string(&mut contents).is_ok() {
                        if let Ok(user_data) = serde_json::from_str::<FreeUserData>(&contents) {
                            info!("Đã đọc backup cho người dùng {} từ {}", user_id, path.display());
                            return Some(user_data);
                        }
                    }
                }
                Err(e) => {
                    error!("Không thể mở file backup {}: {}", path.display(), e);
                }
            }
        }
        
        None
    }
    
    /// Xóa các backup cũ
    async fn cleanup_old_backups(&self) -> Result<(), WalletError> {
        let dir = Path::new(&self.backup_config.backup_dir);
        if !dir.exists() {
            return Ok(());
        }
        
        // Lấy danh sách tất cả các file backup
        let entries = fs::read_dir(dir)
            .map_err(|e| WalletError::Other(format!("Không thể đọc thư mục backup: {}", e)))?;
        
        // Thu thập thông tin về tất cả các file
        let mut files_info = Vec::new();
        
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.is_file() && path.extension().map_or(false, |ext| ext == "json") {
                    if let Ok(metadata) = fs::metadata(&path) {
                        if let Ok(modified) = metadata.modified() {
                            files_info.push((path, modified));
                        }
                    }
                }
            }
        }
        
        // Sắp xếp theo thời gian sửa đổi (mới nhất đầu tiên)
        files_info.sort_by(|a, b| b.1.cmp(&a.1));
        
        // Xóa các file cũ nếu vượt quá số lượng cho phép
        if files_info.len() > self.backup_config.max_backups {
            for (path, _) in files_info.iter().skip(self.backup_config.max_backups) {
                if let Err(e) = fs::remove_file(path) {
                    warn!("Không thể xóa file backup cũ {}: {}", path.display(), e);
                } else {
                    debug!("Đã xóa file backup cũ: {}", path.display());
                }
            }
        }
        
        Ok(())
    }
    
    /// Khôi phục từ backup
    /// 
    /// # Arguments
    /// - `user_id`: ID người dùng cần khôi phục
    /// 
    /// # Returns
    /// - `Ok(Option<FreeUserData>)`: Dữ liệu người dùng nếu khôi phục thành công
    /// - `Err`: Nếu có lỗi
    pub async fn restore_from_backup(&self, user_id: &str) -> Result<Option<FreeUserData>, WalletError> {
        info!("Đang khôi phục dữ liệu cho người dùng {} từ backup", user_id);
        
        let user_data = self.load_from_backup(user_id).await;
        
        // Nếu tìm thấy dữ liệu, cập nhật cache
        if let Some(ref data) = user_data {
            self.user_data_cache.insert(user_id.to_string(), data.clone()).await;
            info!("Đã khôi phục và cập nhật cache cho người dùng {}", user_id);
        } else {
            warn!("Không tìm thấy backup cho người dùng {}", user_id);
        }
        
        Ok(user_data)
    }
    
    /// Kiểm tra và thực hiện backup tự động nếu cần
    async fn check_auto_backup(&self) {
        let now = Utc::now();
        let should_backup = {
            if let Ok(last_backup) = self.last_backup_time.read() {
                let interval = chrono::Duration::minutes(self.backup_config.auto_backup_interval_minutes as i64);
                now - *last_backup > interval
            } else {
                false
            }
        };
        
        if should_backup {
            debug!("Đang thực hiện backup tự động theo lịch");
            if let Err(e) = self.backup_user_data(None).await {
                error!("Backup tự động thất bại: {}", e);
            }
        }
    }
}

impl Default for FreeUserManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Hàm helper để tạo FreeUserData mới
pub fn create_free_user_data(user_id: &str) -> FreeUserData {
    FreeUserData {
        user_id: user_id.to_string(),
        status: UserStatus::Active,
        created_at: Utc::now(),
        last_active: Utc::now(), 
        wallet_addresses: Vec::new(),
        transaction_limit: MAX_FREE_TRANSACTIONS_PER_DAY,
    }
}