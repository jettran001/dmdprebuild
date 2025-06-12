//! Định nghĩa kiểu dữ liệu cho free_user module.

// External imports
use chrono::{DateTime, Utc};
use ethers::types::Address;
use serde::{Deserialize, Serialize};

/// Thông tin người dùng miễn phí.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreeUserData {
    /// ID người dùng
    pub user_id: String,
    /// Thời gian tạo tài khoản
    pub created_at: DateTime<Utc>,
    /// Thời gian hoạt động cuối
    pub last_active: DateTime<Utc>,
    /// Trạng thái tài khoản
    pub status: UserStatus,
    /// Danh sách địa chỉ ví
    pub wallet_addresses: Vec<Address>,
}

/// Trạng thái tài khoản người dùng.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UserStatus {
    /// Tài khoản hoạt động bình thường
    Active,
    /// Tài khoản bị giới hạn (vượt quá giới hạn sử dụng)
    Limited,
    /// Tài khoản bị tạm ngừng
    Suspended,
    /// Tài khoản bị khóa vĩnh viễn
    Banned,
}

/// Bản ghi giao dịch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecord {
    /// ID của giao dịch
    pub tx_id: String,
    /// Địa chỉ ví
    pub wallet_address: Address,
    /// Thời gian giao dịch
    pub timestamp: DateTime<Utc>,
    /// Loại giao dịch
    pub tx_type: TransactionType,
}

/// Loại giao dịch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TransactionType {
    /// Gửi token
    Send,
    /// Nhận token
    Receive,
    /// Swap token
    Swap,
    /// Giao dịch snipe
    Snipe,
}

/// Bản ghi snipebot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnipebotAttempt {
    /// ID của lần thử snipe
    pub attempt_id: String,
    /// Địa chỉ ví
    pub wallet_address: Address,
    /// Thời gian thử
    pub timestamp: DateTime<Utc>,
    /// Kết quả snipe
    pub result: SnipeResult,
}

/// Kết quả của lần thử snipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SnipeResult {
    /// Thành công
    Success,
    /// Thất bại
    Failed,
    /// Bị từ chối (do giới hạn)
    Rejected,
}

/// Số lượng tối đa ví mà người dùng free có thể tạo và quản lý.
///
/// Giới hạn này áp dụng cho toàn bộ người dùng free để tránh lạm dụng
/// tài nguyên hệ thống. Mỗi ví được tính là một địa chỉ trong danh sách
/// `wallet_addresses` của đối tượng `FreeUserData`.
///
/// # Examples
/// ```
/// use wallet::users::free_user::MAX_WALLETS_FREE_USER;
///
/// // Kiểm tra xem người dùng có thể thêm ví mới không
/// if user_data.wallet_addresses.len() >= MAX_WALLETS_FREE_USER {
///     return Err("Đã đạt giới hạn số lượng ví");
/// }
/// ```
pub const MAX_WALLETS_FREE_USER: usize = 3;

/// Số lượng giao dịch tối đa mà người dùng free có thể thực hiện mỗi ngày.
///
/// Giao dịch bao gồm tất cả các loại được định nghĩa trong `TransactionType`:
/// gửi, nhận, swap và snipe. Giới hạn này được reset mỗi ngày vào 00:00 UTC.
///
/// # Examples
/// ```
/// use wallet::users::free_user::MAX_TRANSACTIONS_PER_DAY;
///
/// // Kiểm tra giới hạn giao dịch
/// if daily_transaction_count >= MAX_TRANSACTIONS_PER_DAY {
///     return Err("Đã đạt giới hạn giao dịch trong ngày");
/// }
/// ```
pub const MAX_TRANSACTIONS_PER_DAY: usize = 10;

/// Số lần tối đa người dùng free có thể sử dụng snipebot mỗi ngày.
///
/// Giới hạn này áp dụng cho toàn bộ người dùng, độc lập với số lượng ví họ sở hữu.
/// Mỗi lần cố gắng sử dụng snipebot được ghi lại trong `SnipebotAttempt`.
///
/// # Examples
/// ```
/// use wallet::users::free_user::MAX_SNIPEBOT_ATTEMPTS_PER_DAY;
///
/// // Kiểm tra giới hạn snipebot
/// if daily_snipebot_attempts >= MAX_SNIPEBOT_ATTEMPTS_PER_DAY {
///     return Err("Đã đạt giới hạn sử dụng snipebot trong ngày");
/// }
/// ```
pub const MAX_SNIPEBOT_ATTEMPTS_PER_DAY: usize = 3;

/// Số lần tối đa người dùng free có thể sử dụng snipebot cho mỗi ví mỗi ngày.
///
/// Giới hạn này áp dụng riêng cho mỗi ví của người dùng, đảm bảo không có ví nào
/// được sử dụng quá nhiều lần để snipe token trong ngày.
///
/// # Examples
/// ```
/// use wallet::users::free_user::MAX_SNIPEBOT_ATTEMPTS_PER_WALLET;
///
/// // Kiểm tra giới hạn snipebot cho ví cụ thể
/// if wallet_snipebot_attempts >= MAX_SNIPEBOT_ATTEMPTS_PER_WALLET {
///     return Err("Đã đạt giới hạn sử dụng snipebot cho ví này trong ngày");
/// }
/// ```
pub const MAX_SNIPEBOT_ATTEMPTS_PER_WALLET: usize = 1; 