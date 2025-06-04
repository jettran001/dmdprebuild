/// Metrics collection and reporting module
///
/// This module provides metrics tracking and reporting capabilities
/// for monitoring the SnipeBot performance, trades, and system health.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, Context};
use metrics::{counter, gauge, histogram, Counter, Gauge, Histogram, Key, KeyName, Unit};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use tokio::sync::RwLock;
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::config;

/// Prometheus metrics handle
static mut PROMETHEUS_HANDLE: Option<PrometheusHandle> = None;

/// Metrics namespace
const NAMESPACE: &str = "snipebot";

/// Global metrics registry
pub struct MetricsRegistry {
    /// Whether metrics collection is enabled
    enabled: bool,
    
    /// Current trade metrics
    trade_metrics: RwLock<TradeMetrics>,
    
    /// System metrics
    system_metrics: RwLock<SystemMetrics>,
    
    /// Chain metrics by chain_id
    chain_metrics: RwLock<HashMap<u32, ChainMetrics>>,
}

/// Trade-related metrics
#[derive(Debug, Clone, Default)]
struct TradeMetrics {
    /// Total trades executed
    pub total_trades: u64,
    
    /// Total successful trades
    pub successful_trades: u64,
    
    /// Total failed trades
    pub failed_trades: u64,
    
    /// Total profit in USD
    pub total_profit_usd: f64,
    
    /// Total gas costs in USD
    pub total_gas_cost_usd: f64,
    
    /// Trade win rate (successful / total)
    pub win_rate: f64,
    
    /// Biggest winning trade in USD
    pub biggest_win_usd: f64,
    
    /// Biggest losing trade in USD
    pub biggest_loss_usd: f64,
    
    /// Average trade profit in USD
    pub avg_profit_usd: f64,
    
    /// Average trade duration in seconds
    pub avg_trade_duration_seconds: f64,
}

/// System-related metrics
#[derive(Debug, Clone, Default)]
struct SystemMetrics {
    /// CPU usage percentage
    pub cpu_usage: f64,
    
    /// Memory usage in MB
    pub memory_usage_mb: f64,
    
    /// Disk usage in MB
    pub disk_usage_mb: f64,
    
    /// Cache size in items
    pub cache_size: usize,
    
    /// Cache hit ratio
    pub cache_hit_ratio: f64,
    
    /// Number of active connections
    pub active_connections: usize,
    
    /// API requests per minute
    pub api_requests_per_minute: u64,
    
    /// Average API response time in ms
    pub avg_api_response_time_ms: f64,
}

/// Chain-specific metrics
#[derive(Debug, Clone, Default)]
struct ChainMetrics {
    /// Chain name
    pub chain_name: String,
    
    /// Chain ID
    pub chain_id: u32,
    
    /// RPC requests count
    pub rpc_requests: u64,
    
    /// RPC errors count
    pub rpc_errors: u64,
    
    /// Average RPC response time in ms
    pub avg_rpc_response_time_ms: f64,
    
    /// Gas price in gwei
    pub gas_price_gwei: f64,
    
    /// Trading volume in USD
    pub trading_volume_usd: f64,
    
    /// Number of pending transactions
    pub pending_transactions: u64,
    
    /// Number of active trading pairs
    pub active_trading_pairs: u64,
}

/// Prometheus metrics server
pub struct MetricsServer {
    /// Server address
    address: SocketAddr,
    
    /// Prometheus handle
    handle: PrometheusHandle,
}

impl MetricsRegistry {
    /// Create a new metrics registry
    pub fn new(enabled: bool) -> Arc<Self> {
        Arc::new(Self {
            enabled,
            trade_metrics: RwLock::new(TradeMetrics::default()),
            system_metrics: RwLock::new(SystemMetrics::default()),
            chain_metrics: RwLock::new(HashMap::new()),
        })
    }
    
    /// Initialize metrics collection
    pub async fn init() -> Result<Arc<Self>> {
        // Get config
        let config = config::get_config().await;
        let enabled = config.monitoring.metrics_enabled;
        
        let registry = Self::new(enabled);
        
        // Initialize metrics if enabled
        if enabled {
            info!("Initializing metrics collection");
            registry.setup_metrics().await?;
        }
        
        Ok(registry)
    }
    
    /// Setup metrics collection
    async fn setup_metrics(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        
        // Setup counters, gauges, and histograms
        self.register_trade_metrics();
        self.register_system_metrics();
        
        // Start metrics collection task
        let registry_clone = Arc::new(self.clone());
        tokio::spawn(async move {
            registry_clone.collect_metrics_task().await;
        });
        
        info!("Metrics collection initialized");
        Ok(())
    }
    
    /// Register trade metrics
    fn register_trade_metrics(&self) {
        // Counters
        counter!("snipebot.trades.total", "Total number of trades executed");
        counter!("snipebot.trades.successful", "Number of successful trades");
        counter!("snipebot.trades.failed", "Number of failed trades");
        
        // Gauges
        gauge!("snipebot.trades.profit_usd", "Total profit in USD");
        gauge!("snipebot.trades.gas_cost_usd", "Total gas cost in USD");
        gauge!("snipebot.trades.win_rate", "Trade win rate (successful / total)");
        gauge!("snipebot.trades.biggest_win_usd", "Biggest winning trade in USD");
        gauge!("snipebot.trades.biggest_loss_usd", "Biggest losing trade in USD");
        gauge!("snipebot.trades.avg_profit_usd", "Average trade profit in USD");
        gauge!("snipebot.trades.avg_duration_seconds", "Average trade duration in seconds");
        
        // Histograms
        histogram!("snipebot.trades.profit_distribution", "Distribution of trade profits");
        histogram!("snipebot.trades.duration_distribution", "Distribution of trade durations");
    }
    
    /// Register system metrics
    fn register_system_metrics(&self) {
        // Gauges
        gauge!("snipebot.system.cpu_usage", "CPU usage percentage");
        gauge!("snipebot.system.memory_usage_mb", "Memory usage in MB");
        gauge!("snipebot.system.disk_usage_mb", "Disk usage in MB");
        gauge!("snipebot.system.cache_size", "Cache size in items");
        gauge!("snipebot.system.cache_hit_ratio", "Cache hit ratio");
        gauge!("snipebot.system.active_connections", "Number of active connections");
        gauge!("snipebot.system.api_requests_per_minute", "API requests per minute");
        gauge!("snipebot.system.avg_api_response_time_ms", "Average API response time in ms");
    }
    
    /// Register chain metrics for a specific chain
    fn register_chain_metrics(&self, chain_id: u32, chain_name: &str) {
        let labels = [("chain_id", chain_id.to_string()), ("chain_name", chain_name.to_string())];
        
        // Counters
        counter!("snipebot.chain.rpc_requests", &labels, "RPC requests count");
        counter!("snipebot.chain.rpc_errors", &labels, "RPC errors count");
        
        // Gauges
        gauge!("snipebot.chain.avg_rpc_response_time_ms", &labels, "Average RPC response time in ms");
        gauge!("snipebot.chain.gas_price_gwei", &labels, "Gas price in gwei");
        gauge!("snipebot.chain.trading_volume_usd", &labels, "Trading volume in USD");
        gauge!("snipebot.chain.pending_transactions", &labels, "Number of pending transactions");
        gauge!("snipebot.chain.active_trading_pairs", &labels, "Number of active trading pairs");
    }
    
    /// Background task to collect metrics
    async fn collect_metrics_task(&self) {
        let mut interval = time::interval(Duration::from_secs(15));
        
        loop {
            interval.tick().await;
            
            if !self.enabled {
                continue;
            }
            
            // Update metrics
            if let Err(e) = self.update_trade_metrics().await {
                error!("Failed to update trade metrics: {}", e);
            }
            
            if let Err(e) = self.update_system_metrics().await {
                error!("Failed to update system metrics: {}", e);
            }
            
            if let Err(e) = self.update_chain_metrics().await {
                error!("Failed to update chain metrics: {}", e);
            }
        }
    }
    
    /// Update trade metrics
    async fn update_trade_metrics(&self) -> Result<()> {
        let trade_metrics = self.trade_metrics.read().await;
        
        // Update counters
        counter!("snipebot.trades.total").increment(trade_metrics.total_trades);
        counter!("snipebot.trades.successful").increment(trade_metrics.successful_trades);
        counter!("snipebot.trades.failed").increment(trade_metrics.failed_trades);
        
        // Update gauges
        gauge!("snipebot.trades.profit_usd").set(trade_metrics.total_profit_usd);
        gauge!("snipebot.trades.gas_cost_usd").set(trade_metrics.total_gas_cost_usd);
        gauge!("snipebot.trades.win_rate").set(trade_metrics.win_rate);
        gauge!("snipebot.trades.biggest_win_usd").set(trade_metrics.biggest_win_usd);
        gauge!("snipebot.trades.biggest_loss_usd").set(trade_metrics.biggest_loss_usd);
        gauge!("snipebot.trades.avg_profit_usd").set(trade_metrics.avg_profit_usd);
        gauge!("snipebot.trades.avg_duration_seconds").set(trade_metrics.avg_trade_duration_seconds);
        
        Ok(())
    }
    
    /// Update system metrics
    async fn update_system_metrics(&self) -> Result<()> {
        let system_metrics = self.system_metrics.read().await;
        
        // Update gauges
        gauge!("snipebot.system.cpu_usage").set(system_metrics.cpu_usage);
        gauge!("snipebot.system.memory_usage_mb").set(system_metrics.memory_usage_mb);
        gauge!("snipebot.system.disk_usage_mb").set(system_metrics.disk_usage_mb);
        gauge!("snipebot.system.cache_size").set(system_metrics.cache_size as f64);
        gauge!("snipebot.system.cache_hit_ratio").set(system_metrics.cache_hit_ratio);
        gauge!("snipebot.system.active_connections").set(system_metrics.active_connections as f64);
        gauge!("snipebot.system.api_requests_per_minute").set(system_metrics.api_requests_per_minute as f64);
        gauge!("snipebot.system.avg_api_response_time_ms").set(system_metrics.avg_api_response_time_ms);
        
        Ok(())
    }
    
    /// Update chain metrics
    async fn update_chain_metrics(&self) -> Result<()> {
        let chain_metrics = self.chain_metrics.read().await;
        
        for (chain_id, metrics) in chain_metrics.iter() {
            let labels = [("chain_id", chain_id.to_string()), ("chain_name", metrics.chain_name.clone())];
            
            // Update counters
            counter!("snipebot.chain.rpc_requests", &labels).increment(metrics.rpc_requests);
            counter!("snipebot.chain.rpc_errors", &labels).increment(metrics.rpc_errors);
            
            // Update gauges
            gauge!("snipebot.chain.avg_rpc_response_time_ms", &labels).set(metrics.avg_rpc_response_time_ms);
            gauge!("snipebot.chain.gas_price_gwei", &labels).set(metrics.gas_price_gwei);
            gauge!("snipebot.chain.trading_volume_usd", &labels).set(metrics.trading_volume_usd);
            gauge!("snipebot.chain.pending_transactions", &labels).set(metrics.pending_transactions as f64);
            gauge!("snipebot.chain.active_trading_pairs", &labels).set(metrics.active_trading_pairs as f64);
        }
        
        Ok(())
    }
    
    // Public API for updating metrics
    
    /// Record a completed trade
    pub async fn record_trade(&self, 
                       chain_id: u32,
                       success: bool, 
                       profit_usd: f64, 
                       gas_cost_usd: f64, 
                       duration_seconds: f64) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        
        let mut trade_metrics = self.trade_metrics.write().await;
        
        // Update counts
        trade_metrics.total_trades += 1;
        if success {
            trade_metrics.successful_trades += 1;
        } else {
            trade_metrics.failed_trades += 1;
        }
        
        // Update profit metrics
        trade_metrics.total_profit_usd += profit_usd;
        trade_metrics.total_gas_cost_usd += gas_cost_usd;
        
        // Update win rate
        if trade_metrics.total_trades > 0 {
            trade_metrics.win_rate = trade_metrics.successful_trades as f64 / trade_metrics.total_trades as f64;
        }
        
        // Update biggest win/loss
        if profit_usd > trade_metrics.biggest_win_usd {
            trade_metrics.biggest_win_usd = profit_usd;
        }
        if profit_usd < trade_metrics.biggest_loss_usd {
            trade_metrics.biggest_loss_usd = profit_usd;
        }
        
        // Update average profit
        trade_metrics.avg_profit_usd = trade_metrics.total_profit_usd / trade_metrics.total_trades as f64;
        
        // Update average duration
        trade_metrics.avg_trade_duration_seconds = 
            (trade_metrics.avg_trade_duration_seconds * (trade_metrics.total_trades - 1) as f64 + duration_seconds) 
            / trade_metrics.total_trades as f64;
        
        // Add to histograms
        histogram!("snipebot.trades.profit_distribution").record(profit_usd);
        histogram!("snipebot.trades.duration_distribution").record(duration_seconds);
        
        // Update chain volume
        {
            let mut chain_metrics = self.chain_metrics.write().await;
            if let Some(metrics) = chain_metrics.get_mut(&chain_id) {
                metrics.trading_volume_usd += profit_usd.abs();
            }
        }
        
        Ok(())
    }
    
    /// Update system metrics
    pub async fn update_system(&self,
                        cpu_usage: f64,
                        memory_usage_mb: f64,
                        disk_usage_mb: f64,
                        cache_size: usize,
                        cache_hit_ratio: f64) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        
        let mut system_metrics = self.system_metrics.write().await;
        
        system_metrics.cpu_usage = cpu_usage;
        system_metrics.memory_usage_mb = memory_usage_mb;
        system_metrics.disk_usage_mb = disk_usage_mb;
        system_metrics.cache_size = cache_size;
        system_metrics.cache_hit_ratio = cache_hit_ratio;
        
        Ok(())
    }
    
    /// Record API request
    pub async fn record_api_request(&self, response_time_ms: f64) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        
        let mut system_metrics = self.system_metrics.write().await;
        
        // Update API requests count
        system_metrics.api_requests_per_minute += 1;
        
        // Update average response time
        if system_metrics.api_requests_per_minute > 0 {
            system_metrics.avg_api_response_time_ms = 
                (system_metrics.avg_api_response_time_ms * (system_metrics.api_requests_per_minute - 1) as f64 + response_time_ms) 
                / system_metrics.api_requests_per_minute as f64;
        }
        
        Ok(())
    }
    
    /// Reset API requests per minute (called each minute)
    pub async fn reset_api_requests(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        
        let mut system_metrics = self.system_metrics.write().await;
        system_metrics.api_requests_per_minute = 0;
        
        Ok(())
    }
    
    /// Record RPC request
    pub async fn record_rpc_request(&self, 
                             chain_id: u32, 
                             chain_name: &str,
                             success: bool, 
                             response_time_ms: f64) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        
        let mut chain_metrics = self.chain_metrics.write().await;
        
        // Create chain metrics if not exists
        if !chain_metrics.contains_key(&chain_id) {
            chain_metrics.insert(chain_id, ChainMetrics {
                chain_name: chain_name.to_string(),
                chain_id,
                ..Default::default()
            });
            
            // Register chain metrics
            self.register_chain_metrics(chain_id, chain_name);
        }
        
        // Get chain metrics
        let metrics = match chain_metrics.get_mut(&chain_id) {
            Some(m) => m,
            None => {
                return Err(anyhow::anyhow!("Chain metrics for chain ID {} not found", chain_id));
            }
        };
        
        // Update RPC requests count
        metrics.rpc_requests += 1;
        
        // Update RPC errors count if failed
        if !success {
            metrics.rpc_errors += 1;
        }
        
        // Update average response time
        if metrics.rpc_requests > 0 {
            metrics.avg_rpc_response_time_ms = 
                (metrics.avg_rpc_response_time_ms * (metrics.rpc_requests - 1) as f64 + response_time_ms) 
                / metrics.rpc_requests as f64;
        }
        
        Ok(())
    }
    
    /// Update gas price for a chain
    pub async fn update_gas_price(&self, chain_id: u32, chain_name: &str, gas_price_gwei: f64) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        
        let mut chain_metrics = self.chain_metrics.write().await;
        
        // Create chain metrics if not exists
        if !chain_metrics.contains_key(&chain_id) {
            chain_metrics.insert(chain_id, ChainMetrics {
                chain_name: chain_name.to_string(),
                chain_id,
                ..Default::default()
            });
            
            // Register chain metrics
            self.register_chain_metrics(chain_id, chain_name);
        }
        
        // Get chain metrics
        let metrics = match chain_metrics.get_mut(&chain_id) {
            Some(m) => m,
            None => {
                return Err(anyhow::anyhow!("Chain metrics for chain ID {} not found", chain_id));
            }
        };
        
        // Update gas price
        metrics.gas_price_gwei = gas_price_gwei;
        
        Ok(())
    }
    
    /// Update pending transactions for a chain
    pub async fn update_pending_transactions(&self, chain_id: u32, chain_name: &str, pending_tx_count: u64) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        
        let mut chain_metrics = self.chain_metrics.write().await;
        
        // Create chain metrics if not exists
        if !chain_metrics.contains_key(&chain_id) {
            chain_metrics.insert(chain_id, ChainMetrics {
                chain_name: chain_name.to_string(),
                chain_id,
                ..Default::default()
            });
            
            // Register chain metrics
            self.register_chain_metrics(chain_id, chain_name);
        }
        
        // Get chain metrics
        let metrics = match chain_metrics.get_mut(&chain_id) {
            Some(m) => m,
            None => {
                return Err(anyhow::anyhow!("Chain metrics for chain ID {} not found", chain_id));
            }
        };
        
        // Update pending transactions
        metrics.pending_transactions = pending_tx_count;
        
        Ok(())
    }
}

impl MetricsServer {
    /// Create a new metrics server
    pub fn new(address: SocketAddr) -> Result<Self> {
        // Create builder
        let builder = PrometheusBuilder::new();
        
        // Configure metrics
        let builder = builder
            .with_namespace(NAMESPACE)
            .with_default_buckets([0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]);
        
        // Create recorder
        let handle = builder.install_recorder()?;
        
        // Store handle in global static
        unsafe {
            PROMETHEUS_HANDLE = Some(handle.clone());
        }
        
        Ok(Self {
            address,
            handle,
        })
    }
    
    /// Start the metrics server
    pub async fn start(&self) -> Result<()> {
        info!("Starting metrics server on {}", self.address);
        
        let metrics_page = self.handle.render();
        let address = self.address;
        
        // Start server
        tokio::spawn(async move {
            let make_service = warp::service::make_service_fn(move |_| {
                let metrics_page = metrics_page.clone();
                async move {
                    Ok::<_, std::convert::Infallible>(warp::service::service_fn(move |_| {
                        let metrics_page = metrics_page.clone();
                        async move {
                            let response = match warp::http::Response::builder()
                                .header("Content-Type", "text/plain")
                                .body(metrics_page.clone()) {
                                    Ok(response) => response,
                                    Err(e) => {
                                        eprintln!("Error creating metrics response: {}", e);
                                        warp::http::Response::builder()
                                            .status(500)
                                            .body("Error creating metrics response".to_string())
                                            .expect("Fallback response creation should not fail")
                                    }
                                };
                            
                            Ok::<_, std::convert::Infallible>(response)
                        }
                    }))
                }
            });
            
            warp::serve(make_service).run(address).await;
        });
        
        Ok(())
    }
}

/// Initialize the metrics system
pub async fn init(metrics_enabled: bool, metrics_port: u16) -> Result<Arc<MetricsRegistry>> {
    // Create metrics registry
    let registry = MetricsRegistry::new(metrics_enabled);
    
    if metrics_enabled {
        // Create metrics server
        let address: SocketAddr = format!("0.0.0.0:{}", metrics_port).parse()
            .context("Invalid metrics address")?;
        
        let server = MetricsServer::new(address)?;
        server.start().await?;
        
        info!("Metrics server started on http://{}/metrics", address);
    }
    
    Ok(registry)
}
