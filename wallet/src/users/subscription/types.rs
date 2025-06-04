//! Định nghĩa các kiểu dữ liệu và enum cho hệ thống đăng ký.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};
use uuid::Uuid;
use std::fmt;

use crate::blockchain::types::TokenInfo;
use super::staking::StakeStatus;
use super::events::EventType;

/// Các loại đăng ký dịch vụ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum SubscriptionType {
    /// Gói miễn phí với chức năng giới hạn
    Free,
    /// Gói cao cấp với đầy đủ chức năng
    Premium,
    /// Gói vĩnh viễn với đầy đủ chức năng
    Lifetime,
    /// Gói VIP dành cho người dùng sở hữu NFT
    #[strum(serialize = "vip")]
    VIP,
}

impl Default for SubscriptionType {
    fn default() -> Self {
        Self::Free
    }
}

/// Trạng thái đăng ký của người dùng
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display)]
#[strum(serialize_all = "lowercase")]
pub enum SubscriptionStatus {
    /// Đăng ký đang hoạt động
    Active,
    /// Đăng ký đã hết hạn
    Expired,
    /// Đăng ký đang chờ thanh toán
    Pending,
    /// Đăng ký đã bị hủy
    Cancelled,
}

impl Default for SubscriptionStatus {
    fn default() -> Self {
        Self::Expired
    }
}

/// Token thanh toán được hỗ trợ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[strum(serialize_all = "uppercase")]
pub enum PaymentToken {
    /// Token USDC
    USDC,
    /// Token DMD
    DMD,
}

impl Default for PaymentToken {
    fn default() -> Self {
        Self::USDC
    }
}

/// Thông tin chi tiết về một gói đăng ký
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionPlan {
    /// Loại gói đăng ký
    pub plan_type: SubscriptionType,
    /// Giá của gói (USDC)
    pub price_usdc: Decimal,
    /// Giá của gói (DMD)
    pub price_dmd: Decimal,
    /// Thời hạn gói tính bằng ngày
    pub duration_days: i64,
    /// Số lượng bot tối đa có thể tạo
    pub max_bots: usize,
    /// Số lượng auto-trade tối đa được phép
    pub max_auto_trades: usize,
    /// Danh sách các tính năng được bao gồm
    pub features: Vec<Feature>,
    /// Mô tả về gói
    pub description: String,
    /// Ngày gói được tạo
    pub created_at: DateTime<Utc>,
    /// Ngày gói được cập nhật
    pub updated_at: DateTime<Utc>,
}

/// Các tính năng của hệ thống
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum Feature {
    /// Quản lý bot
    BotManagement,
    /// Giao dịch tự động
    AutoTrade,
    /// Phân tích nâng cao
    AdvancedAnalytics,
    /// Cảnh báo thời gian thực
    RealTimeAlerts,
    /// Giao dịch ưu tiên
    PriorityTrading,
    /// Hỗ trợ VIP
    VipSupport,
    /// API nâng cao
    AdvancedApi,
    /// Quản lý rủi ro
    RiskManagement,
    /// Staking token
    TokenStaking,
}

/// Kết quả kiểm tra giao dịch thanh toán
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionCheckResult {
    /// Giao dịch đã được xác nhận
    Confirmed {
        /// Token sử dụng cho thanh toán
        token: TokenInfo,
        /// Số lượng token
        amount: Decimal,
        /// Loại gói đăng ký
        plan: SubscriptionType,
        /// ID giao dịch blockchain
        tx_id: String,
    },
    /// Giao dịch đang chờ xử lý
    Pending,
    /// Giao dịch thất bại
    Failed {
        /// Lý do thất bại
        reason: String,
    },
    /// Đã hết thời gian chờ
    Timeout,
}

/// Thông tin đăng ký của người dùng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionRecord {
    /// ID đăng ký
    pub id: Uuid,
    /// ID người dùng
    pub user_id: Uuid,
    /// Loại gói đăng ký
    pub subscription_type: SubscriptionType,
    /// Trạng thái đăng ký
    pub status: SubscriptionStatus,
    /// Ngày bắt đầu đăng ký
    pub start_date: DateTime<Utc>,
    /// Ngày kết thúc đăng ký
    pub end_date: DateTime<Utc>,
    /// Token sử dụng để thanh toán
    pub payment_token: PaymentToken,
    /// Số tiền đã thanh toán
    pub amount_paid: Decimal,
    /// ID giao dịch blockchain (nếu có)
    pub transaction_id: Option<String>,
    /// Có tự động gia hạn không
    pub auto_renew: bool,
    /// Số lượng bot đã sử dụng
    pub bots_used: usize,
    /// Số lượng auto-trade đã sử dụng
    pub auto_trades_used: usize,
    /// Ngày tạo bản ghi
    pub created_at: DateTime<Utc>,
    /// Ngày cập nhật bản ghi
    pub updated_at: DateTime<Utc>,
}

/// Định nghĩa các loại sự kiện liên quan đến subscription
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub use super::events::EventType;

/// Thông tin về một sự kiện subscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionEvent {
    /// ID của người dùng
    pub user_id: String,
    /// Loại sự kiện
    pub event_type: EventType,
    /// Thời gian xảy ra sự kiện
    pub timestamp: DateTime<Utc>,
    /// Dữ liệu bổ sung (tùy theo loại sự kiện)
    pub data: Option<String>,
}

/// Trạng thái của việc stake token
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub use super::staking::StakeStatus;

/// Thông tin về việc stake token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStake {
    /// ID của người dùng
    pub user_id: String,
    /// Địa chỉ ví
    pub wallet_address: String,
    /// Loại blockchain
    pub chain_type: String,
    /// Số lượng token đã stake
    pub amount: String,
    /// Thời gian bắt đầu stake
    pub start_date: DateTime<Utc>,
    /// Thời gian kết thúc stake
    pub end_date: DateTime<Utc>,
    /// Trạng thái stake
    pub status: StakeStatus,
    /// Hash giao dịch stake
    pub tx_hash: Option<String>,
}

/// Trạng thái của stake
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StakeStatus {
    /// Đang hoạt động
    Active,
    /// Đã hoàn thành
    Completed,
    /// Đã hủy
    Cancelled,
}

impl fmt::Display for StakeStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StakeStatus::Active => write!(f, "active"),
            StakeStatus::Completed => write!(f, "completed"),
            StakeStatus::Cancelled => write!(f, "cancelled"),
        }
    }
} 