//! Module cung cấp các chức năng xác thực và đăng ký người dùng miễn phí.

// External imports
use anyhow::Context;
use chrono::Utc;
use ethers::types::Address;
use tracing::{debug, info, warn};

// Internal imports
use crate::error::WalletError;
use crate::walletlogic::handler::WalletManagerHandler;
use super::FreeUserManager;
use super::types::*;
use super::create_free_user_data;

impl FreeUserManager {
    /// Xác minh rằng ví được liên kết với user_id thuộc về free_user.
    ///
    /// # Arguments
    /// - `wallet_handler`: Handler để truy xuất user_id từ ví.
    /// - `address`: Địa chỉ ví cần kiểm tra.
    ///
    /// # Returns
    /// - `Ok(true)` nếu ví thuộc về free_user.
    /// - `Ok(false)` nếu ví không thuộc về free_user.
    /// - `Err` nếu có lỗi xảy ra.
    #[flow_from("walletmanager::api")]
    pub async fn verify_free_user<T: WalletManagerHandler>(
        &self,
        wallet_handler: &T,
        address: Address,
    ) -> Result<bool, WalletError> {
        // Lấy user_id từ địa chỉ ví
        let user_id = wallet_handler.get_user_id_internal(address).await?;
        
        // Kiểm tra xem user_id có prefix FREE_ không
        if user_id.starts_with("FREE_") {
            // Kiểm tra xem user_id đã được đăng ký trong hệ thống chưa
            let user_data_opt = self.get_user_data(&user_id).await?;
            if user_data_opt.is_some() {
                return Ok(true);
            } else {
                // Nếu chưa có trong hệ thống, cập nhật hệ thống
                return self.register_user(user_id, address).await;
            }
        }
        
        Ok(false)
    }
    
    /// Đăng ký người dùng miễn phí mới hoặc cập nhật ví mới.
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng.
    /// - `address`: Địa chỉ ví.
    ///
    /// # Returns
    /// - `Ok(true)` nếu đăng ký thành công.
    /// - `Err` nếu có lỗi.
    pub async fn register_user(&self, user_id: String, address: Address) -> Result<bool, WalletError> {
        let user_data_opt = self.get_user_data(&user_id).await?;
        let now = Utc::now();
        
        if let Some(mut data) = user_data_opt {
            // Nếu đã có user, thêm địa chỉ ví mới (nếu chưa có)
            if !data.wallet_addresses.contains(&address) {
                // Kiểm tra giới hạn số ví
                if data.wallet_addresses.len() >= MAX_WALLETS_FREE_USER {
                    warn!("Vượt quá giới hạn số ví cho free user: {}", user_id);
                    return Err(WalletError::InvalidSeedOrKey(
                        format!("Free user chỉ được phép có tối đa {} ví", MAX_WALLETS_FREE_USER)
                    ));
                }
                
                data.wallet_addresses.push(address);
            }
            
            // Cập nhật thời gian hoạt động gần nhất
            data.last_active = now;
            
            // Lưu dữ liệu cập nhật
            self.save_user_data(&user_id, data).await?;
            
            Ok(true)
        } else {
            // Tạo dữ liệu free user mới
            let mut new_user = create_free_user_data(&user_id);
            new_user.wallet_addresses.push(address);
            
            // Lưu thông tin người dùng mới
            self.save_user_data(&user_id, new_user).await?;
            
            // Khởi tạo lịch sử giao dịch trống
            let mut tx_history = self.transaction_history.write().await;
            tx_history.insert(user_id.clone(), Vec::new());
            
            // Khởi tạo lịch sử snipebot trống
            let mut snipe_history = self.snipebot_attempts.write().await;
            snipe_history.insert(user_id, Vec::new());
            
            Ok(true)
        }
    }
    
    /// Kiểm tra trạng thái tài khoản người dùng.
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng cần kiểm tra.
    ///
    /// # Returns
    /// - `Ok(UserStatus)`: Trạng thái tài khoản.
    /// - `Err`: Nếu không tìm thấy người dùng.
    pub async fn check_user_status(&self, user_id: &str) -> Result<UserStatus, WalletError> {
        let user_data_opt = self.get_user_data(user_id).await?;
        
        if let Some(data) = user_data_opt {
            Ok(data.status.clone())
        } else {
            Err(WalletError::InvalidSeedOrKey(format!("User ID không tồn tại: {}", user_id)))
        }
    }
    
    /// Cập nhật trạng thái tài khoản người dùng.
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng cần cập nhật.
    /// - `new_status`: Trạng thái mới.
    ///
    /// # Returns
    /// - `Ok(())`: Nếu cập nhật thành công.
    /// - `Err`: Nếu không tìm thấy người dùng.
    pub async fn update_user_status(&self, user_id: &str, new_status: UserStatus) -> Result<(), WalletError> {
        let user_data_opt = self.get_user_data(user_id).await?;
        
        if let Some(mut data) = user_data_opt {
            data.status = new_status;
            data.last_active = Utc::now();
            self.save_user_data(user_id, data).await?;
            Ok(())
        } else {
            Err(WalletError::InvalidSeedOrKey(format!("User ID không tồn tại: {}", user_id)))
        }
    }
}