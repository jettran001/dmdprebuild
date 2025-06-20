use std::time::Duration;
use chrono::{Utc, DateTime};
use ethers::types::Address;
use wallet::cache::{self, LRUCache, Cache, update_premium_user_cache, update_vip_user_cache, get_premium_user, get_vip_user, invalidate_user_cache};
use wallet::users::premium_user::PremiumUserData;
use wallet::users::vip_user::VipUserData;
use wallet::users::subscription::user_subscription::UserSubscription;
use wallet::users::subscription::types::{SubscriptionType, SubscriptionStatus, PaymentToken};

#[tokio::test]
async fn test_cache_basic_operations() {
    // Tạo cache với capacity 10 và TTL 1 giây
    let mut cache = LRUCache::with_ttl(10, Duration::from_secs(1));
    
    // Thêm dữ liệu vào cache
    cache.insert("key1".to_string(), "value1");
    cache.insert("key2".to_string(), "value2");
    
    // Kiểm tra lấy dữ liệu
    assert_eq!(cache.get(&"key1".to_string()), Some(&"value1"));
    assert_eq!(cache.get(&"key2".to_string()), Some(&"value2"));
    
    // Kiểm tra xóa dữ liệu
    assert_eq!(cache.remove(&"key1".to_string()), Some("value1"));
    assert_eq!(cache.get(&"key1".to_string()), None);
    
    // Kiểm tra LRU eviction
    for i in 0..20 {
        cache.insert(format!("new_key_{}", i), format!("value_{}", i));
    }
    
    // key2 đã bị loại bỏ do LRU và đã thêm 20 key mới
    assert_eq!(cache.get(&"key2".to_string()), None);
}

#[tokio::test]
async fn test_cache_ttl() {
    // Tạo cache với capacity 10 và TTL 100ms
    let mut cache = LRUCache::with_ttl(10, Duration::from_millis(100));
    
    // Thêm dữ liệu vào cache
    cache.insert("key1".to_string(), "value1");
    
    // Kiểm tra ngay lập tức
    assert_eq!(cache.get(&"key1".to_string()), Some(&"value1"));
    
    // Đợi cho TTL hết hạn
    tokio::time::sleep(Duration::from_millis(150)).await;
    
    // Kiểm tra sau khi hết hạn
    assert_eq!(cache.get(&"key1".to_string()), None);
}

#[tokio::test]
async fn test_premium_user_cache() {
    // Tạo dữ liệu PremiumUserData để test
    let user_id = "test_premium_user";
    let now = Utc::now();
    let end_date = now + chrono::Duration::days(30);
    let address = Address::random();
    
    let premium_data = PremiumUserData {
        user_id: user_id.to_string(),
        created_at: now,
        last_active: now,
        wallet_addresses: vec![address],
        subscription_end_date: end_date,
        auto_renew: true,
        payment_history: vec![],
    };
    
    // Cập nhật cache
    update_premium_user_cache(user_id, premium_data.clone()).await;
    
    // Lấy từ cache với loader function không bao giờ được gọi
    let loaded_data = get_premium_user(user_id, |_| async { None }).await;
    assert!(loaded_data.is_some());
    
    let loaded_data = loaded_data.unwrap();
    assert_eq!(loaded_data.user_id, user_id);
    assert_eq!(loaded_data.subscription_end_date, end_date);
    
    // Xóa cache
    invalidate_user_cache(user_id).await;
    
    // Kiểm tra đã xóa thành công
    let loaded_data = get_premium_user(user_id, |_| async { None }).await;
    assert!(loaded_data.is_none());
}

#[tokio::test]
async fn test_vip_user_cache() {
    // Tạo dữ liệu VipUserData để test
    let user_id = "test_vip_user";
    let now = Utc::now();
    let end_date = now + chrono::Duration::days(365);
    let address = Address::random();
    
    let vip_data = VipUserData {
        user_id: user_id.to_string(),
        created_at: now,
        last_active: now,
        wallet_addresses: vec![address],
        subscription_end_date: end_date,
        vip_support_code: "VIP-12345678".to_string(),
        special_privileges: vec!["Priority Support".to_string()],
        nft_info: None,
    };
    
    // Cập nhật cache
    update_vip_user_cache(user_id, vip_data.clone()).await;
    
    // Lấy từ cache với loader function không bao giờ được gọi
    let loaded_data = get_vip_user(user_id, |_| async { None }).await;
    assert!(loaded_data.is_some());
    
    let loaded_data = loaded_data.unwrap();
    assert_eq!(loaded_data.user_id, user_id);
    assert_eq!(loaded_data.subscription_end_date, end_date);
    assert_eq!(loaded_data.special_privileges.len(), 1);
    
    // Xóa cache
    invalidate_user_cache(user_id).await;
    
    // Kiểm tra đã xóa thành công
    let loaded_data = get_vip_user(user_id, |_| async { None }).await;
    assert!(loaded_data.is_none());
}

#[tokio::test]
async fn test_loader_function() {
    // Tạo dữ liệu PremiumUserData để test
    let user_id = "test_loader_user";
    let now = Utc::now();
    let end_date = now + chrono::Duration::days(30);
    let address = Address::random();
    
    // Dữ liệu từ "database"
    let db_data = PremiumUserData {
        user_id: user_id.to_string(),
        created_at: now,
        last_active: now,
        wallet_addresses: vec![address],
        subscription_end_date: end_date,
        auto_renew: true,
        payment_history: vec![],
    };
    
    // Tạo loader function trả về dữ liệu từ "database"
    let loader = |id: String| async move {
        assert_eq!(id, user_id);
        Some(db_data.clone())
    };
    
    // Lấy dữ liệu từ cache (sẽ gọi loader vì cache trống)
    let loaded_data = get_premium_user(user_id, loader).await;
    assert!(loaded_data.is_some());
    
    let loaded_data = loaded_data.unwrap();
    assert_eq!(loaded_data.user_id, user_id);
    
    // Xóa dữ liệu khỏi cache
    invalidate_user_cache(user_id).await;
    
    // Tạo loader mới với biến đếm số lần được gọi
    let mut call_count = 0;
    let new_loader = |id: String| async move {
        call_count += 1;
        assert_eq!(id, user_id);
        Some(db_data.clone())
    };
    
    // Lấy dữ liệu (lần 1)
    let _ = get_premium_user(user_id, new_loader.clone()).await;
    assert_eq!(call_count, 1); // Loader được gọi 1 lần
    
    // Lấy dữ liệu (lần 2) - không gọi loader vì có trong cache
    let _ = get_premium_user(user_id, new_loader.clone()).await;
    assert_eq!(call_count, 1); // Loader vẫn chỉ được gọi 1 lần
}
