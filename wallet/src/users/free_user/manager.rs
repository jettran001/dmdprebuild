//! Module quản lý người dùng miễn phí.

// Standard library imports
use std::collections::HashMap;
use std::sync::Arc;

// External imports
use tokio::sync::RwLock;
use tracing::{info, debug, error};
use chrono::Utc;

// Internal imports
use super::types::*;
use crate::error::WalletError;
use crate::cache::{self, create_async_lru_cache, AsyncCache, LRUCache};
use crate::users::subscription::constants::USER_DATA_CACHE_SECONDS;

/// Quản lý hoạt động của người dùng miễn phí.
pub struct FreeUserManager {
    /// Cache cho thông tin người dùng miễn phí
    user_data_cache: AsyncCache<String, FreeUserData, LRUCache<String, FreeUserData>>,
    /// Lưu trữ lịch sử giao dịch
    pub(crate) transaction_history: Arc<RwLock<HashMap<String, Vec<TransactionRecord>>>>,
    /// Lưu trữ lịch sử snipebot
    pub(crate) snipebot_attempts: Arc<RwLock<HashMap<String, Vec<SnipebotAttempt>>>>,
}

impl FreeUserManager {
    /// Tạo instance FreeUserManager mới.
    pub fn new() -> Self {
        Self {
            user_data_cache: create_async_lru_cache(500, USER_DATA_CACHE_SECONDS),
            transaction_history: Arc::new(RwLock::new(HashMap::new())),
            snipebot_attempts: Arc::new(RwLock::new(HashMap::new())),
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
        
        Ok(())
    }
    
    // Phương thức giả lập load từ database thực tế
    async fn load_user_from_db(&self, user_id: &str) -> Result<Option<FreeUserData>, WalletError> {
        // TODO: Load từ database thực tế
        info!("Tải thông tin người dùng free {} từ database", user_id);
        
        // Giả lập không tìm thấy
        Ok(None)
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