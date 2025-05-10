// External imports
use ethers::types::{Address, U256};
use rust_decimal::Decimal;
use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug, warn, error};
use async_trait::async_trait;

// Internal imports
use crate::defi::blockchain::{BlockchainProvider, BlockchainConfig, BlockchainType};
use crate::defi::chain::ChainId;
use crate::defi::error::DefiError;

// Re-export các provider từ blockchain crate
pub use blockchain::farm::farm_logic::FarmManager as BlockchainFarmManager;
pub use blockchain::stake::stake_logic::StakeManager as BlockchainStakeManager;
pub use blockchain::farm::farm_logic::FarmPoolConfig as BlockchainFarmPoolConfig;
pub use blockchain::stake::stake_logic::StakePoolConfig as BlockchainStakePoolConfig;

/// Cấu hình cho farming pool
#[derive(Debug, Clone)]
pub struct FarmPoolConfig {
    pub pool_address: Address,
    pub router_address: Address,
    pub reward_token_address: Address,
    pub apy: Decimal,
}

impl From<FarmPoolConfig> for BlockchainFarmPoolConfig {
    fn from(config: FarmPoolConfig) -> Self {
        Self {
            pool_address: config.pool_address,
            router_address: config.router_address,
            reward_token_address: config.reward_token_address,
            apy: config.apy,
        }
    }
}

/// Cấu hình cho staking pool
#[derive(Debug, Clone)]
pub struct StakePoolConfig {
    pub pool_address: Address,
    pub token_address: Address,
    pub min_lock_time: u64,
    pub base_apy: Decimal,
    pub bonus_apy: Decimal,
}

impl From<StakePoolConfig> for BlockchainStakePoolConfig {
    fn from(config: StakePoolConfig) -> Self {
        Self {
            pool_address: config.pool_address,
            token_address: config.token_address,
            min_lock_time: config.min_lock_time,
            base_apy: config.base_apy,
            bonus_apy: config.bonus_apy,
        }
    }
}

/// Provider DeFi trait cung cấp các phương thức cho farming và staking
#[async_trait]
pub trait DefiProvider: Send + Sync + 'static {
    /// Trả về chain ID
    fn chain_id(&self) -> ChainId;
    
    /// Trả về loại blockchain
    fn blockchain_type(&self) -> BlockchainType;
    
    /// Thêm một farming pool mới
    async fn add_farm_pool(&self, config: FarmPoolConfig) -> Result<()>;
    
    /// Thêm liquidity vào pool
    async fn add_liquidity(&self, user_id: &str, pool_address: Address, amount: U256) -> Result<String>;
    
    /// Rút liquidity từ pool
    async fn remove_liquidity(&self, user_id: &str, pool_address: Address, amount: U256) -> Result<String>;
    
    /// Lấy số dư liquidity của user trong pool
    async fn get_liquidity_balance(&self, user_id: &str, pool_address: Address) -> Result<U256>;
    
    /// Lấy reward của user trong pool
    async fn get_farm_rewards(&self, user_id: &str, pool_address: Address) -> Result<U256>;
    
    /// Claim rewards từ farming
    async fn claim_farm_rewards(&self, user_id: &str, pool_address: Address) -> Result<String>;
    
    /// Thêm một staking pool mới
    async fn add_stake_pool(&self, config: StakePoolConfig) -> Result<()>;
    
    /// Stake tokens vào pool
    async fn stake(&self, user_id: &str, pool_address: Address, amount: U256, lock_time: u64) -> Result<String>;
    
    /// Unstake tokens từ pool
    async fn unstake(&self, user_id: &str, pool_address: Address, amount: U256) -> Result<String>;
    
    /// Lấy số dư stake của user trong pool
    async fn get_staked_balance(&self, user_id: &str, pool_address: Address) -> Result<U256>;
    
    /// Lấy reward của user trong pool
    async fn get_stake_rewards(&self, user_id: &str, pool_address: Address) -> Result<U256>;
    
    /// Claim rewards từ staking
    async fn claim_stake_rewards(&self, user_id: &str, pool_address: Address) -> Result<String>;
    
    /// Đồng bộ hóa thông tin pools từ blockchain
    async fn sync_pools(&self) -> Result<()>;
}

/// Provider DeFi tích hợp với blockchain
pub struct DefiProviderImpl {
    /// Blockchain provider
    blockchain_provider: Box<dyn BlockchainProvider>,
    /// Farm manager
    farm_manager: Arc<BlockchainFarmManager>,
    /// Stake manager
    stake_manager: Arc<BlockchainStakeManager>,
}

impl DefiProviderImpl {
    /// Tạo một provider mới
    pub fn new(chain_id: ChainId) -> Result<Self, DefiError> {
        let config = BlockchainConfig::new(chain_id);
        
        // Tạo blockchain provider
        let blockchain_provider = crate::defi::blockchain::BlockchainProviderFactory::create_provider(config)
            .await
            .map_err(|e| DefiError::ProviderError(format!("Failed to create blockchain provider: {}", e)))?;
        
        // Tạo farm manager và stake manager
        let farm_manager = Arc::new(BlockchainFarmManager::new());
        let stake_manager = Arc::new(BlockchainStakeManager::new());
        
        Ok(Self {
            blockchain_provider,
            farm_manager,
            stake_manager,
        })
    }
    
    /// Trả về reference đến FarmManager
    pub fn farm_manager(&self) -> &BlockchainFarmManager {
        &self.farm_manager
    }
    
    /// Trả về reference đến StakeManager
    pub fn stake_manager(&self) -> &BlockchainStakeManager {
        &self.stake_manager
    }
}

#[async_trait]
impl DefiProvider for DefiProviderImpl {
    fn chain_id(&self) -> ChainId {
        self.blockchain_provider.chain_id()
    }
    
    fn blockchain_type(&self) -> BlockchainType {
        self.blockchain_provider.blockchain_type()
    }
    
    async fn add_farm_pool(&self, config: FarmPoolConfig) -> Result<()> {
        let blockchain_config = config.into();
        self.farm_manager.add_pool(blockchain_config).await?;
        Ok(())
    }
    
    async fn add_liquidity(&self, user_id: &str, pool_address: Address, amount: U256) -> Result<String> {
        self.farm_manager.add_liquidity(user_id, pool_address, amount).await
    }
    
    async fn remove_liquidity(&self, user_id: &str, pool_address: Address, amount: U256) -> Result<String> {
        self.farm_manager.remove_liquidity(user_id, pool_address, amount).await
    }
    
    async fn get_liquidity_balance(&self, user_id: &str, pool_address: Address) -> Result<U256> {
        self.farm_manager.get_liquidity_balance(user_id, pool_address).await
    }
    
    async fn get_farm_rewards(&self, user_id: &str, pool_address: Address) -> Result<U256> {
        self.farm_manager.get_rewards(user_id, pool_address).await
    }
    
    async fn claim_farm_rewards(&self, user_id: &str, pool_address: Address) -> Result<String> {
        self.farm_manager.claim_rewards(user_id, pool_address).await
    }
    
    async fn add_stake_pool(&self, config: StakePoolConfig) -> Result<()> {
        let blockchain_config = config.into();
        self.stake_manager.add_pool(blockchain_config).await?;
        Ok(())
    }
    
    async fn stake(&self, user_id: &str, pool_address: Address, amount: U256, lock_time: u64) -> Result<String> {
        self.stake_manager.stake(user_id, pool_address, amount, lock_time).await
    }
    
    async fn unstake(&self, user_id: &str, pool_address: Address, amount: U256) -> Result<String> {
        self.stake_manager.unstake(user_id, pool_address, amount).await
    }
    
    async fn get_staked_balance(&self, user_id: &str, pool_address: Address) -> Result<U256> {
        self.stake_manager.get_staked_balance(user_id, pool_address).await
    }
    
    async fn get_stake_rewards(&self, user_id: &str, pool_address: Address) -> Result<U256> {
        self.stake_manager.get_rewards(user_id, pool_address).await
    }
    
    async fn claim_stake_rewards(&self, user_id: &str, pool_address: Address) -> Result<String> {
        self.stake_manager.claim_rewards(user_id, pool_address).await
    }
    
    async fn sync_pools(&self) -> Result<()> {
        // Đồng bộ hóa cả farm và stake pools
        self.farm_manager.sync_pools().await?;
        self.stake_manager.sync_pools().await?;
        
        info!("Đã đồng bộ hóa tất cả các pools");
        Ok(())
    }
}

// Factory cho DefiProvider
pub struct DefiProviderFactory;

impl DefiProviderFactory {
    /// Tạo provider mới
    pub async fn create_provider(chain_id: ChainId) -> Result<Box<dyn DefiProvider>, DefiError> {
        let provider = DefiProviderImpl::new(chain_id)?;
        Ok(Box::new(provider))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_defi_provider() {
        let provider_result = DefiProviderImpl::new(ChainId::EthereumMainnet);
        assert!(provider_result.is_ok(), "Khởi tạo DefiProviderImpl thất bại: {:?}", provider_result.err());
        let provider = provider_result.unwrap();
        // Test farm manager
        let farm_config = FarmPoolConfig {
            pool_address: Address::zero(),
            router_address: Address::zero(),
            reward_token_address: Address::zero(),
            apy: Decimal::from(10),
        };
        assert!(provider.add_farm_pool(farm_config).await.is_ok());
        // Test stake manager
        let stake_config = StakePoolConfig {
            pool_address: Address::zero(),
            token_address: Address::zero(),
            min_lock_time: 86400,
            base_apy: Decimal::from(10),
            bonus_apy: Decimal::from(5),
        };
        assert!(provider.add_stake_pool(stake_config).await.is_ok());
    }
} 