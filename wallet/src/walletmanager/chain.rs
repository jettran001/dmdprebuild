// External imports
use ethers::providers::{Http, Provider};
use serde::{Deserialize, Serialize};

// Standard library imports
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// Internal imports
use crate::error::WalletError;

// Third party imports
use anyhow::Context;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChainType {
    EVM,
    Solana,
    TON,
    NEAR,
    Stellar,
    Sui,
    BTC,
}

pub struct ChainConfig {
    pub chain_id: u64,
    pub chain_type: ChainType,
    pub name: String,
    pub rpc_url: String,
    pub native_token: String,
}

pub trait ChainManager: Send + Sync + 'static {
    async fn get_provider(&self, chain_id: u64) -> Result<Provider<Http>, WalletError>;
    async fn add_chain(&mut self, config: ChainConfig) -> Result<(), WalletError>;
}

#[derive(Debug)]
pub struct DefaultChainManager {
    providers: Arc<RwLock<HashMap<u64, Provider<Http>>>>,
    configs: Arc<RwLock<HashMap<u64, ChainConfig>>>,
}

impl DefaultChainManager {
    pub fn new() -> Self {
        Self {
            providers: Arc::new(RwLock::new(HashMap::new())),
            configs: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl ChainManager for DefaultChainManager {
    async fn get_provider(&self, chain_id: u64) -> Result<Provider<Http>, WalletError> {
        let providers = self.providers.read().await;
        if let Some(provider) = providers.get(&chain_id) {
            return Ok(provider.clone());
        }
        
        let configs = self.configs.read().await;
        if let Some(config) = configs.get(&chain_id) {
            drop(configs);
            let provider = Provider::<Http>::try_from(&config.rpc_url)
                .context(format!("Failed to create provider for chain ID {}", chain_id))
                .map_err(|e| WalletError::ProviderError(e.to_string()))?;
            
            let mut providers = self.providers.write().await;
            providers.insert(chain_id, provider.clone());
            Ok(provider)
        } else {
            Err(WalletError::ChainNotSupported(format!("Chain ID {} not supported", chain_id)))
        }
    }

    async fn add_chain(&mut self, config: ChainConfig) -> Result<(), WalletError> {
        let configs = self.configs.read().await;
        if configs.contains_key(&config.chain_id) {
            return Ok(());
        }

        let provider = Provider::<Http>::try_from(&config.rpc_url)
            .context(format!("Failed to create provider for chain ID {}", config.chain_id))
            .map_err(|e| WalletError::ProviderError(e.to_string()))?;

        let mut providers = self.providers.write().await;
        providers.insert(config.chain_id, provider);

        let mut configs = self.configs.write().await;
        configs.insert(config.chain_id, config);

        Ok(())
    }
}

/// Phiên bản đơn giản của ChainManager không sử dụng async
#[derive(Debug, Clone)]
pub struct SimpleChainManager {
    chains: HashMap<u64, ChainConfig>,
    providers: HashMap<u64, Provider<Http>>,
}

impl SimpleChainManager {
    pub fn new() -> Self {
        let mut chains = HashMap::new();
        let mut providers = HashMap::new();

        let default_chains = vec![
            ChainConfig {
                chain_id: 1,
                chain_type: ChainType::EVM,
                name: "Ethereum Mainnet".to_string(),
                rpc_url: "https://mainnet.infura.io/v3/YOUR_INFURA_KEY".to_string(),
                native_token: "ETH".to_string(),
            },
            ChainConfig {
                chain_id: 7562605,
                chain_type: ChainType::Solana,
                name: "Solana Mainnet".to_string(),
                rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
                native_token: "SOL".to_string(),
            },
        ];

        for config in default_chains {
            if let Ok(provider) = Provider::<Http>::try_from(&config.rpc_url) {
                providers.insert(config.chain_id, provider);
            }
            chains.insert(config.chain_id, config);
        }

        Self { chains, providers }
    }

    pub fn add_chain(&mut self, config: ChainConfig) -> Result<(), WalletError> {
        if self.chains.contains_key(&config.chain_id) {
            return Err(WalletError::ChainNotSupported(format!("Chain ID {} exists", config.chain_id)));
        }
        let provider = Provider::<Http>::try_from(&config.rpc_url)
            .map_err(|e| WalletError::ProviderError(e.to_string()))?;
        self.chains.insert(config.chain_id, config.clone());
        self.providers.insert(config.chain_id, provider);
        Ok(())
    }

    pub fn get_provider(&self, chain_id: u64) -> Result<&Provider<Http>, WalletError> {
        self.providers
            .get(&chain_id)
            .ok_or(WalletError::ChainNotSupported(format!("Chain ID {}", chain_id)))
    }

    pub fn get_chain_config(&self, chain_id: u64) -> Result<&ChainConfig, WalletError> {
        self.chains
            .get(&chain_id)
            .ok_or(WalletError::ChainNotSupported(format!("Chain ID {}", chain_id)))
    }

    pub fn list_chains(&self) -> Vec<&ChainConfig> {
        self.chains.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_get_chain() {
        let mut manager = SimpleChainManager::new();
        let config = ChainConfig {
            chain_id: 999,
            chain_type: ChainType::EVM,
            name: "Test Chain".to_string(),
            rpc_url: "https://test.rpc".to_string(),
            native_token: "TEST".to_string(),
        };
        
        assert!(manager.add_chain(config.clone()).is_ok());
        
        match manager.get_chain_config(999) {
            Ok(retrieved_config) => {
                assert_eq!(retrieved_config.chain_id, config.chain_id);
                assert_eq!(retrieved_config.name, config.name);
            },
            Err(_) => panic!("Should be able to retrieve chain config"),
        }
    }
}