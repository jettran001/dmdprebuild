use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;

/// Service cung cấp chức năng ký MPC
pub struct MpcSigningService {
    /// Key service để quản lý cryptographic keys
    key_service: Arc<dyn KeyService>,
    
    /// Quản lý concurrency để đảm bảo ký an toàn
    signing_lock: Arc<Mutex<()>>,
}

/// Interface cho key service
#[async_trait::async_trait]
pub trait KeyService: Send + Sync + 'static {
    /// Lấy public key
    async fn get_public_key(&self, key_id: &str) -> Result<Vec<u8>>;
    
    /// Ký message với key
    async fn sign_message(&self, key_id: &str, message: &[u8]) -> Result<Vec<u8>>;
}

impl MpcSigningService {
    /// Tạo mới MPC signing service
    pub fn new(key_service: Arc<dyn KeyService>) -> Self {
        Self {
            key_service,
            signing_lock: Arc::new(Mutex::new(())),
        }
    }
    
    /// Ký message sử dụng MPC
    pub async fn sign(&self, key_id: &str, message: &[u8]) -> Result<Vec<u8>> {
        // Khóa để đảm bảo không có race condition khi ký
        let _guard = self.signing_lock.lock().await;
        
        // Thực hiện ký với key service
        self.key_service.sign_message(key_id, message).await
    }
    
    /// Ký nhiều message cùng lúc
    pub async fn batch_sign(&self, key_id: &str, messages: &[Vec<u8>]) -> Result<Vec<Vec<u8>>> {
        let _guard = self.signing_lock.lock().await;
        
        let mut signatures = Vec::with_capacity(messages.len());
        
        for message in messages {
            let signature = self.key_service.sign_message(key_id, message).await?;
            signatures.push(signature);
        }
        
        Ok(signatures)
    }
    
    /// Lấy public key cho key ID
    pub async fn get_public_key(&self, key_id: &str) -> Result<Vec<u8>> {
        self.key_service.get_public_key(key_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::predicate::*;
    use mockall::mock;
    
    // Tạo mock cho KeyService
    mock! {
        KeyServiceMock {}
        #[async_trait::async_trait]
        impl KeyService for KeyServiceMock {
            async fn get_public_key(&self, key_id: &str) -> Result<Vec<u8>>;
            async fn sign_message(&self, key_id: &str, message: &[u8]) -> Result<Vec<u8>>;
        }
    }
    
    #[tokio::test]
    async fn test_signing() {
        let mut mock = MockKeyServiceMock::new();
        
        // Thiết lập expected behavior
        mock.expect_sign_message()
            .with(eq("test_key"), eq(b"test_message".to_vec()))
            .returning(|_, _| Ok(vec![1, 2, 3, 4]));
            
        let service = MpcSigningService::new(Arc::new(mock));
        
        // Test sign
        let result = service.sign("test_key", b"test_message").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![1, 2, 3, 4]);
    }
} 