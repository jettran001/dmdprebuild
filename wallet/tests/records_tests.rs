//! Test cases cho chức năng ghi nhận hoạt động người dùng.

// External imports
use ethers::types::Address;
use uuid::Uuid;
use chrono::Utc;

// Internal imports
use crate::users::free_user::{
    FreeUserManager, TransactionType, SnipeResult,
};

#[tokio::test]
async fn test_record_transaction() {
    // Khởi tạo FreeUserManager
    let manager = FreeUserManager::new();
    
    // Tạo địa chỉ ngẫu nhiên
    let address = Address::random();
    
    // Tạo user_id free user
    let user_id = format!("FREE_{}_123456789abcdef", Utc::now().timestamp());
    
    // Đăng ký người dùng
    let register_result = manager.register_user(user_id.clone(), address).await;
    assert!(register_result.is_ok(), "Đăng ký user thất bại");
    
    // Ghi nhận giao dịch
    let tx_id = Uuid::new_v4().to_string();
    let record_result = manager.record_transaction(
        &user_id, 
        address, 
        tx_id.clone(), 
        TransactionType::Send
    ).await;
    
    assert!(record_result.is_ok(), "Ghi nhận giao dịch thất bại");
    
    // Kiểm tra lịch sử giao dịch
    let tx_history = manager.get_transaction_history(&user_id).await;
    assert!(tx_history.is_ok(), "Lấy lịch sử giao dịch thất bại");
    
    if let Ok(history) = tx_history {
        assert_eq!(history.len(), 1, "Lịch sử giao dịch phải có đúng 1 mục");
        assert_eq!(history[0].tx_id, tx_id, "ID giao dịch không khớp");
        assert_eq!(history[0].wallet_address, address, "Địa chỉ ví không khớp");
        assert_eq!(history[0].tx_type, TransactionType::Send, "Loại giao dịch không khớp");
    } else {
        panic!("Không thể lấy lịch sử giao dịch");
    }
}

#[tokio::test]
async fn test_record_snipebot_attempt() {
    // Khởi tạo FreeUserManager
    let manager = FreeUserManager::new();
    
    // Tạo địa chỉ ngẫu nhiên
    let address = Address::random();
    
    // Tạo user_id free user
    let user_id = format!("FREE_{}_123456789abcdef", Utc::now().timestamp());
    
    // Đăng ký người dùng
    let register_result = manager.register_user(user_id.clone(), address).await;
    assert!(register_result.is_ok(), "Đăng ký user thất bại");
    
    // Ghi nhận lần thử snipebot
    let attempt_id = Uuid::new_v4().to_string();
    let record_result = manager.record_snipebot_attempt(
        &user_id, 
        address, 
        attempt_id.clone(), 
        SnipeResult::Success
    ).await;
    
    assert!(record_result.is_ok(), "Ghi nhận snipebot thất bại");
    
    // Kiểm tra lịch sử snipebot
    let snipe_history = manager.get_snipebot_history(&user_id).await;
    assert!(snipe_history.is_ok(), "Lấy lịch sử snipebot thất bại");
    
    if let Ok(history) = snipe_history {
        assert_eq!(history.len(), 1, "Lịch sử snipebot phải có đúng 1 mục");
        assert_eq!(history[0].attempt_id, attempt_id, "ID lần thử không khớp");
        assert_eq!(history[0].wallet_address, address, "Địa chỉ ví không khớp");
        assert_eq!(history[0].result, SnipeResult::Success, "Kết quả không khớp");
    } else {
        panic!("Không thể lấy lịch sử snipebot");
    }
}