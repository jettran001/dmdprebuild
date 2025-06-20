use network::api::logs_api;
use network::api::DefaultNetworkApi;
use network::config::NetworkConfig;
use network::core::engine::{NetworkEngine, DefaultNetworkEngine};
// use network::logs;
use network::plugins::ws::{WarpWebSocketService, WebSocketService};
use std::net::IpAddr;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    // 1. Khởi tạo logging
    DefaultNetworkEngine::init_logging();

    // 2. Load config
    let config = match NetworkConfig::load("configs/config.yaml") {
        Ok(config) => config,
        Err(e) => {
            error!(target: "network_startup", "[CONFIG ERROR] Không thể tải cấu hình: {}", e);
            // logs::log_network_error(&format!("[CONFIG ERROR] Không thể tải cấu hình: {}", e));
            std::process::exit(1);
        }
    };

    // 2.1. Log các port đang sử dụng
    info!(target: "network_startup", "[PORT MAP] Main API: {} | Redis: {} | IPFS API: {} | IPFS Gateway: {} | gRPC: {} | Libp2p: {} | Nginx: {} | Prometheus: {} | Grafana: {}",
        config.core.port,
        config.plugins.redis.url.split(':').next_back().unwrap_or("6379"),
        config.plugins.ipfs.url.split(':').next_back().unwrap_or("5001"),
        config.plugins.ipfs.gateway.split(':').next_back().unwrap_or("8080"),
        config.plugins.grpc.port,
        config.plugins.libp2p.port,
        config.infra.nginx.as_ref().map(|n| n.port).unwrap_or(80),
        config.infra.prometheus.as_ref().map(|p| p.port).unwrap_or(9090),
        config.infra.grafana.as_ref().map(|g| g.port).unwrap_or(3000)
    );

    // 2.2. Kiểm tra xung đột port
    if let Err(e) = config.check_port_conflicts() {
        error!(target: "network_startup", "[PORT CONFLICT] {}", e);
        // logs::log_network_error(&format!("[PORT CONFLICT] {}", e));
        std::process::exit(1);
    }

    // 3. Khởi tạo engine và plugins với type annotation
    let engine = Arc::new(DefaultNetworkEngine::new());
    if let Err(e) = engine.bootstrap().await {
        error!(target: "network_startup", "[ENGINE BOOTSTRAP ERROR] {}", e);
        // logs::log_network_error(&format!("[ENGINE BOOTSTRAP ERROR] {}", e));
        std::process::exit(1);
    }
    
    // Chuyển đổi thành trait object cho các phần cần NetworkEngine trait
    let engine_trait: Arc<dyn NetworkEngine + Send + Sync> = engine.clone();

    // 4. Khởi tạo API logic (nếu có REST/gRPC server, cần bổ sung hàm start)
    let _api = DefaultNetworkApi::new(engine_trait.clone());

    // 5. Khởi động WebSocket server (có metrics, health, jwt verify)
    let host_ip = match config.core.host.parse::<IpAddr>() {
        Ok(ip) => ip,
        Err(e) => {
            error!(target: "network_startup", "[HOST ERROR] Địa chỉ host không hợp lệ: {}", e);
            // logs::log_network_error(&format!("[HOST ERROR] Địa chỉ host không hợp lệ: {}", e));
            std::process::exit(1);
        }
    };

    let ws_addr = SocketAddr::new(host_ip, config.core.port);
    let ws_service = WarpWebSocketService::new();
    // Mount thêm API logs
    let log_routes = logs_api();
    
    // Sử dụng WebSocketService trait
    let ws_routes = WebSocketService::serve_ws_with_metrics(&ws_service, ws_addr, "/ws", engine_trait.clone()).await;
    
    // Chạy đồng thời WebSocket và API logs
    let (_ws_result, _log_result) = tokio::join!(ws_routes, warp::serve(log_routes).run(ws_addr));

    // 6. (Tuỳ chọn) Theo dõi tín hiệu shutdown, cleanup tài nguyên
}
