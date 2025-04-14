//! Module định nghĩa các hằng số dùng trong quản lý đăng ký và gói dịch vụ.

use rust_decimal::Decimal;
use std::time::Duration as StdDuration;

/// Hằng số cho số lần thử lại giao dịch blockchain
pub const BLOCKCHAIN_TX_RETRY_ATTEMPTS: u8 = 3;
/// Hằng số cho thời gian đợi giữa các lần thử (ms)
pub const BLOCKCHAIN_TX_RETRY_DELAY_MS: u64 = 1000;
/// Hằng số cho số block cần đợi để xác nhận giao dịch
pub const BLOCKCHAIN_TX_CONFIRMATION_BLOCKS: u64 = 3;

/// Thời gian kiểm tra trạng thái đăng ký (giây)
pub const SUBSCRIPTION_CHECK_INTERVAL_SECONDS: u64 = 600; // 10 phút

/// Thời gian tối đa chờ xử lý thanh toán (phút)
pub const PAYMENT_PROCESSING_TIMEOUT_MINUTES: i64 = 30;

/// Thời gian mặc định cho gói dịch vụ (ngày)
pub const DEFAULT_FREE_SUBSCRIPTION_DAYS: i64 = 30;
pub const DEFAULT_PREMIUM_SUBSCRIPTION_DAYS: i64 = 30;
pub const DEFAULT_VIP_SUBSCRIPTION_DAYS: i64 = 90;

/// Giá dịch vụ (USDT)
pub const FREE_SUBSCRIPTION_PRICE: f64 = 0.0;
pub const PREMIUM_SUBSCRIPTION_PRICE: f64 = 9.99;
pub const VIP_SUBSCRIPTION_PRICE: f64 = 99.99;
pub const LIFETIME_SUBSCRIPTION_PRICE: f64 = 299.99;

/// Số lần kiểm tra lại thông tin thanh toán
pub const MAX_PAYMENT_CHECK_RETRIES: usize = 5;

/// Thời gian chờ giữa các lần kiểm tra thanh toán (giây)
pub const PAYMENT_CHECK_RETRY_INTERVAL_SECONDS: u64 = 60;

/// Thời gian cache cho dữ liệu người dùng (giây)
pub const USER_DATA_CACHE_SECONDS: u64 = 300; // 5 phút

/// Thời gian cảnh báo trước khi hết hạn (ngày)
pub const SUBSCRIPTION_EXPIRY_WARNING_DAYS: i64 = 7;

/// Định nghĩa thời gian auto trade
pub const FREE_USER_AUTO_TRADE_MINUTES: i64 = 30;
pub const PREMIUM_USER_AUTO_TRADE_HOURS: i64 = 6;
pub const VIP_USER_AUTO_TRADE_HOURS: i64 = 12;
/// Thời gian auto trade bổ sung cho người dùng VIP gói 12 tháng (giờ)
pub const VIP_STAKE_BONUS_AUTO_TRADE_HOURS: i64 = 3;
/// Tổng thời gian auto trade cho người dùng VIP gói 12 tháng (giờ)
pub const VIP_STAKE_TOTAL_AUTO_TRADE_HOURS: i64 = VIP_USER_AUTO_TRADE_HOURS + VIP_STAKE_BONUS_AUTO_TRADE_HOURS;

/// Số ngày reset cho người dùng miễn phí
pub const FREE_USER_RESET_DAYS: i64 = 7;

/// Thời gian kiểm tra NFT (giây)
pub const NFT_CHECK_INTERVAL_SECONDS: u64 = 3600; // 1 giờ

/// Giới hạn tính năng theo loại đăng ký
pub const FREE_TIER_BOT_LIMIT: usize = 1;
pub const PREMIUM_TIER_BOT_LIMIT: usize = 5;
pub const VIP_TIER_BOT_LIMIT: usize = 15;

/// Thời gian tối đa giữa các giao dịch (auto trade)
pub const FREE_TIER_TRADE_INTERVAL_SECONDS: u64 = 60;
pub const PREMIUM_TIER_TRADE_INTERVAL_SECONDS: u64 = 30;
pub const VIP_TIER_TRADE_INTERVAL_SECONDS: u64 = 10;

/// Thời gian sử dụng tính năng tối đa hàng ngày
pub const FREE_TIER_DAILY_USAGE_MINUTES: i64 = 60;
pub const PREMIUM_TIER_DAILY_USAGE_MINUTES: i64 = 360;
pub const VIP_TIER_DAILY_USAGE_MINUTES: i64 = 1440; // 24 giờ

/// Mã giảm giá mặc định
pub const DEFAULT_DISCOUNT_PERCENTAGE: u8 = 10;
pub const MAX_DISCOUNT_PERCENTAGE: u8 = 50;

/// Thời hạn của mã giảm giá (ngày)
pub const DEFAULT_DISCOUNT_CODE_VALIDITY_DAYS: i64 = 30;

/// Số lần thử lại tối đa khi gặp lỗi kết nối
pub const MAX_CONNECTION_RETRIES: usize = 3;

/// Thời gian chờ giữa các lần thử lại (ms)
pub const CONNECTION_RETRY_DELAY_MS: u64 = 1000;

/// Khoảng thời gian tối đa giữa các giao dịch (giây)
pub const MAX_TRANSACTION_INTERVAL_SECONDS: u64 = 300; // 5 phút

/// Phí giao dịch mặc định
pub const DEFAULT_TRANSACTION_FEE_PERCENTAGE: f64 = 0.1; // 0.1% 

// Các địa chỉ hợp đồng cho các token thanh toán
pub const USDT_CONTRACT_ADDRESS: &str = "0x55d398326f99059fF775485246999027B3197955"; // BSC USDT
pub const BNB_CONTRACT_ADDRESS: &str = "0xB8c77482e45F1F44dE1745F52C74426C631bDD52"; // BNB
pub const BSC_USDC_CONTRACT_ADDRESS: &str = "0x8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d"; // BSC USDC
pub const ETH_CONTRACT_ADDRESS: &str = "0x2170Ed0880ac9A755fd29B2688956BD959F933F8"; // BSC ETH
pub const BTC_CONTRACT_ADDRESS: &str = "0x7130d2A12B9BCbFAe4f2634d864A1Ee1Ce3Ead9c"; // BSC BTC
pub const DCT_CONTRACT_ADDRESS: &str = "0xDC0Cc71957786E2e7b4338F2C0F4c033Cb387E36"; // DiamondChain Token

// Thông tin API của Blockchain Explorer
pub const BSC_API_KEY: &str = "ZWY4RJR64YCT32RMG3U3YJ7SQRZHQJ14XD";
pub const BSC_API_ENDPOINT: &str = "https://api.bscscan.com/api";

// Địa chỉ ví nhận thanh toán của hệ thống
pub const PAYMENT_RECEIVING_ADDRESS: &str = "0x123456789abcdef123456789abcdef123456789a";

// Thời gian chờ tối đa cho giao dịch (theo giây)
pub const MAX_TRANSACTION_WAIT_TIME: i64 = 3600; // 1 giờ

// Giá mặc định cho các gói đăng ký (theo USD)
pub const DEFAULT_PREMIUM_PRICE_USD: f64 = 49.99;
pub const DEFAULT_VIP_PRICE_USD: f64 = 199.99;
pub const DEFAULT_LIFETIME_PRICE_USD: f64 = 999.99;

// Thông tin khuyến mãi
pub const PROMOTION_ACTIVE: bool = true;
pub const PROMOTION_DISCOUNT_PERCENT: f64 = 20.0; // Giảm 20%
pub const PROMOTION_END_DATE: &str = "2024-12-31T23:59:59Z";

// Giới hạn số lượng bot giao dịch cho mỗi gói
pub const FREE_BOT_LIMIT: usize = 1;
pub const PREMIUM_BOT_LIMIT: usize = 5;
pub const VIP_BOT_LIMIT: usize = 15;
pub const LIFETIME_BOT_LIMIT: usize = 15;

// Giới hạn số lượng giao dịch tự động
pub const FREE_AUTO_TRADE_LIMIT: usize = 10;
pub const PREMIUM_AUTO_TRADE_LIMIT: usize = 100;
pub const VIP_AUTO_TRADE_LIMIT: usize = 1000;
pub const LIFETIME_AUTO_TRADE_LIMIT: usize = 1000;

// Thông tin về NFT VIP
pub const VIP_NFT_CONTRACT_ADDRESS: &str = "0x9876543210abcdef9876543210abcdef98765432";
pub const VIP_NFT_CHAIN_ID: u64 = 56; // BSC

// Cài đặt khác
pub const MAX_TRIAL_EXTENSIONS: usize = 1; // Số lần gia hạn dùng thử tối đa 

/// Số ngày mặc định cho gói Free
pub const FREE_SUBSCRIPTION_DAYS: i64 = 30;

/// Số ngày mặc định cho gói Premium
pub const PREMIUM_SUBSCRIPTION_DAYS: i64 = 30;

/// Số ngày cho gói Lifetime (9999 năm)
pub const LIFETIME_SUBSCRIPTION_DAYS: i64 = 9999 * 365;

/// Số lượng bot tối đa cho gói Free
pub const FREE_MAX_BOTS: usize = 1;

/// Số lượng bot tối đa cho gói Premium
pub const PREMIUM_MAX_BOTS: usize = 5;

/// Số lượng bot tối đa cho gói Lifetime
pub const LIFETIME_MAX_BOTS: usize = 10;

/// Số lượng auto-trade tối đa cho gói Free
pub const FREE_MAX_AUTO_TRADES: usize = 0;

/// Số lượng auto-trade tối đa cho gói Premium
pub const PREMIUM_MAX_AUTO_TRADES: usize = 3;

/// Số lượng auto-trade tối đa cho gói Lifetime
pub const LIFETIME_MAX_AUTO_TRADES: usize = 5;

/// Thời gian chờ mặc định cho việc kiểm tra giao dịch
pub const DEFAULT_TRANSACTION_CHECK_TIMEOUT: StdDuration = StdDuration::from_secs(5 * 60);

/// Số lần thử lại tối đa khi kiểm tra giao dịch
pub const MAX_TRANSACTION_CHECK_RETRIES: usize = 10;

/// Thời gian chờ giữa các lần kiểm tra giao dịch
pub const TRANSACTION_CHECK_INTERVAL: StdDuration = StdDuration::from_secs(30);

/// Giá tiền mặc định (USDC)
/// Giá subscription Premium mặc định (USDC)
pub const DEFAULT_PREMIUM_PRICE_USDC: Decimal = Decimal::new(2999, 2); // 29.99

/// Giá subscription Lifetime mặc định (USDC)
pub const DEFAULT_LIFETIME_PRICE_USDC: Decimal = Decimal::new(29900, 2); // 299.00

/// Giá subscription Premium mặc định (DMD)
pub const DEFAULT_PREMIUM_PRICE_DMD: Decimal = Decimal::new(50000, 2); // 500.00

/// Giá subscription Lifetime mặc định (DMD)
pub const DEFAULT_LIFETIME_PRICE_DMD: Decimal = Decimal::new(500000, 2); // 5000.00

/// Số lượng NFT cần để được xem là VIP
pub const VIP_NFT_REQUIREMENT: usize = 1;

/// Thời gian gia hạn trước khi gói đăng ký hết hạn (ngày)
pub const RENEWAL_REMINDER_DAYS: i64 = 7;

/// Phần trăm giảm giá cho gói dịch vụ DMD (0.1 = 10%)
pub const DMD_PAYMENT_DISCOUNT: f64 = 0.1;

/// Khoảng thời gian tối thiểu giữa các lần kiểm tra thanh toán (4 giờ)
pub const MIN_PAYMENT_CHECK_INTERVAL: StdDuration = StdDuration::from_secs(4 * 60 * 60);

/// Số ngày đăng ký từ NFT VIP
pub const NFT_SUBSCRIPTION_DAYS: i64 = 30;

/// Số lần kiểm tra lại giao dịch thanh toán
pub const MAX_PAYMENT_RETRY: usize = 3;

/// Thời gian chờ giữa các lần kiểm tra lại thanh toán (15 phút)
pub const PAYMENT_RETRY_INTERVAL: StdDuration = StdDuration::from_secs(15 * 60);

/// Địa chỉ bộ sưu tập NFT VIP
pub const VIP_NFT_COLLECTION_ADDRESS: &str = "0x1234567890abcdef1234567890abcdef12345678";

/// Giới hạn truy vấn API cho gói Free (5 mỗi ngày)
pub const FREE_API_QUERY_DAILY_LIMIT: u32 = 5;

/// Giới hạn Auto Trade cho gói Premium (1 bot)
pub const PREMIUM_AUTO_TRADE_LIMIT: u32 = 1;

/// Giới hạn Auto Trade cho gói VIP (3 bot)
pub const VIP_AUTO_TRADE_LIMIT: u32 = 3;

/// Giới hạn Auto Trade cho gói Lifetime (5 bot)
pub const LIFETIME_AUTO_TRADE_LIMIT: u32 = 5;

/// Giá đăng ký gói Premium hàng tháng
pub const PREMIUM_MONTHLY_PRICE: Decimal = Decimal::new(1999, 2); // $19.99

/// Giá đăng ký gói Premium hàng năm
pub const PREMIUM_YEARLY_PRICE: Decimal = Decimal::new(19999, 2); // $199.99

/// Giá đăng ký gói VIP hàng tháng
pub const VIP_MONTHLY_PRICE: Decimal = Decimal::new(4999, 2); // $49.99

/// Giá đăng ký gói VIP hàng năm
pub const VIP_YEARLY_PRICE: Decimal = Decimal::new(49999, 2); // $499.99

/// Giá đăng ký gói Lifetime
pub const LIFETIME_PRICE: Decimal = Decimal::new(199999, 2); // $1999.99

/// Mã giảm giá mặc định cho đăng ký hàng năm (20%)
pub const YEARLY_DISCOUNT_PERCENTAGE: u8 = 20;

/// Số ngày gói Free sau khi đăng ký
pub const FREE_TRIAL_DAYS: i64 = 7;

/// Số ngày cảnh báo trước khi hết hạn đăng ký
pub const EXPIRY_WARNING_DAYS: i64 = 3;

/// Thời gian giữa các lần check tự động đăng ký (1 giờ)
pub const SUBSCRIPTION_CHECK_INTERVAL: StdDuration = StdDuration::from_secs(60 * 60);

/// Địa chỉ hợp đồng token USDC mạng Ethereum
pub const ETH_USDC_CONTRACT_ADDRESS: &str = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";

/// Địa chỉ hợp đồng token DMD mạng Ethereum
pub const DMD_CONTRACT_ADDRESS: &str = "0x90f3edc7d5298918f7bb51694134b07356f7d0c7";

// Giới hạn Auto Trade cho mỗi loại gói
pub const FREE_AUTO_TRADE_COUNT: usize = 10;
pub const PREMIUM_AUTO_TRADE_COUNT: usize = 100;
pub const VIP_AUTO_TRADE_COUNT: usize = 1000;
pub const LIFETIME_AUTO_TRADE_COUNT: usize = 1000;

// Giới hạn Auto Trade cho mỗi loại gói (số lượng bot)
pub const FREE_AUTO_TRADE_BOT_LIMIT: u32 = 0;
pub const PREMIUM_AUTO_TRADE_BOT_LIMIT: u32 = 1;
pub const VIP_AUTO_TRADE_BOT_LIMIT: u32 = 3;
pub const LIFETIME_AUTO_TRADE_BOT_LIMIT: u32 = 5;

/// Phần trăm giảm giá cho gói đăng ký 12 tháng 
pub const TWELVE_MONTH_DISCOUNT_PERCENTAGE: u8 = 20;

/// Thời gian khóa token DMD trong gói 12 tháng (ngày)
pub const TWELVE_MONTH_STAKE_DAYS: i64 = 365;

/// Phần trăm lợi nhuận cho token DMD được stake (hàng năm)
pub const STAKED_DMD_APY_PERCENTAGE: Decimal = Decimal::new(3000, 2); // 30%

/// Số lượng tối thiểu DMD cần khóa trong pool stake
pub const MIN_DMD_STAKE_AMOUNT: Decimal = Decimal::new(100000, 2); // 1000 DMD

/// Địa chỉ hợp đồng staking pool DMD
pub const DMD_STAKING_POOL_ADDRESS: &str = "0x89a456789bcdef123456789abcdef123456789ab";

/// Thời gian giữa hai lần tính lợi nhuận staking (ngày)
pub const STAKING_REWARD_INTERVAL_DAYS: i64 = 30;

/// Giá cho việc nâng cấp từ Free lên Premium (USDC)
pub const FREE_TO_PREMIUM_UPGRADE_PRICE_USDC: Decimal = Decimal::new(10000, 2); // $100.00

/// Giá cho việc nâng cấp từ Premium lên VIP (USDC)
pub const PREMIUM_TO_VIP_UPGRADE_PRICE_USDC: Decimal = Decimal::new(30000, 2); // $300.00

/// Giá cho việc nâng cấp từ Free lên VIP trực tiếp (USDC)
pub const FREE_TO_VIP_UPGRADE_PRICE_USDC: Decimal = Decimal::new(40000, 2); // $400.00

/// Giá cho việc nâng cấp từ Free lên VIP trực tiếp (DMD)
pub const FREE_TO_VIP_UPGRADE_PRICE_DMD: Decimal = Decimal::new(100000, 2); // 1000 DMD

/// Giá gốc gói VIP 12 tháng trước khi giảm giá (USDC)
pub const VIP_TWELVE_MONTH_BASE_PRICE_USDC: Decimal = Decimal::new(480000, 2); // $4800.00

/// Giá cho gói đăng ký VIP 12 tháng (USDC)
pub const VIP_TWELVE_MONTH_PRICE_USDC: Decimal = Decimal::new(384000, 2); // $3840.00 (giảm 20% từ $4800)

/// Giá cho gói đăng ký VIP 12 tháng (DMD)
pub const VIP_TWELVE_MONTH_PRICE_DMD: Decimal = Decimal::new(800000, 2); // 8000 DMD (giảm 20%)

/// Loại token ERC-1155 dùng cho staking
pub const DMD_STAKING_TOKEN_ID: u64 = 1155001;

// Auto-trade constants
/// Số giờ mỗi ngày người dùng mặc định có thể sử dụng auto-trade
pub const BASE_USER_AUTO_TRADE_HOURS: u64 = 1;
/// Số giờ mỗi ngày người dùng Premium có thể sử dụng auto-trade
pub const PREMIUM_USER_AUTO_TRADE_HOURS: u64 = 6;
/// Số giờ mỗi ngày người dùng VIP có thể sử dụng auto-trade
pub const VIP_USER_AUTO_TRADE_HOURS: u64 = 12;
/// Số giờ mỗi ngày người dùng VIP 12 tháng được thêm để sử dụng auto-trade
pub const VIP_STAKE_BONUS_AUTO_TRADE_HOURS: u64 = 3;
/// Tổng số giờ mỗi ngày người dùng VIP 12 tháng có thể sử dụng auto-trade
pub const VIP_STAKE_TOTAL_AUTO_TRADE_HOURS: u64 = VIP_USER_AUTO_TRADE_HOURS + VIP_STAKE_BONUS_AUTO_TRADE_HOURS;

/// Ngưỡng phút còn lại để gửi cảnh báo sắp hết thời gian auto-trade
pub const AUTO_TRADE_WARNING_MINUTES: i64 = 60; // Cảnh báo khi còn 60 phút
/// Ngưỡng phần trăm thời gian còn lại để hiển thị cảnh báo (10%)
pub const AUTO_TRADE_TIME_WARNING_THRESHOLD: i64 = 60; // Phút
/// Chu kỳ kiểm tra định kỳ auto-trade (1 giờ)
pub const AUTO_TRADE_PERIODIC_CHECK_INTERVAL: StdDuration = StdDuration::from_secs(3600); 