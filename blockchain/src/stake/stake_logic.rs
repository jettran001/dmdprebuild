//! Module quản lý staking trong blockchain
//!
//! Module này cung cấp các chức năng quản lý staking pools, APY rates, và tương tác
//! của người dùng với hệ thống staking.
//!
//! # Ví dụ:
//! ```
//! use blockchain::stake::StakeManager;
//!
//! async fn example() {
//!     // Tạo stake manager mới
//!     let mut stake_manager = StakeManager::new();
//!
//!     // Thêm pool
//!     let pool_id = stake_manager.add_pool("DMD", 15.0).unwrap();
//!
//!     // Stake tokens
//!     let user_id = "user123";
//!     stake_manager.stake(user_id, pool_id, 100.0).unwrap();
//!
//!     // Claim rewards
//!     let rewards = stake_manager.claim_rewards(user_id, pool_id).unwrap();
//!     println!("Claimed rewards: {}", rewards);
//!
//!     // Unstake tokens
//!     stake_manager.unstake(user_id, pool_id, 50.0).unwrap();
//! }
//! ```

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use serde::{Deserialize, Serialize};
use log::{debug, info, warn, error};

/// Lỗi có thể xảy ra trong quá trình staking
#[derive(Error, Debug)]
pub enum StakeError {
    #[error("Pool không tồn tại: {0}")]
    PoolNotFound(String),
    
    #[error("Người dùng không có stake trong pool: {0}")]
    UserStakeNotFound(String),
    
    #[error("Số lượng token không đủ: yêu cầu {required}, hiện có {available}")]
    InsufficientTokens {
        required: f64,
        available: f64,
    },
    
    #[error("APY không hợp lệ: {0}")]
    InvalidAPY(f64),
    
    #[error("Thời gian staking không hợp lệ")]
    InvalidStakingTime,
    
    #[error("Lỗi hệ thống: {0}")]
    SystemError(String),
}

/// Kết quả của các hoạt động staking
pub type StakeResult<T> = Result<T, StakeError>;

/// Thông tin về một staking pool
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StakePool {
    /// ID của pool
    pub id: String,
    
    /// Tên token được stake
    pub token: String,
    
    /// APY hiện tại (dưới dạng phần trăm)
    pub apy: f64,
    
    /// Tổng số token được stake trong pool
    pub total_staked: f64,
    
    /// Thời gian tạo pool
    pub created_at: u64,
    
    /// Thời gian cập nhật cuối cùng
    pub updated_at: u64,
    
    /// Lịch sử thay đổi APY, lưu dưới dạng (timestamp, apy)
    pub apy_history: Vec<(u64, f64)>,
}

/// Thông tin stake của người dùng
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserStake {
    /// ID của người dùng
    pub user_id: String,
    
    /// ID của pool
    pub pool_id: String,
    
    /// Số lượng token được stake
    pub amount: f64,
    
    /// Rewards đã tích lũy nhưng chưa claim
    pub pending_rewards: f64,
    
    /// Thời gian bắt đầu stake
    pub staked_at: u64,
    
    /// Thời gian cập nhật cuối cùng (để tính rewards)
    pub last_reward_calculation: u64,
}

/// Manager cho hệ thống staking
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StakeManager {
    /// Map của tất cả staking pools, key là pool ID
    pools: HashMap<String, StakePool>,
    
    /// Map của tất cả user stakes, key là "user_id:pool_id"
    user_stakes: HashMap<String, UserStake>,
    
    /// Bộ đếm ID pool
    pool_id_counter: u64,
}

impl StakeManager {
    /// Tạo một manager staking mới
    pub fn new() -> Self {
        Self {
            pools: HashMap::new(),
            user_stakes: HashMap::new(),
            pool_id_counter: 0,
        }
    }
    
    /// Tạo các pools mặc định với token được chỉ định
    pub async fn create_default_pools(&self, token_address: String) -> Result<(), String> {
        info!("Tạo các pools mặc định cho token: {}", token_address);
        
        // TODO: Tạo các pools mặc định dựa trên token_address
        // Ví dụ:
        // 1. Pool 30 ngày với APY 5%
        // 2. Pool 90 ngày với APY 7.5%
        // 3. Pool 180 ngày với APY 10%
        // 4. Pool 365 ngày với APY 15%
        
        Ok(())
    }
    
    /// Thêm pool staking mới
    pub fn add_pool(&mut self, token: &str, apy: f64) -> StakeResult<String> {
        if apy < 0.0 {
            return Err(StakeError::InvalidAPY(apy));
        }
        
        let now = get_current_timestamp();
        let id = format!("pool_{}", self.pool_id_counter);
        self.pool_id_counter += 1;
        
        let pool = StakePool {
            id: id.clone(),
            token: token.to_string(),
            apy,
            total_staked: 0.0,
            created_at: now,
            updated_at: now,
            apy_history: Vec::new(),
        };
        
        self.pools.insert(id.clone(), pool);
        
        info!("Pool staking mới được tạo: {} cho token {}, APY: {}%", id, token, apy);
        Ok(id)
    }
    
    /// Stake tokens vào một pool
    pub fn stake(&mut self, user_id: &str, pool_id: &str, amount: f64) -> StakeResult<()> {
        if amount <= 0.0 {
            return Err(StakeError::InsufficientTokens {
                required: amount,
                available: 0.0,
            });
        }
        
        let pool = self.pools.get_mut(pool_id)
            .ok_or_else(|| StakeError::PoolNotFound(pool_id.to_string()))?;
            
        let now = get_current_timestamp();
        let stake_key = format!("{}:{}", user_id, pool_id);
        
        // Cập nhật hoặc tạo user stake mới
        if let Some(user_stake) = self.user_stakes.get_mut(&stake_key) {
            // Tính rewards tích lũy trước khi thêm tokens mới
            self.calculate_pending_rewards(user_stake, &pool)?;
            
            // Cập nhật thông tin stake
            user_stake.amount += amount;
            user_stake.last_reward_calculation = now;
        } else {
            // Tạo stake mới
            let user_stake = UserStake {
                user_id: user_id.to_string(),
                pool_id: pool_id.to_string(),
                amount,
                pending_rewards: 0.0,
                staked_at: now,
                last_reward_calculation: now,
            };
            
            self.user_stakes.insert(stake_key, user_stake);
        }
        
        // Cập nhật tổng số token đã stake trong pool
        pool.total_staked += amount;
        pool.updated_at = now;
        
        info!("User {} đã stake {} tokens vào pool {}", user_id, amount, pool_id);
        Ok(())
    }
    
    /// Claim rewards từ một pool
    pub fn claim_rewards(&mut self, user_id: &str, pool_id: &str) -> StakeResult<f64> {
        let stake_key = format!("{}:{}", user_id, pool_id);
        let user_stake = self.user_stakes.get_mut(&stake_key)
            .ok_or_else(|| StakeError::UserStakeNotFound(stake_key))?;
            
        let pool = self.pools.get(pool_id)
            .ok_or_else(|| StakeError::PoolNotFound(pool_id.to_string()))?;
            
        // Cập nhật rewards tích lũy
        self.calculate_pending_rewards(user_stake, pool)?;
        
        let rewards = user_stake.pending_rewards;
        user_stake.pending_rewards = 0.0;
        user_stake.last_reward_calculation = get_current_timestamp();
        
        info!("User {} đã claim {} rewards từ pool {}", user_id, rewards, pool_id);
        Ok(rewards)
    }
    
    /// Unstake tokens từ một pool
    pub fn unstake(&mut self, user_id: &str, pool_id: &str, amount: f64) -> StakeResult<f64> {
        if amount <= 0.0 {
            return Err(StakeError::InsufficientTokens {
                required: amount,
                available: 0.0,
            });
        }
        
        let stake_key = format!("{}:{}", user_id, pool_id);
        let user_stake = self.user_stakes.get_mut(&stake_key)
            .ok_or_else(|| StakeError::UserStakeNotFound(stake_key.clone()))?;
            
        if user_stake.amount < amount {
            return Err(StakeError::InsufficientTokens {
                required: amount,
                available: user_stake.amount,
            });
        }
        
        let pool = self.pools.get_mut(pool_id)
            .ok_or_else(|| StakeError::PoolNotFound(pool_id.to_string()))?;
            
        // Cập nhật rewards tích lũy trước khi unstake
        self.calculate_pending_rewards(user_stake, pool)?;
        
        // Xử lý unstaking
        user_stake.amount -= amount;
        user_stake.last_reward_calculation = get_current_timestamp();
        
        // Cập nhật tổng số token đã stake trong pool
        pool.total_staked -= amount;
        pool.updated_at = get_current_timestamp();
        
        // Nếu user đã unstake tất cả tokens, xóa stake
        if user_stake.amount <= 0.0 {
            let pending_rewards = user_stake.pending_rewards;
            self.user_stakes.remove(&stake_key);
            info!("User {} đã unstake tất cả tokens từ pool {}, nhận {} rewards", user_id, pool_id, pending_rewards);
            return Ok(pending_rewards);
        }
        
        info!("User {} đã unstake {} tokens từ pool {}", user_id, amount, pool_id);
        Ok(0.0) // Không có rewards nào được claim
    }
    
    /// Cập nhật APY cho một pool
    pub fn update_apy(&mut self, pool_id: &str, new_apy: f64) -> StakeResult<()> {
        if new_apy < 0.0 {
            return Err(StakeError::InvalidAPY(new_apy));
        }
        
        let pool = self.pools.get_mut(pool_id)
            .ok_or_else(|| StakeError::PoolNotFound(pool_id.to_string()))?;
        
        let now = get_current_timestamp();
        
        // Lưu APY cũ vào lịch sử
        pool.apy_history.push((now, pool.apy));
        
        // Cập nhật APY và timestamp
        pool.apy = new_apy;
        pool.updated_at = now;
        
        info!("APY cho pool {} đã được cập nhật từ {}% thành {}%", 
              pool_id, 
              pool.apy_history.last().map(|(_, apy)| *apy).unwrap_or(0.0), 
              new_apy);
        Ok(())
    }
    
    /// Lấy thông tin về một pool
    pub fn get_pool(&self, pool_id: &str) -> StakeResult<&StakePool> {
        self.pools.get(pool_id)
            .ok_or_else(|| StakeError::PoolNotFound(pool_id.to_string()))
    }
    
    /// Lấy danh sách tất cả pools
    pub fn get_all_pools(&self) -> Vec<&StakePool> {
        self.pools.values().collect()
    }
    
    /// Lấy thông tin stake của một user
    pub fn get_user_stake(&self, user_id: &str, pool_id: &str) -> StakeResult<&UserStake> {
        let stake_key = format!("{}:{}", user_id, pool_id);
        self.user_stakes.get(&stake_key)
            .ok_or_else(|| StakeError::UserStakeNotFound(stake_key))
    }
    
    /// Lấy tất cả stakes của một user
    pub fn get_user_stakes(&self, user_id: &str) -> Vec<&UserStake> {
        self.user_stakes.values()
            .filter(|stake| stake.user_id == user_id)
            .collect()
    }
    
    /// Đồng bộ pools từ blockchain
    pub async fn sync_pools_from_blockchain(&mut self) -> StakeResult<()> {
        // TODO: Implement blockchain sync
        info!("Đồng bộ pools từ blockchain");
        Ok(())
    }
    
    /// Tính toán rewards cho một user stake
    fn calculate_pending_rewards(&mut self, user_stake: &mut UserStake, pool: &StakePool) -> StakeResult<()> {
        let now = get_current_timestamp();
        let time_elapsed = now as i64 - user_stake.last_reward_calculation as i64;
        
        if time_elapsed <= 0 {
            return Ok(());
        }
        
        // Tính toán rewards dựa trên APY, số lượng stake và thời gian
        // APY là phần trăm hàng năm, nên chúng ta cần chuyển đổi thành reward theo giây
        let seconds_in_year = 365 * 24 * 60 * 60;
        let apy_rate_per_second = pool.apy / (100.0 * seconds_in_year as f64);
        
        // Rewards = amount * APY_per_second * seconds_elapsed
        let rewards = user_stake.amount * apy_rate_per_second * time_elapsed as f64;
        
        // Cộng rewards mới vào pending rewards
        user_stake.pending_rewards += rewards;
        
        // Cập nhật thời gian tính toán rewards
        user_stake.last_reward_calculation = now;
        
        debug!("Đã tính toán {} rewards cho user {} trong pool {}", 
               rewards, user_stake.user_id, user_stake.pool_id);
        
        Ok(())
    }
}

/// Lấy timestamp hiện tại dưới dạng giây
fn get_current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_add_pool() {
        let mut manager = StakeManager::new();
        let pool_id = manager.add_pool("DMD", 10.0).unwrap();
        
        assert!(manager.pools.contains_key(&pool_id));
        let pool = manager.pools.get(&pool_id).unwrap();
        assert_eq!(pool.token, "DMD");
        assert_eq!(pool.apy, 10.0);
    }
    
    #[test]
    fn test_stake_and_unstake() {
        let mut manager = StakeManager::new();
        let pool_id = manager.add_pool("DMD", 10.0).unwrap();
        
        // Stake
        manager.stake("user1", &pool_id, 100.0).unwrap();
        
        let stake_key = format!("{}:{}", "user1", pool_id);
        let user_stake = manager.user_stakes.get(&stake_key).unwrap();
        assert_eq!(user_stake.amount, 100.0);
        
        let pool = manager.pools.get(&pool_id).unwrap();
        assert_eq!(pool.total_staked, 100.0);
        
        // Unstake partial
        manager.unstake("user1", &pool_id, 40.0).unwrap();
        
        let user_stake = manager.user_stakes.get(&stake_key).unwrap();
        assert_eq!(user_stake.amount, 60.0);
        
        let pool = manager.pools.get(&pool_id).unwrap();
        assert_eq!(pool.total_staked, 60.0);
        
        // Unstake all
        manager.unstake("user1", &pool_id, 60.0).unwrap();
        
        assert!(!manager.user_stakes.contains_key(&stake_key));
        
        let pool = manager.pools.get(&pool_id).unwrap();
        assert_eq!(pool.total_staked, 0.0);
    }
    
    #[test]
    fn test_rewards() {
        // Note: Testing rewards accurately would require mocking time,
        // which is beyond the scope of this simple test.
        // In a real implementation, we would use a time mocking library
        // or dependency injection for time.
    }
}
