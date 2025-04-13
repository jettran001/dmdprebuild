//! Test cases cho chức năng xác thực và đăng ký người dùng.

// External imports
use chrono::Utc;
use ethers::types::Address;
use std::collections::HashMap;

// Internal imports
use crate::users::free_user::{
    FreeUserManager, UserStatus,
    test_utils::MockWalletHandler,
};

#[tokio::test]
async fn test_verify_free_user() {
    // Khởi tạo FreeUserManager
    let manager = FreeUserManager::new();
    
    // Tạo địa chỉ ngẫu nhiên
    let address = Address::random();
    
    // Tạo user_id free user
    let user_id = format!("FREE_{}_123456789abcdef", Utc::now().timestamp());
    
    // Tạo mock wallet handler
    let mut user_ids = HashMap::new();
    user_ids.insert(address, user_id.clone());
    let handler = MockWalletHandler { user_ids };
    
    // Kiểm tra verify_free_user
    let result = manager.verify_free_user(&handler, address).await;
    assert!(result.is_ok(), "Verify free user thất bại: {:?}", result);
    assert!(result.unwrap(), "Ví phải được xác minh là free user");
    
    // Kiểm tra dữ liệu người dùng
    let user_data = manager.get_user_data(&user_id).await;
    assert!(user_data.is_ok(), "Không thể lấy dữ liệu user: {:?}", user_data);
    let data = user_data.unwrap();
    
    assert_eq!(data.user_id, user_id, "User ID không khớp");
    assert_eq!(data.status, UserStatus::Active, "Trạng thái không đúng");
    assert!(data.wallet_addresses.contains(&address), "Địa chỉ ví không được lưu");
}