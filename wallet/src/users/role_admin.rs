//! Định nghĩa quyền hạn cho role Admin trong hệ thống wallet
//!
//! RoleAdmin đại diện cho người dùng có mọi quyền hạn cao cấp nhất:
//! - Quản trị hệ thống, quản lý user, premium, vip, free
//! - Truy cập, chỉnh sửa, xóa mọi dữ liệu
//! - Thực hiện thao tác đặc biệt: nâng cấp, hạ cấp, ban user, unlock, audit, v.v.
//! - Có thể truy cập mọi API, mọi tính năng không bị giới hạn

use serde::{Serialize, Deserialize};
use dotenv::dotenv;
use std::env;

use super::premium_user::UserRole;

/// Struct đại diện cho quyền hạn Admin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAdmin {
    /// ID của admin
    pub admin_id: String,
    /// Tên hiển thị
    pub name: String,
    /// Địa chỉ ví của admin
    pub wallet_address: String,
    /// Mật khẩu (hash)
    pub password_hash: String,
    /// Enum quyền hạn (luôn là Admin)
    pub role: UserRole,
    /// Ghi chú quyền đặc biệt (nếu có)
    pub special_permissions: Vec<String>,
}

impl RoleAdmin {
    /// Tạo mới một admin với mọi quyền hạn, kèm ví và mật khẩu hash
    pub fn new(admin_id: &str, name: &str, wallet_address: &str, password_hash: &str) -> Self {
        Self {
            admin_id: admin_id.to_string(),
            name: name.to_string(),
            wallet_address: wallet_address.to_string(),
            password_hash: password_hash.to_string(),
            role: UserRole::Admin,
            special_permissions: vec![
                "system_manage_all".to_string(),
                "user_manage_all".to_string(),
                "premium_manage_all".to_string(),
                "vip_manage_all".to_string(),
                "audit".to_string(),
                "ban_unban".to_string(),
                "unlock_all".to_string(),
                "access_all_api".to_string(),
            ],
        }
    }

    /// Tạo mới RoleAdmin từ biến môi trường (.env)
    /// BẮT BUỘC: Gọi dotenv().ok() trước khi sử dụng hàm này ở main
    pub fn load_from_env() -> Result<Self, String> {
        dotenv().ok();
        let admin_id = match env::var("ADMIN_ID") {
            Ok(val) => val,
            Err(_) => {
                error!("ADMIN_ID not set in environment");
                return Err("ADMIN_ID not set".to_string());
            }
        };
        let admin_name = match env::var("ADMIN_NAME") {
            Ok(val) => val,
            Err(_) => {
                error!("ADMIN_NAME not set in environment");
                return Err("ADMIN_NAME not set".to_string());
            }
        };
        let wallet_address = match env::var("ADMIN_WALLET") {
            Ok(val) => val,
            Err(_) => {
                error!("ADMIN_WALLET not set in environment");
                return Err("ADMIN_WALLET not set".to_string());
            }
        };
        let password_hash = match env::var("ADMIN_PASSWORD_HASH") {
            Ok(val) => val,
            Err(_) => {
                error!("ADMIN_PASSWORD_HASH not set in environment");
                return Err("ADMIN_PASSWORD_HASH not set".to_string());
            }
        };
        Ok(Self::new(&admin_id, &admin_name, &wallet_address, &password_hash))
    }
} 