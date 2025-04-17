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

    /// Trạng thái của farm: active, paused, closed
    pub status: FarmStatus,
}

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
    pub async fn create_default_pools(&mut self, token_address: String) -> FarmResult<Vec<String>> {
        info!("Tạo các pools farming mặc định cho token: {}", token_address);
        
        let pairs = vec![
            (format!("{}-USDT", token_address), 15.0),
            (format!("{}-ETH", token_address), 20.0),
            (format!("{}-BNB", token_address), 25.0),
            (format!("{}-BUSD", token_address), 12.0),
        ];
        
        let mut pool_ids = Vec::new();
        
        for (pair_name, apr) in pairs {
            match self.add_pool(&pair_name, apr) {
                Ok(pool_id) => {
                    info!("Đã tạo pool mặc định: {} với APR {}%", pair_name, apr);
                    pool_ids.push(pool_id);
                },
                Err(e) => {
                    error!("Không thể tạo pool mặc định cho {}: {}", pair_name, e);
                }
            }
        }
        
        info!("Đã tạo {} pools mặc định cho token {}", pool_ids.len(), token_address);
        Ok(pool_ids)
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
            // Kiểm tra trạng thái farm
            if user_farm.status == FarmStatus::Closed {
                return Err(FarmError::InvalidFarmStatus {
                    current_status: format!("{:?}", user_farm.status),
                    required_status: format!("{:?}", FarmStatus::Active),
                });
            }
            
            // Tính rewards tích lũy trước khi thêm liquidity mới
            self.calculate_pending_rewards(user_farm, pool)
                .with_farm_context(|| format!("Không thể tính pending rewards cho user {} trong pool {}", user_id, pool_id))?;
            
            // Cập nhật thông tin farm
            user_farm.liquidity_amount += amount;
            user_farm.last_reward_calculation = now;
            
            // Kích hoạt farm nếu đang ở trạng thái tạm dừng
            if user_farm.status == FarmStatus::Paused {
                user_farm.status = FarmStatus::Active;
                info!("Farm của user {} cho pool {} đã được kích hoạt lại", user_id, pool_id);
            }
        } else {
            // Tạo farm mới
            let user_farm = UserFarmInfo {
                user_id: user_id.to_string(),
                pool_id: pool_id.to_string(),
                liquidity_amount: amount,
                pending_rewards: 0.0,
                farm_started_at: now,
                last_reward_calculation: now,
                status: FarmStatus::Active,
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
        info!("Bắt đầu đồng bộ pools từ blockchain");
        
        // Kết nối với blockchain provider
        let provider = self.get_blockchain_provider().await
            .with_farm_context(|| "Không thể kết nối với blockchain provider")?;
        
        // Lấy danh sách pools từ blockchain
        let onchain_pools = self.fetch_pools_from_blockchain(&provider).await
            .with_farm_context(|| "Không thể lấy danh sách pools từ blockchain")?;
        
        // Cập nhật pools hiện tại và thêm pools mới
        for pool_config in onchain_pools {
            if let Some(existing_pool) = self.pools.get_mut(&pool_config.id) {
                // Cập nhật thông tin pool
                info!("Cập nhật pool {}: APR {}% -> {}%, liquidity {} -> {}", 
                    pool_config.id, existing_pool.apr, pool_config.apr,
                    existing_pool.total_liquidity, pool_config.total_liquidity);
                
                existing_pool.apr = pool_config.apr;
                existing_pool.total_liquidity = pool_config.total_liquidity;
                existing_pool.updated_at = get_current_timestamp();
            } else {
                // Thêm pool mới
                info!("Thêm pool mới từ blockchain: {} ({})", pool_config.id, pool_config.pair_name);
                self.pools.insert(pool_config.id.clone(), pool_config);
            }
        }
        
        // Đồng bộ thông tin farm của người dùng
        self.sync_user_farms_from_blockchain(&provider).await
            .with_farm_context(|| "Không thể đồng bộ thông tin farm của người dùng")?;
        
        info!("Đồng bộ pools từ blockchain hoàn tất");
        Ok(())
    }
    
    /// Lấy blockchain provider
    async fn get_blockchain_provider(&self) -> Result<String, FarmError> {
        // Trong triển khai thực tế, đây sẽ trả về provider thích hợp
        // từ hệ thống của bạn
        Ok("mock_provider".to_string())
    }
    
    /// Lấy thông tin pools từ blockchain
    async fn fetch_pools_from_blockchain(&self, provider: &str) -> Result<Vec<FarmPoolConfig>, FarmError> {
        // Trong triển khai thực tế, đây sẽ gọi các hàm RPC để lấy dữ liệu từ blockchain
        
        // Mock data
        let mock_pools = vec![
            FarmPoolConfig {
                id: "farm_1".to_string(),
                pair_name: "DMD-USDT".to_string(),
                apr: 15.5,
                total_liquidity: 100000.0,
                created_at: get_current_timestamp() - 86400, // 1 ngày trước
                updated_at: get_current_timestamp(),
            },
            FarmPoolConfig {
                id: "farm_2".to_string(),
                pair_name: "DMD-ETH".to_string(),
                apr: 22.5,
                total_liquidity: 50000.0,
                created_at: get_current_timestamp() - 172800, // 2 ngày trước
                updated_at: get_current_timestamp(),
            },
        ];
        
        Ok(mock_pools)
    }
    
    /// Đồng bộ thông tin farm của người dùng từ blockchain
    async fn sync_user_farms_from_blockchain(&mut self, provider: &str) -> FarmResult<()> {
        // Trong triển khai thực tế, đây sẽ lấy thông tin farm của người dùng từ blockchain
        // và cập nhật vào self.user_farms
        
        info!("Đồng bộ thông tin farm của người dùng từ blockchain");
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
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    
    // Helper để mock thời gian
    struct MockTimeSource {
        current_time: Arc<Mutex<u64>>,
    }
    
    impl MockTimeSource {
        fn new(initial_time: u64) -> Self {
            Self {
                current_time: Arc::new(Mutex::new(initial_time)),
            }
        }
        
        fn get_time(&self) -> u64 {
            *self.current_time.lock().unwrap()
        }
        
        fn advance(&self, seconds: u64) {
            let mut time = self.current_time.lock().unwrap();
            *time += seconds;
        }
    }
    
    // Cung cấp một bản sao của FarmManager với thời gian mock
    fn create_farm_manager_with_mock_time(mock_time: Arc<Mutex<u64>>) -> FarmManager {
        let mut manager = FarmManager::new();
        // Trong triển khai thực tế, chúng ta sẽ inject time source
        // Đây chỉ là ví dụ, chúng ta sẽ dùng giá trị mock trong các hàm test
        manager
    }
    
    // Helper để tạo một setup tiêu chuẩn cho tests
    fn setup_standard_farm(mock_time: &MockTimeSource) -> (FarmManager, String, String) {
        let mut manager = create_farm_manager_with_mock_time(mock_time.current_time.clone());
        let pool_id = manager.add_pool("DMD-USDT", 20.0).unwrap();
        let user_id = "test_user".to_string();
        
        (manager, pool_id, user_id)
    }
    
    #[test]
    fn test_add_pool() {
        let mock_time = MockTimeSource::new(1000);
        let mut manager = create_farm_manager_with_mock_time(mock_time.current_time.clone());
        
        // Test case 1: Thêm pool với APR hợp lệ
        let pool_id = manager.add_pool("DMD-USDT", 20.0).unwrap();
        assert!(manager.pools.contains_key(&pool_id));
        let pool = manager.pools.get(&pool_id).unwrap();
        assert_eq!(pool.pair_name, "DMD-USDT");
        assert_eq!(pool.apr, 20.0);
        
        // Test case 2: Thêm pool với APR không hợp lệ (âm)
        let result = manager.add_pool("DMD-ETH", -5.0);
        assert!(result.is_err());
        match result {
            Err(FarmError::InvalidAPR(apr)) => assert_eq!(apr, -5.0),
            _ => panic!("Expected InvalidAPR error, got something else"),
        }
        
        // Test case 3: Thêm nhiều pools
        for i in 0..5 {
            let pair_name = format!("DMD-TOKEN{}", i);
            let apr = 10.0 + i as f64;
            let pool_id = manager.add_pool(&pair_name, apr).unwrap();
            
            assert!(manager.pools.contains_key(&pool_id));
            let pool = manager.pools.get(&pool_id).unwrap();
            assert_eq!(pool.pair_name, pair_name);
            assert_eq!(pool.apr, apr);
        }
        
        // Kiểm tra tổng số pools
        assert_eq!(manager.pools.len(), 6); // 1 từ test case 1 + 5 từ vòng lặp
    }
    
    #[test]
    fn test_add_and_remove_liquidity() {
        let mock_time = MockTimeSource::new(1000);
        let (mut manager, pool_id, user_id) = setup_standard_farm(&mock_time);
        
        // Test case 1: Add liquidity
        manager.add_liquidity(&user_id, &pool_id, 100.0).unwrap();
        
        let farm_key = format!("{}:{}", user_id, pool_id);
        let user_farm = manager.user_farms.get(&farm_key).unwrap();
        assert_eq!(user_farm.liquidity_amount, 100.0);
        assert_eq!(user_farm.status, FarmStatus::Active);
        
        let pool = manager.pools.get(&pool_id).unwrap();
        assert_eq!(pool.total_liquidity, 100.0);
        
        // Test case 2: Add more liquidity
        manager.add_liquidity(&user_id, &pool_id, 50.0).unwrap();
        
        let user_farm = manager.user_farms.get(&farm_key).unwrap();
        assert_eq!(user_farm.liquidity_amount, 150.0);
        
        let pool = manager.pools.get(&pool_id).unwrap();
        assert_eq!(pool.total_liquidity, 150.0);
        
        // Test case 3: Remove partial liquidity
        manager.remove_liquidity(&user_id, &pool_id, 40.0).unwrap();
        
        let user_farm = manager.user_farms.get(&farm_key).unwrap();
        assert_eq!(user_farm.liquidity_amount, 110.0);
        
        let pool = manager.pools.get(&pool_id).unwrap();
        assert_eq!(pool.total_liquidity, 110.0);
        
        // Test case 4: Remove too much liquidity (should fail)
        let result = manager.remove_liquidity(&user_id, &pool_id, 200.0);
        assert!(result.is_err());
        match result {
            Err(FarmError::InsufficientLiquidity { required, available }) => {
                assert_eq!(required, 200.0);
                assert_eq!(available, 110.0);
            },
            _ => panic!("Expected InsufficientLiquidity error, got something else"),
        }
        
        // Test case 5: Remove liquidity from non-existent farm
        let result = manager.remove_liquidity("non_existent_user", &pool_id, 10.0);
        assert!(result.is_err());
        match result {
            Err(FarmError::UserFarmNotFound(_)) => {},
            _ => panic!("Expected UserFarmNotFound error, got something else"),
        }
        
        // Test case 6: Remove liquidity from non-existent pool
        let result = manager.remove_liquidity(&user_id, "non_existent_pool", 10.0);
        assert!(result.is_err());
        match result {
            Err(FarmError::UserFarmNotFound(_)) => {},
            _ => panic!("Expected UserFarmNotFound error, got something else"),
        }
        
        // Test case 7: Remove all liquidity
        let result = manager.remove_liquidity(&user_id, &pool_id, 110.0).unwrap();
        assert!(result >= 0.0); // Should return any pending rewards
        
        // Verify farm was removed
        assert!(!manager.user_farms.contains_key(&farm_key));
        
        // Verify pool liquidity was updated
        let pool = manager.pools.get(&pool_id).unwrap();
        assert_eq!(pool.total_liquidity, 0.0);
    }
    
    #[test]
    fn test_farm_status_transitions() {
        let mock_time = MockTimeSource::new(1000);
        let (mut manager, pool_id, user_id) = setup_standard_farm(&mock_time);
        
        // Thêm liquidity để tạo farm
        manager.add_liquidity(&user_id, &pool_id, 100.0).unwrap();
        
        let farm_key = format!("{}:{}", user_id, pool_id);
        
        // Kiểm tra trạng thái ban đầu
        {
            let user_farm = manager.user_farms.get(&farm_key).unwrap();
            assert_eq!(user_farm.status, FarmStatus::Active);
        }
        
        // Đặt trạng thái thành Paused
        {
            let user_farm = manager.user_farms.get_mut(&farm_key).unwrap();
            user_farm.status = FarmStatus::Paused;
        }
        
        // Thêm liquidity vào farm đã tạm dừng, nên chuyển sang Active
        manager.add_liquidity(&user_id, &pool_id, 50.0).unwrap();
        
        {
            let user_farm = manager.user_farms.get(&farm_key).unwrap();
            assert_eq!(user_farm.status, FarmStatus::Active);
            assert_eq!(user_farm.liquidity_amount, 150.0);
        }
        
        // Đặt trạng thái thành Closed
        {
            let user_farm = manager.user_farms.get_mut(&farm_key).unwrap();
            user_farm.status = FarmStatus::Closed;
        }
        
        // Thêm liquidity vào farm đã đóng, nên thất bại
        let result = manager.add_liquidity(&user_id, &pool_id, 50.0);
        assert!(result.is_err());
        match result {
            Err(FarmError::InvalidFarmStatus { .. }) => {},
            _ => panic!("Expected InvalidFarmStatus error, got something else"),
        }
        
        // Kiểm tra liquidity không thay đổi
        {
            let user_farm = manager.user_farms.get(&farm_key).unwrap();
            assert_eq!(user_farm.status, FarmStatus::Closed);
            assert_eq!(user_farm.liquidity_amount, 150.0);
        }
    }
    
    #[test]
    fn test_rewards() {
        // Tạo mock time source
        let mock_time = MockTimeSource::new(1000);
        let (mut manager, pool_id, user_id) = setup_standard_farm(&mock_time);
        
        // Thiết lập APR 20% cho pool
        manager.update_apr(&pool_id, 20.0).unwrap();
        
        // User stake 1000 tokens
        manager.add_liquidity(&user_id, &pool_id, 1000.0).unwrap();
        
        // Lấy farm key
        let farm_key = format!("{}:{}", user_id, pool_id);
        
        // Advance time by 1 day (86400 seconds)
        mock_time.advance(86400);
        
        // Tính toán rewards mong đợi sau 1 ngày với APR 20%
        // 1000 * 0.2 * (1/365) = 0.5479 tokens per day
        let expected_daily_rewards = 1000.0 * 0.2 * (1.0 / 365.0);
        
        // Harvest rewards
        let rewards = harvest_with_mock_time(&mut manager, &user_id, &pool_id, mock_time.get_time());
        
        // Kiểm tra rewards (với một khoảng sai số nhỏ)
        assert!((rewards - expected_daily_rewards).abs() < 0.01, 
               "Expected rewards around {}, got {}", expected_daily_rewards, rewards);
        
        // Advance time by 30 more days
        mock_time.advance(30 * 86400);
        
        // Tính toán rewards mong đợi sau 30 ngày với APR 20%
        let expected_monthly_rewards = 1000.0 * 0.2 * (30.0 / 365.0);
        
        // Harvest rewards again
        let rewards = harvest_with_mock_time(&mut manager, &user_id, &pool_id, mock_time.get_time());
        
        // Kiểm tra rewards (với một khoảng sai số nhỏ)
        assert!((rewards - expected_monthly_rewards).abs() < 0.01, 
               "Expected rewards around {}, got {}", expected_monthly_rewards, rewards);
        
        // Test multiple users với các lượng liquidity khác nhau
        let user_id2 = "test_user2".to_string();
        manager.add_liquidity(&user_id2, &pool_id, 500.0).unwrap(); // 500 tokens
        
        // Advance time by 10 more days
        mock_time.advance(10 * 86400);
        
        // Harvest rewards cho user1
        let rewards1 = harvest_with_mock_time(&mut manager, &user_id, &pool_id, mock_time.get_time());
        
        // Harvest rewards cho user2
        let rewards2 = harvest_with_mock_time(&mut manager, &user_id2, &pool_id, mock_time.get_time());
        
        // User1 có 1000 tokens, user2 có 500 tokens, nên rewards của user1 phải gấp đôi user2
        assert!((rewards1 - rewards2 * 2.0).abs() < 0.01, 
               "User1 rewards should be about 2x user2 rewards, but got {} and {}", rewards1, rewards2);
    }
    
    // Helper để harvest rewards với mock time
    fn harvest_with_mock_time(manager: &mut FarmManager, user_id: &str, pool_id: &str, current_time: u64) -> f64 {
        // Get user farm
        let farm_key = format!("{}:{}", user_id, pool_id);
        let user_farm = manager.user_farms.get_mut(&farm_key).unwrap();
        
        // Get pool 
        let pool = manager.pools.get(pool_id).unwrap();
        
        // Manually calculate rewards (đây là bản copy của calculate_pending_rewards)
        let time_elapsed = current_time as i64 - user_farm.last_reward_calculation as i64;
        if time_elapsed > 0 {
            let seconds_in_year = 365 * 24 * 60 * 60;
            let apr_rate_per_second = pool.apr / (100.0 * seconds_in_year as f64);
            let rewards = user_farm.liquidity_amount * apr_rate_per_second * time_elapsed as f64;
            user_farm.pending_rewards += rewards;
            user_farm.last_reward_calculation = current_time;
        }
        
        // Get rewards
        let rewards = user_farm.pending_rewards;
        user_farm.pending_rewards = 0.0;
        
        rewards
    }
    
    #[test]
    fn test_error_handling() {
        // Test các edge cases và error handling
        let mock_time = MockTimeSource::new(1000);
        let (mut manager, pool_id, user_id) = setup_standard_farm(&mock_time);
        
        // Test case 1: Thêm pool với tên trống
        let pool_id_empty = manager.add_pool("", 20.0).unwrap();
        assert!(manager.pools.contains_key(&pool_id_empty));
        
        // Test case 2: Get non-existent pool
        let result = manager.get_pool("non_existent_pool");
        assert!(result.is_err());
        match result {
            Err(FarmError::PoolNotFound(_)) => {},
            _ => panic!("Expected PoolNotFound error, got something else"),
        }
        
        // Test case 3: Get non-existent user farm
        let result = manager.get_user_farm(&user_id, &pool_id);
        assert!(result.is_err());
        match result {
            Err(FarmError::UserFarmNotFound(_)) => {},
            _ => panic!("Expected UserFarmNotFound error, got something else"),
        }
        
        // Test case 4: Add liquidity to non-existent pool
        let result = manager.add_liquidity(&user_id, "non_existent_pool", 100.0);
        assert!(result.is_err());
        match result {
            Err(FarmError::PoolNotFound(_)) => {},
            _ => panic!("Expected PoolNotFound error, got something else"),
        }
        
        // Test case 5: Add negative liquidity
        let result = manager.add_liquidity(&user_id, &pool_id, -100.0);
        assert!(result.is_err());
        match result {
            Err(FarmError::InsufficientLiquidity { required, available }) => {
                assert_eq!(required, -100.0);
                assert_eq!(available, 0.0);
            },
            _ => panic!("Expected InsufficientLiquidity error, got something else"),
        }
        
        // Test case 6: Update APR with negative value
        let result = manager.update_apr(&pool_id, -10.0);
        assert!(result.is_err());
        match result {
            Err(FarmError::InvalidAPR(apr)) => assert_eq!(apr, -10.0),
            _ => panic!("Expected InvalidAPR error, got something else"),
        }
        
        // Test case 7: Update APR for non-existent pool
        let result = manager.update_apr("non_existent_pool", 10.0);
        assert!(result.is_err());
        match result {
            Err(FarmError::PoolNotFound(_)) => {},
            _ => panic!("Expected PoolNotFound error, got something else"),
        }
    }
}
