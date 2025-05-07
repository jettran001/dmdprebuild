use async_trait::async_trait;
use crate::core::engine::NetworkEngine;
use std::sync::Arc;
use warp::Filter;
use std::fs;
use tracing::{info, warn, error};

use crate::errors::{NetworkError, Result as NetworkResult};
use crate::security::api_validation::{
    LogDomainValidator, PluginTypeValidator, MetricsValidator, HealthValidator, 
    create_default_validators
};

#[async_trait]
pub trait NetworkApi: Send + Sync {
    async fn health_check(&self) -> NetworkResult<String>;
    async fn metrics(&self) -> NetworkResult<String>;
    fn version(&self) -> String;
    // --- Plugin management API ---
    async fn list_plugins(&self) -> NetworkResult<Vec<String>>;
    async fn plugin_status(&self, plugin_type: String) -> NetworkResult<String>;
    fn unregister_plugin(&self, plugin_type: String) -> NetworkResult<String>;
    fn check_plugin_health(&self, plugin_type: String) -> NetworkResult<String>;
}

pub struct DefaultNetworkApi {
    engine: Arc<dyn NetworkEngine + Send + Sync + 'static>,
    validators: (
        LogDomainValidator,
        PluginTypeValidator,
        MetricsValidator,
        HealthValidator
    ),
}

impl DefaultNetworkApi {
    pub fn new(engine: Arc<dyn NetworkEngine + Send + Sync + 'static>) -> Self {
        let validators = create_default_validators();
        Self { 
            engine,
            validators 
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
            .filter(|&status| *status == crate::core::types::PluginStatus::Active)
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
        
        use crate::core::types::PluginType;
        let plugin_type = plugin_type.parse::<PluginType>()
            .map_err(|_| NetworkError::ValidationError("Invalid plugin type format".to_string()))?;
        
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

    fn unregister_plugin(&self, plugin_type: String) -> NetworkResult<String> {
        // Validate input
        self.validators.1.validate(&plugin_type)?;
        
        // Log request
        info!(endpoint = "unregister_plugin", plugin_type = %plugin_type, "Plugin unregister requested");
        
        use crate::core::types::PluginType;
        let plugin_type = plugin_type.parse::<PluginType>()
            .map_err(|_| NetworkError::ValidationError("Invalid plugin type format".to_string()))?;
        
        self.engine.unregister_plugin(&plugin_type)
            .map_err(|e| {
                error!(endpoint = "unregister_plugin", plugin_type = ?plugin_type, error = %e, "Error unregistering plugin");
                NetworkError::EngineError(format!("Error unregistering plugin: {}", e))
            })?;
        
        Ok("Unregistered successfully".to_string())
    }

    fn check_plugin_health(&self, plugin_type: String) -> NetworkResult<String> {
        // Validate input
        self.validators.1.validate(&plugin_type)?;
        
        // Log request
        info!(endpoint = "check_plugin_health", plugin_type = %plugin_type, "Plugin health check requested");
        
        use crate::core::types::PluginType;
        let plugin_type = plugin_type.parse::<PluginType>()
            .map_err(|_| NetworkError::ValidationError("Invalid plugin type format".to_string()))?;
        
        let healthy = self.engine.check_plugin_health(&plugin_type)
            .map_err(|e| {
                error!(endpoint = "check_plugin_health", plugin_type = ?plugin_type, error = %e, "Error checking plugin health");
                NetworkError::EngineError(format!("Error checking plugin health: {}", e))
            })?;
        
        Ok(if healthy { "Healthy".to_string() } else { "Unhealthy".to_string() })
    }
}

/// API endpoint cho logs
/// 
/// # Returns
/// LogsApiFilter - Type alias cho warp Filter để xử lý truy vấn logs
pub fn logs_api() -> LogsApiFilter {
    warp::path!("api" / "logs")
        .and(warp::get())
        .and_then(get_logs)
}

/// Type alias rõ ràng cho kiểu trả về của logs_api
pub type LogsApiFilter = impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone;
