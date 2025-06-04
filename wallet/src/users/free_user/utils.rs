/// Các hàm tiện ích cho module free_user.
///
/// Module này cung cấp các hàm hỗ trợ cho việc quản lý người dùng miễn phí,
/// bao gồm các hàm để tạo dữ liệu người dùng, kiểm tra giới hạn và xử lý
/// các yêu cầu đặc biệt của người dùng free.

use chrono::{DateTime, Utc};
use ethers::types::Address;
use std::collections::HashMap;
use crate::users::free_user::types::{FreeUserData, UserStatus, TransactionType, SnipeResult};
use crate::users::subscription::constants::FREE_USER_RESET_DAYS;

/// Tạo dữ liệu người dùng free mới.
///
/// Hàm này tạo một đối tượng FreeUserData mới với các thông số mặc định
/// cho người dùng miễn phí. UserStatus được đặt là Active và số ví được
/// khởi tạo là 0.
///
/// # Arguments
/// * `user_id` - ID người dùng cần tạo dữ liệu
/// * `address` - Địa chỉ ví ban đầu (nếu có)
///
/// # Returns
/// Đối tượng FreeUserData mới với các thông số mặc định
///
/// # Examples
/// ```
/// use wallet::users::free_user::create_free_user_data;
/// use ethers::types::Address;
///
/// let user_data = create_free_user_data("user123", Some(Address::random()));
/// assert_eq!(user_data.user_id, "user123");
/// assert_eq!(user_data.wallet_addresses.len(), 1); // Một ví
/// ```
pub fn create_free_user_data(user_id: &str, address: Option<Address>) -> FreeUserData {
    let now = Utc::now();
    let mut wallet_addresses = Vec::new();
    
    if let Some(addr) = address {
        wallet_addresses.push(addr);
    }
    
    FreeUserData {
        user_id: user_id.to_string(),
        created_at: now,
        last_active: now,
        status: UserStatus::Active,
        wallet_addresses,
        transaction_count: 0,
        daily_transactions: HashMap::new(),
        last_reset: now,
        next_reset: now + chrono::Duration::days(FREE_USER_RESET_DAYS),
    }
}

/// Kiểm tra nếu người dùng miễn phí cần được reset số liệu giao dịch.
///
/// Hàm này so sánh thời gian hiện tại với thời gian reset tiếp theo được lưu
/// trong dữ liệu người dùng để xác định xem có cần reset số liệu giao dịch hay không.
///
/// # Arguments
/// * `user_data` - Tham chiếu đến dữ liệu người dùng cần kiểm tra
/// * `now` - Thời gian hiện tại để so sánh (thường là Utc::now())
///
/// # Returns
/// * `true` - Nếu cần reset số liệu giao dịch
/// * `false` - Nếu chưa đến thời gian reset
///
/// # Examples
/// ```
/// use wallet::users::free_user::should_reset_transaction_data;
/// use wallet::users::free_user::create_free_user_data;
/// use chrono::Utc;
///
/// let mut user_data = create_free_user_data("user123", None);
/// // Giả định next_reset là quá khứ
/// user_data.next_reset = Utc::now() - chrono::Duration::hours(1);
/// 
/// assert!(should_reset_transaction_data(&user_data, Utc::now()));
/// ```
pub fn should_reset_transaction_data(user_data: &FreeUserData, now: DateTime<Utc>) -> bool {
    now >= user_data.next_reset
}

/// Reset số liệu giao dịch cho người dùng miễn phí.
///
/// Hàm này reset lại số liệu giao dịch của người dùng free, đặt số giao dịch về 0,
/// xóa danh sách giao dịch hàng ngày, và cập nhật thời gian reset tiếp theo.
///
/// # Arguments
/// * `user_data` - Tham chiếu đến dữ liệu người dùng cần reset
/// * `now` - Thời gian hiện tại (thường là Utc::now())
///
/// # Examples
/// ```
/// use wallet::users::free_user::{reset_transaction_data, create_free_user_data};
/// use chrono::Utc;
///
/// let mut user_data = create_free_user_data("user123", None);
/// user_data.transaction_count = 5; // Giả định có 5 giao dịch
///
/// reset_transaction_data(&mut user_data, Utc::now());
/// assert_eq!(user_data.transaction_count, 0); // Đã reset về 0
/// ```
pub fn reset_transaction_data(user_data: &mut FreeUserData, now: DateTime<Utc>) {
    user_data.transaction_count = 0;
    user_data.daily_transactions.clear();
    user_data.last_reset = now;
    user_data.next_reset = now + chrono::Duration::days(FREE_USER_RESET_DAYS);
}

/// Ghi nhận kết quả snipe cho người dùng.
///
/// Ghi lại kết quả của nỗ lực snipe vào lịch sử snipe của người dùng free.
/// Cũng cập nhật số lượng giao dịch nếu snipe thành công.
///
/// # Arguments
/// * `user_data` - Tham chiếu đến dữ liệu người dùng cần cập nhật
/// * `result` - Kết quả của nỗ lực snipe (Success, Failed, Rejected)
/// * `details` - Thông tin chi tiết về kết quả snipe (nếu có)
///
/// # Returns
/// Số lượng snipe thành công hiện tại của người dùng
///
/// # Examples
/// ```
/// use wallet::users::free_user::{record_snipe_result, create_free_user_data, SnipeResult};
///
/// let mut user_data = create_free_user_data("user123", None);
/// let success_count = record_snipe_result(&mut user_data, SnipeResult::Success, Some("Token XYZ".to_string()));
/// ```
pub fn record_snipe_result(
    user_data: &mut FreeUserData, 
    result: SnipeResult,
    details: Option<String>
) -> usize {
    if result == SnipeResult::Success {
        user_data.transaction_count += 1;
    }
    
    // Đáng lẽ phải lưu kết quả vào danh sách snipe history,
    // nhưng struct FreeUserData trong ví dụ này không có trường đó
    
    user_data.transaction_count
}

/// Kiểm tra xem người dùng có thể thực hiện giao dịch mới không.
///
/// Hàm này kiểm tra các giới hạn giao dịch của người dùng free để xác định
/// xem họ có thể thực hiện giao dịch mới hay không, dựa trên số lượng giao dịch
/// đã thực hiện và giới hạn tối đa được phép.
///
/// # Arguments
/// * `user_data` - Tham chiếu đến dữ liệu người dùng cần kiểm tra
/// * `tx_type` - Loại giao dịch muốn thực hiện
///
/// # Returns
/// * `true` - Nếu người dùng có thể thực hiện giao dịch mới
/// * `false` - Nếu người dùng đã đạt giới hạn giao dịch
///
/// # Examples
/// ```
/// use wallet::users::free_user::{can_perform_transaction, create_free_user_data, TransactionType};
///
/// let mut user_data = create_free_user_data("user123", None);
/// assert!(can_perform_transaction(&user_data, TransactionType::Send));
/// ```
pub fn can_perform_transaction(user_data: &FreeUserData, tx_type: TransactionType) -> bool {
    use crate::users::free_user::MAX_TRANSACTIONS_PER_DAY;
    
    // Check if user has reached transaction limit
    if user_data.transaction_count >= MAX_TRANSACTIONS_PER_DAY {
        return false;
    }
    
    // Specific limits for transaction types could be implemented here
    match tx_type {
        TransactionType::Send => true,
        TransactionType::Receive => true, // Usually not limited
        TransactionType::Swap => user_data.transaction_count < (MAX_TRANSACTIONS_PER_DAY / 2),
        TransactionType::Snipe => user_data.transaction_count < (MAX_TRANSACTIONS_PER_DAY / 3),
    }
} 