//! Module cung cấp các truy vấn thông tin người dùng miễn phí.

// Internal imports
use crate::error::WalletError;
use super::FreeUserManager;
use super::types::*;

impl FreeUserManager {
    /// Lấy thông tin người dùng.
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng.
    ///
    /// # Returns
    /// - `Ok(FreeUserData)`: Thông tin người dùng.
    /// - `Err`: Nếu không tìm thấy người dùng.
    pub async fn get_user_data(&self, user_id: &str) -> Result<FreeUserData, WalletError> {
        let user_data = self.user_data.read().await;
        
        if let Some(data) = user_data.get(user_id) {
            Ok(data.clone())
        } else {
            Err(WalletError::InvalidSeedOrKey(format!("User ID không tồn tại: {}", user_id)))
        }
    }
    
    /// Lấy lịch sử giao dịch của người dùng.
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng.
    ///
    /// # Returns
    /// - `Ok(Vec<TransactionRecord>)`: Lịch sử giao dịch.
    /// - `Err`: Nếu không tìm thấy người dùng.
    pub async fn get_transaction_history(&self, user_id: &str) -> Result<Vec<TransactionRecord>, WalletError> {
        let tx_history = self.transaction_history.read().await;
        
        if let Some(history) = tx_history.get(user_id) {
            Ok(history.clone())
        } else {
            Err(WalletError::InvalidSeedOrKey(format!("User ID không tồn tại: {}", user_id)))
        }
    }
    
    /// Lấy lịch sử sử dụng snipebot của người dùng.
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng.
    ///
    /// # Returns
    /// - `Ok(Vec<SnipebotAttempt>)`: Lịch sử sử dụng snipebot.
    /// - `Err`: Nếu không tìm thấy người dùng.
    pub async fn get_snipebot_history(&self, user_id: &str) -> Result<Vec<SnipebotAttempt>, WalletError> {
        let snipe_history = self.snipebot_attempts.read().await;
        
        if let Some(history) = snipe_history.get(user_id) {
            Ok(history.clone())
        } else {
            Err(WalletError::InvalidSeedOrKey(format!("User ID không tồn tại: {}", user_id)))
        }
    }
}