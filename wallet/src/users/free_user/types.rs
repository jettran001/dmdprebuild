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

// Hằng số giới hạn của free user
pub const MAX_WALLETS_FREE_USER: usize = 3;
pub const MAX_TRANSACTIONS_PER_DAY: usize = 10;
pub const MAX_SNIPEBOT_ATTEMPTS_PER_DAY: usize = 3;
pub const MAX_SNIPEBOT_ATTEMPTS_PER_WALLET: usize = 1;
