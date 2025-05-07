//! Module ghi nhận các hoạt động của người dùng miễn phí.

// External imports
use chrono::Utc;
use ethers::types::Address;

// Internal imports
use crate::error::WalletError;
use super::FreeUserManager;
use super::types::*;

impl FreeUserManager {
    /// Ghi nhận giao dịch mới.
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng.
    /// - `wallet_address`: Địa chỉ ví thực hiện giao dịch.
    /// - `tx_id`: ID giao dịch.
    /// - `tx_type`: Loại giao dịch.
    ///
    /// # Returns
    /// - `Ok(())`: Nếu ghi nhận thành công.
    /// - `Err`: Nếu có lỗi.
    pub async fn record_transaction(
        &self,
        user_id: &str,
        wallet_address: Address,
        tx_id: String,
        tx_type: TransactionType,
    ) -> Result<(), WalletError> {
        let mut tx_history = self.transaction_history.write().await;
        
        if let Some(history) = tx_history.get_mut(user_id) {
            // Tạo bản ghi mới
            let record = TransactionRecord {
                tx_id,
                wallet_address,
                timestamp: Utc::now(),
                tx_type,
            };
            
            // Thêm vào lịch sử
            history.push(record);
            
            Ok(())
        } else {
            Err(WalletError::InvalidSeedOrKey(format!("User ID không tồn tại: {}", user_id)))
        }
    }
    
    /// Ghi nhận lần sử dụng snipebot mới.
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng.
    /// - `wallet_address`: Địa chỉ ví sử dụng snipebot.
    /// - `attempt_id`: ID lần thử.
    /// - `result`: Kết quả snipe.
    ///
    /// # Returns
    /// - `Ok(())`: Nếu ghi nhận thành công.
    /// - `Err`: Nếu có lỗi.
    pub async fn record_snipebot_attempt(
        &self,
        user_id: &str,
        wallet_address: Address,
        attempt_id: String,
        result: SnipeResult,
    ) -> Result<(), WalletError> {
        let mut snipe_history = self.snipebot_attempts.write().await;
        
        if let Some(history) = snipe_history.get_mut(user_id) {
            // Tạo bản ghi mới
            let record = SnipebotAttempt {
                attempt_id,
                wallet_address,
                timestamp: Utc::now(),
                result,
            };
            
            // Thêm vào lịch sử
            history.push(record);
            
            Ok(())
        } else {
            Err(WalletError::InvalidSeedOrKey(format!("User ID không tồn tại: {}", user_id)))
        }
    }
}