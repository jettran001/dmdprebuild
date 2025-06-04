use crate::security::input_validation::security;
use tokio::sync::Mutex as TokioMutex;
use tracing::{info, warn};
use tokio::sync::Notify;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use async_trait::async_trait;
use crate::infra::service_traits::ServiceError;

/// Maximum allowed MQTT payload size in bytes
const MAX_MQTT_PAYLOAD_SIZE: usize = 256 * 1024; // 256KB

/// Trait for MQTT messaging service (async)
#[async_trait]
pub trait MessagingMqttService: Send + Sync + 'static {
    /// Connect to MQTT broker (async)
    async fn connect(&self, url: &str) -> Result<(), ServiceError>;
    
    /// Connect to MQTT broker with authentication (async)
    async fn connect_with_auth(&self, url: &str, username: &str, password: &str) -> Result<(), ServiceError>;
    
    /// Publish a message to a topic (async)
    async fn publish(&self, topic: &str, message: &[u8], qos: u8) -> Result<(), ServiceError>;
    
    /// Subscribe to a topic (async)
    async fn subscribe(&self, topic: &str, qos: u8, callback: Box<dyn Fn(&str, &[u8]) + Send + Sync>) -> Result<String, ServiceError>;
    
    /// Unsubscribe from a topic (async)
    async fn unsubscribe(&self, subscription_id: &str) -> Result<(), ServiceError>;
    
    /// Health check (async)
    async fn health_check(&self) -> Result<bool, ServiceError>;
    
    /// Validate input (sync, remains same)
    fn validate_input(&self, topic: &str, payload: &[u8], url: &str) -> Result<(), ServiceError>;
    
    /// Check payload size (sync)
    fn check_payload_size(&self, payload: &[u8]) -> Result<(), ServiceError> {
        if payload.len() > MAX_MQTT_PAYLOAD_SIZE {
            return Err(ServiceError::ValidationError(format!(
                "MQTT payload size {} exceeds maximum allowed size {}",
                payload.len(), MAX_MQTT_PAYLOAD_SIZE
            )));
        }
        Ok(())
    }
}

/// Default implementation of MessagingMqttService (mock)
pub struct DefaultMessagingMqttService;

#[async_trait]
impl MessagingMqttService for DefaultMessagingMqttService {
    async fn connect(&self, url: &str) -> Result<(), ServiceError> {
        security::check_xss(url, "mqtt_url").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        info!("[MessagingMqttService] Connecting to MQTT broker at {}", url);
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        Ok(())
    }
    
    async fn connect_with_auth(&self, url: &str, username: &str, password: &str) -> Result<(), ServiceError> {
        security::check_xss(url, "mqtt_url").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        security::check_xss(username, "mqtt_username").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        security::check_xss(password, "mqtt_password").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        info!("[MessagingMqttService] Connecting to MQTT broker at {} with auth", url);
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        Ok(())
    }
    
    async fn publish(&self, topic: &str, message: &[u8], qos: u8) -> Result<(), ServiceError> {
        self.validate_input(topic, message, "mock://mqtt").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        self.check_payload_size(message).map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        info!("[MessagingMqttService] Publish to topic: {} ({} bytes) with qos {}", topic, message.len(), qos);
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        Ok(())
    }
    
    async fn subscribe(&self, topic: &str, qos: u8, _callback: Box<dyn Fn(&str, &[u8]) + Send + Sync>) -> Result<String, ServiceError> {
        security::check_xss(topic, "mqtt_topic").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        security::check_sql_injection(topic, "mqtt_topic").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        info!("[MessagingMqttService] Subscribe to topic: {} with qos {}", topic, qos);
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        Ok("mock_sub_id".to_string())
    }
    
    async fn unsubscribe(&self, subscription_id: &str) -> Result<(), ServiceError> {
        info!("[MessagingMqttService] Unsubscribe from id: {}", subscription_id);
        Ok(())
    }
    
    async fn health_check(&self) -> Result<bool, ServiceError> {
        Ok(true)
    }
    
    fn validate_input(&self, topic: &str, payload: &[u8], url: &str) -> Result<(), ServiceError> {
        let topic_str = topic;
        let payload_str = std::str::from_utf8(payload).unwrap_or("");
        security::check_xss(topic_str, "mqtt_topic").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        security::check_sql_injection(topic_str, "mqtt_topic").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        security::check_xss(payload_str, "mqtt_payload").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        security::check_sql_injection(payload_str, "mqtt_payload").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        security::check_xss(url, "mqtt_url").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        security::check_sql_injection(url, "mqtt_url").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        Ok(())
    }
}

pub struct MqttConnectionPool {
    pool: TokioMutex<Vec<String>>,
    max_size: usize,
    stop_cleanup: Arc<AtomicBool>,
    notify_cleanup: Arc<Notify>,
}

impl MqttConnectionPool {
    pub fn new(max_size: usize) -> Self {
        let pool = TokioMutex::new(Vec::with_capacity(max_size));
        let stop_cleanup = Arc::new(AtomicBool::new(false));
        let notify_cleanup = Arc::new(Notify::new());
        let pool_arc = Arc::new(Self { pool, max_size, stop_cleanup: stop_cleanup.clone(), notify_cleanup: notify_cleanup.clone() });
        let pool_clone = pool_arc.clone();
        tokio::spawn(async move {
            loop {
                if pool_clone.stop_cleanup.load(Ordering::SeqCst) { break; }
                // Nếu pool lưu object lớn, có thể kiểm tra idle timeout ở đây
                // Hiện tại chỉ mock, không xóa gì
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {},
                    _ = pool_clone.notify_cleanup.notified() => { break; }
                }
            }
        });
        Arc::try_unwrap(pool_arc).unwrap_or_else(|arc| (*arc).clone())
    }
    
    pub async fn shutdown(&self) {
        self.stop_cleanup.store(true, Ordering::SeqCst);
        self.notify_cleanup.notify_waiters();
    }
    
    pub async fn get_connection(&self) -> Option<String> {
        let mut pool = self.pool.lock().await;
        pool.pop()
    }
    
    pub async fn return_connection(&self, conn: String) -> Result<(), ServiceError> {
        let mut pool = self.pool.lock().await;
        if pool.len() < self.max_size {
            pool.push(conn);
        }
        Ok(())
    }
}

impl Clone for MqttConnectionPool {
    fn clone(&self) -> Self {
        Self {
            pool: TokioMutex::new(Vec::with_capacity(self.max_size)),
            max_size: self.max_size,
            stop_cleanup: self.stop_cleanup.clone(),
            notify_cleanup: self.notify_cleanup.clone(),
        }
    }
}

impl DefaultMessagingMqttService {
    /// Publish with retry (async, with timeout and proper error handling)
    pub async fn publish_with_retry(
        &self,
        topic: &str,
        payload: &[u8],
        max_retries: u32,
        retry_delay_ms: u64,
        timeout_ms: u64
    ) -> Result<bool, ServiceError> {
        // Check payload size first
        self.check_payload_size(payload)?;
        
        let mut attempts = 0;
        while attempts < max_retries {
            // Add timeout to each publish attempt
            match tokio::time::timeout(
                tokio::time::Duration::from_millis(timeout_ms),
                self.publish(topic, payload, 0)
            ).await {
                Ok(result) => {
                    match result {
                        Ok(()) => return Ok(true),
                        Err(e) => {
                            warn!("[MessagingMqttService] Publish error: {}", e);
                        }
                    }
                },
                Err(_) => {
                    warn!("[MessagingMqttService] Publish timeout after {}ms (attempt {})", timeout_ms, attempts + 1);
                }
            }
            
            attempts += 1;
            if attempts < max_retries {
                tokio::time::sleep(std::time::Duration::from_millis(retry_delay_ms)).await;
            }
        }
        
        Err(ServiceError::TimeoutError(format!(
            "Failed to publish to {} after {} retries", topic, max_retries
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_test::block_on;

    #[test]
    fn test_mqtt_connection_pool_basic() {
        let pool = MqttConnectionPool::new(2);
        block_on(pool.return_connection("conn1".to_string())).unwrap();
        block_on(pool.return_connection("conn2".to_string())).unwrap();
        assert_eq!(block_on(pool.get_connection()).is_some(), true);
        assert_eq!(block_on(pool.get_connection()).is_some(), true);
        assert_eq!(block_on(pool.get_connection()).is_none(), true);
    }

    #[tokio::test]
    async fn test_publish_with_retry_success() {
        let mqtt = DefaultMessagingMqttService;
        assert!(mqtt.publish_with_retry("topic", b"payload", 3, 10, 100).await.is_ok());
    }

    #[test]
    fn test_check_payload_size_valid() {
        let mqtt = DefaultMessagingMqttService;
        let small_payload = vec![0; 1000];
        assert!(mqtt.check_payload_size(&small_payload).is_ok());
    }

    #[test]
    fn test_check_payload_size_too_large() {
        let mqtt = DefaultMessagingMqttService;
        // Create payload larger than MAX_MQTT_PAYLOAD_SIZE
        let large_payload = vec![0; MAX_MQTT_PAYLOAD_SIZE + 1];
        assert!(mqtt.check_payload_size(&large_payload).is_err());
    }

    #[test]
    fn test_mqtt_validate_input_safe() {
        let mqtt = DefaultMessagingMqttService;
        assert!(mqtt.validate_input("topic", b"payload", "mqtt://localhost:1883").is_ok());
    }

    #[test]
    fn test_mqtt_validate_input_xss() {
        let mqtt = DefaultMessagingMqttService;
        assert!(mqtt.validate_input("<script>", b"payload", "mqtt://localhost:1883").is_err());
    }

    #[test]
    fn test_mqtt_validate_input_sql() {
        let mqtt = DefaultMessagingMqttService;
        assert!(mqtt.validate_input("topic", b"1; DROP TABLE users", "mqtt://localhost:1883").is_err());
    }
}
