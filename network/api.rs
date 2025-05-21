use async_trait::async_trait;
use crate::core::engine::NetworkEngine;
use crate::core::engine::PluginType;
use std::sync::Arc;
use warp::Filter;
use tracing::{info, warn, error};

use crate::errors::{NetworkError, Result as NetworkResult};
use crate::security::api_validation::{
    LogDomainValidator, PluginTypeValidator, MetricsValidator, HealthValidator, 
    create_default_validators
};

/// Type alias cho API logs đơn giản
pub type LogsApiFilter = warp::filters::BoxedFilter<(Box<dyn warp::Reply>,)>;

/// Utility function để wrap một filter và kết quả của nó thành Box<dyn warp::Reply>
fn with_boxed_reply<F, R>(filter: F) -> warp::filters::BoxedFilter<(Box<dyn warp::Reply>,)>
where
    F: warp::Filter<Extract = (R,), Error = warp::Rejection> + Clone + Send + Sync + 'static,
    R: warp::Reply + 'static,
{
    filter.map(|reply| Box::new(reply) as Box<dyn warp::Reply>).boxed()
}

/// Tạo route API logs đơn giản
/// 
/// API này cho phép truy xuất log từ hệ thống, với các endpoint:
/// - GET /api/logs - lấy tất cả log gần đây
/// - GET /api/logs/{domain} - lấy log của một domain cụ thể (network, blockchain, wallet...)
pub fn logs_api() -> LogsApiFilter {
    let (log_validator, _, _, _) = create_default_validators();
    let log_validator = Arc::new(log_validator);
    
    // GET /api/logs
    let get_logs = warp::path("logs")
        .and(warp::path::end())
        .and(warp::get())
        .map(|| {
            // Implement logic to retrieve all recent logs
            warp::reply::json(&vec![
                "Latest log entries would be returned here".to_string(),
                "Implement actual log retrieval from your logging system".to_string()
            ])
        });
    
    // GET /api/logs/{domain}
    let log_validator_clone = log_validator.clone();
    let get_domain_logs = warp::path("logs")
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .and(warp::get())
        .and_then(move |domain: String| {
            let validator = log_validator_clone.clone();
            async move {
                // Validate domain parameter
                match validator.validate(&domain) {
                    Ok(_) => {
                        // Implement logic to retrieve logs for specific domain
                        let response = warp::reply::json(&vec![
                            format!("Logs for domain: {}", domain),
                            "Implement actual domain-specific log retrieval".to_string()
                        ]);
                        Ok(response)
                    },
                    Err(_) => Err(warp::reject::custom(NetworkError::ValidationError(
                        format!("Invalid log domain: {}", domain)
                    )))
                }
            }
        });
    
    // Áp dụng CORS trước khi bọc trong Box<dyn Reply>
    let cors = warp::cors().allow_any_origin();
    let get_logs_with_cors = get_logs.with(cors.clone());
    let domain_logs_with_cors = get_domain_logs.with(cors);
    
    // Kết hợp các route
    let combined = get_logs_with_cors.or(domain_logs_with_cors);
    
    // Wrap kết quả cuối cùng vào BoxedFilter có kiểu đúng theo LogsApiFilter
    with_boxed_reply(combined)
}

#[async_trait]
pub trait NetworkApi: Send + Sync {
    async fn health_check(&self) -> NetworkResult<String>;
    async fn metrics(&self) -> NetworkResult<String>;
    fn version(&self) -> String;
    // --- Plugin management API ---
    async fn list_plugins(&self) -> NetworkResult<Vec<String>>;
    async fn plugin_status(&self, plugin_type: String) -> NetworkResult<String>;
    async fn unregister_plugin(&self, plugin_type: String) -> NetworkResult<String>;
    async fn check_plugin_health(&self, plugin_type: String) -> NetworkResult<String>;
}

pub struct DefaultNetworkApi {
    engine: Arc<dyn NetworkEngine + Send + Sync + 'static>,
    validators: (
        Arc<LogDomainValidator>,
        Arc<PluginTypeValidator>,
        Arc<MetricsValidator>,
        Arc<HealthValidator>
    ),
}

impl DefaultNetworkApi {
    pub fn new(engine: Arc<dyn NetworkEngine + Send + Sync + 'static>) -> Self {
        let (log_validator, plugin_type_validator, metrics_validator, health_validator) = create_default_validators();
        Self { 
            engine,
            validators: (
                Arc::new(log_validator),
                Arc::new(plugin_type_validator),
                Arc::new(metrics_validator),
                Arc::new(health_validator)
            )
        }
    }
}

#[async_trait]
impl NetworkApi for DefaultNetworkApi {
    async fn health_check(&self) -> NetworkResult<String> {
        // Validate input (không cần tham số)
        self.validators.3.validate()?;
        
        // Log request
        info!(endpoint = "health_check", "Health check requested");
        
        let plugin_statuses = self.engine.get_all_plugin_statuses().await
            .map_err(|e| {
                error!(endpoint = "health_check", error = %e, "Error checking plugin status");
                NetworkError::EngineError(format!("Error checking plugin status: {}", e))
            })?;
            
        let active_count = plugin_statuses.values()
            .filter(|&status| *status == crate::core::engine::PluginStatus::Active)
            .count();
            
        Ok(format!("Healthy. Active plugins: {}/{}", active_count, plugin_statuses.len()))
    }
    
    async fn metrics(&self) -> NetworkResult<String> {
        // Validate input (không cần tham số)
        self.validators.2.validate()?;
        
        // Log request
        info!(endpoint = "metrics", "Metrics requested");
        
        self.engine.get_metrics().await
            .map_err(|e| {
                error!(endpoint = "metrics", error = %e, "Error collecting metrics");
                NetworkError::EngineError(format!("Error collecting metrics: {}", e))
            })
    }
    
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    async fn list_plugins(&self) -> NetworkResult<Vec<String>> {
        // Log request
        info!(endpoint = "list_plugins", "Plugin list requested");
        
        let plugins = self.engine.list_plugins().await
            .map_err(|e| {
                error!(endpoint = "list_plugins", error = %e, "Error listing plugins");
                NetworkError::EngineError(format!("Error listing plugins: {}", e))
            })?;
        Ok(plugins.into_iter().map(|p| format!("{:?}", p)).collect())
    }

    async fn plugin_status(&self, plugin_type: String) -> NetworkResult<String> {
        // Validate input
        self.validators.1.validate(&plugin_type)?;
        
        // Log request
        info!(endpoint = "plugin_status", plugin_type = %plugin_type, "Plugin status requested");
        
        // Xử lý chuyển đổi từ String sang PluginType thủ công thay vì dùng ?
        let plugin_type = match plugin_type.parse::<PluginType>() {
            Ok(pt) => pt,
            Err(e) => return Err(NetworkError::ValidationError(format!("Invalid plugin type: {}", e)))
        };
        
        let statuses = self.engine.get_all_plugin_statuses().await
            .map_err(|e| {
                error!(endpoint = "plugin_status", plugin_type = ?plugin_type, error = %e, "Error getting plugin status");
                NetworkError::EngineError(format!("Error getting plugin status: {}", e))
            })?;
        
        match statuses.get(&plugin_type) {
            Some(status) => Ok(format!("{:?}", status)),
            None => {
                warn!(endpoint = "plugin_status", plugin_type = ?plugin_type, "Plugin not found");
                Err(NetworkError::NotFoundError(format!("Plugin {:?} not found", plugin_type)))
            }
        }
    }

    async fn unregister_plugin(&self, plugin_type: String) -> NetworkResult<String> {
        // Validate input
        self.validators.1.validate(&plugin_type)?;
        
        // Log request
        info!(endpoint = "unregister_plugin", plugin_type = %plugin_type, "Plugin unregister requested");
        
        // Xử lý chuyển đổi từ String sang PluginType thủ công thay vì dùng ?
        let plugin_type = match plugin_type.parse::<PluginType>() {
            Ok(pt) => pt,
            Err(e) => return Err(NetworkError::ValidationError(format!("Invalid plugin type: {}", e)))
        };
        
        self.engine.unregister_plugin(&plugin_type).await
            .map_err(|e| {
                error!(endpoint = "unregister_plugin", plugin_type = ?plugin_type, error = %e, "Error unregistering plugin");
                NetworkError::EngineError(format!("Error unregistering plugin: {}", e))
            })?;
        
        Ok("Unregistered successfully".to_string())
    }

    async fn check_plugin_health(&self, plugin_type: String) -> NetworkResult<String> {
        // Validate input
        self.validators.1.validate(&plugin_type)?;
        
        // Log request
        info!(endpoint = "check_plugin_health", plugin_type = %plugin_type, "Plugin health check requested");
        
        // Xử lý chuyển đổi từ String sang PluginType thủ công thay vì dùng ?
        let plugin_type = match plugin_type.parse::<PluginType>() {
            Ok(pt) => pt,
            Err(e) => return Err(NetworkError::ValidationError(format!("Invalid plugin type: {}", e)))
        };
        
        let healthy = self.engine.check_plugin_health(&plugin_type).await
            .map_err(|e| {
                error!(endpoint = "check_plugin_health", plugin_type = ?plugin_type, error = %e, "Error checking plugin health");
                NetworkError::EngineError(format!("Error checking plugin health: {}", e))
            })?;
        
        Ok(if healthy { "Healthy".to_string() } else { "Unhealthy".to_string() })
    }
}
