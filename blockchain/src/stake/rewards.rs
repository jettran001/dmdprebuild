//! Module tính toán và phân phối rewards
//!
//! Module này cung cấp các chức năng:
//! - Tính toán rewards dựa trên thời gian stake và APY
//! - Phân phối rewards cho người dùng
//! - Quản lý rewards pool

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use async_trait::async_trait;
use ethers::types::{Address, U256};
use tracing::{info, warn, error, debug};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::stake::{StakePoolConfig, UserStakeInfo};
use super::constants::*;

/// Thông tin rewards pool
#[derive(Debug, Clone)]
pub struct RewardsPool {
    /// Địa chỉ pool
    pub pool_address: Address,
    /// Tổng rewards đã phân phối
    pub total_distributed: U256,
    /// Tổng rewards còn lại
    pub remaining_rewards: U256,
    /// Thời gian cập nhật cuối cùng
    pub last_update_time: u64,
    /// APY hiện tại
    pub current_apy: f64,
}

/// Multiplication factor để tính APY, giả sử 4 chữ số thập phân
const APY_PRECISION: u64 = 10000;

/// Trait cho RewardCalculator
#[async_trait]
pub trait RewardCalculator: Send + Sync {
    /// Tính toán phần thưởng đang chờ cho một người dùng
    async fn calculate_pending_rewards(&self, pool: &StakePoolConfig, user_stake: &UserStakeInfo) -> Result<U256>;
    
    /// Cập nhật APY cho pool
    async fn update_pool_apy(&self, pool_address: Address, new_apy: u64) -> Result<()>;
    
    /// Phân phối phần thưởng
    async fn distribute_rewards(&self, pool_address: Address) -> Result<U256>;
}

/// Standard reward calculator implementation
pub struct StandardRewardCalculator {
    /// Validator instance
    validator: Arc<dyn crate::stake::Validator>,
}

impl StandardRewardCalculator {
    /// Tạo calculator mới
    pub fn new(validator: Arc<dyn crate::stake::Validator>) -> Self {
        Self { validator }
    }
    
    /// Tính toán thời gian hiện tại (timestamp)
    fn get_current_time() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
    
    /// Tính toán phần thưởng dựa trên số lượng stake, thời gian, và APY
    fn calculate_rewards_by_time(
        &self,
        staked_amount: U256, 
        start_time: u64,
        end_time: u64,
        apy: u64
    ) -> Result<U256> {
        // Kiểm tra giá trị đầu vào
        if end_time <= start_time {
            return Ok(U256::zero());
        }
        
        // Thời gian stake tính bằng giây
        let staking_duration = end_time - start_time;
        
        // Số giây trong một năm
        let seconds_per_year: u64 = 365 * 24 * 60 * 60;
        
        // Tính phần thưởng: amount * (apy / APY_PRECISION) * (duration / seconds_per_year)
        // Để tránh tràn số và mất độ chính xác, chúng ta sẽ sử dụng checked_mul và checked_div
        
        // 1. Tính amount * apy
        let amount_mul_apy = staked_amount
            .checked_mul(U256::from(apy))
            .ok_or_else(|| anyhow::anyhow!("Overflow in reward calculation: amount * apy"))?;
        
        // 2. Tính kết quả * duration
        let result_mul_duration = amount_mul_apy
            .checked_mul(U256::from(staking_duration))
            .ok_or_else(|| anyhow::anyhow!("Overflow in reward calculation: result * duration"))?;
            
        // 3. Chia cho APY_PRECISION * seconds_per_year
        let precision_mul_seconds = U256::from(APY_PRECISION)
            .checked_mul(U256::from(seconds_per_year))
            .ok_or_else(|| anyhow::anyhow!("Overflow in reward calculation: precision * seconds"))?;
            
        let rewards = result_mul_duration
            .checked_div(precision_mul_seconds)
            .ok_or_else(|| anyhow::anyhow!("Division by zero in reward calculation"))?;
            
        debug!(
            "Reward calculation: amount={:?}, start_time={}, end_time={}, apy={}, rewards={:?}",
            staked_amount, start_time, end_time, apy, rewards
        );
            
        Ok(rewards)
    }
    
    /// Áp dụng bonus dựa trên thời gian lock
    fn apply_time_bonus(&self, rewards: U256, lock_time: u64, min_lock_time: u64, max_lock_time: u64) -> Result<U256> {
        // Nếu thời gian lock ngắn hơn minimum, không có bonus
        if lock_time <= min_lock_time {
            return Ok(rewards);
        }
        
        // Nếu thời gian lock dài hơn maximum, áp dụng bonus tối đa (50%)
        if lock_time >= max_lock_time {
            return Ok(rewards.checked_mul(U256::from(150))
                .ok_or_else(|| anyhow::anyhow!("Overflow in bonus calculation"))?
                .checked_div(U256::from(100))
                .ok_or_else(|| anyhow::anyhow!("Division by zero in bonus calculation"))?);
        }
        
        // Tính toán bonus tỷ lệ thuận với thời gian lock
        // Bonus từ 0% đến 50% dựa trên thời gian lock
        let lock_range = max_lock_time - min_lock_time;
        let lock_above_min = lock_time - min_lock_time;
        
        // Tính phần trăm bonus (0-50)
        let bonus_percent = (lock_above_min * 50) / lock_range;
        
        // Áp dụng bonus vào rewards
        let bonus_multiplier = 100 + bonus_percent;
        let bonus_rewards = rewards
            .checked_mul(U256::from(bonus_multiplier))
            .ok_or_else(|| anyhow::anyhow!("Overflow in bonus calculation"))?
            .checked_div(U256::from(100))
            .ok_or_else(|| anyhow::anyhow!("Division by zero in bonus calculation"))?;
            
        debug!(
            "Bonus calculation: rewards={:?}, lock_time={}, bonus_percent={}, final_rewards={:?}",
            rewards, lock_time, bonus_percent, bonus_rewards
        );
            
        Ok(bonus_rewards)
    }
    
    /// Xử lý trường hợp compound interest nếu được bật
    fn apply_compound_interest(
        &self, 
        staked_amount: U256, 
        start_time: u64,
        end_time: u64,
        apy: u64,
        compound_frequency: u64
    ) -> Result<U256> {
        // Nếu không compound hoặc tần suất là 0, sử dụng tính toán simple interest
        if compound_frequency == 0 {
            return self.calculate_rewards_by_time(staked_amount, start_time, end_time, apy);
        }
        
        // Tính toán chu kỳ compound (tính bằng giây)
        let compound_period = match compound_frequency {
            1 => 365 * 24 * 60 * 60, // Yearly
            2 => 30 * 24 * 60 * 60,  // Monthly
            3 => 7 * 24 * 60 * 60,   // Weekly
            4 => 24 * 60 * 60,       // Daily
            _ => return self.calculate_rewards_by_time(staked_amount, start_time, end_time, apy), // Default to simple interest
        };
        
        let total_duration = end_time - start_time;
        let number_of_periods = total_duration / compound_period;
        
        // Nếu chưa đủ 1 chu kỳ, sử dụng tính toán simple interest
        if number_of_periods == 0 {
            return self.calculate_rewards_by_time(staked_amount, start_time, end_time, apy);
        }
        
        // Tính toán compound interest: P * (1 + r/n)^(n*t) - P
        // Trong đó:
        // P = Principal (staked_amount)
        // r = Annual rate (apy / APY_PRECISION)
        // n = Number of times interest is compounded per year
        // t = Time in years
        
        let mut accumulated_amount = staked_amount;
        let mut current_time = start_time;
        
        for _ in 0..number_of_periods {
            let period_end = current_time + compound_period;
            let period_rewards = self.calculate_rewards_by_time(
                accumulated_amount, 
                current_time, 
                period_end, 
                apy
            )?;
            
            // Cộng dồn phần thưởng vào số tiền gốc
            accumulated_amount = accumulated_amount
                .checked_add(period_rewards)
                .ok_or_else(|| anyhow::anyhow!("Overflow in compound calculation"))?;
                
            current_time = period_end;
        }
        
        // Tính phần thưởng cho thời gian còn lại
        if current_time < end_time {
            let remaining_rewards = self.calculate_rewards_by_time(
                accumulated_amount, 
                current_time, 
                end_time, 
                apy
            )?;
            
            accumulated_amount = accumulated_amount
                .checked_add(remaining_rewards)
                .ok_or_else(|| anyhow::anyhow!("Overflow in compound calculation"))?;
        }
        
        // Phần thưởng = tổng số tiền tích lũy - số tiền gốc ban đầu
        let total_rewards = accumulated_amount
            .checked_sub(staked_amount)
            .ok_or_else(|| anyhow::anyhow!("Underflow in compound calculation"))?;
            
        debug!(
            "Compound calculation: initial={:?}, periods={}, final_amount={:?}, rewards={:?}",
            staked_amount, number_of_periods, accumulated_amount, total_rewards
        );
            
        Ok(total_rewards)
    }
}

#[async_trait]
impl RewardCalculator for StandardRewardCalculator {
    async fn calculate_pending_rewards(&self, pool: &StakePoolConfig, user_stake: &UserStakeInfo) -> Result<U256> {
        // Kiểm tra nếu người dùng đã stake
        if user_stake.staked_amount == U256::zero() {
            return Ok(U256::zero());
        }
        
        // Lấy thời gian hiện tại
        let current_time = Self::get_current_time();
        
        // Xác định thời điểm kết thúc tính toán rewards
        let end_time = if pool.compound_enabled {
            // Nếu compound, tính rewards cho đến thời điểm hiện tại
            current_time
        } else {
            // Nếu không compound, chỉ tính rewards đến cuối thời gian lock
            std::cmp::min(current_time, user_stake.start_time + user_stake.lock_time)
        };
        
        // Nếu thời gian bắt đầu sau thời gian kết thúc, không có rewards
        if user_stake.start_time >= end_time {
            return Ok(U256::zero());
        }
        
        // Tính toán rewards dựa trên phương pháp phù hợp
        let base_rewards = if pool.compound_enabled {
            self.apply_compound_interest(
                user_stake.staked_amount,
                user_stake.start_time,
                end_time,
                pool.apy,
                pool.compound_frequency
            )?
        } else {
            self.calculate_rewards_by_time(
                user_stake.staked_amount,
                user_stake.start_time,
                end_time,
                pool.apy
            )?
        };
        
        // Áp dụng bonus dựa trên thời gian lock
        let rewards_with_bonus = self.apply_time_bonus(
            base_rewards,
            user_stake.lock_time,
            pool.min_lock_time,
            pool.max_lock_time
        )?;
        
        // Trừ đi phần thưởng đã claim
        let pending_rewards = rewards_with_bonus
            .checked_sub(user_stake.claimed_rewards)
            .unwrap_or(U256::zero());
            
        info!(
            "Calculated rewards for user {:?}: base={:?}, with_bonus={:?}, pending={:?}",
            user_stake.user_address, base_rewards, rewards_with_bonus, pending_rewards
        );
            
        Ok(pending_rewards)
    }

    async fn update_pool_apy(&self, pool_address: Address, new_apy: u64) -> Result<()> {
        // Kiểm tra nếu người gọi có quyền thay đổi APY
        // TODO: Implement xác thực quyền
        
        // TODO: Cập nhật APY trong smart contract hoặc database
        
        info!("Updated APY for pool {:?}: {}", pool_address, new_apy);
        Ok(())
    }

    async fn distribute_rewards(&self, pool_address: Address) -> Result<U256> {
        // TODO: Implement phân phối phần thưởng
        // 1. Lấy danh sách tất cả người dùng trong pool
        // 2. Tính toán phần thưởng cho từng người
        // 3. Chuyển tokens
        
        // Dummy implementation
        Ok(U256::zero())
    }
}
