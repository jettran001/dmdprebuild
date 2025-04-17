//! Module routers cho stake pools
//!
//! Module này cung cấp các chức năng:
//! - Định tuyến giao dịch đến các stake pools
//! - Quản lý pool selection
//! - Tối ưu hóa rewards

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use async_trait::async_trait;
use ethers::types::{Address, U256};
use tracing::{info, warn, error};

use crate::stake::{StakePoolConfig, UserStakeInfo};
use super::constants::*;

/// Thông tin routing
#[derive(Debug, Clone)]
pub struct RouteInfo {
    /// Địa chỉ pool được chọn
    pub pool_address: Address,
    /// APY dự kiến
    pub expected_apy: f64,
    /// Thời gian lock đề xuất
    pub suggested_lock_time: u64,
    /// Số lượng token tối ưu
    pub optimal_amount: U256,
}

/// Trait cho staking router
#[async_trait]
pub trait StakingRouter: Send + Sync {
    /// Tìm pool tối ưu cho stake
    async fn find_optimal_pool(
        &self,
        user_address: Address,
        amount: U256,
        lock_time: u64,
    ) -> Result<RouteInfo>;
    
    /// Cập nhật thông tin routing
    async fn update_routing_info(&self, pool_address: Address) -> Result<()>;
    
    /// Lấy danh sách pools được đề xuất
    async fn get_recommended_pools(
        &self,
        user_address: Address,
        amount: U256,
    ) -> Result<Vec<RouteInfo>>;
}

/// Implement StakingRouter
pub struct StakingRouterImpl {
    /// Cache cho routing info
    routing_cache: Arc<RwLock<Vec<RouteInfo>>>,
}

impl StakingRouterImpl {
    /// Tạo router mới
    pub fn new() -> Self {
        Self {
            routing_cache: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    /// Tính toán điểm tối ưu cho pool
    fn calculate_pool_score(pool: &StakePoolConfig, amount: U256, lock_time: u64) -> f64 {
        let apy_score = pool.current_apy / 100.0;
        let stake_score = if pool.total_staked > U256::zero() {
            amount.as_u128() as f64 / pool.total_staked.as_u128() as f64
        } else {
            1.0
        };
        let lock_score = if lock_time >= pool.min_lock_time {
            (lock_time - pool.min_lock_time) as f64 / (MAX_LOCK_TIME - pool.min_lock_time) as f64
        } else {
            0.0
        };
        
        // Trọng số cho các yếu tố
        let apy_weight = 0.5;
        let stake_weight = 0.3;
        let lock_weight = 0.2;
        
        apy_score * apy_weight + stake_score * stake_weight + lock_score * lock_weight
    }
}

#[async_trait]
impl StakingRouter for StakingRouterImpl {
    async fn find_optimal_pool(
        &self,
        user_address: Address,
        amount: U256,
        lock_time: u64,
    ) -> Result<RouteInfo> {
        let routing_cache = self.routing_cache.read().await;
        
        // Tìm pool có điểm số cao nhất
        let mut best_pool = None;
        let mut best_score = 0.0;
        
        for route in routing_cache.iter() {
            let score = Self::calculate_pool_score(
                &StakePoolConfig {
                    address: route.pool_address,
                    token_address: Address::zero(),
                    min_lock_time: route.suggested_lock_time,
                    current_apy: route.expected_apy,
                    total_staked: U256::zero(),
                    max_validators: MAX_VALIDATORS,
                },
                amount,
                lock_time,
            );
            
            if score > best_score {
                best_score = score;
                best_pool = Some(route.clone());
            }
        }
        
        if let Some(pool) = best_pool {
            Ok(pool)
        } else {
            Err(anyhow::anyhow!("No suitable pool found"))
        }
    }

    async fn update_routing_info(&self, pool_address: Address) -> Result<()> {
        // TODO: Implement logic cập nhật thông tin routing
        // 1. Lấy thông tin pool mới nhất
        // 2. Tính toán APY và thời gian lock tối ưu
        // 3. Cập nhật cache

        // Triển khai thực tế
        let mut routing_cache = self.routing_cache.write().await;
        
        // Tìm xem pool đã tồn tại trong cache chưa
        let pool_index = routing_cache.iter().position(|r| r.pool_address == pool_address);
        
        // Lấy thông tin pool từ blockchain hoặc database
        // Trong triển khai thực tế, cần gọi contract để lấy thông tin mới nhất
        let pool_info = self.fetch_pool_info_from_blockchain(pool_address).await?;
        
        let optimal_lock_time = self.calculate_optimal_lock_time(&pool_info);
        let expected_apy = self.calculate_expected_apy(&pool_info);
        let optimal_amount = self.calculate_optimal_stake_amount(&pool_info);
        
        let route_info = RouteInfo {
            pool_address,
            expected_apy,
            suggested_lock_time: optimal_lock_time,
            optimal_amount,
        };
        
        // Cập nhật cache
        if let Some(index) = pool_index {
            routing_cache[index] = route_info;
            info!("Updated routing info for pool: {:?}", pool_address);
        } else {
            routing_cache.push(route_info);
            info!("Added new routing info for pool: {:?}", pool_address);
        }
        
        Ok(())
    }

    /// Lấy thông tin pool từ blockchain
    async fn fetch_pool_info_from_blockchain(&self, pool_address: Address) -> Result<super::StakePoolConfig> {
        // TODO: Triển khai thực tế - gọi contract để lấy thông tin
        
        // Mock data cho development và testing
        let pool_info = super::StakePoolConfig {
            address: pool_address,
            token_address: Address::zero(), // Cần thay thế với địa chỉ thực tế
            min_lock_time: *super::constants::MIN_LOCK_TIME,
            current_apy: 10.0, // APY mặc định: 10%
            total_staked: U256::from(1_000_000_000_000_000_000u128), // 1 ETH
            max_validators: *super::constants::MAX_VALIDATORS,
        };
        
        Ok(pool_info)
    }
    
    /// Tính toán thời gian lock tối ưu
    fn calculate_optimal_lock_time(&self, pool: &super::StakePoolConfig) -> u64 {
        // Thuật toán đơn giản: chọn thời gian lock giữa min và max
        let min_lock_time = *super::constants::MIN_LOCK_TIME;
        let max_lock_time = *super::constants::MAX_LOCK_TIME;
        
        // Nếu APY cao, khuyến khích lock thời gian ngắn hơn
        // Nếu APY thấp, khuyến khích lock thời gian dài hơn
        let optimal_ratio = if pool.current_apy > 15.0 {
            0.3 // Chỉ 30% khoảng thời gian
        } else if pool.current_apy > 10.0 {
            0.5 // 50% khoảng thời gian
        } else {
            0.7 // 70% khoảng thời gian
        };
        
        let lock_range = max_lock_time - min_lock_time;
        min_lock_time + (lock_range as f64 * optimal_ratio) as u64
    }
    
    /// Tính toán APY dự kiến
    fn calculate_expected_apy(&self, pool: &super::StakePoolConfig) -> f64 {
        // Trong triển khai thực tế, cần tính toán APY dựa trên nhiều yếu tố:
        // - Lịch sử APY
        // - Độ biến động thị trường
        // - Trạng thái pool hiện tại
        
        // Đơn giản chỉ dùng current APY
        pool.current_apy
    }
    
    /// Tính toán số lượng token stake tối ưu
    fn calculate_optimal_stake_amount(&self, pool: &super::StakePoolConfig) -> U256 {
        // Trong triển khai thực tế, tính toán số lượng stake tối ưu dựa trên:
        // - Tổng số token đã stake
        // - Ảnh hưởng của việc thêm liquidity mới
        // - Phân phối rewards
        
        // Mặc định: đề xuất 1% tổng số token đã stake
        let default_optimal = U256::from(10_000_000_000_000_000_000u128); // 10 ETH
        
        if pool.total_staked == U256::zero() {
            return default_optimal;
        }
        
        let one_percent = pool.total_staked
            .checked_div(U256::from(100))
            .unwrap_or(default_optimal);
            
        // Không đề xuất quá 10% tổng stake
        let ten_percent = pool.total_staked
            .checked_div(U256::from(10))
            .unwrap_or(default_optimal);
            
        std::cmp::min(one_percent, ten_percent)
    }

    async fn get_recommended_pools(
        &self,
        user_address: Address,
        amount: U256,
    ) -> Result<Vec<RouteInfo>> {
        let routing_cache = self.routing_cache.read().await;
        
        // Sắp xếp pools theo điểm số
        let mut pools: Vec<RouteInfo> = routing_cache.clone();
        pools.sort_by(|a, b| {
            let score_a = Self::calculate_pool_score(
                &StakePoolConfig {
                    address: a.pool_address,
                    token_address: Address::zero(),
                    min_lock_time: a.suggested_lock_time,
                    current_apy: a.expected_apy,
                    total_staked: U256::zero(),
                    max_validators: MAX_VALIDATORS,
                },
                amount,
                a.suggested_lock_time,
            );
            
            let score_b = Self::calculate_pool_score(
                &StakePoolConfig {
                    address: b.pool_address,
                    token_address: Address::zero(),
                    min_lock_time: b.suggested_lock_time,
                    current_apy: b.expected_apy,
                    total_staked: U256::zero(),
                    max_validators: MAX_VALIDATORS,
                },
                amount,
                b.suggested_lock_time,
            );
            
            score_b.partial_cmp(&score_a).unwrap()
        });
        
        // Lấy top 3 pools
        Ok(pools.into_iter().take(3).collect())
    }
}
