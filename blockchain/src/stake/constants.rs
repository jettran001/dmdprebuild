//! Các hằng số cho module stake
//!
//! Chứa các giá trị cố định được sử dụng trong toàn bộ module stake

use std::time::Duration;
use ethers::types::U256;

/// Thời gian tối thiểu giữa các lần claim rewards (giây)
pub const MIN_CLAIM_INTERVAL: u64 = 3600; // 1 giờ

/// Thời gian lock tối thiểu cho stake (giây)
pub const MIN_LOCK_TIME: u64 = 86400; // 24 giờ

/// Thời gian lock tối đa cho stake (giây)
pub const MAX_LOCK_TIME: u64 = 31536000; // 1 năm

/// Số validator tối thiểu cho một pool
pub const MIN_VALIDATORS: u32 = 3;

/// Số validator tối đa cho một pool
pub const MAX_VALIDATORS: u32 = 100;

/// Phần trăm phí tối thiểu cho unstake sớm
pub const EARLY_UNSTAKE_FEE_PERCENT: u64 = 10; // 10%

/// Số block tối thiểu giữa các lần cập nhật APY
pub const MIN_BLOCKS_BETWEEN_APY_UPDATE: u64 = 100;

/// Số block tối thiểu giữa các lần phân phối rewards
pub const MIN_BLOCKS_BETWEEN_REWARD_DISTRIBUTION: u64 = 1000;

/// Thời gian timeout cho các giao dịch (ms)
pub const TRANSACTION_TIMEOUT: Duration = Duration::from_secs(30);

/// Số lần retry tối đa cho các giao dịch
pub const MAX_TRANSACTION_RETRIES: u32 = 3;

/// Thời gian chờ giữa các lần retry (ms)
pub const RETRY_DELAY: Duration = Duration::from_secs(1);

/// Số lượng token tối thiểu để stake
pub const MIN_STAKE_AMOUNT: U256 = U256([1000000000000000000, 0, 0, 0]); // 1 token

/// Số lượng token tối đa để stake
pub const MAX_STAKE_AMOUNT: U256 = U256([1000000000000000000000000, 0, 0, 0]); // 1,000,000 tokens

/// Phần trăm rewards tối thiểu cho validator
pub const MIN_VALIDATOR_REWARD_PERCENT: u64 = 5; // 5%

/// Phần trăm rewards tối đa cho validator
pub const MAX_VALIDATOR_REWARD_PERCENT: u64 = 20; // 20%

/// Thời gian cache cho pool info (giây)
pub const POOL_INFO_CACHE_TIME: u64 = 300; // 5 phút

/// Thời gian cache cho user stake info (giây)
pub const USER_STAKE_CACHE_TIME: u64 = 60; // 1 phút

/// Thời gian cache cho rewards (giây)
pub const REWARDS_CACHE_TIME: u64 = 300; // 5 phút

/// Số lượng decimals mặc định cho token
pub const DEFAULT_TOKEN_DECIMALS: u8 = 18;

/// Địa chỉ contract staking mặc định
pub const DEFAULT_STAKING_CONTRACT: &str = "0x0000000000000000000000000000000000000000";

/// Địa chỉ token mặc định
pub const DEFAULT_TOKEN_ADDRESS: &str = "0x0000000000000000000000000000000000000000";

/// APY mặc định cho pool mới
pub const DEFAULT_APY: f64 = 10.0; // 10%

/// Phần trăm phí tối thiểu cho unstake
pub const MIN_UNSTAKE_FEE_PERCENT: u64 = 1; // 1%

/// Phần trăm phí tối đa cho unstake
pub const MAX_UNSTAKE_FEE_PERCENT: u64 = 50; // 50% 