//! Các hàm tiện ích cho module subscription.

use chrono::{DateTime, Utc};
use ethers::types::{Address, U256};
use tracing::{info, warn, error};

use crate::error::WalletError;
use super::types::{SubscriptionType, PaymentToken};

/// Tính giá trị token dựa trên loại token và loại đăng ký.
///
/// # Arguments
/// * `subscription_type` - Loại đăng ký (Premium, VIP)
/// * `payment_token` - Loại token thanh toán (DMD, USDC)
///
/// # Returns
/// Giá trị token cần thanh toán
pub fn calculate_payment_amount(
    subscription_type: SubscriptionType,
    payment_token: PaymentToken,
) -> U256 {
    match (subscription_type, payment_token) {
        (SubscriptionType::Premium, PaymentToken::DMD) => U256::from(100),
        (SubscriptionType::Premium, PaymentToken::USDC) => U256::from(100),
        (SubscriptionType::VIP, PaymentToken::DMD) => U256::from(300),
        (SubscriptionType::VIP, PaymentToken::USDC) => U256::from(300),
        (SubscriptionType::Free, _) => U256::from(0),
    }
}

/// Kiểm tra định dạng của hash giao dịch.
///
/// # Arguments
/// * `tx_hash` - Hash giao dịch cần kiểm tra
///
/// # Returns
/// `true` nếu định dạng hợp lệ, `false` nếu không
pub fn validate_transaction_hash(tx_hash: &str) -> bool {
    // Kiểm tra độ dài và định dạng (0x + 64 ký tự hex)
    if tx_hash.len() != 66 || !tx_hash.starts_with("0x") {
        return false;
    }
    
    // Kiểm tra các ký tự còn lại là hex
    for c in tx_hash[2..].chars() {
        if !c.is_ascii_hexdigit() {
            return false;
        }
    }
    
    true
}

/// Xác định thời gian reset tiếp theo cho auto_trade.
///
/// # Arguments
/// * `subscription_type` - Loại đăng ký
/// * `current_time` - Thời gian hiện tại
///
/// # Returns
/// Thời điểm reset tiếp theo
pub fn get_next_reset_time(
    subscription_type: SubscriptionType,
    current_time: DateTime<Utc>,
) -> DateTime<Utc> {
    use super::constants::{FREE_USER_RESET_DAYS, VIP_RESET_HOUR};
    
    match subscription_type {
        SubscriptionType::Free => {
            // Free user: reset sau mỗi 7 ngày
            current_time + chrono::Duration::days(FREE_USER_RESET_DAYS)
        },
        SubscriptionType::Premium => {
            // Premium user: reset sau 0h UTC mỗi ngày
            let tomorrow = current_time.date_naive().succ_opt()
                .unwrap_or_else(|| {
                    // Fallback nếu không thể lấy ngày tiếp theo (rất hiếm)
                    current_time.date_naive() + chrono::Duration::days(1)
                });
            
            // Tạo datetime với 0h
            Utc.from_utc_datetime(&tomorrow.and_hms_opt(0, 0, 0)
                .unwrap_or_else(|| {
                    // Fallback sử dụng thời gian hiện tại + 1 ngày
                    let naive_dt = current_time.naive_utc() + chrono::Duration::days(1);
                    chrono::NaiveDateTime::new(naive_dt.date(), chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap_or_default())
                }))
        },
        SubscriptionType::VIP => {
            // VIP user: reset sau 7h UTC mỗi ngày
            let tomorrow = current_time.date_naive().succ_opt()
                .unwrap_or_else(|| {
                    // Fallback nếu không thể lấy ngày tiếp theo (rất hiếm)
                    current_time.date_naive() + chrono::Duration::days(1)
                });
            
            // Tạo datetime với VIP_RESET_HOUR
            Utc.from_utc_datetime(&tomorrow.and_hms_opt(VIP_RESET_HOUR, 0, 0)
                .unwrap_or_else(|| {
                    // Fallback sử dụng thời gian hiện tại + 1 ngày
                    let naive_dt = current_time.naive_utc() + chrono::Duration::days(1);
                    chrono::NaiveDateTime::new(naive_dt.date(), chrono::NaiveTime::from_hms_opt(VIP_RESET_HOUR, 0, 0).unwrap_or_default())
                }))
        },
        _ => current_time + chrono::Duration::days(1),
    }
}

/// Kiểm tra nếu có cần reset auto_trade không.
///
/// # Arguments
/// * `last_reset` - Thời điểm reset cuối cùng
/// * `current_time` - Thời gian hiện tại
/// * `subscription_type` - Loại đăng ký
///
/// # Returns
/// `true` nếu cần reset, `false` nếu không
pub fn should_reset_auto_trade(
    last_reset: DateTime<Utc>,
    current_time: DateTime<Utc>,
    subscription_type: SubscriptionType,
) -> bool {
    use super::constants::{FREE_USER_RESET_DAYS, VIP_RESET_HOUR};
    
    match subscription_type {
        SubscriptionType::Free => {
            // Free user: reset sau mỗi 7 ngày
            (current_time - last_reset).num_days() >= FREE_USER_RESET_DAYS
        },
        SubscriptionType::Premium => {
            // Premium user: reset sau 0h UTC mỗi ngày
            let last_day = last_reset.date_naive().day();
            let current_day = current_time.date_naive().day();
            
            // Nếu khác ngày và thời gian hiện tại đã qua 0h UTC
            last_day != current_day
        },
        SubscriptionType::VIP => {
            // VIP user: reset sau VIP_RESET_HOUR UTC mỗi ngày
            let last_day = last_reset.date_naive().day();
            let current_day = current_time.date_naive().day();
            
            // Nếu khác ngày và thời gian hiện tại đã qua VIP_RESET_HOUR UTC
            last_day != current_day && current_time.hour() >= VIP_RESET_HOUR
        }
    }
}

/// Ghi log thông tin đăng ký của người dùng.
///
/// # Arguments
/// * `user_id` - ID người dùng
/// * `log_message` - Thông điệp cần ghi log
/// * `error` - Lỗi nếu có
pub fn log_subscription_activity(
    user_id: &str,
    log_message: &str,
    error: Option<&WalletError>,
) {
    if let Some(err) = error {
        error!("[Subscription] User {}: {} - Error: {}", user_id, log_message, err);
    } else {
        tracing::info!("[Subscription] User {}: {}", user_id, log_message);
    }
}

/// Ghi log hoạt động liên quan đến đăng ký
pub fn log_subscription_activity_str(user_id: &str, message: String) {
    info!(
        target: "subscription_activity",
        user_id = %user_id,
        timestamp = %Utc::now().to_rfc3339(),
        "{}", message
    );
}

/// Chuyển đổi chuỗi timestamp thành DateTime<Utc>
pub fn parse_timestamp(timestamp: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
}

/// Tính số ngày giữa hai ngày
pub fn days_between(start: DateTime<Utc>, end: DateTime<Utc>) -> i64 {
    let duration = end.signed_duration_since(start);
    duration.num_days()
}

/// Kiểm tra xem một ngày có nằm trong khoảng thời gian không
pub fn is_date_in_range(date: DateTime<Utc>, start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
    date >= start && date <= end
}

/// Định dạng ngày tháng thành chuỗi thân thiện
pub fn format_date_friendly(date: DateTime<Utc>) -> String {
    date.format("%d/%m/%Y").to_string()
}

/// Chuyển đổi chuỗi hex thành bytes
pub fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, hex::FromHexError> {
    let hex_str = if hex.starts_with("0x") {
        &hex[2..]
    } else {
        hex
    };
    
    hex::decode(hex_str)
} 