use std::sync::Arc;
use sqlx::postgres::PgPoolOptions;
use ethers::types::Address;
use anyhow::{Context, Result};

mod processor;
mod sdk;
mod migrations;

use processor::bridge_orchestrator::BridgeOrchestrator;
// Giả định có implement cho các client này
use processor::layerzero::LayerZeroClientImpl;
use processor::wormhole::WormholeClientImpl;

/// Default values for configuration khi không có biến môi trường
const DEFAULT_DB_URL: &str = "postgres://postgres:postgres@localhost:5432/bridge";
const DEFAULT_API_URL: &str = "https://api.wormhole.com";
const DEFAULT_API_KEY: &str = "test_api_key";
const DEFAULT_RPC_URL: &str = "http://localhost:8545";
const DEFAULT_CONTRACT_ADDR: &str = "0x0000000000000000000000000000000000000000";

#[tokio::main]
async fn main() -> Result<()> {
    // Thiết lập logging
    env_logger::init();
    
    // Kết nối database với xử lý lỗi
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|e| {
            log::warn!("DATABASE_URL not set ({}), using default: {}", e, DEFAULT_DB_URL);
            DEFAULT_DB_URL.to_string()
        });
    
    log::info!("Connecting to database...");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .context("Failed to connect to database")?;
    
    // Chạy migrations để thiết lập database
    log::info!("Running database migrations...");
    migrations::run_migrations(&pool).await
        .context("Failed to run database migrations")?;
    log::info!("Migrations completed successfully");
    
    // Khởi tạo clients với xử lý lỗi
    let layerzero_api_key = std::env::var("LAYERZERO_API_KEY")
        .unwrap_or_else(|e| {
            log::warn!("LAYERZERO_API_KEY not set ({}), using default key (not secure for production)", e);
            DEFAULT_API_KEY.to_string()
        });
    
    let layerzero_client = Arc::new(LayerZeroClientImpl::new(layerzero_api_key));
    log::info!("LayerZero client initialized");
    
    let wormhole_api_url = std::env::var("WORMHOLE_API_URL")
        .unwrap_or_else(|e| {
            log::warn!("WORMHOLE_API_URL not set ({}), using default URL: {}", e, DEFAULT_API_URL);
            DEFAULT_API_URL.to_string()
        });
    
    let wormhole_client = Arc::new(WormholeClientImpl::new(wormhole_api_url));
    log::info!("Wormhole client initialized");
    
    // Khởi tạo bridge orchestrator với xử lý lỗi
    let bsc_ws_url = std::env::var("BSC_WS_URL")
        .unwrap_or_else(|e| {
            log::warn!("BSC_WS_URL not set ({}), using default URL: {}", e, DEFAULT_RPC_URL);
            DEFAULT_RPC_URL.to_string()
        });
    
    let near_rpc_url = std::env::var("NEAR_RPC_URL")
        .unwrap_or_else(|e| {
            log::warn!("NEAR_RPC_URL not set ({}), using default URL: {}", e, DEFAULT_RPC_URL);
            DEFAULT_RPC_URL.to_string()
        });
    
    let solana_rpc_url = std::env::var("SOLANA_RPC_URL")
        .unwrap_or_else(|e| {
            log::warn!("SOLANA_RPC_URL not set ({}), using default URL: {}", e, DEFAULT_RPC_URL);
            DEFAULT_RPC_URL.to_string()
        });
    
    let bsc_bridge_contract = std::env::var("BSC_BRIDGE_CONTRACT")
        .unwrap_or_else(|e| {
            log::warn!("BSC_BRIDGE_CONTRACT not set ({}), using default address: {}", e, DEFAULT_CONTRACT_ADDR);
            DEFAULT_CONTRACT_ADDR.to_string()
        })
        .parse::<Address>()
        .context("Failed to parse BSC bridge contract address")?;
    
    let bsc_token_contract = std::env::var("BSC_TOKEN_CONTRACT")
        .unwrap_or_else(|e| {
            log::warn!("BSC_TOKEN_CONTRACT not set ({}), using default address: {}", e, DEFAULT_CONTRACT_ADDR);
            DEFAULT_CONTRACT_ADDR.to_string()
        })
        .parse::<Address>()
        .context("Failed to parse BSC token contract address")?;
    
    log::info!("Initializing bridge orchestrator...");
    let bridge_orchestrator = Arc::new(
        BridgeOrchestrator::new(
            &bsc_ws_url,
            &near_rpc_url,
            &solana_rpc_url,
            bsc_bridge_contract,
            bsc_token_contract,
            layerzero_client,
            wormhole_client,
            pool.clone(),
        ).await
        .context("Failed to initialize bridge orchestrator")?
    );
    log::info!("Bridge orchestrator initialized successfully");
    
    // Khởi tạo API server
    log::info!("Setting up API routes...");
    let app = sdk::api::bridge_routes(bridge_orchestrator.clone());
    
    // Khởi chạy service lắng nghe events trên các chain
    let orchestrator_clone = bridge_orchestrator.clone();
    log::info!("Starting BSC event listener...");
    tokio::spawn(async move {
        if let Err(e) = orchestrator_clone.listen_bsc_events().await {
            log::error!("Error listening to BSC events: {}", e);
        }
    });
    
    // Khởi chạy API server
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 3000));
    log::info!("API server running on http://{}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .context("API server failed")?;
    
    Ok(())
} 