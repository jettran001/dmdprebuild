// Module này chứa các thành phần chung cho farming và staking
//! Module base cho các chức năng farming
//!
//! Module này cung cấp các chức năng cơ bản dùng chung cho farming và staking:
//! - Định nghĩa cấu trúc dữ liệu chung
//! - Tính toán rewards
//! - Xử lý lỗi chung
//! - Tiện ích và extension traits

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use serde::{Deserialize, Serialize};
use log::{debug, info, warn, error};
use anyhow;

/// Lỗi có thể xảy ra trong quá trình farming
#[derive(Error, Debug)]
pub enum FarmError {
    #[error("Pool không tồn tại: {0}")]
    PoolNotFound(String),
    
    #[error("Người dùng không có khoản đầu tư trong pool: {0}")]
    UserFarmNotFound(String),
    
    #[error("Số lượng tokens không đủ: yêu cầu {required}, hiện có {available}")]
    InsufficientLiquidity {
        required: f64,
        available: f64,
    },
    
    #[error("APR không hợp lệ: {0}")]
    InvalidAPR(f64),
    
    #[error("Thời gian farming không hợp lệ")]
    InvalidFarmingTime,
    
    #[error("Lỗi hệ thống: {0}")]
    SystemError(String),
    
    #[error("Lỗi blockchain: {0}")]
    BlockchainError(String),
    
    #[error("Lỗi khi thực hiện giao dịch: {0}")]
    TransactionError(String),
    
    #[error("Lỗi đồng bộ hóa dữ liệu: {0}")]
    SyncError(String),
    
    #[error("Trạng thái farm không hợp lệ: {current_status}, yêu cầu {required_status}")]
    InvalidFarmStatus {
        current_status: String,
        required_status: String,
    },
    
    #[error("Lỗi IO: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Lỗi serialize/deserialize dữ liệu: {0}")]
    SerializationError(#[from] serde_json::Error),
    
    #[error("Lỗi không xác định: {0}")]
    Unknown(String),
}

impl From<anyhow::Error> for FarmError {
    fn from(err: anyhow::Error) -> Self {
        FarmError::Unknown(format!("{:#}", err))
    }
}

/// Extension cho Result để thêm context
pub trait FarmResultExt<T, E> {
    /// Thêm context cho lỗi
    fn with_farm_context<C, F>(self, context: F) -> Result<T, FarmError>
    where
        F: FnOnce() -> C,
        C: std::fmt::Display;
}

impl<T, E> FarmResultExt<T, E> for Result<T, E>
where
    E: Into<FarmError>,
{
    fn with_farm_context<C, F>(self, context: F) -> Result<T, FarmError>
    where
        F: FnOnce() -> C,
        C: std::fmt::Display,
    {
        self.map_err(|err| {
            let farm_err = err.into();
            match farm_err {
                FarmError::Unknown(msg) => FarmError::Unknown(format!("{}: {}", context(), msg)),
                FarmError::SystemError(msg) => FarmError::SystemError(format!("{}: {}", context(), msg)),
                FarmError::BlockchainError(msg) => FarmError::BlockchainError(format!("{}: {}", context(), msg)),
                FarmError::TransactionError(msg) => FarmError::TransactionError(format!("{}: {}", context(), msg)),
                FarmError::SyncError(msg) => FarmError::SyncError(format!("{}: {}", context(), msg)),
                _ => farm_err,
            }
        })
    }
}

/// Kết quả của các hoạt động farming
pub type FarmResult<T> = Result<T, FarmError>;

/// Trạng thái của farm
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum FarmStatus {
    /// Đang hoạt động, tích lũy rewards
    Active,
    /// Tạm dừng, không tích lũy rewards
    Paused,
    /// Đã đóng, không thể thêm liquidity
    Closed,
    /// Chờ xử lý (ví dụ: đang chờ xác nhận giao dịch)
    Pending,
}

/// Trait cho các thao tác cơ bản farm pool
pub trait BaseFarmPool {
    /// Loại định danh pool
    type PoolId;
    /// Loại dữ liệu cho cấu hình pool 
    type PoolConfig;
    /// Loại định danh người dùng
    type UserId;
    /// Loại dữ liệu cho thông tin người dùng
    type UserInfo;
    /// Loại dữ liệu cho số lượng
    type AmountType;
    
    /// Lấy thông tin pool theo ID
    fn get_pool(&self, pool_id: &Self::PoolId) -> FarmResult<&Self::PoolConfig>;
    
    /// Lấy tất cả các pools
    fn get_all_pools(&self) -> Vec<&Self::PoolConfig>;
    
    /// Lấy thông tin farm của người dùng
    fn get_user_farm(
        &self, 
        user_id: &Self::UserId, 
        pool_id: &Self::PoolId
    ) -> FarmResult<&Self::UserInfo>;
    
    /// Lấy tất cả farm của người dùng
    fn get_user_farms(&self, user_id: &Self::UserId) -> Vec<&Self::UserInfo>;
    
    /// Cập nhật APR của pool
    fn update_apr(
        &mut self, 
        pool_id: &Self::PoolId, 
        new_apr: f64
    ) -> FarmResult<()>;
}

/// Trait cho farming operations
pub trait FarmingOperations: BaseFarmPool {
    /// Thêm liquidity vào pool
    fn add_liquidity(
        &mut self,
        user_id: &Self::UserId,
        pool_id: &Self::PoolId,
        amount: Self::AmountType
    ) -> FarmResult<()>;
    
    /// Rút liquidity từ pool
    fn remove_liquidity(
        &mut self,
        user_id: &Self::UserId,
        pool_id: &Self::PoolId,
        amount: Self::AmountType
    ) -> FarmResult<Self::AmountType>;
    
    /// Thu hoạch rewards
    fn harvest(
        &mut self,
        user_id: &Self::UserId,
        pool_id: &Self::PoolId
    ) -> FarmResult<Self::AmountType>;
}

/// Trait cho các phương thức đồng bộ với blockchain
pub trait BlockchainSyncOperations: BaseFarmPool {
    /// Đồng bộ pools từ blockchain
    async fn sync_pools_from_blockchain(&mut self) -> FarmResult<()>;
    
    /// Đồng bộ thông tin farm của người dùng từ blockchain
    async fn sync_user_farms_from_blockchain(&mut self) -> FarmResult<()>;
}

/// Utility function để lấy timestamp hiện tại
pub fn get_current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}

/// Utility function để tính toán rewards
pub fn calculate_rewards(
    amount: f64,
    apr: f64,
    start_time: u64,
    end_time: u64
) -> FarmResult<f64> {
    if end_time <= start_time {
        return Err(FarmError::InvalidFarmingTime);
    }
    
    let duration_seconds = end_time - start_time;
    let duration_years = duration_seconds as f64 / (365.0 * 24.0 * 60.0 * 60.0);
    
    let rewards = amount * apr / 100.0 * duration_years;
    Ok(rewards)
}

/// Utility function để kiểm tra giá trị APR hợp lệ
pub fn validate_apr(apr: f64) -> FarmResult<()> {
    if apr < 0.0 || apr > 1000.0 {
        return Err(FarmError::InvalidAPR(apr));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_calculate_rewards() {
        // Test với 100 tokens, 10% APR, thời gian 1 năm
        let amount = 100.0;
        let apr = 10.0; // 10%
        
        let start_time = 0;
        let end_time = 365 * 24 * 60 * 60; // 1 năm
        
        let rewards = calculate_rewards(amount, apr, start_time, end_time).unwrap();
        assert_eq!(rewards, 10.0); // 10% của 100 tokens = 10 tokens
        
        // Test với thời gian không hợp lệ
        let invalid_result = calculate_rewards(amount, apr, 100, 50);
        assert!(invalid_result.is_err());
        
        // Test với một nửa năm
        let half_year = (365 / 2) * 24 * 60 * 60;
        let rewards = calculate_rewards(amount, apr, 0, half_year).unwrap();
        assert_eq!(rewards, 5.0); // 5% của 100 tokens = 5 tokens
    }
    
    #[test]
    fn test_validate_apr() {
        assert!(validate_apr(0.0).is_ok());
        assert!(validate_apr(10.0).is_ok());
        assert!(validate_apr(100.0).is_ok());
        assert!(validate_apr(1000.0).is_ok());
        
        assert!(validate_apr(-1.0).is_err());
        assert!(validate_apr(1001.0).is_err());
    }
} 