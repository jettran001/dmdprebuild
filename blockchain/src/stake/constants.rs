//! Module chứa các hằng số dùng cho staking

use rust_decimal::Decimal;
use ethers::types::U256;

/// Thời gian lock tối đa (1 năm tính bằng giây)
pub const MAX_LOCK_TIME: u64 = 365 * 86400;

/// Timeout cho các giao dịch blockchain (5 giây)
pub const BLOCKCHAIN_TIMEOUT: u64 = 5000;

/// Số lượng token tối thiểu để stake (100 DMD) - 10^18 wei
pub const MIN_STAKE_AMOUNT: U256 = U256([100_000_000_000_000_000_000u64, 0, 0, 0]);

/// APY cho 7 ngày staking (10%)
pub const SEVEN_DAYS_APY: Decimal = Decimal::from_parts(100, 0, 0, false, 1);

/// APY cho 30 ngày staking (15%)
pub const THIRTY_DAYS_APY: Decimal = Decimal::from_parts(150, 0, 0, false, 1);

/// APY cho 90 ngày staking (20%)
pub const NINETY_DAYS_APY: Decimal = Decimal::from_parts(200, 0, 0, false, 1);

/// APY cho 180 ngày staking (25%)
pub const ONE_EIGHTY_DAYS_APY: Decimal = Decimal::from_parts(250, 0, 0, false, 1);

/// APY cho 365 ngày staking (30%)
pub const THREE_SIXTY_FIVE_DAYS_APY: Decimal = Decimal::from_parts(300, 0, 0, false, 1); 