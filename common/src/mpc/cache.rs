use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Service cung cấp cache cho MPC để tránh handshake lại mỗi lần
pub struct MpcCacheService {
    /// Cache cho tokens
    token_cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    
    /// Thời gian tồn tại mặc định của cache entry
    default_ttl: Duration,
}

/// Cache entry với TTL
struct CacheEntry {
    /// Dữ liệu được cache
    data: Vec<u8>,
    
    /// Thời điểm hết hạn
    expires_at: Instant,
}

impl MpcCacheService {
    /// Tạo mới cache service
    pub fn new(ttl_minutes: u64) -> Self {
        Self {
            token_cache: Arc::new(RwLock::new(HashMap::new())),
            default_ttl: Duration::from_secs(ttl_minutes * 60),
        }
    }
    
    /// Thêm một entry vào cache
    pub async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<(), String> {
        let expires_at = Instant::now() + ttl.unwrap_or(self.default_ttl);
        
        let entry = CacheEntry {
            data: value,
            expires_at,
        };
        
        let mut cache = self.token_cache.write().await;
        cache.insert(key.to_string(), entry);
        
        Ok(())
    }
    
    /// Lấy dữ liệu từ cache
    pub async fn get(&self, key: &str) -> Option<Vec<u8>> {
        let cache = self.token_cache.read().await;
        
        if let Some(entry) = cache.get(key) {
            if Instant::now() < entry.expires_at {
                return Some(entry.data.clone());
            }
        }
        
        None
    }
    
    /// Xóa cache entry
    pub async fn delete(&self, key: &str) -> bool {
        let mut cache = self.token_cache.write().await;
        cache.remove(key).is_some()
    }
    
    /// Làm sạch các entry hết hạn
    pub async fn cleanup(&self) -> usize {
        let mut cache = self.token_cache.write().await;
        let before_count = cache.len();
        
        // Giữ lại những entry chưa hết hạn
        cache.retain(|_, entry| Instant::now() < entry.expires_at);
        
        before_count - cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;
    
    #[tokio::test]
    async fn test_cache_operations() {
        let cache = MpcCacheService::new(10); // 10 phút TTL
        
        // Set và get
        let key = "test_key";
        let value = vec![1, 2, 3, 4];
        
        cache.set(key, value.clone(), None).await.unwrap();
        
        let retrieved = cache.get(key).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), value);
        
        // Delete
        let deleted = cache.delete(key).await;
        assert!(deleted);
        
        let after_delete = cache.get(key).await;
        assert!(after_delete.is_none());
    }
    
    #[tokio::test]
    async fn test_cache_expiration() {
        let cache = MpcCacheService::new(10); // 10 phút TTL mặc định
        
        // Set với TTL ngắn (100ms)
        let key = "expire_key";
        let value = vec![5, 6, 7, 8];
        
        cache.set(key, value.clone(), Some(Duration::from_millis(100))).await.unwrap();
        
        // Kiểm tra ngay lập tức
        let before_expire = cache.get(key).await;
        assert!(before_expire.is_some());
        
        // Đợi hết hạn
        sleep(Duration::from_millis(150)).await;
        
        // Kiểm tra sau khi hết hạn
        let after_expire = cache.get(key).await;
        assert!(after_expire.is_none());
    }
} 