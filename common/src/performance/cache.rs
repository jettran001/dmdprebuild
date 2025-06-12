use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use std::time::{Duration, Instant};
// use std::hash::Hash;  // Unused import
use anyhow::Result;

/// Service quản lý cache để tối ưu hiệu năng
pub struct CacheService {
    /// In-memory cache
    memory_cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    
    /// Cấu hình
    config: CacheConfig,
}

/// Cấu hình cache
pub struct CacheConfig {
    /// Thời gian tồn tại mặc định (giây)
    pub default_ttl: u64,
    
    /// Kích thước cache tối đa
    pub max_size: usize,
    
    /// Tần suất dọn dẹp (giây)
    pub cleanup_interval: u64,
}

/// Cache entry
struct CacheEntry {
    /// Dữ liệu
    data: Vec<u8>,
    
    /// Thời điểm tạo
    _created_at: Instant,
    
    /// Thời điểm hết hạn
    expires_at: Instant,
    
    /// Số lần truy cập
    access_count: u64,
    
    /// Thời điểm truy cập gần nhất
    last_accessed: Instant,
}

/// Cache key builder
pub struct CacheKey {
    parts: Vec<String>,
}

impl Default for CacheKey {
    fn default() -> Self {
        Self::new()
    }
}

impl CacheKey {
    /// Tạo mới cache key
    pub fn new() -> Self {
        Self {
            parts: Vec::new(),
        }
    }
    
    /// Thêm phần vào key
    pub fn add<T: ToString>(&mut self, part: T) -> &mut Self {
        self.parts.push(part.to_string());
        self
    }
    
    /// Chuyển đổi thành string
    pub fn build(&self) -> String {
        self.parts.join(":")
    }
}

impl CacheService {
    /// Tạo mới cache service
    pub fn new(config: CacheConfig) -> Self {
        Self {
            memory_cache: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }
    
    /// Tạo mới với cấu hình mặc định
    pub fn with_default_config() -> Self {
        Self::new(CacheConfig {
            default_ttl: 300, // 5 phút
            max_size: 10000,  // 10000 entries
            cleanup_interval: 60, // 1 phút
        })
    }
    
    /// Set giá trị vào cache
    pub async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
        let now = Instant::now();
        let ttl = ttl.unwrap_or_else(|| Duration::from_secs(self.config.default_ttl));
        
        let entry = CacheEntry {
            data: value,
            _created_at: now,
            expires_at: now + ttl,
            access_count: 0,
            last_accessed: now,
        };
        
        let mut cache = self.memory_cache.write().await;
        
        // Kiểm tra kích thước cache
        if cache.len() >= self.config.max_size {
            // Nếu cache đầy, xóa các entry cũ nhất
            self.evict_oldest_entries(&mut cache, 100); // Xóa 100 entry cũ nhất
        }
        
        cache.insert(key.to_string(), entry);
        
        Ok(())
    }
    
    /// Lấy giá trị từ cache
    pub async fn get(&self, key: &str) -> Option<Vec<u8>> {
        let now = Instant::now();
        let mut cache = self.memory_cache.write().await;
        
        if let Some(entry) = cache.get_mut(key) {
            // Kiểm tra xem entry có hết hạn chưa
            if now < entry.expires_at {
                // Cập nhật thông tin truy cập
                entry.access_count += 1;
                entry.last_accessed = now;
                
                return Some(entry.data.clone());
            } else {
                // Nếu hết hạn, xóa khỏi cache
                cache.remove(key);
            }
        }
        
        None
    }
    
    /// Xóa entry khỏi cache
    pub async fn delete(&self, key: &str) -> bool {
        let mut cache = self.memory_cache.write().await;
        cache.remove(key).is_some()
    }
    
    /// Làm sạch cache
    pub async fn clear(&self) {
        let mut cache = self.memory_cache.write().await;
        cache.clear();
    }
    
    /// Lấy thông tin về cache
    pub async fn stats(&self) -> CacheStats {
        let cache = self.memory_cache.read().await;
        
        let now = Instant::now();
        let total_entries = cache.len();
        let expired_entries = cache.values().filter(|e| now >= e.expires_at).count();
        let valid_entries = total_entries - expired_entries;
        
        CacheStats {
            total_entries,
            valid_entries,
            expired_entries,
        }
    }
    
    /// Làm sạch các entry hết hạn
    pub async fn cleanup(&self) -> usize {
        let mut cache = self.memory_cache.write().await;
        let now = Instant::now();
        let before_count = cache.len();
        
        cache.retain(|_, entry| now < entry.expires_at);
        
        before_count - cache.len()
    }
    
    /// Xóa các entry cũ nhất dựa trên thời gian truy cập
    fn evict_oldest_entries(&self, cache: &mut HashMap<String, CacheEntry>, count: usize) {
        // Tạo danh sách key và thời gian truy cập gần nhất
        let mut entries: Vec<(String, Instant)> = cache
            .iter()
            .map(|(k, v)| (k.clone(), v.last_accessed))
            .collect();
        
        // Sắp xếp theo thời gian truy cập (cũ nhất lên đầu)
        entries.sort_by(|a, b| a.1.cmp(&b.1));
        
        // Lấy count entry cũ nhất
        let to_remove = entries.into_iter().take(count).map(|(k, _)| k).collect::<Vec<_>>();
        
        // Xóa khỏi cache
        for key in to_remove {
            cache.remove(&key);
        }
    }
}

/// Thống kê cache
pub struct CacheStats {
    /// Tổng số entry
    pub total_entries: usize,
    
    /// Số entry còn hiệu lực
    pub valid_entries: usize,
    
    /// Số entry đã hết hạn
    pub expired_entries: usize,
}

/// Generic cache trait
pub trait Cacheable {
    /// Chuyển đổi thành bytes để cache
    fn to_bytes(&self) -> Result<Vec<u8>>;
    
    /// Chuyển đổi từ bytes thành object
    fn from_bytes(bytes: &[u8]) -> Result<Self> where Self: Sized;
    
    /// Lấy cache key
    fn cache_key(&self) -> String;
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_cache_operations() {
        let cache = CacheService::with_default_config();
        
        // Set
        cache.set("test_key", vec![1, 2, 3, 4], None).await.unwrap();
        
        // Get
        let value = cache.get("test_key").await;
        assert!(value.is_some());
        assert_eq!(value.unwrap(), vec![1, 2, 3, 4]);
        
        // Delete
        let deleted = cache.delete("test_key").await;
        assert!(deleted);
        
        // Get sau khi delete
        let value = cache.get("test_key").await;
        assert!(value.is_none());
    }
    
    #[tokio::test]
    async fn test_cache_expiration() {
        let cache = CacheService::with_default_config();
        
        // Set với TTL rất nhỏ
        cache.set("expire_key", vec![5, 6, 7, 8], Some(Duration::from_millis(100))).await.unwrap();
        
        // Kiểm tra ngay lập tức
        let value = cache.get("expire_key").await;
        assert!(value.is_some());
        
        // Đợi hết hạn
        tokio::time::sleep(Duration::from_millis(150)).await;
        
        // Kiểm tra sau khi hết hạn
        let value = cache.get("expire_key").await;
        assert!(value.is_none());
    }
} 