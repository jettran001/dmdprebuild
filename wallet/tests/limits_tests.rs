//! Test cases cho chức năng kiểm tra giới hạn tính năng.

// External imports
use ethers::types::Address;
use uuid::Uuid;
use chrono::Utc;

// Internal imports
use crate::users::free_user::{
    FreeUserManager, TransactionType, SnipeResult,
    types::{MAX_TRANSACTIONS_PER_DAY, MAX_SNIPEBOT_ATTEMPTS_PER_DAY, MAX_WALLETS_FREE_USER}
};

#[tokio::test]
async fn test_transaction_limits() {
    // Khởi tạo FreeUserManager
    let manager = FreeUserManager::new();
    
    // Tạo địa chỉ ngẫu nhiên
    let address = Address::random();
    
    // Tạo user_id free user
    let user_id = format!("FREE_{}_123456789abcdef", Utc::now().timestamp());
    
    // Đăng ký người dùng
    let register_result = manager.register_user(user_id.clone(), address).await;
    assert!(register_result.is_ok(), "Đăng ký user thất bại");
    
    // Kiểm tra giới hạn giao dịch
    for i in 0..MAX_TRANSACTIONS_PER_DAY {
        let tx_id = Uuid::new_v4().to_string();
        let record_result = manager.record_transaction(
            &user_id, 
            address, 
            tx_id, 
            TransactionType::Send
        ).await;
        
        assert!(record_result.is_ok(), "Ghi nhận giao dịch {} thất bại", i);
    }
    
    // Kiểm tra có thể giao dịch không
    let can_tx = manager.can_perform_transaction(&user_id).await;
    assert!(can_tx.is_ok(), "Kiểm tra giới hạn giao dịch lỗi");
    
    if let Ok(can_transact) = can_tx {
        assert!(!can_transact, "Người dùng vượt quá giới hạn giao dịch vẫn có thể giao dịch");
    } else {
        panic!("Kiểm tra khả năng giao dịch thất bại");
    }
}

#[tokio::test]
async fn test_snipebot_limits() {
    // Khởi tạo FreeUserManager
    let manager = FreeUserManager::new();
    
    // Tạo địa chỉ ngẫu nhiên
    let address = Address::random();
    
    // Tạo user_id free user
    let user_id = format!("FREE_{}_123456789abcdef", Utc::now().timestamp());
    
    // Đăng ký người dùng
    let register_result = manager.register_user(user_id.clone(), address).await;
    assert!(register_result.is_ok(), "Đăng ký user thất bại");
    
    // Kiểm tra giới hạn snipebot
    for i in 0..MAX_SNIPEBOT_ATTEMPTS_PER_DAY {
        let attempt_id = Uuid::new_v4().to_string();
        let record_result = manager.record_snipebot_attempt(
            &user_id, 
            address, 
            attempt_id, 
            SnipeResult::Success
        ).await;
        
        assert!(record_result.is_ok(), "Ghi nhận snipe {} thất bại", i);
    }
    
    // Kiểm tra có thể sử dụng snipebot không
    let can_snipe = manager.can_use_snipebot(&user_id).await;
    assert!(can_snipe.is_ok(), "Kiểm tra giới hạn snipebot lỗi");
    
    if let Ok(can_use) = can_snipe {
        assert!(!can_use, "Người dùng vượt quá giới hạn snipebot vẫn có thể snipe");
    } else {
        panic!("Kiểm tra khả năng sử dụng snipebot thất bại");
    }
}

#[tokio::test]
async fn test_wallet_limit() {
    // Khởi tạo FreeUserManager
    let manager = FreeUserManager::new();
    
    // Tạo user_id free user
    let user_id = format!("FREE_{}_123456789abcdef", Utc::now().timestamp());
    
    // Thêm địa chỉ ví đến giới hạn
    for i in 0..MAX_WALLETS_FREE_USER {
        let address = Address::random();
        let result = manager.register_user(user_id.clone(), address).await;
        assert!(result.is_ok(), "Không thể đăng ký ví thứ {}", i + 1);
    }
    
    // Thêm ví vượt quá giới hạn
    let extra_address = Address::random();
    let result = manager.register_user(user_id.clone(), extra_address).await;
    assert!(result.is_err(), "Có thể đăng ký ví vượt quá giới hạn");
}