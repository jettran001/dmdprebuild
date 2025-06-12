/// Configuration module for SnipeBot
///
/// This module defines the configuration structures used throughout the SnipeBot
/// application, including chain configurations, API keys, trading strategies, etc.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use serde::{Serialize, Deserialize};
use tokio::sync::RwLock;
use anyhow::{Result, Context, bail};
use once_cell::sync::Lazy;
use tracing::{info, warn, error};

use crate::types::{ChainType, TradeType};

/// Global configuration instance
pub static CONFIG: Lazy<Arc<RwLock<BotConfig>>> = Lazy::new(|| {
    let config = BotConfig::default();
    Arc::new(RwLock::new(config))
});

/// Main configuration structure for the SnipeBot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    /// General bot settings
    pub general: GeneralConfig,
    
    /// Chain-specific configurations
    pub chains: HashMap<u32, ChainConfig>,
    
    /// Trading strategies configuration
    pub trading: TradingConfig,
    
    /// Risk management settings
    pub risk: RiskConfig,
    
    /// API configurations
    pub api: ApiConfig,
    
    /// Advanced settings
    pub advanced: AdvancedConfig,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            chains: HashMap::new(),
            trading: TradingConfig::default(),
            risk: RiskConfig::default(),
            api: ApiConfig::default(),
            advanced: AdvancedConfig::default(),
        }
    }
}

/// General bot settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// Bot name/identifier
    pub bot_name: String,
    
    /// Log level (error, warn, info, debug, trace)
    pub log_level: String,
    
    /// Enable metrics collection
    pub enable_metrics: bool,
    
    /// Maximum number of concurrent trades
    pub max_concurrent_trades: usize,
    
    /// Default chain ID to use
    pub default_chain_id: u32,
    
    /// Cache configuration
    pub cache: CacheConfig,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            bot_name: "DiamondChain SnipeBot".to_string(),
            log_level: "info".to_string(),
            enable_metrics: true,
            max_concurrent_trades: 5,
            default_chain_id: 1, // Ethereum mainnet
            cache: CacheConfig::default(),
        }
    }
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Cache type (redis, memory)
    pub cache_type: String,
    
    /// Redis URL (if using redis)
    pub redis_url: Option<String>,
    
    /// Maximum memory cache items
    pub memory_max_items: usize,
    
    /// Default TTL in seconds
    pub default_ttl: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            cache_type: "memory".to_string(),
            redis_url: None,
            memory_max_items: 10000,
            default_ttl: 300, // 5 minutes
        }
    }
}

/// Chain-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// Chain ID
    pub chain_id: u32,
    
    /// Chain name
    pub name: String,
    
    /// Chain type
    pub chain_type: ChainType,
    
    /// RPC endpoints
    pub rpc_urls: Vec<String>,
    
    /// Websocket endpoints
    pub ws_urls: Option<Vec<String>>,
    
    /// Block explorer URL
    pub explorer_url: String,
    
    /// Block explorer API key
    pub explorer_api_key: Option<String>,
    
    /// Native token symbol
    pub native_token: String,
    
    /// Native token decimals
    pub native_decimals: u8,
    
    /// Default DEX router address
    pub default_router: String,
    
    /// Alternative DEX routers
    pub alt_routers: HashMap<String, String>,
    
    /// Default base tokens for trading
    pub base_tokens: Vec<String>,
    
    /// Gas price strategy
    pub gas_strategy: GasStrategy,
    
    /// Is this chain active
    pub active: bool,
}

/// Gas pricing strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasStrategy {
    /// Type of strategy (fixed, percentage, dynamic)
    pub strategy_type: String,
    
    /// Fixed gas price in gwei (if fixed)
    pub fixed_gas_gwei: Option<f64>,
    
    /// Percentage above base gas price (if percentage)
    pub percentage_above_base: Option<f64>,
    
    /// Use EIP-1559 (if supported by chain)
    pub use_eip1559: bool,
    
    /// Priority fee for EIP-1559 in gwei
    pub priority_fee_gwei: Option<f64>,
    
    /// Maximum gas price willing to pay in gwei
    pub max_gas_gwei: f64,
}

impl Default for GasStrategy {
    fn default() -> Self {
        Self {
            strategy_type: "percentage".to_string(),
            fixed_gas_gwei: None,
            percentage_above_base: Some(10.0), // 10% above base
            use_eip1559: true,
            priority_fee_gwei: Some(1.5),
            max_gas_gwei: 300.0,
        }
    }
}

/// Trading configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingConfig {
    /// Default slippage percentage
    pub default_slippage: f64,
    
    /// Default deadline in minutes
    pub default_deadline_minutes: u32,
    
    /// Auto-approve tokens before trading
    pub auto_approve: bool,
    
    /// Enabled trading strategies
    pub enabled_strategies: Vec<String>,
    
    /// Smart trade configuration
    pub smart_trade: SmartTradeConfig,
    
    /// MEV trading configuration
    pub mev_trade: MevTradeConfig,
    
    /// Trade size limits
    pub trade_limits: TradeLimits,
}

impl Default for TradingConfig {
    fn default() -> Self {
        Self {
            default_slippage: 1.0, // 1%
            default_deadline_minutes: 5,
            auto_approve: true,
            enabled_strategies: vec!["manual".to_string(), "smart".to_string()],
            smart_trade: SmartTradeConfig::default(),
            mev_trade: MevTradeConfig::default(),
            trade_limits: TradeLimits::default(),
        }
    }
}

/// Smart trade configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartTradeConfig {
    /// Enable trailing stop loss
    pub enable_trailing_stop_loss: bool,
    
    /// Default trailing stop percentage
    pub default_trailing_stop_percent: f64,
    
    /// Enable auto take profit
    pub enable_auto_take_profit: bool,
    
    /// Default take profit percentage
    pub default_take_profit_percent: f64,
    
    /// Auto-sell on security issues
    pub auto_sell_on_security_issues: bool,
    
    /// Maximum hold time in hours
    pub max_hold_time_hours: Option<u32>,
    
    /// Retry failed transactions
    pub retry_failed_tx: bool,
    
    /// Maximum retry attempts
    pub max_retry_attempts: u32,
}

impl Default for SmartTradeConfig {
    fn default() -> Self {
        Self {
            enable_trailing_stop_loss: true,
            default_trailing_stop_percent: 5.0,
            enable_auto_take_profit: true,
            default_take_profit_percent: 50.0,
            auto_sell_on_security_issues: true,
            max_hold_time_hours: Some(24),
            retry_failed_tx: true,
            max_retry_attempts: 3,
        }
    }
}

/// MEV trading configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MevTradeConfig {
    /// Enable MEV trading
    pub enabled: bool,
    
    /// Minimum profit in USD to execute
    pub min_profit_usd: f64,
    
    /// Opportunity types to monitor
    pub opportunity_types: Vec<String>,
    
    /// Execution methods allowed
    pub execution_methods: Vec<String>,
    
    /// Private RPC endpoints for MEV
    pub private_rpcs: HashMap<u32, String>,
}

impl Default for MevTradeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_profit_usd: 10.0,
            opportunity_types: vec!["arbitrage".to_string(), "sandwich".to_string()],
            execution_methods: vec!["standard".to_string(), "private".to_string()],
            private_rpcs: HashMap::new(),
        }
    }
}

/// Trade size limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeLimits {
    /// Minimum trade size in USD
    pub min_trade_usd: f64,
    
    /// Maximum trade size in USD
    pub max_trade_usd: f64,
    
    /// Maximum percentage of portfolio per trade
    pub max_portfolio_percent: f64,
}

impl Default for TradeLimits {
    fn default() -> Self {
        Self {
            min_trade_usd: 10.0,
            max_trade_usd: 1000.0,
            max_portfolio_percent: 10.0,
        }
    }
}

/// Risk management configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    /// Maximum risk score for tokens (0-100)
    pub max_risk_score: u8,
    
    /// Auto-reject tokens with issues
    pub auto_reject_dangerous: bool,
    
    /// Maximum acceptable buy tax
    pub max_buy_tax: f64,
    
    /// Maximum acceptable sell tax
    pub max_sell_tax: f64,
    
    /// Minimum liquidity in USD
    pub min_liquidity_usd: f64,
    
    /// Required security checks
    pub required_checks: Vec<String>,
    
    /// Stop trading on consecutive losses
    pub stop_on_consecutive_losses: Option<u32>,
    
    /// Maximum daily loss in USD
    pub max_daily_loss_usd: Option<f64>,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_risk_score: 50,
            auto_reject_dangerous: true,
            max_buy_tax: 10.0,
            max_sell_tax: 10.0,
            min_liquidity_usd: 10000.0,
            required_checks: vec![
                "honeypot".to_string(),
                "ownership".to_string(),
                "tax".to_string(),
                "liquidity".to_string(),
            ],
            stop_on_consecutive_losses: Some(3),
            max_daily_loss_usd: Some(100.0),
        }
    }
}

/// API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    /// Enable API server
    pub enable_api: bool,
    
    /// API server port
    pub port: u16,
    
    /// API key for authentication
    pub api_key: Option<String>,
    
    /// Enable JWT authentication
    pub use_jwt: bool,
    
    /// JWT secret
    pub jwt_secret: Option<String>,
    
    /// CORS allowed origins
    pub cors_origins: Vec<String>,
    
    /// Rate limiting (requests per minute)
    pub rate_limit: Option<u32>,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            enable_api: true,
            port: 3000,
            api_key: None,
            use_jwt: false,
            jwt_secret: None,
            cors_origins: vec!["*".to_string()],
            rate_limit: Some(100),
        }
    }
}

/// Advanced configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedConfig {
    /// Enable debug mode
    pub debug_mode: bool,
    
    /// Simulate transactions (don't send to blockchain)
    pub simulation_mode: bool,
    
    /// Custom contract addresses
    pub contract_addresses: HashMap<String, String>,
    
    /// External API configurations
    pub external_apis: ExternalApiConfig,
    
    /// Thread pool size
    pub thread_pool_size: Option<usize>,
    
    /// Custom node operators
    pub node_operators: Vec<String>,
}

impl Default for AdvancedConfig {
    fn default() -> Self {
        Self {
            debug_mode: false,
            simulation_mode: false,
            contract_addresses: HashMap::new(),
            external_apis: ExternalApiConfig::default(),
            thread_pool_size: None,
            node_operators: Vec::new(),
        }
    }
}

/// External API configurations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalApiConfig {
    /// Coingecko API key
    pub coingecko_api_key: Option<String>,
    
    /// Etherscan API key
    pub etherscan_api_key: Option<String>,
    
    /// Go+ API key
    pub goplus_api_key: Option<String>,
    
    /// Other API keys
    pub other_apis: HashMap<String, String>,
}

impl Default for ExternalApiConfig {
    fn default() -> Self {
        Self {
            coingecko_api_key: None,
            etherscan_api_key: None,
            goplus_api_key: None,
            other_apis: HashMap::new(),
        }
    }
}

/// Configuration manager for handling config loading/saving
pub struct ConfigManager {
    /// Current configuration
    config: RwLock<BotConfig>,
    
    /// Configuration file path
    config_path: String,
}

impl ConfigManager {
    /// Create a new configuration manager
    pub fn new(config_path: &str) -> Self {
        Self {
            config: RwLock::new(BotConfig::default()),
            config_path: config_path.to_string(),
        }
    }
    
    /// Load configuration from file
    pub async fn load(&self) -> Result<()> {
        let path = Path::new(&self.config_path);
        
        if !path.exists() {
            info!("Configuration file not found, using default configuration");
            return Ok(());
        }
        
        let config_content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read configuration file: {}", self.config_path))?;
        
        let config: BotConfig = serde_yaml::from_str(&config_content)
            .context("Failed to parse configuration file")?;
        
        *self.config.write().await = config;
        info!("Configuration loaded from {}", self.config_path);
        
        Ok(())
    }
    
    /// Save configuration to file
    pub async fn save(&self) -> Result<()> {
        let config = self.config.read().await;
        let config_yaml = serde_yaml::to_string(&*config)
            .context("Failed to serialize configuration")?;
        
        let path = Path::new(&self.config_path);
        
        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory: {:?}", parent))?;
            }
        }
        
        fs::write(path, config_yaml)
            .with_context(|| format!("Failed to write configuration to file: {}", self.config_path))?;
        
        info!("Configuration saved to {}", self.config_path);
        Ok(())
    }
    
    /// Get the current configuration
    pub async fn get_config(&self) -> BotConfig {
        self.config.read().await.clone()
    }
    
    /// Update the configuration
    pub async fn update_config(&self, config: BotConfig) -> Result<()> {
        *self.config.write().await = config;
        self.save().await?;
        Ok(())
    }
    
    /// Get a specific chain configuration
    pub async fn get_chain_config(&self, chain_id: u32) -> Result<ChainConfig> {
        let config = self.config.read().await;
        
        config.chains.get(&chain_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Chain configuration not found for chain ID {}", chain_id))
    }
    
    /// Update a specific chain configuration
    pub async fn update_chain_config(&self, chain_config: ChainConfig) -> Result<()> {
        let chain_id = chain_config.chain_id;
        let mut config = self.config.write().await;
        
        config.chains.insert(chain_id, chain_config);
        drop(config);
        
        self.save().await?;
        Ok(())
    }
    
    /// Get configuration as singleton
    pub fn get_instance() -> Arc<Self> {
        static INSTANCE: tokio::sync::OnceCell<Arc<ConfigManager>> = tokio::sync::OnceCell::const_new();
        
        INSTANCE.get_or_init(|| async {
            Arc::new(ConfigManager::new("config/bot_config.yaml"))
        }).unwrap_or_else(|e| {
            // Log error but don't panic
            error!("Failed to initialize config manager: {}, using a new instance", e);
            Arc::new(ConfigManager::new("config/bot_config.yaml"))
        })
    }
}

/// Create a default Ethereum chain configuration
pub fn default_ethereum_config() -> ChainConfig {
    ChainConfig {
        chain_id: 1,
        name: "Ethereum".to_string(),
        chain_type: ChainType::EVM(1),
        rpc_urls: vec!["https://eth.llamarpc.com".to_string()],
        ws_urls: Some(vec!["wss://eth.llamarpc.com".to_string()]),
        explorer_url: "https://etherscan.io".to_string(),
        explorer_api_key: None,
        native_token: "ETH".to_string(),
        native_decimals: 18,
        default_router: "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D".to_string(), // Uniswap V2
        alt_routers: {
            let mut routers = HashMap::new();
            routers.insert("UniswapV3".to_string(), "0xE592427A0AEce92De3Edee1F18E0157C05861564".to_string());
            routers.insert("Sushiswap".to_string(), "0xd9e1cE17f2641f24aE83637ab66a2cca9C378B9F".to_string());
            routers
        },
        base_tokens: vec![
            "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string(), // WETH
            "0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string(), // USDT
            "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(), // USDC
        ],
        gas_strategy: GasStrategy::default(),
        active: true,
    }
}

/// Create a default BSC chain configuration
pub fn default_bsc_config() -> ChainConfig {
    ChainConfig {
        chain_id: 56,
        name: "BSC".to_string(),
        chain_type: ChainType::EVM(56),
        rpc_urls: vec!["https://bsc-dataseed.binance.org".to_string()],
        ws_urls: Some(vec!["wss://bsc-ws-node.nariox.org:443".to_string()]),
        explorer_url: "https://bscscan.com".to_string(),
        explorer_api_key: None,
        native_token: "BNB".to_string(),
        native_decimals: 18,
        default_router: "0x10ED43C718714eb63d5aA57B78B54704E256024E".to_string(), // PancakeSwap V2
        alt_routers: {
            let mut routers = HashMap::new();
            routers.insert("BiSwap".to_string(), "0x3a6d8cA21D1CF76F653A67577FA0D27453350dD8".to_string());
            routers
        },
        base_tokens: vec![
            "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c".to_string(), // WBNB
            "0xe9e7CEA3DedcA5984780Bafc599bD69ADd087D56".to_string(), // BUSD
            "0x55d398326f99059fF775485246999027B3197955".to_string(), // USDT
        ],
        gas_strategy: GasStrategy {
            strategy_type: "fixed".to_string(),
            fixed_gas_gwei: Some(5.0),
            percentage_above_base: None,
            use_eip1559: false,
            priority_fee_gwei: None,
            max_gas_gwei: 10.0,
        },
        active: true,
    }
}

/// Initialize default configuration with common chains
pub fn initialize_default_config() -> BotConfig {
    let mut config = BotConfig::default();
    
    // Add Ethereum config
    let eth_config = default_ethereum_config();
    config.chains.insert(eth_config.chain_id, eth_config);
    
    // Add BSC config
    let bsc_config = default_bsc_config();
    config.chains.insert(bsc_config.chain_id, bsc_config);
    
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_config() {
        let config = BotConfig::default();
        assert_eq!(config.general.bot_name, "DiamondChain SnipeBot");
        assert_eq!(config.general.default_chain_id, 1);
        assert_eq!(config.trading.default_slippage, 1.0);
    }
    
    #[test]
    fn test_initialize_default_config() {
        let config = initialize_default_config();
        assert!(config.chains.contains_key(&1)); // Ethereum
        assert!(config.chains.contains_key(&56)); // BSC
        
        if let Some(eth_config) = config.chains.get(&1) {
            assert_eq!(eth_config.name, "Ethereum");
            assert_eq!(eth_config.native_token, "ETH");
        } else {
            panic!("Ethereum config should be present");
        }
        
        if let Some(bsc_config) = config.chains.get(&56) {
            assert_eq!(bsc_config.name, "BSC");
            assert_eq!(bsc_config.native_token, "BNB");
        } else {
            panic!("BSC config should be present");
        }
    }
}
