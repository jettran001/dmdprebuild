//! Module quản lý farming và liquidity pools trong blockchain
//!
//! Module này cung cấp các chức năng cho farming và liquidity pools:
//! - Quản lý farming pools
//! - Quản lý liquidity pools
//! - Tính toán rewards
//! - Add/remove liquidity
//!
//! # Ví dụ:
//! ```
//! use blockchain::farm::FarmManager;
//!
//! async fn example() {
//!     // Tạo farm manager mới
//!     let mut farm_manager = FarmManager::new();
//!     
//!     // Thêm pool mới
//!     let pool_id = farm_manager.add_pool("DMD-USDT", 25.0).unwrap();
//!     
//!     // Add liquidity
//!     let user_id = "user123";
//!     farm_manager.add_liquidity(user_id, pool_id, 100.0).unwrap();
//!     
//!     // Claim rewards
//!     let rewards = farm_manager.harvest(user_id, pool_id).unwrap();
//!     println!("Harvested rewards: {}", rewards);
//!     
//!     // Remove liquidity
//!     farm_manager.remove_liquidity(user_id, pool_id, 50.0).unwrap();
//! }
//! ```

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use serde::{Deserialize, Serialize};
use log::{debug, info, warn, error};

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
}

/// Kết quả của các hoạt động farming
pub type FarmResult<T> = Result<T, FarmError>;

/// Cấu hình của farming pool
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FarmPoolConfig {
    /// ID của pool
    pub id: String,
    
    /// Tên của pair (VD: DMD-USDT)
    pub pair_name: String,
    
    /// APR hiện tại (dưới dạng phần trăm)
    pub apr: f64,
    
    /// Tổng liquidity trong pool
    pub total_liquidity: f64,
    
    /// Thời gian tạo pool
    pub created_at: u64,
    
    /// Thời gian cập nhật cuối cùng
    pub updated_at: u64,
}

/// Thông tin farm của người dùng
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserFarmInfo {
    /// ID của người dùng
    pub user_id: String,
    
    /// ID của pool
    pub pool_id: String,
    
    /// Số lượng liquidity đã thêm vào
    pub liquidity_amount: f64,
    
    /// Rewards đã tích lũy nhưng chưa claim
    pub pending_rewards: f64,
    
    /// Thời gian bắt đầu farming
    pub farm_started_at: u64,
    
    /// Thời gian cập nhật cuối cùng (để tính rewards)
    pub last_reward_calculation: u64,
}

/// Manager cho hệ thống farming
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FarmManager {
    /// Map của tất cả farming pools, key là pool ID
    pools: HashMap<String, FarmPoolConfig>,
    
    /// Map của tất cả user farms, key là "user_id:pool_id"
    user_farms: HashMap<String, UserFarmInfo>,
    
    /// Tổng số liquidity đã thêm vào toàn bộ hệ thống
    total_system_liquidity: f64,
    
    /// Tổng số rewards đã phân phối
    total_rewards_distributed: f64,
    
    /// Bộ đếm ID pool
    pool_id_counter: u64,
}

impl FarmManager {
    /// Tạo một manager farming mới
    pub fn new() -> Self {
        Self {
            pools: HashMap::new(),
            user_farms: HashMap::new(),
            total_system_liquidity: 0.0,
            total_rewards_distributed: 0.0,
            pool_id_counter: 0,
        }
    }
    
    /// Tạo các pools mặc định với token được chỉ định
    pub async fn create_default_pools(&self, token_address: String) -> Result<(), String> {
        info!("Tạo các pools farming mặc định cho token: {}", token_address);
        
        // TODO: Tạo các farm pools mặc định dựa trên token_address
        // Ví dụ:
        // 1. Pool DMD-USDT với APR 15%
        // 2. Pool DMD-ETH với APR 20%
        // 3. Pool DMD-BNB với APR 25%
        
        Ok(())
    }
    
    /// Thêm farming pool mới
    pub fn add_pool(&mut self, pair_name: &str, apr: f64) -> FarmResult<String> {
        if apr < 0.0 {
            return Err(FarmError::InvalidAPR(apr));
        }
        
        let now = get_current_timestamp();
        let id = format!("farm_{}", self.pool_id_counter);
        self.pool_id_counter += 1;
        
        let pool = FarmPoolConfig {
            id: id.clone(),
            pair_name: pair_name.to_string(),
            apr,
            total_liquidity: 0.0,
            created_at: now,
            updated_at: now,
        };
        
        self.pools.insert(id.clone(), pool);
        
        info!("Pool farming mới được tạo: {} cho pair {}, APR: {}%", id, pair_name, apr);
        Ok(id)
    }
    
    /// Thêm liquidity vào pool
    pub fn add_liquidity(&mut self, user_id: &str, pool_id: &str, amount: f64) -> FarmResult<()> {
        if amount <= 0.0 {
            return Err(FarmError::InsufficientLiquidity {
                required: amount,
                available: 0.0,
            });
        }
        
        let pool = self.pools.get_mut(pool_id)
            .ok_or_else(|| FarmError::PoolNotFound(pool_id.to_string()))?;
            
        let now = get_current_timestamp();
        let farm_key = format!("{}:{}", user_id, pool_id);
        
        // Cập nhật hoặc tạo user farm mới
        if let Some(user_farm) = self.user_farms.get_mut(&farm_key) {
            // Tính rewards tích lũy trước khi thêm liquidity mới
            self.calculate_pending_rewards(user_farm, pool)?;
            
            // Cập nhật thông tin farm
            user_farm.liquidity_amount += amount;
            user_farm.last_reward_calculation = now;
        } else {
            // Tạo farm mới
            let user_farm = UserFarmInfo {
                user_id: user_id.to_string(),
                pool_id: pool_id.to_string(),
                liquidity_amount: amount,
                pending_rewards: 0.0,
                farm_started_at: now,
                last_reward_calculation: now,
            };
            
            self.user_farms.insert(farm_key, user_farm);
        }
        
        // Cập nhật tổng số liquidity trong pool
        pool.total_liquidity += amount;
        pool.updated_at = now;
        
        // Cập nhật tổng số liquidity toàn hệ thống
        self.total_system_liquidity += amount;
        
        info!("User {} đã thêm {} liquidity vào pool {} tại thời điểm {}", 
              user_id, amount, pool_id, now);
        Ok(())
    }
    
    /// Rút liquidity từ pool
    pub fn remove_liquidity(&mut self, user_id: &str, pool_id: &str, amount: f64) -> FarmResult<f64> {
        if amount <= 0.0 {
            return Err(FarmError::InsufficientLiquidity {
                required: amount,
                available: 0.0,
            });
        }
        
        let farm_key = format!("{}:{}", user_id, pool_id);
        let user_farm = self.user_farms.get_mut(&farm_key)
            .ok_or_else(|| FarmError::UserFarmNotFound(farm_key.clone()))?;
            
        if user_farm.liquidity_amount < amount {
            return Err(FarmError::InsufficientLiquidity {
                required: amount,
                available: user_farm.liquidity_amount,
            });
        }
        
        let pool = self.pools.get_mut(pool_id)
            .ok_or_else(|| FarmError::PoolNotFound(pool_id.to_string()))?;
            
        // Tính rewards tích lũy trước khi rút liquidity
        self.calculate_pending_rewards(user_farm, pool)?;
        
        // Xử lý rút liquidity
        user_farm.liquidity_amount -= amount;
        user_farm.last_reward_calculation = get_current_timestamp();
        
        // Cập nhật tổng số liquidity trong pool
        pool.total_liquidity -= amount;
        pool.updated_at = get_current_timestamp();
        
        // Cập nhật tổng số liquidity toàn hệ thống
        self.total_system_liquidity -= amount;
        
        // Nếu user đã rút toàn bộ liquidity, trả về rewards và xóa thông tin farm
        if user_farm.liquidity_amount <= 0.0 {
            let pending_rewards = user_farm.pending_rewards;
            self.user_farms.remove(&farm_key);
            
            // Cập nhật tổng số rewards đã phân phối
            self.total_rewards_distributed += pending_rewards;
            
            info!("User {} đã rút toàn bộ liquidity từ pool {}, nhận {} rewards", user_id, pool_id, pending_rewards);
            return Ok(pending_rewards);
        }
        
        info!("User {} đã rút {} liquidity từ pool {}", user_id, amount, pool_id);
        Ok(0.0) // Không có rewards nào được claim
    }
    
    /// Thu hoạch rewards từ pool
    pub fn harvest(&mut self, user_id: &str, pool_id: &str) -> FarmResult<f64> {
        let farm_key = format!("{}:{}", user_id, pool_id);
        let user_farm = self.user_farms.get_mut(&farm_key)
            .ok_or_else(|| FarmError::UserFarmNotFound(farm_key))?;
            
        let pool = self.pools.get(pool_id)
            .ok_or_else(|| FarmError::PoolNotFound(pool_id.to_string()))?;
            
        // Cập nhật rewards tích lũy
        self.calculate_pending_rewards(user_farm, pool)?;
        
        let rewards = user_farm.pending_rewards;
        user_farm.pending_rewards = 0.0;
        user_farm.last_reward_calculation = get_current_timestamp();
        
        // Cập nhật tổng số rewards đã phân phối
        self.total_rewards_distributed += rewards;
        
        info!("User {} đã harvest {} rewards từ pool {}", user_id, rewards, pool_id);
        Ok(rewards)
    }
    
    /// Cập nhật APR cho pool
    pub fn update_apr(&mut self, pool_id: &str, new_apr: f64) -> FarmResult<()> {
        if new_apr < 0.0 {
            return Err(FarmError::InvalidAPR(new_apr));
        }
        
        let pool = self.pools.get_mut(pool_id)
            .ok_or_else(|| FarmError::PoolNotFound(pool_id.to_string()))?;
            
        // Cập nhật APR và timestamp
        pool.apr = new_apr;
        pool.updated_at = get_current_timestamp();
        
        info!("APR cho pool {} đã được cập nhật thành {}%", pool_id, new_apr);
        Ok(())
    }
    
    /// Lấy thông tin về pool
    pub fn get_pool(&self, pool_id: &str) -> FarmResult<&FarmPoolConfig> {
        self.pools.get(pool_id)
            .ok_or_else(|| FarmError::PoolNotFound(pool_id.to_string()))
    }
    
    /// Lấy danh sách tất cả pools
    pub fn get_all_pools(&self) -> Vec<&FarmPoolConfig> {
        self.pools.values().collect()
    }
    
    /// Lấy thông tin farm của user
    pub fn get_user_farm(&self, user_id: &str, pool_id: &str) -> FarmResult<&UserFarmInfo> {
        let farm_key = format!("{}:{}", user_id, pool_id);
        self.user_farms.get(&farm_key)
            .ok_or_else(|| FarmError::UserFarmNotFound(farm_key))
    }
    
    /// Lấy tất cả farms của user
    pub fn get_user_farms(&self, user_id: &str) -> Vec<&UserFarmInfo> {
        self.user_farms.values()
            .filter(|farm| farm.user_id == user_id)
            .collect()
    }
    
    /// Lấy tổng liquidity trong hệ thống
    pub fn get_total_system_liquidity(&self) -> f64 {
        self.total_system_liquidity
    }
    
    /// Lấy tổng rewards đã phân phối
    pub fn get_total_rewards_distributed(&self) -> f64 {
        self.total_rewards_distributed
    }
    
    /// Đồng bộ pools từ blockchain
    pub async fn sync_pools_from_blockchain(&mut self) -> FarmResult<()> {
        // TODO: Implement blockchain sync
        info!("Đồng bộ farming pools từ blockchain");
        Ok(())
    }
    
    /// Tính toán rewards cho một user farm
    fn calculate_pending_rewards(&mut self, user_farm: &mut UserFarmInfo, pool: &FarmPoolConfig) -> FarmResult<()> {
        let now = get_current_timestamp();
        let time_elapsed = now as i64 - user_farm.last_reward_calculation as i64;
        
        if time_elapsed <= 0 {
            return Ok(());
        }
        
        // Tính toán rewards dựa trên APR, số lượng liquidity và thời gian
        // APR là phần trăm hàng năm, nên chúng ta cần chuyển đổi thành reward theo giây
        let seconds_in_year = 365 * 24 * 60 * 60;
        let apr_rate_per_second = pool.apr / (100.0 * seconds_in_year as f64);
        
        // Rewards = liquidity_amount * APR_per_second * seconds_elapsed
        let rewards = user_farm.liquidity_amount * apr_rate_per_second * time_elapsed as f64;
        
        // Cộng rewards mới vào pending rewards
        user_farm.pending_rewards += rewards;
        
        // Cập nhật thời gian tính toán rewards
        user_farm.last_reward_calculation = now;
        
        debug!("Đã tính toán {} rewards cho user {} trong pool {}", 
               rewards, user_farm.user_id, user_farm.pool_id);
        
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
        let mut manager = FarmManager::new();
        let pool_id = manager.add_pool("DMD-USDT", 20.0).unwrap();
        
        assert!(manager.pools.contains_key(&pool_id));
        let pool = manager.pools.get(&pool_id).unwrap();
        assert_eq!(pool.pair_name, "DMD-USDT");
        assert_eq!(pool.apr, 20.0);
    }
    
    #[test]
    fn test_add_and_remove_liquidity() {
        let mut manager = FarmManager::new();
        let pool_id = manager.add_pool("DMD-USDT", 20.0).unwrap();
        
        // Add liquidity
        manager.add_liquidity("user1", &pool_id, 100.0).unwrap();
        
        let farm_key = format!("{}:{}", "user1", pool_id);
        let user_farm = manager.user_farms.get(&farm_key).unwrap();
        assert_eq!(user_farm.liquidity_amount, 100.0);
        
        let pool = manager.pools.get(&pool_id).unwrap();
        assert_eq!(pool.total_liquidity, 100.0);
        
        // Remove partial liquidity
        manager.remove_liquidity("user1", &pool_id, 40.0).unwrap();
        
        let user_farm = manager.user_farms.get(&farm_key).unwrap();
        assert_eq!(user_farm.liquidity_amount, 60.0);
        
        let pool = manager.pools.get(&pool_id).unwrap();
        assert_eq!(pool.total_liquidity, 60.0);
        
        // Remove all liquidity
        manager.remove_liquidity("user1", &pool_id, 60.0).unwrap();
        
        assert!(!manager.user_farms.contains_key(&farm_key));
        
        let pool = manager.pools.get(&pool_id).unwrap();
        assert_eq!(pool.total_liquidity, 0.0);
    }
    
    #[test]
    fn test_rewards() {
        // Note: Testing rewards accurately would require mocking time,
        // which is beyond the scope of this simple test.
        // In a real implementation, we would use a time mocking library
        // or dependency injection for time.
    }
}
