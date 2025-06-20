/// Health check module for SnipeBot
///
/// This module provides health check functionalities to monitor the system's health
/// and ensure all components are running properly.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, Context};
use async_trait::async_trait;
use tokio::sync::RwLock;
use tokio::time::timeout;
use serde::{Serialize, Deserialize};
use tracing::{debug, error, info, warn};
use warp;

/// Health check trait for components that need to be monitored
#[async_trait]
pub trait HealthCheck: Send + Sync + 'static {
    /// Name of the component being checked
    fn name(&self) -> &str;
    
    /// Check if the component is healthy
    async fn check_health(&self) -> Result<HealthStatus>;
    
    /// Get component-specific health metrics
    async fn get_metrics(&self) -> HashMap<String, String>;
    
    /// Custom action to take when component is unhealthy
    async fn on_unhealthy(&self) -> Result<()> {
        // Default implementation does nothing
        Ok(())
    }
}

/// Health manager for coordinating health checks
pub struct HealthManager {
    /// List of registered health checks
    health_checks: RwLock<Vec<Arc<dyn HealthCheck>>>,
    
    /// Last check results
    last_results: RwLock<HashMap<String, HealthCheckResult>>,
    
    /// Global application health
    overall_health: RwLock<SystemHealth>,
    
    /// Check interval in seconds
    check_interval_seconds: u64,
}

/// System health information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHealth {
    /// System status
    pub status: HealthStatus,
    
    /// Timestamp of last check
    pub last_check: u64,
    
    /// Uptime in seconds
    pub uptime_seconds: u64,
    
    /// System start time (Unix timestamp)
    pub start_time: u64,
    
    /// Component health statuses
    pub components: HashMap<String, HealthCheckResult>,
    
    /// Global system metrics
    pub metrics: HashMap<String, String>,
}

/// Health check result for a single component
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    /// Component name
    pub name: String,
    
    /// Component status
    pub status: HealthStatus,
    
    /// Timestamp of last check
    pub last_check: u64,
    
    /// Reason for current status
    pub reason: Option<String>,
    
    /// Component metrics
    pub metrics: HashMap<String, String>,
    
    /// History of status changes
    pub history: Vec<StatusChange>,
}

/// Health status enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Component is healthy
    Healthy,
    
    /// Component is degraded but still functioning
    Degraded,
    
    /// Component is unhealthy and not functioning
    Unhealthy,
    
    /// Component status is unknown
    Unknown,
}

/// Status change event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusChange {
    /// Previous status
    pub previous: HealthStatus,
    
    /// Current status
    pub current: HealthStatus,
    
    /// Timestamp of change
    pub timestamp: u64,
    
    /// Reason for change
    pub reason: Option<String>,
}

impl HealthManager {
    /// Create a new health manager
    pub fn new(check_interval_seconds: u64) -> Self {
        let current_time = chrono::Utc::now().timestamp() as u64;
        
        let system_health = SystemHealth {
            status: HealthStatus::Unknown,
            last_check: current_time,
            uptime_seconds: 0,
            start_time: current_time,
            components: HashMap::<String, HealthCheckResult>::new(),
            metrics: HashMap::<String, String>::new(),
        };
        
        Self {
            health_checks: RwLock::new(Vec::new()),
            last_results: RwLock::new(HashMap::<String, HealthCheckResult>::new()),
            overall_health: RwLock::new(system_health),
            check_interval_seconds,
        }
    }
    
    /// Register a health check component
    pub async fn register(&self, check: Arc<dyn HealthCheck>) {
        let mut checks = self.health_checks.write().await;
        checks.push(check);
    }
    
    /// Start the health check monitoring loop
    pub async fn start_monitoring(&self) -> Result<()> {
        info!("Starting health check monitoring with interval of {} seconds", self.check_interval_seconds);
        
        // Initial health check
        self.check_all().await?;
        
        // Spawn task for periodic health checks
        let self_arc = Arc::new(self.clone());
        tokio::spawn(async move {
            let interval = Duration::from_secs(self_arc.check_interval_seconds);
            let mut interval = tokio::time::interval(interval);
            
            loop {
                interval.tick().await;
                if let Err(e) = self_arc.check_all().await {
                    error!("Error during health check: {}", e);
                }
            }
        });
        
        Ok(())
    }
    
    /// Run health check on all registered components
    pub async fn check_all(&self) -> Result<SystemHealth> {
        let mut overall_status = HealthStatus::Healthy;
        let checks = self.health_checks.read().await;
        let current_time = chrono::Utc::now().timestamp() as u64;
        
        // Clone current results for updating
        let mut results = self.last_results.read().await.clone();
        
        // Run all health checks
        for check in checks.iter() {
            let name = check.name().to_string();
            debug!("Running health check for {}", name);
            
            // Run health check with timeout
            let check_result = match timeout(Duration::from_secs(5), check.check_health()).await {
                Ok(Ok(status)) => status,
                Ok(Err(e)) => {
                    error!("Health check for {} failed: {}", name, e);
                    HealthStatus::Unhealthy
                },
                Err(_) => {
                    error!("Health check for {} timed out", name);
                    HealthStatus::Unhealthy
                }
            };
            
            // Get component metrics
            let metrics = match check.get_metrics().await {
                Ok(m) => m,
                Err(e) => {
                    error!("Failed to get metrics for {}: {}", name, e);
                    HashMap::<String, String>::new()
                }
            };
            
            // Determine overall status (worst case)
            if check_result == HealthStatus::Unhealthy {
                overall_status = HealthStatus::Unhealthy;
            } else if check_result == HealthStatus::Degraded && overall_status == HealthStatus::Healthy {
                overall_status = HealthStatus::Degraded;
            }
            
            // Update or create result entry
            if let Some(existing) = results.get(&name) {
                let mut updated = existing.clone();
                
                // Check if status changed
                if existing.status != check_result {
                    // Call unhealthy handler if component became unhealthy
                    if check_result == HealthStatus::Unhealthy {
                        if let Err(e) = check.on_unhealthy().await {
                            error!("Error in unhealthy handler for {}: {}", name, e);
                        }
                    }
                    
                    // Record status change
                    updated.history.push(StatusChange {
                        previous: existing.status,
                        current: check_result,
                        timestamp: current_time,
                        reason: None,
                    });
                    
                    // Limit history size
                    if updated.history.len() > 10 {
                        updated.history.remove(0);
                    }
                }
                
                updated.status = check_result;
                updated.last_check = current_time;
                updated.metrics = metrics;
                
                results.insert(name, updated);
            } else {
                // Create new result entry
                let result = HealthCheckResult {
                    name: name.clone(),
                    status: check_result,
                    last_check: current_time,
                    reason: None,
                    metrics,
                    history: vec![
                        StatusChange {
                            previous: HealthStatus::Unknown,
                            current: check_result,
                            timestamp: current_time,
                            reason: None,
                        }
                    ],
                };
                
                results.insert(name, result);
            }
        }
        
        // Update system health
        let mut system_health = self.overall_health.write().await;
        let start_time = system_health.start_time;
        
        *system_health = SystemHealth {
            status: overall_status,
            last_check: current_time,
            uptime_seconds: current_time - start_time,
            start_time,
            components: results.clone(),
            metrics: self.get_system_metrics().await,
        };
        
        // Update last results
        *self.last_results.write().await = results;
        
        Ok(system_health.clone())
    }
    
    /// Get current system health
    pub async fn get_health(&self) -> SystemHealth {
        self.overall_health.read().await.clone()
    }
    
    /// Get health status for a specific component
    pub async fn get_component_health(&self, component_name: &str) -> Option<HealthCheckResult> {
        self.last_results.read().await.get(component_name).cloned()
    }
    
    /// Get system-wide metrics
    async fn get_system_metrics(&self) -> HashMap<String, String> {
        let mut metrics = HashMap::<String, String>::new();
        
        // Add basic system metrics
        metrics.insert("cpu_usage".to_string(), format!("{:.2}%", get_cpu_usage()));
        metrics.insert("memory_usage".to_string(), format!("{:.2}%", get_memory_usage()));
        metrics.insert("disk_free".to_string(), format!("{:.2}GB", get_disk_free()));
        
        // Add component count metrics
        let component_count = self.health_checks.read().await.len();
        metrics.insert("component_count".to_string(), component_count.to_string());
        
        // Count components by status
        let results = self.last_results.read().await;
        let healthy_count = results.values().filter(|r| r.status == HealthStatus::Healthy).count();
        let degraded_count = results.values().filter(|r| r.status == HealthStatus::Degraded).count();
        let unhealthy_count = results.values().filter(|r| r.status == HealthStatus::Unhealthy).count();
        
        metrics.insert("healthy_components".to_string(), healthy_count.to_string());
        metrics.insert("degraded_components".to_string(), degraded_count.to_string());
        metrics.insert("unhealthy_components".to_string(), unhealthy_count.to_string());
        
        metrics
    }
    
    /// Create a new health manager as singleton
    pub fn get_instance() -> Arc<Self> {
        static INSTANCE: tokio::sync::OnceCell<Arc<HealthManager>> = tokio::sync::OnceCell::const_new();
        
        INSTANCE.get_or_init(|| async {
            Arc::new(HealthManager::new(60)) // Default to 60 second interval
        }).unwrap_or_else(|e| {
            // Log error but don't panic
            error!("Failed to initialize health manager: {}, using a new instance", e);
            Arc::new(HealthManager::new(60))
        })
    }
}

impl Clone for HealthManager {
    fn clone(&self) -> Self {
        // This is safe because we're only cloning the internal structure
        // and maintaining the same RwLocks
        Self {
            health_checks: RwLock::new(Vec::new()),
            last_results: RwLock::new(HashMap::<String, HealthCheckResult>::new()),
            overall_health: RwLock::new(SystemHealth {
                status: HealthStatus::Unknown,
                last_check: 0,
                uptime_seconds: 0,
                start_time: 0,
                components: HashMap::<String, HealthCheckResult>::new(),
                metrics: HashMap::<String, String>::new(),
            }),
            check_interval_seconds: self.check_interval_seconds,
        }
    }
}

/// Simple CPU usage metric (dummy implementation)
fn get_cpu_usage() -> f64 {
    // In a real system, this would call OS functions
    // or read /proc/stat on Linux
    rand::random::<f64>() * 100.0
}

/// Simple memory usage metric (dummy implementation)
fn get_memory_usage() -> f64 {
    // In a real system, this would call OS functions
    // or read from /proc/meminfo on Linux
    rand::random::<f64>() * 1000.0
}

/// Simple disk free space metric (dummy implementation)
fn get_disk_free() -> f64 {
    // In a real system, this would use std::fs 
    // or OS-specific functions
    rand::random::<f64>() * 100.0 + 50.0
}

/// RPC Connection health check
pub struct RpcHealthCheck {
    /// Chain ID
    chain_id: u32,
    
    /// Chain name
    chain_name: String,
    
    /// RPC URL
    rpc_url: String,
    
    /// Adapter reference
    adapter: Arc<dyn RpcAdapter>,
}

/// WebSocket connection health check
pub struct WebSocketHealthCheck {
    /// Chain ID
    chain_id: u32,
    
    /// Chain name
    chain_name: String,
    
    /// WebSocket URL
    ws_url: String,
    
    /// Last known block number
    last_block: RwLock<u64>,
    
    /// Last update time
    last_update: RwLock<Instant>,
}

/// RPC adapter trait for blockchain providers
#[async_trait]
pub trait RpcAdapter: Send + Sync + 'static {
    /// Get the latest block number
    async fn get_latest_block(&self) -> Result<u64>;
    
    /// Get the chain ID
    async fn get_chain_id(&self) -> Result<u32>;
    
    /// Get the gas price
    async fn get_gas_price(&self) -> Result<u64>;
}

impl RpcHealthCheck {
    /// Create a new RPC health check
    pub fn new(chain_id: u32, chain_name: &str, rpc_url: &str, adapter: Arc<dyn RpcAdapter>) -> Self {
        Self {
            chain_id,
            chain_name: chain_name.to_string(),
            rpc_url: rpc_url.to_string(),
            adapter,
        }
    }
}

impl WebSocketHealthCheck {
    /// Create a new WebSocket health check
    pub fn new(chain_id: u32, chain_name: &str, ws_url: &str) -> Self {
        Self {
            chain_id,
            chain_name: chain_name.to_string(),
            ws_url: ws_url.to_string(),
            last_block: RwLock::new(0),
            last_update: RwLock::new(Instant::now()),
        }
    }
    
    /// Update the last known block
    pub async fn update_block(&self, block_number: u64) {
        let mut last_block = self.last_block.write().await;
        let mut last_update = self.last_update.write().await;
        
        *last_block = block_number;
        *last_update = Instant::now();
    }
}

#[async_trait]
impl HealthCheck for RpcHealthCheck {
    fn name(&self) -> &str {
        &self.chain_name
    }
    
    async fn check_health(&self) -> Result<HealthStatus> {
        // Check if we can get the latest block
        match self.adapter.get_latest_block().await {
            Ok(_) => {
                // Also check chain ID to make sure we're connected to the right chain
                match self.adapter.get_chain_id().await {
                    Ok(chain_id) => {
                        if chain_id == self.chain_id {
                            Ok(HealthStatus::Healthy)
                        } else {
                            warn!("Chain ID mismatch for {}: expected {}, got {}", 
                                self.chain_name, self.chain_id, chain_id);
                            Ok(HealthStatus::Unhealthy)
                        }
                    },
                    Err(e) => {
                        error!("Failed to get chain ID for {}: {}", self.chain_name, e);
                        Ok(HealthStatus::Degraded)
                    }
                }
            },
            Err(e) => {
                error!("RPC health check failed for {}: {}", self.chain_name, e);
                Ok(HealthStatus::Unhealthy)
            }
        }
    }
    
    async fn get_metrics(&self) -> HashMap<String, String> {
        let mut metrics = HashMap::<String, String>::new();
        metrics.insert("rpc_url".to_string(), self.rpc_url.clone());
        
        // Add block number if available
        if let Ok(block) = self.adapter.get_latest_block().await {
            metrics.insert("latest_block".to_string(), block.to_string());
        }
        
        // Add gas price if available
        if let Ok(gas) = self.adapter.get_gas_price().await {
            metrics.insert("gas_price".to_string(), format!("{} gwei", gas / 1_000_000_000));
        }
        
        metrics
    }
    
    async fn on_unhealthy(&self) -> Result<()> {
        // Log warning about unhealthy RPC
        warn!("RPC connection for {} is unhealthy. Consider switching to a backup RPC.", self.chain_name);
        Ok(())
    }
}

#[async_trait]
impl HealthCheck for WebSocketHealthCheck {
    fn name(&self) -> &str {
        &self.chain_name
    }
    
    async fn check_health(&self) -> Result<HealthStatus> {
        let last_update = self.last_update.read().await;
        let elapsed = last_update.elapsed();
        
        // If we haven't received a block in 2 minutes, WebSocket is unhealthy
        if elapsed > Duration::from_secs(120) {
            return Ok(HealthStatus::Unhealthy);
        }
        
        // If we haven't received a block in 30 seconds, WebSocket is degraded
        if elapsed > Duration::from_secs(30) {
            return Ok(HealthStatus::Degraded);
        }
        
        Ok(HealthStatus::Healthy)
    }
    
    async fn get_metrics(&self) -> HashMap<String, String> {
        let mut metrics = HashMap::<String, String>::new();
        metrics.insert("ws_url".to_string(), self.ws_url.clone());
        
        let last_block = self.last_block.read().await;
        metrics.insert("last_block".to_string(), last_block.to_string());
        
        let last_update = self.last_update.read().await;
        let elapsed = last_update.elapsed().as_secs();
        metrics.insert("seconds_since_update".to_string(), elapsed.to_string());
        
        metrics
    }
    
    async fn on_unhealthy(&self) -> Result<()> {
        // Log warning about unhealthy WebSocket
        warn!("WebSocket connection for {} is unhealthy. No blocks received in the last 2 minutes.", self.chain_name);
        Ok(())
    }
}

/// Database health check
pub struct DatabaseHealthCheck {
    /// Database name
    name: String,
    
    /// Database connection string
    connection_string: String,
    
    /// Is connection pool active
    is_active: bool,
}

impl DatabaseHealthCheck {
    /// Create a new database health check
    pub fn new(name: &str, connection_string: &str) -> Self {
        Self {
            name: name.to_string(),
            connection_string: connection_string.to_string(),
            is_active: true,
        }
    }
    
    /// Set the active state
    pub fn set_active(&mut self, active: bool) {
        self.is_active = active;
    }
}

#[async_trait]
impl HealthCheck for DatabaseHealthCheck {
    fn name(&self) -> &str {
        &self.name
    }
    
    async fn check_health(&self) -> Result<HealthStatus> {
        // Simple check - in a real implementation, would try a query
        if self.is_active {
            Ok(HealthStatus::Healthy)
        } else {
            Ok(HealthStatus::Unhealthy)
        }
    }
    
    async fn get_metrics(&self) -> HashMap<String, String> {
        let mut metrics = HashMap::<String, String>::new();
        
        // Mask sensitive information in connection string
        let masked = mask_connection_string(&self.connection_string);
        metrics.insert("connection".to_string(), masked);
        metrics.insert("is_active".to_string(), self.is_active.to_string());
        
        metrics
    }
}

/// Helper to mask sensitive parts of connection strings
fn mask_connection_string(conn_str: &str) -> String {
    // Mask password in connection string
    if let Some(pwd_start) = conn_str.find("password=") {
        if let Some(pwd_end) = conn_str[pwd_start..].find(';') {
            let start = &conn_str[0..pwd_start + 9]; // include "password="
            let end = &conn_str[pwd_start + pwd_end..];
            return format!("{}*****{}", start, end);
        }
    }
    
    // If no standard password format, mask anything after @ until next / or :
    if let Some(at_pos) = conn_str.find('@') {
        if let Some(pwd_start) = conn_str[..at_pos].rfind(':') {
            let start = &conn_str[0..pwd_start + 1]; // include ":"
            let end = &conn_str[at_pos..];
            return format!("{}*****{}", start, end);
        }
    }
    
    conn_str.to_string()
}

/// Lấy danh sách lỗi từ file .bugs
pub async fn read_bugs_file() -> Result<HashMap<String, Vec<String>>, anyhow::Error> {
    let bugs_content = tokio::fs::read_to_string(".bugs").await?;
    let mut result: HashMap<String, Vec<String>> = HashMap::new();
    
    let mut current_section = String::from("general");
    let mut current_entries: Vec<String> = Vec::new();
    
    for line in bugs_content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        
        if line.starts_with("## ") {
            // Lưu section trước đó
            if !current_entries.is_empty() {
                result.insert(current_section, current_entries);
                current_entries = Vec::new();
            }
            
            // Bắt đầu section mới
            current_section = line.trim_start_matches("## ").to_string();
        } else if line.starts_with("- ") {
            current_entries.push(line.trim_start_matches("- ").to_string());
        } else if !line.starts_with("#") {
            // Bỏ qua các dòng comment
            current_entries.push(line.to_string());
        }
    }
    
    // Lưu section cuối cùng
    if !current_entries.is_empty() {
        result.insert(current_section, current_entries);
    }
    
    Ok(result)
}

/// Handler for bugs API endpoint
pub async fn bugs_api_handler(req: warp::filters::path::FullPath) -> Result<HashMap<String, serde_json::Value>, warp::Rejection> {
    let path = req.as_str();
    info!("API call to bugs endpoint: {}", path);
    
    // Read bugs file
    let bugs = match read_bugs_file().await {
        Ok(data) => data,
        Err(e) => {
            error!("Error reading bugs file: {}", e);
            return Ok(HashMap::from([
                ("status".to_string(), serde_json::Value::String("error".to_string())),
                ("message".to_string(), serde_json::Value::String(format!("Error reading bugs file: {}", e))),
            ]));
        }
    };
    
    // Convert to JSON response
    let mut response = HashMap::new();
    response.insert("status".to_string(), serde_json::Value::String("success".to_string()));
    
    // Chuyển đổi bugs sang JSON, xử lý lỗi một cách an toàn
    let bugs_json = match serde_json::to_value(bugs) {
        Ok(json) => json,
        Err(e) => {
            error!("Error converting bugs to JSON: {}", e);
            serde_json::Value::Object(serde_json::Map::new())
        }
    };
    
    response.insert("data".to_string(), bugs_json);
    
    Ok(response)
}

/// Handler for token analysis API endpoint
pub async fn analyze_token_api_handler(req: warp::filters::path::FullPath, body: HashMap<String, String>) -> Result<HashMap<String, serde_json::Value>, warp::Rejection> {
    let path = req.as_str();
    info!("API call to analyze token endpoint: {}, with {} parameters", path, body.len());
    
    // Get token address from request body
    let token_address = match body.get("token_address") {
        Some(addr) => addr,
        None => {
            return Ok(HashMap::from([
                ("status".to_string(), serde_json::Value::String("error".to_string())),
                ("message".to_string(), serde_json::Value::String("Missing token_address parameter".to_string())),
            ]));
        }
    };
    
    let chain_id = body.get("chain_id").and_then(|id| id.parse::<u32>().ok());
    
    info!("Analyzing token {} on chain {:?}", token_address, chain_id);
    
    // Token analysis result
    let mut result = HashMap::new();
    result.insert("token_address".to_string(), serde_json::Value::String(token_address.clone()));
    result.insert("status".to_string(), serde_json::Value::String("ANALYSIS_PENDING".to_string()));
    
    // Add mock analysis data for demo purposes
    let safety_analysis = HashMap::from([
        ("is_honeypot".to_string(), serde_json::Value::String("false".to_string())),
        ("blacklist_detected".to_string(), serde_json::Value::String("false".to_string())),
        ("owner_privileges".to_string(), serde_json::Value::String("low".to_string())),
        ("buy_tax".to_string(), serde_json::Value::String("2%".to_string())),
        ("sell_tax".to_string(), serde_json::Value::String("2%".to_string())),
    ]);
    
    let lp_analysis = HashMap::from([
        ("lp_tokens_locked".to_string(), serde_json::Value::String("true".to_string())),
        ("lock_duration".to_string(), serde_json::Value::String("365 days".to_string())),
        ("liquidity_percentage".to_string(), serde_json::Value::String("85%".to_string())),
        ("liquidity_value".to_string(), serde_json::Value::String("$250,000".to_string())),
    ]);
    
    result.insert("safety_analysis".to_string(), serde_json::Value::Object(safety_analysis.into_iter().collect()));
    result.insert("liquidity_analysis".to_string(), serde_json::Value::Object(lp_analysis.into_iter().collect()));
    
    // Return result
    let mut response = HashMap::new();
    response.insert("status".to_string(), serde_json::Value::String("success".to_string()));
    response.insert("data".to_string(), serde_json::Value::Object(result.into_iter().collect()));
    
    Ok(response)
}

/// Add bugs and analysis routes to the API server
pub fn add_bugs_and_analysis_routes(routes: &mut Vec<Box<dyn warp::Filter<Extract = (warp::reply::Json,), Error = warp::Rejection> + Send + Sync>>) -> Result<(), anyhow::Error> {
    // Route for bugs API
    let bugs_route = warp::path("bugs")
        .and(warp::get())
        .and(warp::path::full())
        .and_then(|path| async move {
            match bugs_api_handler(path).await {
                Ok(reply) => Ok(warp::reply::json(&reply)),
                Err(e) => Ok(warp::reply::json(&format!("Error: {}", e))),
            }
        });
    
    // Route for token analysis API
    let analyze_route = warp::path("analyze")
        .and(warp::post())
        .and(warp::path::full())
        .and(warp::body::json())
        .and_then(|path, body| async move {
            match analyze_token_api_handler(path, body).await {
                Ok(reply) => Ok(warp::reply::json(&reply)),
                Err(e) => Ok(warp::reply::json(&format!("Error: {}", e))),
            }
        });
    
    routes.push(Box::new(bugs_route) as Box<dyn warp::Filter<Extract = (warp::reply::Json,), Error = warp::Rejection> + Send + Sync>);
    routes.push(Box::new(analyze_route) as Box<dyn warp::Filter<Extract = (warp::reply::Json,), Error = warp::Rejection> + Send + Sync>);
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_health_manager() {
        let manager = HealthManager::new(60);
        
        // Create a mock health check
        struct MockHealthCheck;
        
        #[async_trait]
        impl HealthCheck for MockHealthCheck {
            fn name(&self) -> &str { "mock" }
            
            async fn check_health(&self) -> Result<HealthStatus> {
                Ok(HealthStatus::Healthy)
            }
            
            async fn get_metrics(&self) -> HashMap<String, String> {
                let mut metrics = HashMap::<String, String>::new();
                metrics.insert("test".to_string(), "value".to_string());
                metrics
            }
        }
        
        // Register the mock health check
        manager.register(Arc::new(MockHealthCheck)).await;
        
        // Run health check
        let health = match manager.check_all().await {
            Ok(h) => h,
            Err(e) => {
                panic!("Health check failed: {}", e);
            }
        };
        
        // Verify results
        assert_eq!(health.status, HealthStatus::Healthy);
        assert_eq!(health.components.len(), 1);
        assert!(health.components.contains_key("mock"));
        assert_eq!(health.components["mock"].status, HealthStatus::Healthy);
    }
    
    #[test]
    fn test_mask_connection_string() {
        // Test standard format
        let conn = "Server=myserver;Database=mydb;User Id=admin;Password=mypassword;";
        let masked = mask_connection_string(conn);
        assert_eq!(masked, "Server=myserver;Database=mydb;User Id=admin;Password=*****;");
        
        // Test URL format
        let conn = "postgres://user:password@host:5432/database";
        let masked = mask_connection_string(conn);
        assert_eq!(masked, "postgres://user:*****@host:5432/database");
    }
}
