/// Trait for Kafka messaging service
use crate::security::input_validation::security;
use tokio::sync::Mutex as TokioMutex;
use tracing::{info, warn, error, debug};
use tokio::sync::Notify;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use async_trait::async_trait;
use crate::infra::service_traits::ServiceError;

/// Maximum allowed Kafka message size in bytes
const MAX_KAFKA_PAYLOAD_SIZE: usize = 1024 * 1024; // 1MB

#[async_trait]
pub trait MessagingKafkaService: Send + Sync + 'static {
    async fn connect(&self, brokers: &[String]) -> Result<(), ServiceError>;
    async fn send(&self, topic: &str, message: &[u8]) -> Result<bool, ServiceError>;
    async fn subscribe(&self, topic: &str, callback: Box<dyn Fn(&[u8]) + Send + Sync>) -> Result<String, ServiceError>;
    async fn unsubscribe(&self, subscription_id: &str) -> Result<(), ServiceError>;
    async fn health_check(&self) -> Result<bool, ServiceError>;
}

/// Default implementation of MessagingKafkaService (mock)
pub struct DefaultMessagingKafkaService;

#[async_trait]
impl MessagingKafkaService for DefaultMessagingKafkaService {
    async fn connect(&self, brokers: &[String]) -> Result<(), ServiceError> {
        for broker in brokers {
            security::check_xss(broker, "kafka_broker").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
            security::check_sql_injection(broker, "kafka_broker").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        }
        info!("[MessagingKafkaService] Connected to kafka brokers: {:?}", brokers);
        Ok(())
    }
    
    async fn send(&self, topic: &str, message: &[u8]) -> Result<bool, ServiceError> {
        security::check_xss(topic, "kafka_topic").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        security::check_sql_injection(topic, "kafka_topic").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        
        if let Ok(msg_str) = std::str::from_utf8(message) {
            security::check_xss(msg_str, "kafka_message").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
            security::check_sql_injection(msg_str, "kafka_message").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        }
        
        if message.len() > MAX_KAFKA_PAYLOAD_SIZE {
            return Err(ServiceError::ValidationError(format!("Kafka payload too large: {} bytes", message.len())));
        }
        
        info!("[MessagingKafkaService] Send message to topic: {}, size: {} bytes", topic, message.len());
        
        // Mock successful send
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        
        Ok(true)
    }
    
    async fn subscribe(&self, topic: &str, _callback: Box<dyn Fn(&[u8]) + Send + Sync>) -> Result<String, ServiceError> {
        security::check_xss(topic, "kafka_topic").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        security::check_sql_injection(topic, "kafka_topic").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        info!("[MessagingKafkaService] Subscribe to topic: {}", topic);
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        Ok("mock_sub_id".to_string())
    }
    async fn unsubscribe(&self, subscription_id: &str) -> Result<(), ServiceError> {
        info!("[MessagingKafkaService] Unsubscribe from id: {}", subscription_id);
        Ok(())
    }
    async fn health_check(&self) -> Result<bool, ServiceError> {
        Ok(true)
    }
}

pub struct KafkaConnectionPool {
    pool: TokioMutex<Vec<String>>, // mock connection pool
    max_size: usize,
    stop_cleanup: Arc<AtomicBool>,
    notify_cleanup: Arc<Notify>,
}

impl KafkaConnectionPool {
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
    
    pub async fn return_connection(&self, conn: String) -> Result<(), String> {
        let mut pool = self.pool.lock().await;
        if pool.len() < self.max_size {
            pool.push(conn);
        }
        Ok(())
    }
    
    pub fn validate_input(&self, conn_str: &str) -> Result<(), String> {
        security::check_xss(conn_str, "kafka_conn").map_err(|e| e.to_string())?;
        security::check_sql_injection(conn_str, "kafka_conn").map_err(|e| e.to_string())?;
        Ok(())
    }
}

impl DefaultMessagingKafkaService {
    /// Send with retry (async, with timeout and proper error handling)
    pub async fn send_with_retry(
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
            // Add timeout to each send attempt
            match tokio::time::timeout(
                tokio::time::Duration::from_millis(timeout_ms),
                self.send(topic, payload)
            ).await {
                Ok(result) => {
                    match result {
                        Ok(true) => return Ok(true),
                        Ok(false) => {
                            warn!("[MessagingKafkaService] Send failed to topic: {} (attempt {})", topic, attempts + 1);
                        },
                        Err(e) => {
                            warn!("[MessagingKafkaService] Send error: {} (attempt {})", e, attempts + 1);
                        }
                    }
                },
                Err(_) => {
                    warn!("[MessagingKafkaService] Send timeout after {}ms (attempt {})", timeout_ms, attempts + 1);
                }
            }
            
            attempts += 1;
            if attempts < max_retries {
                tokio::time::sleep(tokio::time::Duration::from_millis(retry_delay_ms)).await;
            }
        }
        
        Err(ServiceError::TimeoutError(format!("Failed to send after {} attempts", max_retries)))
    }

    pub fn validate_input(&self, topic: &str, message: &[u8], _uri: &str) -> Result<(), ServiceError> {
        security::check_xss(topic, "kafka_topic").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        security::check_sql_injection(topic, "kafka_topic").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        if let Ok(msg_str) = std::str::from_utf8(message) {
            security::check_xss(msg_str, "kafka_message").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
            security::check_sql_injection(msg_str, "kafka_message").map_err(|e| ServiceError::ValidationError(e.to_string()))?;
        }
        Ok(())
    }

    pub fn check_payload_size(&self, message: &[u8]) -> Result<(), ServiceError> {
        if message.len() > MAX_KAFKA_PAYLOAD_SIZE {
            return Err(ServiceError::ValidationError(format!("Kafka payload too large: {} bytes", message.len())));
        }
        Ok(())
    }
}

impl Clone for KafkaConnectionPool {
    fn clone(&self) -> Self {
        Self {
            pool: TokioMutex::new(Vec::with_capacity(self.max_size)),
            max_size: self.max_size,
            stop_cleanup: self.stop_cleanup.clone(),
            notify_cleanup: self.notify_cleanup.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_test::block_on;

    #[test]
    fn test_kafka_connection_pool_basic() {
        let pool = KafkaConnectionPool::new(2);
        block_on(pool.return_connection("conn1".to_string())).unwrap();
        block_on(pool.return_connection("conn2".to_string())).unwrap();
        assert_eq!(block_on(pool.get_connection()).is_some(), true);
        assert_eq!(block_on(pool.get_connection()).is_some(), true);
        assert_eq!(block_on(pool.get_connection()).is_none(), true);
    }

    #[tokio::test]
    async fn test_send_with_retry_success() {
        let kafka = DefaultMessagingKafkaService;
        assert!(kafka.send_with_retry("topic", b"payload", 3, 10, 100).await.is_ok());
    }
    
    #[test]
    fn test_check_payload_size_valid() {
        let kafka = DefaultMessagingKafkaService;
        let small_payload = vec![0; 1000];
        assert!(kafka.check_payload_size(&small_payload).is_ok());
    }

    #[test]
    fn test_check_payload_size_too_large() {
        let kafka = DefaultMessagingKafkaService;
        // Create payload larger than MAX_KAFKA_PAYLOAD_SIZE
        let large_payload = vec![0; MAX_KAFKA_PAYLOAD_SIZE + 1];
        assert!(kafka.check_payload_size(&large_payload).is_err());
    }

    #[test]
    fn test_kafka_validate_input_safe() {
        let pool = KafkaConnectionPool::new(2);
        assert!(pool.validate_input("safe_conn").is_ok());
    }

    #[test]
    fn test_kafka_validate_input_xss() {
        let pool = KafkaConnectionPool::new(2);
        assert!(pool.validate_input("<script>alert('XSS')</script>").is_err());
    }

    #[test]
    fn test_kafka_validate_input_sql() {
        let pool = KafkaConnectionPool::new(2);
        assert!(pool.validate_input("1; DROP TABLE users").is_err());
    }
}
