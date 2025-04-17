//! Các hằng số cho module stake
//!
//! Chứa các giá trị cố định được sử dụng trong toàn bộ module stake
//! Các giá trị có thể được ghi đè từ biến môi trường

use std::time::Duration;
use std::env;
use ethers::types::U256;
use lazy_static::lazy_static;
use tracing::{info, warn};

/// Helper để lấy giá trị từ biến môi trường hoặc dùng giá trị mặc định
fn get_env_or_default<T: std::str::FromStr>(key: &str, default: T) -> T {
    match env::var(key) {
        Ok(val) => match val.parse::<T>() {
            Ok(parsed) => {
                info!("Loaded config from environment: {} = {}", key, val);
                parsed
            },
            Err(_) => {
                warn!("Could not parse environment variable: {}, using default value", key);
                default
            }
        },
        Err(_) => default,
    }
}

/// Helper để lấy giá trị U256 từ biến môi trường
fn get_u256_from_env(key: &str, default: U256) -> U256 {
    match env::var(key) {
        Ok(val) => match val.parse::<u128>() {
            Ok(parsed) => {
                info!("Loaded U256 config from environment: {} = {}", key, val);
                U256::from(parsed)
            },
            Err(_) => {
                warn!("Could not parse U256 environment variable: {}, using default value", key);
                default
            }
        },
        Err(_) => default,
    }
}

lazy_static! {
    /// Thời gian tối thiểu giữa các lần claim rewards (giây)
    pub static ref MIN_CLAIM_INTERVAL: u64 = get_env_or_default("STAKE_MIN_CLAIM_INTERVAL", 3600); // 1 giờ
    
    /// Thời gian lock tối thiểu cho stake (giây)
    pub static ref MIN_LOCK_TIME: u64 = get_env_or_default("STAKE_MIN_LOCK_TIME", 86400); // 24 giờ
    
    /// Thời gian lock tối đa cho stake (giây)
    pub static ref MAX_LOCK_TIME: u64 = get_env_or_default("STAKE_MAX_LOCK_TIME", 31536000); // 1 năm
    
    /// Số validator tối thiểu cho một pool
    pub static ref MIN_VALIDATORS: u32 = get_env_or_default("STAKE_MIN_VALIDATORS", 3);
    
    /// Số validator tối đa cho một pool
    pub static ref MAX_VALIDATORS: u32 = get_env_or_default("STAKE_MAX_VALIDATORS", 100);
    
    /// Phần trăm phí tối thiểu cho unstake sớm
    pub static ref EARLY_UNSTAKE_FEE_PERCENT: u64 = get_env_or_default("STAKE_EARLY_UNSTAKE_FEE_PERCENT", 10); // 10%
    
    /// Số block tối thiểu giữa các lần cập nhật APY
    pub static ref MIN_BLOCKS_BETWEEN_APY_UPDATE: u64 = get_env_or_default("STAKE_MIN_BLOCKS_BETWEEN_APY_UPDATE", 100);
    
    /// Số block tối thiểu giữa các lần phân phối rewards
    pub static ref MIN_BLOCKS_BETWEEN_REWARD_DISTRIBUTION: u64 = get_env_or_default("STAKE_MIN_BLOCKS_BETWEEN_REWARD_DISTRIBUTION", 1000);
    
    /// Số lần retry tối đa cho các giao dịch
    pub static ref MAX_TRANSACTION_RETRIES: u32 = get_env_or_default("STAKE_MAX_TRANSACTION_RETRIES", 3);
    
    /// Số lượng token tối thiểu để stake
    pub static ref MIN_STAKE_AMOUNT: U256 = get_u256_from_env("STAKE_MIN_AMOUNT", U256::from(1000000000000000000u64)); // 1 token
    
    /// Số lượng token tối đa để stake
    pub static ref MAX_STAKE_AMOUNT: U256 = get_u256_from_env("STAKE_MAX_AMOUNT", U256::from_dec_str("1000000000000000000000000").unwrap()); // 1,000,000 tokens
    
    /// Phần trăm rewards tối thiểu cho validator
    pub static ref MIN_VALIDATOR_REWARD_PERCENT: u64 = get_env_or_default("STAKE_MIN_VALIDATOR_REWARD_PERCENT", 5); // 5%
    
    /// Phần trăm rewards tối đa cho validator
    pub static ref MAX_VALIDATOR_REWARD_PERCENT: u64 = get_env_or_default("STAKE_MAX_VALIDATOR_REWARD_PERCENT", 20); // 20%
    
    /// Thời gian cache cho pool info (giây)
    pub static ref POOL_INFO_CACHE_TIME: u64 = get_env_or_default("STAKE_POOL_INFO_CACHE_TIME", 300); // 5 phút
    
    /// Thời gian cache cho user stake info (giây)
    pub static ref USER_STAKE_CACHE_TIME: u64 = get_env_or_default("STAKE_USER_STAKE_CACHE_TIME", 60); // 1 phút
    
    /// Thời gian cache cho rewards (giây)
    pub static ref REWARDS_CACHE_TIME: u64 = get_env_or_default("STAKE_REWARDS_CACHE_TIME", 300); // 5 phút
    
    /// Số lượng decimals mặc định cho token
    pub static ref DEFAULT_TOKEN_DECIMALS: u8 = get_env_or_default("STAKE_DEFAULT_TOKEN_DECIMALS", 18);
    
    /// APY mặc định cho pool mới
    pub static ref DEFAULT_APY: f64 = get_env_or_default("STAKE_DEFAULT_APY", 10.0); // 10%
    
    /// Phần trăm phí tối thiểu cho unstake
    pub static ref MIN_UNSTAKE_FEE_PERCENT: u64 = get_env_or_default("STAKE_MIN_UNSTAKE_FEE_PERCENT", 1); // 1%
    
    /// Phần trăm phí tối đa cho unstake
    pub static ref MAX_UNSTAKE_FEE_PERCENT: u64 = get_env_or_default("STAKE_MAX_UNSTAKE_FEE_PERCENT", 50); // 50%
    
    /// Số penalties tối đa cho phép để trở thành validator
    pub static ref MAX_ALLOWED_PENALTIES: u32 = get_env_or_default("STAKE_MAX_ALLOWED_PENALTIES", 3);
    
    /// Số lượng token tối đa trong một pool
    pub static ref MAX_TOKENS_PER_POOL: u32 = get_env_or_default("STAKE_MAX_TOKENS_PER_POOL", 10);
}

// Các hằng số không thể được định nghĩa dưới dạng lazy_static
/// Thời gian timeout cho các giao dịch (ms)
pub fn transaction_timeout() -> Duration {
    Duration::from_secs(get_env_or_default("STAKE_TRANSACTION_TIMEOUT_SECS", 30))
}

/// Thời gian chờ giữa các lần retry (ms)
pub fn retry_delay() -> Duration {
    Duration::from_secs(get_env_or_default("STAKE_RETRY_DELAY_SECS", 1))
}

/// Địa chỉ contract staking mặc định
pub fn default_staking_contract() -> &'static str {
    Box::leak(env::var("STAKE_DEFAULT_CONTRACT")
        .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string())
        .into_boxed_str())
}

/// Địa chỉ token mặc định
pub fn default_token_address() -> &'static str {
    Box::leak(env::var("STAKE_DEFAULT_TOKEN")
        .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string())
        .into_boxed_str())
} 