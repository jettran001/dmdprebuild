//! Module quản lý giới hạn tính năng cho người dùng miễn phí.

// External imports
use chrono::{Duration, Utc};
use tracing::{warn};

// Internal imports
use crate::error::WalletError;
use super::FreeUserManager;
use super::types::*;

impl FreeUserManager {
    /// Kiểm tra xem người dùng có thể thực hiện giao dịch không.
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng.
    ///
    /// # Returns
    /// - `Ok(true)`: Nếu có thể thực hiện giao dịch.
    /// - `Ok(false)`: Nếu không thể thực hiện giao dịch.
    /// - `Err`: Nếu có lỗi.
    pub async fn can_perform_transaction(&self, user_id: &str) -> Result<bool, WalletError> {
        // Kiểm tra trạng thái tài khoản
        let status = self.check_user_status(user_id).await?;
        
        if status != UserStatus::Active {
            return Ok(false);
        }
        
        // Kiểm tra số lượng giao dịch trong ngày
        let tx_history = self.transaction_history.read().await;
        
        if let Some(history) = tx_history.get(user_id) {
            let now = Utc::now();
            let one_day_ago = now - Duration::days(1);
            
            // Đếm số giao dịch trong 24h qua
            let recent_tx_count = history.iter()
                .filter(|tx| tx.timestamp > one_day_ago)
                .count();
            
            if recent_tx_count >= MAX_TRANSACTIONS_PER_DAY {
                warn!("Free user {} đã đạt giới hạn giao dịch hàng ngày", user_id);
                return Ok(false);
            }
        }
        
        Ok(true)
    }
    
    /// Kiểm tra xem người dùng có thể sử dụng snipebot không.
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng.
    ///
    /// # Returns
    /// - `Ok(true)`: Nếu có thể sử dụng snipebot.
    /// - `Ok(false)`: Nếu không thể sử dụng snipebot.
    /// - `Err`: Nếu có lỗi.
    pub async fn can_use_snipebot(&self, user_id: &str) -> Result<bool, WalletError> {
        // Kiểm tra trạng thái tài khoản
        let status = self.check_user_status(user_id).await?;
        
        if status != UserStatus::Active {
            return Ok(false);
        }
        
        // Kiểm tra số lần sử dụng snipebot trong ngày
        let snipe_history = self.snipebot_attempts.read().await;
        
        if let Some(history) = snipe_history.get(user_id) {
            let now = Utc::now();
            let one_day_ago = now - Duration::days(1);
            
            // Đếm số lần snipe trong 24h qua
            let recent_snipe_count = history.iter()
                .filter(|snipe| snipe.timestamp > one_day_ago)
                .count();
            
            if recent_snipe_count >= MAX_SNIPEBOT_ATTEMPTS_PER_DAY {
                warn!("Free user {} đã đạt giới hạn sử dụng snipebot hàng ngày", user_id);
                return Ok(false);
            }
        }
        
        Ok(true)
    }
    
    /// Kiểm tra nếu người dùng có thể nâng cấp lên premium.
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng.
    ///
    /// # Returns
    /// - `Ok(true)`: Nếu có thể nâng cấp.
    /// - `Ok(false)`: Nếu không thể nâng cấp.
    /// - `Err`: Nếu có lỗi.
    pub async fn can_upgrade_to_premium(&self, user_id: &str) -> Result<bool, WalletError> {
        // Kiểm tra trạng thái tài khoản
        let status = self.check_user_status(user_id).await?;
        
        // Chỉ cho phép tài khoản active nâng cấp
        if status == UserStatus::Active {
            return Ok(true);
        }
        
        Ok(false)
    }
}