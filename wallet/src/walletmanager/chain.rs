use anyhow::Context;
use ethers::providers::{Http, Provider};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::WalletError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainType {
    EVM,
    Solana,
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
    fn get_provider(&self, chain_id: u64) -> Result<Provider<Http>, WalletError>;
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
    fn get_provider(&self, chain_id: u64) -> Result<Provider<Http>, WalletError> {
        let providers = self.providers.blocking_read();
        match providers.get(&chain_id) {
            Some(provider) => Ok(provider.clone()),
            None => {
                let configs = self.configs.blocking_read();
                if let Some(config) = configs.get(&chain_id) {
                    drop(configs);
                    let provider = Provider::<Http>::try_from(&config.rpc_url)
                        .context(format!("Failed to create provider for chain ID {}", chain_id))
                        .map_err(|e| WalletError::ProviderError(e.to_string()))?;
                    
                    let mut providers = self.providers.blocking_write();
                    providers.insert(chain_id, provider.clone());
                    Ok(provider)
                } else {
                    Err(WalletError::ChainNotSupported(format!("Chain ID {} not supported", chain_id)))
                }
            }
        }
    }

    async fn add_chain(&mut self, config: ChainConfig) -> Result<(), WalletError> {
        // Kiểm tra trước rồi mới thêm để tránh tạo provider không cần thiết
        {
            let configs = self.configs.read().await;
            if configs.contains_key(&config.chain_id) {
                return Ok(());
            }
        }

        // Thử tạo provider để đảm bảo RPC URL hợp lệ
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChainType {
    EVM,
    Solana,
    TON,
    NEAR,
    Stellar,
    Sui,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    pub chain_id: u64,
    pub chain_type: ChainType,
    pub name: String,
    pub rpc_url: String,
    pub native_token: String,
}

#[derive(Debug, Clone)]
pub struct ChainManager {
    chains: HashMap<u64, ChainConfig>,
    providers: HashMap<u64, Provider<Http>>,
}

impl ChainManager {
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
    fn test_add_chain() {
        let mut manager = ChainManager::new();
        let config = ChainConfig {
            chain_id: 999,
            chain_type: ChainType::EVM,
            name: "Test Chain".to_string(),
            rpc_url: "https://test-rpc.com".to_string(),
            native_token: "TEST".to_string(),
        };
        assert!(manager.add_chain(config.clone()).is_ok());
        assert_eq!(manager.get_chain_config(999).unwrap(), &config);
    }

    #[test]
    fn test_list_chains() {
        let manager = ChainManager::new();
        let chains = manager.list_chains();
        assert!(!chains.is_empty());
    }
}