//! Định nghĩa các kiểu dữ liệu và enum cho hệ thống đăng ký.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};
use uuid::Uuid;

use crate::blockchain::types::TokenInfo;

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
    /// Tạo và quản lý bot
    BotManagement,
    /// Giao dịch tự động
    AutoTrade,
    /// Phân tích thị trường nâng cao
    AdvancedAnalytics,
    /// Thông báo theo thời gian thực
    RealTimeAlerts,
    /// Giao dịch ưu tiên
    PriorityTrading,
    /// Hỗ trợ người dùng VIP
    VipSupport,
    /// API nâng cao
    AdvancedApi,
    /// Công cụ quản lý rủi ro
    RiskManagement,
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