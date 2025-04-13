//! Module quản lý người dùng miễn phí.

// Standard library imports
use std::collections::HashMap;
use std::sync::Arc;

// External imports
use tokio::sync::RwLock;

// Internal imports
use super::types::*;

/// Quản lý hoạt động của người dùng miễn phí.
pub struct FreeUserManager {
    /// Lưu trữ thông tin người dùng miễn phí
    pub(crate) user_data: Arc<RwLock<HashMap<String, FreeUserData>>>,
    /// Lưu trữ lịch sử giao dịch
    pub(crate) transaction_history: Arc<RwLock<HashMap<String, Vec<TransactionRecord>>>>,
    /// Lưu trữ lịch sử snipebot
    pub(crate) snipebot_attempts: Arc<RwLock<HashMap<String, Vec<SnipebotAttempt>>>>,
}

impl FreeUserManager {
    /// Tạo instance FreeUserManager mới.
    pub fn new() -> Self {
        Self {
            user_data: Arc::new(RwLock::new(HashMap::new())),
            transaction_history: Arc::new(RwLock::new(HashMap::new())),
            snipebot_attempts: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for FreeUserManager {
    fn default() -> Self {
        Self::new()
    }
}