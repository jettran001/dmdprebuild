use std::sync::Arc;
use sqlx::postgres::PgPoolOptions;
use ethers::types::Address;

mod processor;
mod sdk;
mod migrations;

use processor::bridge_orchestrator::BridgeOrchestrator;
// Giả định có implement cho các client này
use processor::layerzero::LayerZeroClientImpl;
use processor::wormhole::WormholeClientImpl;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Thiết lập logging
    env_logger::init();
    
    // Kết nối database
    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;
    
    // Chạy migrations để thiết lập database
    log::info!("Running database migrations...");
    migrations::run_migrations(&pool).await?;
    log::info!("Migrations completed successfully");
    
    // Khởi tạo clients
    let layerzero_client = Arc::new(LayerZeroClientImpl::new(
        std::env::var("LAYERZERO_API_KEY").expect("LAYERZERO_API_KEY must be set")
    ));
    
    let wormhole_client = Arc::new(WormholeClientImpl::new(
        std::env::var("WORMHOLE_API_URL").expect("WORMHOLE_API_URL must be set")
    ));
    
    // Khởi tạo bridge orchestrator
    let bridge_orchestrator = Arc::new(
        BridgeOrchestrator::new(
            &std::env::var("BSC_WS_URL").expect("BSC_WS_URL must be set"),
            &std::env::var("NEAR_RPC_URL").expect("NEAR_RPC_URL must be set"),
            &std::env::var("SOLANA_RPC_URL").expect("SOLANA_RPC_URL must be set"),
            std::env::var("BSC_BRIDGE_CONTRACT").expect("BSC_BRIDGE_CONTRACT must be set").parse::<Address>()?,
            std::env::var("BSC_TOKEN_CONTRACT").expect("BSC_TOKEN_CONTRACT must be set").parse::<Address>()?,
            layerzero_client,
            wormhole_client,
            pool.clone(),
        ).await?
    );
    
    // Khởi tạo API server
    let app = sdk::api::bridge_routes(bridge_orchestrator.clone());
    
    // Khởi chạy service lắng nghe events trên các chain
    let orchestrator_clone = bridge_orchestrator.clone();
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
        .await?;
    
    Ok(())
} 