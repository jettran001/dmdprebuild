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
pub mod notifications;

// Re-export của common::bridge_types để các module trong snipebot có thể sử dụng
pub use common::bridge_types;

// Re-export các types giao dịch từ common
pub use common::trading_types;

// Thêm re-export cho shared coordinator
pub use tradelogic::traits::{
    TradeCoordinator, ExecutorType, OpportunityPriority, SharedOpportunity, SharedOpportunityType, 
};
pub use tradelogic::coordinator::create_trade_coordinator;

// Re-export cấu trúc dữ liệu quan trọng từ config
pub use crate::config::initialize_default_config;

// Re-export các kiểu trader behavior để tránh trùng lặp
pub use crate::tradelogic::common::types::{
    TraderBehaviorType, TraderExpertiseLevel, GasBehavior, TraderBehaviorAnalysis
};

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OnceCell;

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

/// Application state containing main components of the SnipeBot
///
/// This struct serves as the central orchestration point for all major components of the bot,
/// including configuration, executors, chain adapters, and background tasks. It manages
/// the lifecycle of these components and provides methods to initialize, start, and stop
/// the services.
pub struct AppState {
    /// Global configuration manager that provides access to all bot settings
    /// and can reload configuration at runtime
    pub config: Arc<ConfigManager>,
    
    /// Health manager for monitoring system and service health
    /// Tracks the health of RPC connections, services, and other critical components
    pub health_manager: Arc<HealthManager>,
    
    /// Collection of blockchain adapters for each supported chain
    /// Provides standardized interfaces for interacting with different blockchains
    pub chain_adapters: Arc<Vec<Arc<EvmAdapter>>>,
    
    /// Collection of trade executors implementing different trading strategies
    /// Each executor follows the TradeExecutor trait and can be started/stopped independently
    pub trade_executors: Vec<Arc<dyn TradeExecutor<Config = config::BotConfig, TradeResult = tradelogic::common::types::TradeResult>>>,
    
    /// Channel for sending shutdown signals to gracefully terminate services
    /// Used by the stop() method to initiate shutdown sequence
    pub shutdown_tx: Option<oneshot::Sender<()>>,
    
    /// Handles to background tasks spawned by the application
    /// Allows tracking and potentially joining these tasks during shutdown
    pub background_tasks: Vec<JoinHandle<()>>,
}

/// Tạo một shared coordinator toàn cục dùng chung cho tất cả các module
static GLOBAL_COORDINATOR: OnceCell<Arc<dyn TradeCoordinator>> = OnceCell::const_new();

/// Khởi tạo global coordinator
/// 
/// Hàm này khởi tạo global coordinator nếu chưa được khởi tạo trước đó.
/// Nếu coordinator đã tồn tại, trả về instance hiện có.
/// Nếu khởi tạo thất bại, tạo một fallback coordinator.
/// 
/// # Returns
/// * `Arc<dyn TradeCoordinator>` - Instance của coordinator
pub async fn initialize_global_coordinator() -> Arc<dyn TradeCoordinator> {
    // Kiểm tra nếu coordinator đã được khởi tạo trước đó
    if let Some(coordinator) = GLOBAL_COORDINATOR.get() {
        return coordinator.clone();
    }

    // Khởi tạo coordinator nếu chưa có
    match GLOBAL_COORDINATOR.get_or_try_init(|| async {
        info!("Initializing global trade coordinator");
        
        // Tạo coordinator bằng factory function
        let coordinator = match tradelogic::coordinator::create_trade_coordinator() {
            Ok(coordinator) => {
                info!("Global trade coordinator initialized successfully");
                coordinator
            },
            Err(e) => {
                // Log lỗi chi tiết
                error!("Failed to initialize global trade coordinator: {}", e);
                error!("This may cause unexpected behavior in modules that depend on the coordinator");
                // Tạo fallback coordinator
                warn!("Creating fallback coordinator");
                tradelogic::coordinator::create_fallback_coordinator()
            }
        };
        
        Ok(coordinator)
    }).await {
        Ok(coordinator) => coordinator.clone(),
        Err(e) => {
            // Xử lý trường hợp lỗi khi khởi tạo
            error!("Critical error initializing global coordinator: {}", e);
            // Tạo fallback coordinator để tránh panic
            warn!("Using fallback coordinator instead");
            tradelogic::coordinator::create_fallback_coordinator()
        }
    }
}

/// Lấy global coordinator sẵn có hoặc khởi tạo mới nếu chưa có
/// 
/// Trả về Arc<dyn TradeCoordinator> đảm bảo luôn có một coordinator có thể sử dụng
pub async fn get_global_coordinator() -> Arc<dyn TradeCoordinator> {
    initialize_global_coordinator().await
}

impl AppState {
    /// Create a new application state with given configuration
    pub fn new(config_manager: Arc<ConfigManager>) -> Self {
        Self {
            config: config_manager,
            health_manager: HealthManager::get_instance(),
            chain_adapters: Arc::new(Vec::new()),
            trade_executors: Vec::new(),
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
            if chain_config.rpc_urls.is_empty() {
                warn!("No RPC URLs configured for chain {}, skipping health check", chain_id);
            } else {
                // Truy cập an toàn phần tử đầu tiên sử dụng .get(0)
                if let Some(rpc_url) = chain_config.rpc_urls.get(0) {
                    let adapter_clone = adapter.clone();
                    // Thay đổi cách cast không an toàn sang tạo wrapper mới
                    let health_check = Arc::new(RpcHealthCheck::new(
                        *chain_id,
                        &chain_config.name,
                        rpc_url,
                        adapter_clone,
                    ));
                    
                    self.health_manager.register(health_check).await;
                } else {
                    // Trường hợp này không nên xảy ra vì đã kiểm tra empty ở trên
                    // Nhưng vẫn xử lý để đảm bảo an toàn tuyệt đối
                    warn!("RPC URLs vector is unexpectedly empty for chain {}", chain_id);
                }
            }
            
            adapters.push(Arc::new(adapter));
        }
        
        self.chain_adapters = Arc::new(adapters);
        Ok(())
    }
    
    /// Initialize trade executors
    async fn init_trade_executors(&mut self, config: &BotConfig) -> Result<()> {
        // Initialize manual trader if enabled
        info!("Initializing trade executors");
        let trade_coordinator = get_global_coordinator().await;
        let manual_executor = ManualTradeExecutor::new(
            self.chain_adapters.clone(),
            config.trading.default_slippage,
            config.trading.default_deadline_minutes,
            trade_coordinator.clone(),
        );
        
        let manual_executor = Arc::new(manual_executor) as Arc<dyn TradeExecutor<Config = config::BotConfig, TradeResult = tradelogic::common::types::TradeResult>>;
        self.trade_executors.push(manual_executor);
        
        // Initialize smart trader if enabled
        if config.trading.enabled_strategies.contains(&"smart".to_string()) {
            info!("Initializing smart trade executor");
            let smart_executor = SmartTradeExecutor::new(
                self.chain_adapters.clone(),
                config.trading.smart_trade.clone(),
            ).await?;
            
            let smart_executor = Arc::new(smart_executor) as Arc<dyn TradeExecutor<Config = config::BotConfig, TradeResult = tradelogic::common::types::TradeResult>>;
            self.trade_executors.push(smart_executor);
        }
        
        // Initialize MEV bot if enabled
        if config.trading.enabled_strategies.contains(&"mev".to_string()) && config.trading.mev_trade.enabled {
            info!("Initializing MEV bot");
            let mev_bot = MevBot::new(
                self.chain_adapters.clone(),
                config.trading.mev_trade.clone(),
            ).await?;
            
            let mev_bot = Arc::new(mev_bot) as Arc<dyn TradeExecutor<Config = config::BotConfig, TradeResult = tradelogic::common::types::TradeResult>>;
            self.trade_executors.push(mev_bot);
        }
        
        Ok(())
    }
    
    /// Start all services
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting SnipeBot services");
        
        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);
        
        // Start all trade executors
        for executor in &self.trade_executors {
            executor.start().await?;
        }
        
        // Spawn shutdown handler
        let chain_adapters = self.chain_adapters.clone();
        let trade_executors = self.trade_executors.clone();
        
        let shutdown_task = tokio::spawn(async move {
            if let Err(e) = shutdown_rx.await {
                error!("Shutdown signal error: {}", e);
            }
            
            info!("Shutdown signal received, stopping services");
            
            // Stop all trade executors
            for executor in &trade_executors {
                if let Err(e) = executor.stop().await {
                    error!("Error stopping trade executor: {}", e);
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
        info!("Stopping SnipeBot application");
        
        // Clone self.shutdown_tx ra ngoài và lấy giá trị inside
        if let Some(ref tx) = self.shutdown_tx {
            // Clone tx để tránh việc move out of a shared reference
            let tx_clone = tx.clone();
            if let Err(_e) = tx_clone.send(()) {
                error!("Error sending shutdown signal: already shut down");
            }
        }
        
        // Wait for a short delay to allow tasks to clean up
        tokio::time::sleep(Duration::from_millis(500)).await;
        
        info!("SnipeBot stopped successfully");
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

/// Initialize API server
pub async fn init_api_server(app_state: Arc<AppState>) -> Result<()> {
    use warp::Filter;
    
    let config = app_state.config.get_config().await;
    
    // Return early if API is disabled
    if !config.api.enable_api {
        info!("API server is disabled in config");
        return Ok(());
    }
    
    info!("Initializing API server on port {}", config.api.port);
    
    // Clone app_state for warp filters
    let app_state_filter = warp::any().map(move || app_state.clone());
    
    // Set up routes
    let mut api_routes = Vec::new();
    
    // Add health check route
    let health_route = warp::path("health")
        .and(warp::get())
        .and_then(move || {
            let health = app_state.health_manager.clone();
            async move {
                match health.get_health().await {
                    health => Ok::<_, warp::Rejection>(warp::reply::json(&health)),
                }
            }
        });
    
    api_routes.push(Box::new(health_route) as Box<dyn warp::Filter<Extract = (warp::reply::Json,), Error = warp::Rejection> + Send + Sync>);
    
    // Add bugs and analysis routes if health::add_bugs_and_analysis_routes is available
    if let Ok(()) = health::add_bugs_and_analysis_routes(&mut api_routes) {
        debug!("Added bugs and analysis routes");
    }
    
    // Initialize metrics route if enabled
    if config.general.enable_metrics {
        let metrics_route = warp::path("metrics")
            .and(warp::get())
            .and_then(metric::metrics_handler);
            
        api_routes.push(Box::new(metrics_route) as Box<dyn warp::Filter<Extract = (warp::reply::Json,), Error = warp::Rejection> + Send + Sync>);
    }
    
    // Initialize other routes
    // ...
    
    // Combine all routes
    let routes = api_routes.into_iter()
        .fold(
            warp::any()
                .map(|| warp::reply::with_status("Not Found", warp::http::StatusCode::NOT_FOUND))
                .boxed()
                .map(|reply| reply as warp::reply::Json), 
            |acc, route| acc.or(route).boxed()
        );
    
    // Configure CORS
    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST", "OPTIONS"])
        .allow_headers(vec!["Content-Type", "Authorization"]);
    
    // Apply CORS and start the server
    let port = config.api.port;
    let server = warp::serve(routes.with(cors))
        .run(([0, 0, 0, 0], port));
    
    tokio::spawn(async move {
        if let Err(e) = server.await {
            error!("API server error: {}", e);
        }
    });
    
    info!("API server initialized and running on port {}", port);
    Ok(())
}
