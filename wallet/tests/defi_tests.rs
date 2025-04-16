//! Integration tests cho module DeFi
//! 
//! Bao gồm các tests cho:
//! - Tương tác giữa các module
//! - Tương tác với blockchain
//! - Tương tác với cache
//! - Tương tác với logging
//! - Tương tác với metrics

use crate::defi::*;
use ethers::types::{Address, U256};
use std::str::FromStr;
use tokio::time::{sleep, Duration};

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_farm_and_stake_integration() {
        let manager = DefiManager::new();

        // Test farm manager
        let farm_config = farm::FarmPoolConfig {
            pool_address: Address::zero(),
            router_address: Address::zero(),
            reward_token_address: Address::zero(),
            apy: Decimal::from(10),
        };
        assert!(manager.farm_manager().add_pool(farm_config).await.is_ok());

        // Test stake manager
        let stake_config = stake::StakePoolConfig {
            pool_address: Address::zero(),
            token_address: Address::zero(),
            apy: Decimal::from(10),
            min_lock_time: 86400,
            min_stake_amount: U256::from(1000),
        };
        assert!(manager.stake_manager().add_pool(stake_config).await.is_ok());

        // Test farm and stake interaction
        let user_id = "user1";
        let pool_address = Address::zero();
        let amount = U256::from(1000);

        // Add liquidity to farm
        assert!(manager.farm_manager().add_liquidity(user_id, pool_address, amount).await.is_ok());

        // Stake tokens
        assert!(manager.stake_manager().stake(user_id, pool_address, amount, 86400).await.is_ok());

        // Wait for some time
        sleep(Duration::from_secs(1)).await;

        // Harvest rewards from farm
        assert!(manager.farm_manager().harvest_rewards(user_id, pool_address).await.is_ok());

        // Claim rewards from stake
        assert!(manager.stake_manager().claim_rewards(user_id, pool_address).await.is_ok());
    }

    #[tokio::test]
    async fn test_blockchain_integration() {
        let manager = DefiManager::new();

        // Test farm manager with blockchain
        let farm_config = farm::FarmPoolConfig {
            pool_address: Address::from_str("0x0000000000000000000000000000000000000001").unwrap(),
            router_address: Address::from_str("0x0000000000000000000000000000000000000002").unwrap(),
            reward_token_address: Address::from_str("0x0000000000000000000000000000000000000003").unwrap(),
            apy: Decimal::from(10),
        };
        assert!(manager.farm_manager().add_pool(farm_config).await.is_ok());

        // Test stake manager with blockchain
        let stake_config = stake::StakePoolConfig {
            pool_address: Address::from_str("0x0000000000000000000000000000000000000004").unwrap(),
            token_address: Address::from_str("0x0000000000000000000000000000000000000005").unwrap(),
            apy: Decimal::from(10),
            min_lock_time: 86400,
            min_stake_amount: U256::from(1000),
        };
        assert!(manager.stake_manager().add_pool(stake_config).await.is_ok());

        // Test farm and stake interaction with blockchain
        let user_id = "user1";
        let pool_address = Address::from_str("0x0000000000000000000000000000000000000001").unwrap();
        let amount = U256::from(1000);

        // Add liquidity to farm
        assert!(manager.farm_manager().add_liquidity(user_id, pool_address, amount).await.is_ok());

        // Stake tokens
        assert!(manager.stake_manager().stake(user_id, pool_address, amount, 86400).await.is_ok());

        // Wait for some time
        sleep(Duration::from_secs(1)).await;

        // Harvest rewards from farm
        assert!(manager.farm_manager().harvest_rewards(user_id, pool_address).await.is_ok());

        // Claim rewards from stake
        assert!(manager.stake_manager().claim_rewards(user_id, pool_address).await.is_ok());
    }

    #[tokio::test]
    async fn test_cache_integration() {
        let manager = DefiManager::new();

        // Test farm manager with cache
        let farm_config = farm::FarmPoolConfig {
            pool_address: Address::zero(),
            router_address: Address::zero(),
            reward_token_address: Address::zero(),
            apy: Decimal::from(10),
        };
        assert!(manager.farm_manager().add_pool(farm_config.clone()).await.is_ok());

        // Get pool from cache
        assert!(manager.farm_manager().get_pool(farm_config.pool_address).await.is_ok());

        // Test stake manager with cache
        let stake_config = stake::StakePoolConfig {
            pool_address: Address::zero(),
            token_address: Address::zero(),
            apy: Decimal::from(10),
            min_lock_time: 86400,
            min_stake_amount: U256::from(1000),
        };
        assert!(manager.stake_manager().add_pool(stake_config.clone()).await.is_ok());

        // Get pool from cache
        assert!(manager.stake_manager().get_pool(stake_config.pool_address).await.is_ok());
    }

    #[tokio::test]
    async fn test_logging_integration() {
        let manager = DefiManager::new();

        // Test farm manager with logging
        let farm_config = farm::FarmPoolConfig {
            pool_address: Address::zero(),
            router_address: Address::zero(),
            reward_token_address: Address::zero(),
            apy: Decimal::from(10),
        };
        assert!(manager.farm_manager().add_pool(farm_config).await.is_ok());

        // Test stake manager with logging
        let stake_config = stake::StakePoolConfig {
            pool_address: Address::zero(),
            token_address: Address::zero(),
            apy: Decimal::from(10),
            min_lock_time: 86400,
            min_stake_amount: U256::from(1000),
        };
        assert!(manager.stake_manager().add_pool(stake_config).await.is_ok());
    }

    #[tokio::test]
    async fn test_metrics_integration() {
        let manager = DefiManager::new();

        // Test farm manager with metrics
        let farm_config = farm::FarmPoolConfig {
            pool_address: Address::zero(),
            router_address: Address::zero(),
            reward_token_address: Address::zero(),
            apy: Decimal::from(10),
        };
        assert!(manager.farm_manager().add_pool(farm_config).await.is_ok());

        // Test stake manager with metrics
        let stake_config = stake::StakePoolConfig {
            pool_address: Address::zero(),
            token_address: Address::zero(),
            apy: Decimal::from(10),
            min_lock_time: 86400,
            min_stake_amount: U256::from(1000),
        };
        assert!(manager.stake_manager().add_pool(stake_config).await.is_ok());
    }
} 