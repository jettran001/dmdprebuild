//! Module utils cung cấp các hàm tiện ích và công cụ hỗ trợ cho hệ thống quản lý đăng ký.
//! 
//! Module này bao gồm:
//! * Các hàm tính toán thời gian và ngày đăng ký (reset time, ngày hết hạn)
//! * Các công cụ xác thực giao dịch blockchain
//! * Hàm tính toán số lượng token thanh toán dựa trên loại đăng ký
//! * Các công cụ ghi log và theo dõi sự kiện đăng ký
//! * Công cụ lập lịch và chạy các tác vụ định kỳ liên quan đến đăng ký
//! * Các tiện ích xử lý ngày tháng và mã hóa
//! 
//! Module này đóng vai trò hỗ trợ cho toàn bộ hệ thống đăng ký, cung cấp các 
//! chức năng cơ bản được sử dụng bởi các module khác như auto_trade, 
//! payment, vip, premium, và manager.

//! Các hàm tiện ích cho module subscription.

use chrono::{DateTime, Duration, Utc};
use ethers::types::{Address, U256};
use tracing::{info, warn, error};
use log::{debug, error, info};
use std::sync::Arc;
use tokio::time;
use serde_json::json;
use std::str::FromStr;

use crate::error::WalletError;
use super::types::{SubscriptionType, PaymentToken, EventType, SubscriptionEvent, SubscriptionStatus, UserSubscription};
use super::manager::SubscriptionManager;

/// Tính giá trị token dựa trên loại token và loại đăng ký.
///
/// Hàm này tính toán số lượng token cần thiết để thanh toán cho một gói đăng ký
/// cụ thể, dựa trên loại token được chọn để thanh toán.
///
/// # Arguments
/// * `subscription_type` - Loại đăng ký (Premium, VIP)
/// * `payment_token` - Loại token thanh toán (DMD, USDC)
///
/// # Returns
/// Giá trị token cần thanh toán (U256)
///
/// # Examples
/// ```
/// use wallet::users::subscription::utils::calculate_payment_amount;
/// use wallet::users::subscription::types::{SubscriptionType, PaymentToken};
/// use ethers::types::U256;
///
/// let amount = calculate_payment_amount(SubscriptionType::Premium, PaymentToken::DMD);
/// assert_eq!(amount, U256::from(100));
/// ```
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
/// Hàm này xác nhận xem một chuỗi có đúng định dạng của một hash giao dịch Ethereum hay không.
/// Một hash giao dịch Ethereum hợp lệ phải bắt đầu bằng '0x' và theo sau là 64 ký tự hex.
///
/// # Arguments
/// * `tx_hash` - Hash giao dịch cần kiểm tra
///
/// # Returns
/// `true` nếu định dạng hợp lệ, `false` nếu không
///
/// # Examples
/// ```
/// use wallet::users::subscription::utils::validate_transaction_hash;
///
/// // Hash hợp lệ
/// assert!(validate_transaction_hash("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"));
///
/// // Hash không hợp lệ (thiếu 0x)
/// assert!(!validate_transaction_hash("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"));
///
/// // Hash không hợp lệ (độ dài không đúng)
/// assert!(!validate_transaction_hash("0x1234"));
///
/// // Hash không hợp lệ (chứa ký tự không phải hex)
/// assert!(!validate_transaction_hash("0x1234567890abcdef1234567890abcdefg1234567890abcdef1234567890abcde"));
/// ```
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
/// Hàm này tính toán thời điểm mà auto_trade sẽ được reset tiếp theo, 
/// dựa trên loại đăng ký của người dùng. Mỗi loại đăng ký có chu kỳ reset khác nhau:
/// - Free: reset sau mỗi 7 ngày
/// - Premium: reset vào lúc 0h UTC mỗi ngày
/// - VIP: reset vào lúc 7h UTC mỗi ngày (có thể thay đổi thông qua VIP_RESET_HOUR)
///
/// # Arguments
/// * `subscription_type` - Loại đăng ký
/// * `current_time` - Thời gian hiện tại
///
/// # Returns
/// Thời điểm reset tiếp theo (DateTime<Utc>)
///
/// # Examples
/// ```
/// use wallet::users::subscription::utils::get_next_reset_time;
/// use wallet::users::subscription::types::SubscriptionType;
/// use chrono::{DateTime, TimeZone, Utc};
///
/// // Lấy thời gian reset cho user Free
/// let now = Utc::now();
/// let reset_time = get_next_reset_time(SubscriptionType::Free, now);
/// assert!(reset_time > now);
/// assert_eq!((reset_time - now).num_days(), 7);
/// ```
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
/// Hàm này xác định liệu thời gian auto_trade có cần được reset hay không, 
/// dựa trên thời điểm reset cuối cùng, thời gian hiện tại và loại đăng ký.
/// Quy tắc reset phụ thuộc vào loại đăng ký:
/// - Free: reset sau mỗi 7 ngày
/// - Premium: reset sau 0h UTC mỗi ngày
/// - VIP: reset sau VIP_RESET_HOUR UTC mỗi ngày
///
/// # Arguments
/// * `last_reset` - Thời điểm reset cuối cùng
/// * `current_time` - Thời gian hiện tại
/// * `subscription_type` - Loại đăng ký
///
/// # Returns
/// `true` nếu cần reset, `false` nếu không
///
/// # Examples
/// ```
/// use wallet::users::subscription::utils::should_reset_auto_trade;
/// use wallet::users::subscription::types::SubscriptionType;
/// use chrono::{DateTime, Duration, TimeZone, Utc};
///
/// let now = Utc::now();
/// let eight_days_ago = now - Duration::days(8);
/// 
/// // Người dùng Free đã quá 7 ngày, cần reset
/// assert!(should_reset_auto_trade(eight_days_ago, now, SubscriptionType::Free));
///
/// let yesterday = now - Duration::days(1);
/// // Người dùng Premium, đã qua ngày mới, cần reset
/// assert!(should_reset_auto_trade(yesterday, now, SubscriptionType::Premium));
/// ```
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
/// Hàm này ghi log các hoạt động liên quan đến đăng ký của người dùng vào hệ thống log.
/// Nếu có lỗi, sẽ sử dụng mức log ERROR, nếu không sẽ sử dụng mức log INFO.
///
/// # Arguments
/// * `user_id` - ID người dùng
/// * `log_message` - Thông điệp cần ghi log
/// * `error` - Lỗi nếu có
///
/// # Examples
/// ```
/// use wallet::users::subscription::utils::log_subscription_activity;
/// use wallet::error::WalletError;
///
/// // Log thành công
/// log_subscription_activity("user123", "Đăng ký gói Premium thành công", None);
///
/// // Log lỗi
/// let error = WalletError::InvalidTransactionHash;
/// log_subscription_activity("user123", "Thanh toán thất bại", Some(&error));
/// ```
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

/// Ghi log hoạt động liên quan đến đăng ký với định dạng có cấu trúc
///
/// Hàm này ghi log các hoạt động liên quan đến đăng ký vào hệ thống log
/// với định dạng có cấu trúc, bao gồm user_id và timestamp.
///
/// # Arguments
/// * `user_id` - ID người dùng
/// * `message` - Thông điệp cần ghi log
///
/// # Examples
/// ```
/// use wallet::users::subscription::utils::log_subscription_activity_str;
///
/// log_subscription_activity_str("user123", "Đăng ký gói Premium thành công".to_string());
/// ```
pub fn log_subscription_activity_str(user_id: &str, message: String) {
    info!(
        target: "subscription_activity",
        user_id = %user_id,
        timestamp = %Utc::now().to_rfc3339(),
        "{}", message
    );
}

/// Chuyển đổi chuỗi timestamp thành DateTime<Utc>
///
/// Hàm này phân tích một chuỗi timestamp theo định dạng RFC 3339
/// và chuyển đổi thành đối tượng DateTime trong múi giờ UTC.
///
/// # Arguments
/// * `timestamp` - Chuỗi timestamp theo định dạng RFC 3339 cần phân tích
///
/// # Returns
/// * `Some(DateTime<Utc>)` - Nếu phân tích thành công
/// * `None` - Nếu chuỗi không đúng định dạng
///
/// # Examples
/// ```
/// use wallet::users::subscription::utils::parse_timestamp;
///
/// let dt = parse_timestamp("2023-01-01T12:00:00Z");
/// assert!(dt.is_some());
///
/// let invalid_dt = parse_timestamp("not-a-timestamp");
/// assert!(invalid_dt.is_none());
/// ```
pub fn parse_timestamp(timestamp: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
}

/// Tính số ngày giữa hai ngày
///
/// Hàm này tính toán số ngày chênh lệch giữa hai thời điểm,
/// với kết quả là một số nguyên có dấu (ngày).
///
/// # Arguments
/// * `start` - Thời điểm bắt đầu
/// * `end` - Thời điểm kết thúc
///
/// # Returns
/// Số ngày giữa hai thời điểm (end - start)
///
/// # Examples
/// ```
/// use wallet::users::subscription::utils::days_between;
/// use chrono::{TimeZone, Utc};
///
/// let start = Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap();
/// let end = Utc.with_ymd_and_hms(2023, 1, 10, 0, 0, 0).unwrap();
///
/// assert_eq!(days_between(start, end), 9);
/// assert_eq!(days_between(end, start), -9);
/// ```
pub fn days_between(start: DateTime<Utc>, end: DateTime<Utc>) -> i64 {
    let duration = end.signed_duration_since(start);
    duration.num_days()
}

/// Kiểm tra xem một ngày có nằm trong khoảng thời gian không
///
/// Hàm này xác định xem một thời điểm có nằm trong khoảng từ start đến end hay không,
/// bao gồm cả thời điểm start và end.
///
/// # Arguments
/// * `date` - Thời điểm cần kiểm tra
/// * `start` - Thời điểm bắt đầu của khoảng thời gian
/// * `end` - Thời điểm kết thúc của khoảng thời gian
///
/// # Returns
/// * `true` - Nếu date nằm trong khoảng [start, end]
/// * `false` - Nếu date nằm ngoài khoảng [start, end]
///
/// # Examples
/// ```
/// use wallet::users::subscription::utils::is_date_in_range;
/// use chrono::{TimeZone, Utc};
///
/// let start = Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap();
/// let end = Utc.with_ymd_and_hms(2023, 1, 10, 0, 0, 0).unwrap();
/// let date_in = Utc.with_ymd_and_hms(2023, 1, 5, 0, 0, 0).unwrap();
/// let date_out = Utc.with_ymd_and_hms(2023, 1, 15, 0, 0, 0).unwrap();
///
/// assert!(is_date_in_range(date_in, start, end));
/// assert!(!is_date_in_range(date_out, start, end));
/// ```
pub fn is_date_in_range(date: DateTime<Utc>, start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
    date >= start && date <= end
}

/// Định dạng ngày tháng thành chuỗi thân thiện với hỗ trợ múi giờ
///
/// Chuyển đổi đối tượng DateTime từ UTC sang múi giờ của người dùng (nếu được cung cấp)
/// và trả về chuỗi ngày tháng có định dạng dễ đọc (DD/MM/YYYY).
///
/// # Arguments
/// * `date` - Đối tượng DateTime cần định dạng (UTC)
/// * `timezone` - Múi giờ của người dùng (theo định dạng IANA như "Asia/Ho_Chi_Minh"), mặc định là UTC
///
/// # Returns
/// Chuỗi ngày tháng đã được định dạng theo múi giờ của người dùng
///
/// # Examples
/// ```
/// use wallet::users::subscription::utils::format_date_friendly;
/// use chrono::{TimeZone, Utc};
///
/// let date = Utc.with_ymd_and_hms(2023, 5, 15, 10, 30, 0).unwrap();
/// // Format theo múi giờ UTC (mặc định)
/// assert_eq!(format_date_friendly(date, None), "15/05/2023");
/// // Format theo múi giờ Asia/Ho_Chi_Minh
/// assert_eq!(format_date_friendly(date, Some("Asia/Ho_Chi_Minh")), "15/05/2023");
/// ```
pub fn format_date_friendly(date: DateTime<Utc>, timezone: Option<&str>) -> String {
    match timezone {
        Some(tz_str) => {
            // Cố gắng phân tích múi giờ
            match chrono_tz::Tz::from_str(tz_str) {
                Ok(tz) => {
                    // Chuyển đổi từ UTC sang múi giờ người dùng
                    let local_time = date.with_timezone(&tz);
                    local_time.format("%d/%m/%Y").to_string()
                }
                Err(_) => {
                    // Nếu không phân tích được múi giờ, quay lại UTC
                    warn!("Không thể nhận dạng múi giờ '{}', sử dụng UTC thay thế", tz_str);
                    date.format("%d/%m/%Y").to_string()
                }
            }
        }
        None => {
            // Sử dụng UTC nếu không có múi giờ được chỉ định
            date.format("%d/%m/%Y").to_string()
        }
    }
}

/// Chuyển đổi chuỗi hex thành bytes
///
/// Hàm này chuyển đổi một chuỗi ký tự hex (có thể có tiền tố "0x") 
/// thành mảng bytes. Hàm tự động xử lý tiền tố "0x" nếu có.
///
/// # Arguments
/// * `hex` - Chuỗi hex cần chuyển đổi, có thể có tiền tố "0x"
///
/// # Returns
/// * `Ok(Vec<u8>)` - Mảng bytes nếu chuyển đổi thành công
/// * `Err(hex::FromHexError)` - Lỗi nếu chuỗi hex không hợp lệ
///
/// # Examples
/// ```
/// use wallet::users::subscription::utils::hex_to_bytes;
///
/// // Với tiền tố 0x
/// let bytes = hex_to_bytes("0x0123456789abcdef").unwrap();
/// assert_eq!(bytes, vec![0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]);
///
/// // Không có tiền tố 0x
/// let bytes = hex_to_bytes("0123456789abcdef").unwrap();
/// assert_eq!(bytes, vec![0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]);
/// ```
pub fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, hex::FromHexError> {
    let hex_str = if hex.starts_with("0x") {
        &hex[2..]
    } else {
        hex
    };
    
    hex::decode(hex_str)
}

/// Thời gian giữa các lần kiểm tra subscription (1 ngày)
const SUBSCRIPTION_CHECK_INTERVAL: u64 = 24 * 60 * 60;

/// TaskScheduler quản lý các tác vụ định kỳ liên quan đến subscription
pub struct SubscriptionTaskScheduler {
    /// SubscriptionManager được sử dụng để thực hiện các tác vụ
    subscription_manager: Arc<SubscriptionManager>,
}

impl SubscriptionTaskScheduler {
    /// Tạo mới TaskScheduler
    /// 
    /// # Arguments
    /// - `subscription_manager`: SubscriptionManager sử dụng để thực hiện các tác vụ
    pub fn new(subscription_manager: Arc<SubscriptionManager>) -> Self {
        Self {
            subscription_manager,
        }
    }
    
    /// Bắt đầu các tác vụ định kỳ
    /// 
    /// # Returns
    /// - `tokio::task::JoinHandle`: Handle để kiểm soát task
    pub fn start_periodic_tasks(&self) -> tokio::task::JoinHandle<()> {
        let subscription_manager = self.subscription_manager.clone();
        
        // Spawn một task mới
        tokio::spawn(async move {
            info!("Bắt đầu các tác vụ định kỳ cho subscription");
            
            // Loop vô hạn
            loop {
                // Kiểm tra các subscription hết hạn
                match subscription_manager.check_and_downgrade_expired_subscriptions().await {
                    Ok(downgraded_users) => {
                        if !downgraded_users.is_empty() {
                            info!("Đã hạ cấp {} người dùng do hết hạn subscription", downgraded_users.len());
                        }
                    }
                    Err(e) => {
                        error!("Lỗi khi kiểm tra subscription hết hạn: {}", e);
                    }
                }
                
                // Chờ đến lần kiểm tra tiếp theo
                debug!("Chờ {} giây đến lần kiểm tra subscription tiếp theo", SUBSCRIPTION_CHECK_INTERVAL);
                time::sleep(time::Duration::from_secs(SUBSCRIPTION_CHECK_INTERVAL)).await;
            }
        })
    }
    
    /// Lên lịch kiểm tra ngay lập tức
    /// 
    /// # Returns
    /// - `Result<Vec<String>, WalletError>`: Danh sách các user bị hạ cấp
    pub async fn check_now(&self) -> Result<Vec<String>, WalletError> {
        self.subscription_manager.check_and_downgrade_expired_subscriptions().await
    }
}

/// Khởi tạo scheduler và bắt đầu các tác vụ định kỳ
/// 
/// # Arguments
/// - `subscription_manager`: SubscriptionManager
/// 
/// # Returns
/// - `SubscriptionTaskScheduler`: Task scheduler đã được khởi tạo
pub fn setup_subscription_scheduler(
    subscription_manager: Arc<SubscriptionManager>
) -> SubscriptionTaskScheduler {
    let scheduler = SubscriptionTaskScheduler::new(subscription_manager);
    
    // Bắt đầu các tác vụ định kỳ
    scheduler.start_periodic_tasks();
    
    info!("Đã thiết lập scheduler cho việc kiểm tra subscription định kỳ");
    scheduler
}

/// Module tiện ích cho subscription
#[derive(Debug)]
pub struct SubscriptionUtils;

impl SubscriptionUtils {
    /// Kiểm tra subscription đã hết hạn chưa
    ///
    /// Phương thức này xác định xem một subscription có hết hạn hay không
    /// bằng cách so sánh ngày hết hạn với thời gian hiện tại.
    ///
    /// # Arguments
    /// * `subscription` - Đối tượng UserSubscription cần kiểm tra
    ///
    /// # Returns
    /// * `true` - Nếu subscription đã hết hạn (thời điểm hiện tại > end_date)
    /// * `false` - Nếu subscription còn hiệu lực hoặc không có ngày hết hạn
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::utils::SubscriptionUtils;
    /// use wallet::users::subscription::UserSubscription;
    /// use chrono::{Duration, Utc};
    ///
    /// // Tạo subscription đã hết hạn
    /// let mut expired_sub = UserSubscription::new("user1", /* ... */);
    /// expired_sub.end_date = Some(Utc::now() - Duration::days(1));
    /// assert!(SubscriptionUtils::is_expired(&expired_sub));
    /// ```
    pub fn is_expired(subscription: &UserSubscription) -> bool {
        if let Some(end_date) = subscription.end_date {
            Utc::now() > end_date
        } else {
            // Nếu không có end_date, coi như subscription không hết hạn
            false
        }
    }

    /// Kiểm tra subscription sắp hết hạn (trong vòng days_threshold ngày)
    ///
    /// Phương thức này xác định xem một subscription có sắp hết hạn trong
    /// một khoảng thời gian nhất định hay không. Hữu ích để gửi thông báo
    /// cho người dùng trước khi subscription hết hạn.
    ///
    /// # Arguments
    /// * `subscription` - Đối tượng UserSubscription cần kiểm tra
    /// * `days_threshold` - Số ngày ngưỡng để xác định "sắp hết hạn"
    ///
    /// # Returns
    /// * `true` - Nếu subscription sẽ hết hạn trong `days_threshold` ngày tới
    /// * `false` - Nếu subscription không sắp hết hạn hoặc không có ngày hết hạn
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::utils::SubscriptionUtils;
    /// use wallet::users::subscription::UserSubscription;
    /// use chrono::{Duration, Utc};
    ///
    /// // Tạo subscription sắp hết hạn trong 5 ngày
    /// let mut sub = UserSubscription::new("user1", /* ... */);
    /// sub.end_date = Some(Utc::now() + Duration::days(3));
    /// 
    /// // Kiểm tra với ngưỡng 7 ngày
    /// assert!(SubscriptionUtils::is_expiring_soon(&sub, 7));
    /// 
    /// // Kiểm tra với ngưỡng 2 ngày
    /// assert!(!SubscriptionUtils::is_expiring_soon(&sub, 2));
    /// ```
    pub fn is_expiring_soon(subscription: &UserSubscription, days_threshold: i64) -> bool {
        if let Some(end_date) = subscription.end_date {
            let threshold = Utc::now() + Duration::days(days_threshold);
            Utc::now() < end_date && end_date <= threshold
        } else {
            false
        }
    }

    /// Tính số ngày còn lại của subscription
    ///
    /// Phương thức này tính toán số ngày còn lại cho đến khi subscription hết hạn.
    /// Nếu subscription đã hết hạn, trả về 0. Nếu không có ngày hết hạn, trả về None.
    ///
    /// # Arguments
    /// * `subscription` - Đối tượng UserSubscription cần tính toán
    ///
    /// # Returns
    /// * `Some(i64)` - Số ngày còn lại tính từ hiện tại đến ngày hết hạn
    /// * `None` - Nếu subscription không có ngày hết hạn
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::utils::SubscriptionUtils;
    /// use wallet::users::subscription::UserSubscription;
    /// use chrono::{Duration, Utc};
    ///
    /// // Tạo subscription còn 10 ngày
    /// let mut sub = UserSubscription::new("user1", /* ... */);
    /// sub.end_date = Some(Utc::now() + Duration::days(10));
    /// 
    /// let days = SubscriptionUtils::days_remaining(&sub);
    /// assert!(days.is_some());
    /// assert_eq!(days.unwrap(), 10);
    /// ```
    pub fn days_remaining(subscription: &UserSubscription) -> Option<i64> {
        subscription.end_date.map(|end_date| {
            let now = Utc::now();
            if end_date > now {
                let duration = end_date.signed_duration_since(now);
                duration.num_days()
            } else {
                0
            }
        })
    }

    /// Tạo một sự kiện subscription
    ///
    /// Phương thức này tạo một đối tượng sự kiện subscription mới với
    /// thông tin cơ bản như user ID, loại sự kiện và dữ liệu bổ sung.
    /// Sự kiện được tạo với timestamp là thời điểm hiện tại.
    ///
    /// # Arguments
    /// * `user_id` - ID người dùng liên quan đến sự kiện
    /// * `event_type` - Loại sự kiện (ví dụ: SubscriptionExpired, PaymentSuccess, v.v.)
    /// * `data` - Dữ liệu bổ sung về sự kiện (optional)
    ///
    /// # Returns
    /// Đối tượng SubscriptionEvent đã được khởi tạo
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::utils::SubscriptionUtils;
    /// use wallet::users::subscription::events::EventType;
    ///
    /// let event = SubscriptionUtils::create_event(
    ///     "user123",
    ///     EventType::PaymentSuccess,
    ///     Some(r#"{"tx_hash":"0x1234..."}"#.to_string())
    /// );
    ///
    /// assert_eq!(event.user_id, "user123");
    /// assert_eq!(event.event_type, EventType::PaymentSuccess);
    /// ```
    pub fn create_event(
        user_id: &str,
        event_type: EventType,
        data: Option<String>,
    ) -> SubscriptionEvent {
        SubscriptionEvent {
            user_id: user_id.to_string(),
            event_type,
            timestamp: Utc::now(),
            data,
        }
    }

    /// Tạo sự kiện sắp hết hạn
    pub fn create_expiring_soon_event(subscription: &UserSubscription) -> SubscriptionEvent {
        let data = json!({
            "subscription_type": subscription.subscription_type.to_string(),
            "days_remaining": Self::days_remaining(subscription).unwrap_or(0),
            "end_date": subscription.end_date,
        })
        .to_string();

        Self::create_event(
            &subscription.user_id,
            EventType::SubscriptionExpiringSoon,
            Some(data),
        )
    }

    /// Tạo sự kiện đã hết hạn
    pub fn create_expired_event(subscription: &UserSubscription) -> SubscriptionEvent {
        let data = json!({
            "previous_type": subscription.subscription_type.to_string(),
            "end_date": subscription.end_date,
        })
        .to_string();

        Self::create_event(
            &subscription.user_id,
            EventType::SubscriptionExpired,
            Some(data),
        )
    }

    /// Tính ngày kết thúc dựa trên ngày bắt đầu và số ngày
    pub fn calculate_end_date(start_date: DateTime<Utc>, duration_days: i64) -> DateTime<Utc> {
        start_date + Duration::days(duration_days)
    }

    /// Log sự kiện subscription
    pub fn log_subscription_event(event: &SubscriptionEvent) {
        let event_name = event.event_type.to_string();
        info!(
            "Sự kiện subscription: {} - user_id={} - timestamp={}",
            event_name, event.user_id, event.timestamp
        );
    }

    /// Xử lý subscription hết hạn
    pub fn process_expired_subscription(subscription: &mut UserSubscription) -> Option<SubscriptionEvent> {
        if subscription.status == SubscriptionStatus::Active && Self::is_expired(subscription) {
            // Cập nhật trạng thái thành Expired
            subscription.status = SubscriptionStatus::Expired;
            
            // Nếu không phải Free, hạ cấp về Free
            if subscription.subscription_type != SubscriptionType::Free {
                let previous_type = subscription.subscription_type.clone();
                subscription.subscription_type = SubscriptionType::Free;
                
                warn!(
                    "Subscription hết hạn: user_id={}, type={:?} => Free", 
                    subscription.user_id, previous_type
                );
                
                return Some(Self::create_expired_event(subscription));
            }
        }
        
        None
    }

    /// Tạo sự kiện sắp hết hạn cho subscription
    ///
    /// Phương thức này tạo một sự kiện thông báo rằng subscription sắp hết hạn,
    /// bao gồm thông tin như loại subscription, số ngày còn lại và ngày hết hạn.
    ///
    /// # Arguments
    /// * `subscription` - Đối tượng UserSubscription sắp hết hạn
    ///
    /// # Returns
    /// Đối tượng SubscriptionEvent có loại là SubscriptionExpiringSoon
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::utils::SubscriptionUtils;
    /// use wallet::users::subscription::UserSubscription;
    /// use chrono::{Duration, Utc};
    ///
    /// // Tạo subscription sắp hết hạn
    /// let mut sub = UserSubscription::new("user1", /* ... */);
    /// sub.end_date = Some(Utc::now() + Duration::days(5));
    ///
    /// let event = SubscriptionUtils::create_expiring_soon_event(&sub);
    /// assert_eq!(event.user_id, "user1");
    /// assert_eq!(event.event_type, EventType::SubscriptionExpiringSoon);
    /// ```
    pub fn create_expiring_soon_event(subscription: &UserSubscription) -> SubscriptionEvent {
        let data = json!({
            "subscription_type": subscription.subscription_type.to_string(),
            "days_remaining": Self::days_remaining(subscription).unwrap_or(0),
            "end_date": subscription.end_date,
        })
        .to_string();

        Self::create_event(
            &subscription.user_id,
            EventType::SubscriptionExpiringSoon,
            Some(data),
        )
    }

    /// Tạo sự kiện hết hạn cho subscription
    ///
    /// Phương thức này tạo một sự kiện thông báo rằng subscription đã hết hạn,
    /// bao gồm thông tin như loại subscription trước đây và ngày hết hạn.
    ///
    /// # Arguments
    /// * `subscription` - Đối tượng UserSubscription đã hết hạn
    ///
    /// # Returns
    /// Đối tượng SubscriptionEvent có loại là SubscriptionExpired
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::utils::SubscriptionUtils;
    /// use wallet::users::subscription::UserSubscription;
    /// use chrono::{Duration, Utc};
    ///
    /// // Tạo subscription đã hết hạn
    /// let mut sub = UserSubscription::new("user1", /* ... */);
    /// sub.end_date = Some(Utc::now() - Duration::days(1));
    ///
    /// let event = SubscriptionUtils::create_expired_event(&sub);
    /// assert_eq!(event.user_id, "user1");
    /// assert_eq!(event.event_type, EventType::SubscriptionExpired);
    /// ```
    pub fn create_expired_event(subscription: &UserSubscription) -> SubscriptionEvent {
        let data = json!({
            "previous_type": subscription.subscription_type.to_string(),
            "end_date": subscription.end_date,
        })
        .to_string();

        Self::create_event(
            &subscription.user_id,
            EventType::SubscriptionExpired,
            Some(data),
        )
    }

    /// Tính ngày kết thúc dựa trên ngày bắt đầu và thời hạn
    ///
    /// Phương thức này tính toán ngày kết thúc của subscription dựa trên
    /// ngày bắt đầu và số ngày đăng ký.
    ///
    /// # Arguments
    /// * `start_date` - Ngày bắt đầu subscription
    /// * `duration_days` - Thời hạn subscription tính theo ngày
    ///
    /// # Returns
    /// Ngày kết thúc subscription (DateTime<Utc>)
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::utils::SubscriptionUtils;
    /// use chrono::Utc;
    ///
    /// let start_date = Utc::now();
    /// let end_date = SubscriptionUtils::calculate_end_date(start_date, 30);
    /// assert_eq!((end_date - start_date).num_days(), 30);
    /// ```
    pub fn calculate_end_date(start_date: DateTime<Utc>, duration_days: i64) -> DateTime<Utc> {
        start_date + Duration::days(duration_days)
    }

    /// Ghi log sự kiện liên quan đến subscription
    ///
    /// Phương thức này ghi log thông tin về một sự kiện subscription,
    /// bao gồm loại sự kiện, ID người dùng và thời gian xảy ra.
    ///
    /// # Arguments
    /// * `event` - Đối tượng SubscriptionEvent cần ghi log
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::utils::SubscriptionUtils;
    /// use wallet::users::subscription::events::EventType;
    ///
    /// let event = SubscriptionUtils::create_event("user123", EventType::PaymentSuccess, None);
    /// SubscriptionUtils::log_subscription_event(&event);
    /// // Sẽ ghi log: "Sự kiện subscription: PaymentSuccess - user_id=user123 - timestamp=..."
    /// ```
    pub fn log_subscription_event(event: &SubscriptionEvent) {
        let event_name = event.event_type.to_string();
        info!(
            "Sự kiện subscription: {} - user_id={} - timestamp={}",
            event_name, event.user_id, event.timestamp
        );
    }

    /// Xử lý subscription hết hạn
    ///
    /// Phương thức này thực hiện quy trình xử lý khi một subscription hết hạn:
    /// 1. Đánh dấu trạng thái thành Expired
    /// 2. Hạ cấp từ Premium/VIP xuống Free (nếu không phải Free)
    /// 3. Tạo và trả về sự kiện SubscriptionExpired
    ///
    /// # Arguments
    /// * `subscription` - Đối tượng UserSubscription cần xử lý
    ///
    /// # Returns
    /// * `Some(SubscriptionEvent)` - Sự kiện hết hạn nếu subscription được hạ cấp
    /// * `None` - Nếu subscription không cần xử lý (đã là Free hoặc không hết hạn)
    ///
    /// # Examples
    /// ```
    /// use wallet::users::subscription::utils::SubscriptionUtils;
    /// use wallet::users::subscription::{UserSubscription, SubscriptionType};
    /// use chrono::{Duration, Utc};
    ///
    /// // Tạo subscription Premium đã hết hạn
    /// let mut sub = UserSubscription::new("user1", /* ... */);
    /// sub.end_date = Some(Utc::now() - Duration::days(1));
    /// sub.subscription_type = SubscriptionType::Premium;
    ///
    /// // Xử lý hết hạn
    /// let event = SubscriptionUtils::process_expired_subscription(&mut sub);
    /// assert!(event.is_some());
    /// assert_eq!(sub.subscription_type, SubscriptionType::Free);
    /// assert_eq!(sub.status, SubscriptionStatus::Expired);
    /// ```
    pub fn process_expired_subscription(subscription: &mut UserSubscription) -> Option<SubscriptionEvent> {
        if subscription.status == SubscriptionStatus::Active && Self::is_expired(subscription) {
            // Cập nhật trạng thái thành Expired
            subscription.status = SubscriptionStatus::Expired;
            
            // Nếu không phải Free, hạ cấp về Free
            if subscription.subscription_type != SubscriptionType::Free {
                let previous_type = subscription.subscription_type.clone();
                subscription.subscription_type = SubscriptionType::Free;
                
                warn!(
                    "Subscription hết hạn: user_id={}, type={:?} => Free", 
                    subscription.user_id, previous_type
                );
                
                return Some(Self::create_expired_event(subscription));
            }
        }
        
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::users::subscription::types::{PaymentToken, SubscriptionStatus, SubscriptionType};

    fn create_test_subscription(days_offset: i64) -> UserSubscription {
        let now = Utc::now();
        UserSubscription {
            user_id: "test_user".to_string(),
            subscription_type: SubscriptionType::Premium,
            start_date: now - Duration::days(30),
            end_date: Some(now + Duration::days(days_offset)),
            payment_address: None,
            payment_tx_hash: None,
            payment_token: Some(PaymentToken::USDC),
            payment_amount: Some("10.0".to_string()),
            status: SubscriptionStatus::Active,
            nft_info: None,
            non_nft_status: None,
            created_at: now - Duration::days(30),
            updated_at: now,
        }
    }

    #[test]
    fn test_is_expired() {
        // Subscription hết hạn (hết hạn 5 ngày trước)
        let expired_subscription = create_test_subscription(-5);
        assert!(SubscriptionUtils::is_expired(&expired_subscription));

        // Subscription chưa hết hạn (còn 5 ngày)
        let active_subscription = create_test_subscription(5);
        assert!(!SubscriptionUtils::is_expired(&active_subscription));
    }

    #[test]
    fn test_is_expiring_soon() {
        // Subscription sắp hết hạn trong 5 ngày
        let expiring_subscription = create_test_subscription(5);
        assert!(SubscriptionUtils::is_expiring_soon(&expiring_subscription, 7));
        assert!(!SubscriptionUtils::is_expiring_soon(&expiring_subscription, 3));

        // Subscription còn lâu mới hết hạn (30 ngày)
        let not_expiring_subscription = create_test_subscription(30);
        assert!(!SubscriptionUtils::is_expiring_soon(&not_expiring_subscription, 7));
    }

    #[test]
    fn test_days_remaining() {
        // Subscription còn 5 ngày
        let subscription = create_test_subscription(5);
        assert_eq!(SubscriptionUtils::days_remaining(&subscription), Some(5));

        // Subscription đã hết hạn
        let expired_subscription = create_test_subscription(-5);
        assert_eq!(SubscriptionUtils::days_remaining(&expired_subscription), Some(0));
    }

    #[test]
    fn test_process_expired_subscription() {
        // Tạo subscription đã hết hạn
        let mut expired_subscription = create_test_subscription(-5);
        
        // Xử lý subscription hết hạn
        let event = SubscriptionUtils::process_expired_subscription(&mut expired_subscription);
        
        // Kiểm tra kết quả, sử dụng assert! thay vì panic!
        assert!(event.is_some(), "Phải trả về một sự kiện");
        
        // Sử dụng match để kiểm tra giá trị bên trong Some
        match event {
            Some(event) => assert_eq!(event.event_type, EventType::SubscriptionExpired, "Event type phải là SubscriptionExpired"),
            None => assert!(false, "Không nhận được event như mong đợi")
        }
        
        assert_eq!(expired_subscription.status, SubscriptionStatus::Expired, "Status phải là Expired");
        assert_eq!(expired_subscription.subscription_type, SubscriptionType::Free, "Subscription type phải là Free");
    }
} 