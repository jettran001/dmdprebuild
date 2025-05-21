//! DiamondChain SnipeBot Library
//!
//! This is the main library crate for the DiamondChain SnipeBot DeFi trading bot.
//! It provides core functionality for trading on decentralized exchanges,
//! with advanced features for token analysis, risk management, and MEV strategies.

pub mod types;
pub mod config;
pub mod cache;
pub mod metric;
pub mod health;
pub mod chain_adapters;
pub mod tradelogic;
pub mod analys;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, Context};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::config::{BotConfig, ConfigManager};
use crate::tradelogic::traits::TradeExecutor;
use crate::tradelogic::manual_trade::ManualTradeExecutor;
use crate::tradelogic::smart_trade::executor::SmartTradeExecutor;
use crate::tradelogic::mev_logic::bot::MevBot;
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::health::{HealthManager, RpcHealthCheck};

/// Application state containing main components
pub struct AppState {
    /// Global configuration
    pub config: Arc<ConfigManager>,
    
    /// Health manager
    pub health_manager: Arc<HealthManager>,
    
    /// Chain adapters
    pub chain_adapters: Arc<Vec<Arc<EvmAdapter>>>,
    
    /// Manual trade executor
    pub manual_executor: Option<Arc<ManualTradeExecutor>>,
    
    /// Smart trade executor
    pub smart_executor: Option<Arc<SmartTradeExecutor>>,
    
    /// MEV bot
    pub mev_bot: Option<Arc<MevBot>>,
    
    /// Shutdown signal sender
    pub shutdown_tx: Option<oneshot::Sender<()>>,
    
    /// Background tasks
    pub background_tasks: Vec<JoinHandle<()>>,
}

impl AppState {
    /// Create a new application state with given configuration
    pub fn new(config_manager: Arc<ConfigManager>) -> Self {
        Self {
            config: config_manager,
            health_manager: HealthManager::get_instance(),
            chain_adapters: Arc::new(Vec::new()),
            manual_executor: None,
            smart_executor: None,
            mev_bot: None,
            shutdown_tx: None,
            background_tasks: Vec::new(),
        }
    }
    
    /// Initialize the application state
    pub async fn init(&mut self) -> Result<()> {
        // Load config
        info!("Initializing SnipeBot application");
        
        let config = self.config.get_config().await;
        debug!("Configuration loaded");
        
        // Initialize metrics
        let metrics_port = if config.general.enable_metrics {
            Some(9090) // Use a fixed port for metrics
        } else {
            None
        };
        
        metric::init_metrics(metrics_port).await?;
        debug!("Metrics initialized");
        
        // Initialize health manager
        self.health_manager.start_monitoring().await?;
        debug!("Health manager initialized");
        
        // Initialize chain adapters
        self.init_chain_adapters(&config).await?;
        debug!("Chain adapters initialized");
        
        // Initialize trade executors
        self.init_trade_executors(&config).await?;
        debug!("Trade executors initialized");
        
        info!("SnipeBot initialization complete");
        Ok(())
    }
    
    /// Initialize chain adapters
    async fn init_chain_adapters(&mut self, config: &BotConfig) -> Result<()> {
        let mut adapters = Vec::new();
        
        // Create and initialize adapters for each active chain
        for (chain_id, chain_config) in &config.chains {
            if !chain_config.active {
                continue;
            }
            
            info!("Initializing adapter for chain: {} (ID: {})", chain_config.name, chain_id);
            
            // Create the adapter
            let adapter = EvmAdapter::new(*chain_id, chain_config.clone());
            adapter.init().await?;
            
            // Register chain metrics
            metric::register_chain_metrics(*chain_id, &chain_config.name).await?;
            
            // Register health check for this chain
            let rpc_url = chain_config.rpc_urls.first()
                .ok_or_else(|| anyhow::anyhow!("No RPC URLs configured for chain {}", chain_id))?;
                
            let adapter_clone = adapter.clone();
            let health_check = Arc::new(RpcHealthCheck::new(
                *chain_id,
                &chain_config.name,
                rpc_url,
                adapter_clone as Arc<dyn crate::health::RpcAdapter>,
            ));
            
            self.health_manager.register(health_check).await;
            
            adapters.push(Arc::new(adapter));
        }
        
        self.chain_adapters = Arc::new(adapters);
        Ok(())
    }
    
    /// Initialize trade executors
    async fn init_trade_executors(&mut self, config: &BotConfig) -> Result<()> {
        // Initialize manual trader if enabled
        info!("Initializing trade executors");
        let manual_executor = ManualTradeExecutor::new(
            self.chain_adapters.clone(),
            config.trading.default_slippage,
            config.trading.default_deadline_minutes,
        );
        
        self.manual_executor = Some(Arc::new(manual_executor));
        
        // Initialize smart trader if enabled
        if config.trading.enabled_strategies.contains(&"smart".to_string()) {
            info!("Initializing smart trade executor");
            let smart_executor = SmartTradeExecutor::new(
                self.chain_adapters.clone(),
                config.trading.smart_trade.clone(),
            ).await?;
            
            self.smart_executor = Some(Arc::new(smart_executor));
        }
        
        // Initialize MEV bot if enabled
        if config.trading.enabled_strategies.contains(&"mev".to_string()) && config.trading.mev_trade.enabled {
            info!("Initializing MEV bot");
            let mev_bot = MevBot::new(
                self.chain_adapters.clone(),
                config.trading.mev_trade.clone(),
            ).await?;
            
            self.mev_bot = Some(Arc::new(mev_bot));
        }
        
        Ok(())
    }
    
    /// Start all services
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting SnipeBot services");
        
        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);
        
        // Start manual trade executor
        if let Some(executor) = &self.manual_executor {
            executor.start().await?;
        }
        
        // Start smart trade executor if enabled
        if let Some(executor) = &self.smart_executor {
            executor.start().await?;
        }
        
        // Start MEV bot if enabled
        if let Some(bot) = &self.mev_bot {
            bot.start().await?;
        }
        
        // Spawn shutdown handler
        let chain_adapters = self.chain_adapters.clone();
        let manual_executor = self.manual_executor.clone();
        let smart_executor = self.smart_executor.clone();
        let mev_bot = self.mev_bot.clone();
        
        let shutdown_task = tokio::spawn(async move {
            if let Err(e) = shutdown_rx.await {
                error!("Shutdown signal error: {}", e);
            }
            
            info!("Shutdown signal received, stopping services");
            
            // Stop MEV bot
            if let Some(bot) = mev_bot {
                if let Err(e) = bot.stop().await {
                    error!("Error stopping MEV bot: {}", e);
                }
            }
            
            // Stop smart trade executor
            if let Some(executor) = smart_executor {
                if let Err(e) = executor.stop().await {
                    error!("Error stopping smart trade executor: {}", e);
                }
            }
            
            // Stop manual trade executor
            if let Some(executor) = manual_executor {
                if let Err(e) = executor.stop().await {
                    error!("Error stopping manual trade executor: {}", e);
                }
            }
            
            // Allow some time for graceful shutdown
            tokio::time::sleep(Duration::from_secs(2)).await;
            
            info!("Shutdown complete");
        });
        
        self.background_tasks.push(shutdown_task);
        
        info!("All services started");
        Ok(())
    }
    
    /// Stop all services
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping SnipeBot services");
        
        // Send shutdown signal
        if let Some(shutdown_tx) = &self.shutdown_tx {
            if let Err(e) = shutdown_tx.send(()) {
                error!("Error sending shutdown signal: {}", e);
            }
        }
        
        Ok(())
    }
}

/// Initialize logging
pub fn init_logging(log_level: &str) -> Result<()> {
    let level = match log_level.to_lowercase().as_str() {
        "trace" => tracing::Level::TRACE,
        "debug" => tracing::Level::DEBUG,
        "info" => tracing::Level::INFO,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => tracing::Level::INFO,
    };
    
    // Initialize tracing subscriber
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(false)
        .finish();
        
    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set global default subscriber")?;
        
    info!("Logging initialized at {} level", log_level);
    Ok(())
}

/// Version information
pub mod version {
    /// Current version from Cargo.toml
    pub const VERSION: &str = env!("CARGO_PKG_VERSION");
    
    /// Build timestamp
    pub const BUILD_TIMESTAMP: &str = env!("VERGEN_BUILD_TIMESTAMP");
    
    /// Git SHA
    pub const GIT_SHA: &str = env!("VERGEN_GIT_SHA");
    
    /// Get full version string
    pub fn full_version() -> String {
        format!("{} ({})", VERSION, GIT_SHA)
    }
}

/// Get a greeting message with version info
pub fn greeting() -> String {
    format!(
        "DiamondChain SnipeBot v{} starting up",
        version::VERSION
    )
}
