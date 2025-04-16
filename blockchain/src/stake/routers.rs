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
        Ok(())
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
