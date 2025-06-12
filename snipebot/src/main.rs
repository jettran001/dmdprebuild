/// DiamondChain SnipeBot - Main entry point
///
/// This file contains the main function and serves as the entry point
/// for the DiamondChain SnipeBot application.

use std::sync::Arc;
use anyhow::{Result, Context};
use clap::{Parser, Subcommand};
use tokio::signal;
use tracing::{info, error};

// Sử dụng các API được re-export từ lib.rs thay vì truy cập trực tiếp các module con
use snipebot::{
    AppState,
    init_logging,
    greeting,
    config::ConfigManager,
    config::initialize_default_config,
    chain_adapters::evm_adapter::EvmAdapter,
    get_global_coordinator
};

/// Command line arguments
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Config file path
    #[arg(short, long, default_value = "config/bot_config.yaml")]
    config: String,
    
    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,
    
    /// Subcommand
    #[command(subcommand)]
    command: Option<Commands>,
}

/// Subcommands
#[derive(Subcommand)]
enum Commands {
    /// Generate default configuration
    Init,
    
    /// Run the bot
    Run,
    
    /// Run API server only
    Api,
    
    /// Test a connection to the specified chain
    TestChain {
        /// Chain ID to test
        #[arg(short, long)]
        chain_id: u32,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let cli = Cli::parse();
    
    // Initialize logging
    init_logging(&cli.log_level)?;
    
    // Show greeting
    println!("{}", greeting());
    
    // Create config manager
    let config_manager = Arc::new(ConfigManager::new(&cli.config));
    
    // Khởi tạo global coordinator sớm để đảm bảo tính nhất quán
    let _coordinator = get_global_coordinator().await;
    
    // Process commands
    match cli.command.unwrap_or(Commands::Run) {
        Commands::Init => {
            // Generate default configuration
            init_config(config_manager).await?;
        }
        Commands::Run => {
            // Run the bot
            run_bot(config_manager).await?;
        }
        Commands::Api => {
            // Run API server only
            run_api_server(config_manager).await?;
        }
        Commands::TestChain { chain_id } => {
            // Test chain connection
            test_chain_connection(config_manager, chain_id).await?;
        }
    }
    
    Ok(())
}

/// Initialize configuration with defaults
async fn init_config(config_manager: Arc<ConfigManager>) -> Result<()> {
    info!("Initializing default configuration at {}", config_manager.config_path);
    
    // Generate default config - sử dụng hàm đã re-export
    let default_config = initialize_default_config();
    
    // Save to file
    config_manager.update_config(default_config).await?;
    
    info!("Default configuration generated successfully");
    info!("You can now edit the configuration file and run the bot");
    
    Ok(())
}

/// Run the bot
async fn run_bot(config_manager: Arc<ConfigManager>) -> Result<()> {
    // Load configuration
    config_manager.load().await?;
    
    // Create app state
    let mut app_state = AppState::new(config_manager);
    
    // Initialize and start the bot
    app_state.init().await?;
    app_state.start().await?;
    
    info!("Bot started successfully");
    
    // Wait for shutdown signal
    wait_for_shutdown().await;
    
    // Stop the bot
    app_state.stop().await?;
    
    info!("Bot stopped successfully");
    Ok(())
}

/// Run API server only
async fn run_api_server(config_manager: Arc<ConfigManager>) -> Result<()> {
    // Load configuration
    config_manager.load().await?;
    let config = config_manager.get_config().await;
    
    // Check if API is enabled
    if !config.api.enable_api {
        return Err(anyhow::anyhow!("API is disabled in configuration"));
    }
    
    // Create app state for API server
    let mut app_state = AppState::new(config_manager);
    
    // Initialize app state
    app_state.init().await?;
    
    // Initialize API server
    info!("Starting API server on port {}", config.api.port);
    snipebot::init_api_server(Arc::new(app_state)).await?;
    
    info!("API server started. Press Ctrl+C to stop");
    wait_for_shutdown().await;
    
    info!("API server stopped");
    Ok(())
}

/// Test chain connection
async fn test_chain_connection(config_manager: Arc<ConfigManager>, chain_id: u32) -> Result<()> {
    // Load configuration
    config_manager.load().await?;
    
    // Get chain configuration
    let chain_config = config_manager.get_chain_config(chain_id).await
        .context(format!("Chain ID {} not found in configuration", chain_id))?;
    
    info!("Testing connection to chain {} ({})", chain_config.name, chain_id);
    
    // Create EVM adapter
    let adapter = EvmAdapter::new(chain_id, chain_config);
    
    // Initialize adapter
    adapter.init().await?;
    
    // Test connection by getting latest block
    let block_number = adapter.get_latest_block().await?;
    let chain_id_from_node = adapter.get_chain_id().await?;
    let gas_price = adapter.get_gas_price().await?;
    
    info!("Connection successful!");
    info!("Latest block: {}", block_number);
    info!("Chain ID from node: {}", chain_id_from_node);
    info!("Gas price: {} gwei", gas_price / 1_000_000_000);
    
    Ok(())
}

/// Wait for shutdown signal (Ctrl+C)
async fn wait_for_shutdown() {
    match signal::ctrl_c().await {
        Ok(()) => info!("Shutdown signal received"),
        Err(e) => error!("Failed to listen for shutdown signal: {}", e),
    }
}
