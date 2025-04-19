//! Các hằng số cho module DeFi
//! 
//! Bao gồm các hằng số cho:
//! - APY cho các gói staking
//! - Timeout cho các cuộc gọi blockchain
//! - Địa chỉ contract
//! - Số lượng tối thiểu
//! - Slippage tolerance

use rust_decimal::Decimal;
use ethers::types::U256;

/// APY cho gói staking 7 ngày
pub const SEVEN_DAYS_APY: Decimal = Decimal::from_str("0.1").unwrap(); // 10%

/// APY cho gói staking 30 ngày
pub const THIRTY_DAYS_APY: Decimal = Decimal::from_str("0.15").unwrap(); // 15%

/// APY cho gói staking 90 ngày
pub const NINETY_DAYS_APY: Decimal = Decimal::from_str("0.2").unwrap(); // 20%

/// APY cho gói staking 180 ngày
pub const ONE_HUNDRED_EIGHTY_DAYS_APY: Decimal = Decimal::from_str("0.25").unwrap(); // 25%

/// APY cho gói staking 365 ngày
pub const THREE_HUNDRED_SIXTY_FIVE_DAYS_APY: Decimal = Decimal::from_str("0.3").unwrap(); // 30%

/// Timeout cho các cuộc gọi blockchain (ms)
pub const BLOCKCHAIN_TIMEOUT: u64 = 30000;

/// Số lần thử lại tối đa cho các cuộc gọi blockchain
pub const MAX_RETRIES: u32 = 3;

/// Slippage tolerance cho các giao dịch (0.5%)
pub const SLIPPAGE_TOLERANCE: Decimal = Decimal::from_str("0.005").unwrap();

/// Số lượng token tối thiểu để stake
pub const MIN_STAKE_AMOUNT: U256 = U256::from(1000);

/// Số lượng token tối thiểu để add liquidity
pub const MIN_LIQUIDITY_AMOUNT: U256 = U256::from(1000);

/// Thời gian lock tối thiểu cho staking (giây)
pub const MIN_LOCK_TIME: u64 = 86400; // 1 ngày

/// Thời gian lock tối đa cho staking (giây)
pub const MAX_LOCK_TIME: u64 = 31536000; // 1 năm

/// Địa chỉ contract DMD token
pub const DMD_TOKEN_ADDRESS: &str = "0x0000000000000000000000000000000000000001";

/// Địa chỉ contract router
pub const ROUTER_ADDRESS: &str = "0x0000000000000000000000000000000000000002";

/// Địa chỉ contract factory
pub const FACTORY_ADDRESS: &str = "0x0000000000000000000000000000000000000003";

/// Địa chỉ contract staking
pub const STAKING_ADDRESS: &str = "0x0000000000000000000000000000000000000004";

/// Địa chỉ contract farming
pub const FARMING_ADDRESS: &str = "0x0000000000000000000000000000000000000005"; 