//! Module tích hợp với các blockchain khác nhau cho DeFi
//! 
//! Module này hỗ trợ tích hợp với các blockchain sau:
//! - Ethereum (mainnet, các testnet)
//! - BSC (Binance Smart Chain)
//! - Polygon
//! - Arbitrum
//! - Avalanche
//! - Solana
//! - Near
//! - Optimism (mới)
//! - Base (mới)
//! - zkSync Era (mới)
//! - Cosmos (mới)
//! - Tron (mới)
//! - Hedera (mới)
//! - Diamond (mới)
//! 
//! ## Flow:
//! ```
//! DeFi -> Blockchain Provider -> Blockchain Network
//! ```
//! 
//! ## Ví dụ:
//! ```
//! use wallet::defi::blockchain::{get_provider, ChainId};
//! use ethers::providers::Provider;
//! 
//! #[tokio::main]
//! async fn main() {
//!     let provider = get_provider(ChainId::EthereumMainnet).unwrap();
//!     let block_number = provider.get_block_number().await.unwrap();
//!     println!("Current block number: {}", block_number);
//! }
//! ```

use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use std::collections::HashMap;
use tokio::sync::RwLock;
use ethers::prelude::{Provider, Http, Middleware, JsonRpcClient};
use ethers::types::{Address, U256, H256, Chain as EthersChain};
use once_cell::sync::Lazy;
use anyhow::Result;
use serde::{Serialize, Deserialize};
use thiserror::Error;
use tracing::{info, warn, error, debug};

use super::chain::ChainId;
use super::error::DefiError;
use super::constants::BLOCKCHAIN_TIMEOUT;

/// Cache cho các provider
static PROVIDER_CACHE: Lazy<RwLock<HashMap<ChainId, Arc<Provider<Http>>>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

/// Lấy provider cho một chain
///
/// # Arguments
/// * `chain_id` - Chain ID muốn lấy provider
///
/// # Returns
/// * `Result<Arc<Provider<Http>>, DefiError>` - Provider cho chain hoặc lỗi
///
/// # Examples
/// ```
/// use wallet::defi::blockchain::{get_provider, ChainId};
///
/// #[tokio::main]
/// async fn main() {
///     let provider = get_provider(ChainId::EthereumMainnet).unwrap();
///     let block_number = provider.get_block_number().await.unwrap();
///     println!("Current block number: {}", block_number);
/// }
/// ```
pub fn get_provider(chain_id: ChainId) -> Result<Arc<Provider<Http>>, DefiError> {
    if !chain_id.is_evm() {
        return Err(DefiError::ChainNotSupported(chain_id.to_string()));
    }

    // Thử lấy từ cache trước
    {
        let cache = PROVIDER_CACHE.blocking_read();
        if let Some(provider) = cache.get(&chain_id) {
            return Ok(provider.clone());
        }
    }

    // Nếu không có trong cache, tạo mới
    let rpc_url = chain_id.default_rpc_url();
    let provider = Provider::<Http>::try_from(rpc_url)
        .map_err(|e| DefiError::ProviderError(format!("Failed to create provider: {}", e)))?;
    
    // Thiết lập timeout
    let provider = provider.interval(Duration::from_millis(BLOCKCHAIN_TIMEOUT));

    let provider = Arc::new(provider);

    // Lưu vào cache
    {
        let mut cache = PROVIDER_CACHE.blocking_write();
        cache.insert(chain_id, provider.clone());
    }

    Ok(provider)
}

/// Loại blockchain
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlockchainType {
    /// EVM-compatible blockchains (Ethereum, BSC, Polygon, etc.)
    Evm,
    /// Solana blockchain
    Solana,
    /// Near blockchain
    Near,
    /// Tron blockchain
    Tron,
    /// Diamond blockchain
    Diamond,
    /// Cosmos blockchain
    Cosmos,
    /// Hedera blockchain
    Hedera,
}

impl BlockchainType {
    /// Trả về loại blockchain dựa trên ChainId
    pub fn from_chain_id(chain_id: ChainId) -> Self {
        match chain_id {
            ChainId::SolanaMainnet | ChainId::SolanaDevnet | ChainId::SolanaTestnet => Self::Solana,
            ChainId::NearMainnet | ChainId::NearTestnet => Self::Near,
            ChainId::TronMainnet | ChainId::TronNile | ChainId::TronShasta => Self::Tron,
            ChainId::CosmosMainnet | ChainId::CosmosTestnet => Self::Cosmos,
            ChainId::HederaMainnet | ChainId::HederaTestnet => Self::Hedera,
            ChainId::DiamondMainnet | ChainId::DiamondTestnet => Self::Diamond,
            _ => Self::Evm,
        }
    }
}

/// Cấu hình blockchain provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainConfig {
    /// Chain ID
    pub chain_id: ChainId,
    /// RPC URL
    pub rpc_url: String,
    /// Timeout (ms)
    pub timeout_ms: u64,
    /// Số lần retry tối đa
    pub max_retries: u32,
}

impl BlockchainConfig {
    /// Tạo cấu hình mới với các giá trị mặc định
    pub fn new(chain_id: ChainId) -> Self {
        Self {
            chain_id,
            rpc_url: chain_id.default_rpc_url().to_string(),
            timeout_ms: BLOCKCHAIN_TIMEOUT,
            max_retries: super::constants::MAX_RETRIES,
        }
    }

    /// Thiết lập RPC URL
    pub fn with_rpc_url(mut self, rpc_url: &str) -> Self {
        self.rpc_url = rpc_url.to_string();
        self
    }

    /// Thiết lập timeout
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Thiết lập số lần retry tối đa
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }
}

/// Trait cho BlockchainProvider
pub trait BlockchainProvider: Send + Sync {
    /// Trả về chain ID
    fn chain_id(&self) -> ChainId;
    
    /// Trả về loại blockchain
    fn blockchain_type(&self) -> BlockchainType;
    
    /// Kiểm tra kết nối
    async fn is_connected(&self) -> Result<bool, DefiError>;
    
    /// Lấy số block hiện tại
    async fn get_block_number(&self) -> Result<u64, DefiError>;
    
    /// Lấy số dư của một địa chỉ
    async fn get_balance(&self, address: &str) -> Result<U256, DefiError>;
}

/// EVM Provider
pub struct EvmProvider {
    /// Cấu hình
    pub config: BlockchainConfig,
    /// Provider
    provider: Arc<Provider<Http>>,
}

impl EvmProvider {
    /// Tạo provider mới
    pub fn new(config: BlockchainConfig) -> Result<Self, DefiError> {
        if !config.chain_id.is_evm() {
            return Err(DefiError::ChainNotSupported(config.chain_id.to_string()));
        }

        let provider = Provider::<Http>::try_from(config.rpc_url.as_str())
            .map_err(|e| DefiError::ProviderError(format!("Failed to create provider: {}", e)))?;
        
        // Thiết lập timeout
        let provider = provider.interval(Duration::from_millis(config.timeout_ms));

        Ok(Self {
            config,
            provider: Arc::new(provider),
        })
    }

    /// Trả về provider gốc
    pub fn raw_provider(&self) -> Arc<Provider<Http>> {
        self.provider.clone()
    }
}

impl BlockchainProvider for EvmProvider {
    fn chain_id(&self) -> ChainId {
        self.config.chain_id
    }

    fn blockchain_type(&self) -> BlockchainType {
        BlockchainType::Evm
    }

    async fn is_connected(&self) -> Result<bool, DefiError> {
        match self.provider.get_block_number().await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn get_block_number(&self) -> Result<u64, DefiError> {
        self.provider.get_block_number().await
            .map(|bn| bn.as_u64())
            .map_err(|e| DefiError::ProviderError(format!("Failed to get block number: {}", e)))
    }

    async fn get_balance(&self, address: &str) -> Result<U256, DefiError> {
        let address = address.parse::<Address>()
            .map_err(|e| DefiError::InvalidConfig(format!("Invalid address: {}", e)))?;

        self.provider.get_balance(address, None).await
            .map_err(|e| DefiError::ProviderError(format!("Failed to get balance: {}", e)))
    }
}

/// Factory để tạo provider phù hợp với từng blockchain
pub struct BlockchainProviderFactory;

impl BlockchainProviderFactory {
    /// Tạo provider phù hợp với blockchain type
    pub async fn create_provider(config: BlockchainConfig) -> Result<Box<dyn BlockchainProvider>, DefiError> {
        let blockchain_type = BlockchainType::from_chain_id(config.chain_id);
        
        match blockchain_type {
            BlockchainType::Evm => {
                let provider = EvmProvider::new(config)?;
                Ok(Box::new(provider))
            },
            BlockchainType::Solana => {
                // Solana provider implementation
                use crate::defi::blockchain::non_evm::solana::create_solana_provider;
                create_solana_provider(config).await
            },
            BlockchainType::Near => {
                // Near provider implementation
                use crate::defi::blockchain::non_evm::near::create_near_provider;
                create_near_provider(config).await
            },
            BlockchainType::Tron => {
                // Tron provider implementation
                use crate::defi::blockchain::non_evm::tron::create_tron_provider;
                create_tron_provider(config).await
            },
            BlockchainType::Cosmos => {
                // Cosmos provider implementation
                use crate::defi::blockchain::non_evm::cosmos::create_cosmos_provider;
                create_cosmos_provider(config).await
            },
            BlockchainType::Hedera => {
                // Hedera provider implementation
                use crate::defi::blockchain::non_evm::hedera::create_hedera_provider;
                create_hedera_provider(config).await
            },
            BlockchainType::Diamond => {
                // Sử dụng Diamond provider đã triển khai
                use crate::defi::blockchain::non_evm::diamond::create_diamond_provider;
                create_diamond_provider(config).await
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_id() {
        assert_eq!(ChainId::EthereumMainnet.as_u64(), 1);
        assert_eq!(ChainId::BscMainnet.as_u64(), 56);
        assert_eq!(ChainId::OptimismMainnet.as_u64(), 10);
        assert_eq!(ChainId::BaseMainnet.as_u64(), 8453);
        assert_eq!(ChainId::ZkSyncMainnet.as_u64(), 324);
    }

    #[test]
    fn test_is_evm_compatible() {
        assert!(ChainId::EthereumMainnet.is_evm());
        assert!(ChainId::BscMainnet.is_evm());
        assert!(ChainId::OptimismMainnet.is_evm());
        assert!(!ChainId::SolanaMainnet.is_evm());
        assert!(!ChainId::TronMainnet.is_evm());
        assert!(!ChainId::CosmosMainnet.is_evm());
    }

    #[test]
    fn test_is_testnet() {
        assert!(!ChainId::EthereumMainnet.is_testnet());
        assert!(ChainId::EthereumGoerli.is_testnet());
        assert!(ChainId::OptimismGoerli.is_testnet());
        assert!(ChainId::ZkSyncTestnet.is_testnet());
    }

    // ... other tests ...
} 