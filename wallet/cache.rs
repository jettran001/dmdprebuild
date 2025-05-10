// wallet/cache.rs
//! Module cung cấp các tiện ích cache cho toàn bộ domain wallet.
//! 
//! Module này cung cấp các cấu trúc dữ liệu và hàm tiện ích để quản lý cache nhằm 
//! tối ưu hóa hiệu năng khi truy vấn thông tin người dùng và đăng ký.
//!
//! # Ví dụ
//! ```
//! use crate::cache::{Cache, LRUCache};
//!
//! let mut user_cache = LRUCache::new(100);
//! user_cache.insert("user_id_1".to_string(), user_data);
//! if let Some(user) = user_cache.get("user_id_1") {
//!     // Sử dụng dữ liệu từ cache
//! }
//! ```

use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};
use std::marker::PhantomData;
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use std::sync::Arc;
use std::borrow::Borrow;
use tracing::{debug, warn};
use std::future::Future;
use std::pin::Pin;

use lru::LruCache;
use tokio::sync::{Mutex};
use tracing::{info};

/// Trait định nghĩa các phương thức cần thiết cho một cache
pub trait Cache<K, V> {
    /// Thêm một cặp key-value vào cache
    fn insert(&mut self, key: K, value: V);
    
    /// Lấy giá trị từ cache theo key
    fn get<Q>(&mut self, key: &Q) -> Option<&V> 
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized;
    
    /// Kiểm tra một key có tồn tại trong cache không
    fn contains<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized;
    
    /// Xóa một entry từ cache
    fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized;
    
    /// Xóa tất cả các entry từ cache
    fn clear(&mut self);
    
    /// Trả về số lượng entry trong cache
    fn len(&self) -> usize;
    
    /// Kiểm tra xem cache có rỗng không
    fn is_empty(&self) -> bool;
}

/// Entry trong LRU cache
struct LRUEntry<K, V> {
    key: K,
    value: V,
    expiry: Option<Instant>,
    last_accessed: Instant,
}

/// LRU Cache (Least Recently Used)
/// Cache này sẽ loại bỏ các entry ít được sử dụng nhất khi đạt đến giới hạn
pub struct LRUCache<K, V> {
    entries: Vec<LRUEntry<K, V>>,
    lookup: HashMap<K, usize>,
    capacity: usize,
    default_ttl: Option<Duration>,
}

impl<K, V> LRUCache<K, V>
where
    K: Eq + Hash + Clone,
{
    /// Tạo một LRUCache mới với capacity cho trước
    pub fn new(capacity: usize) -> Self {
        LRUCache {
            entries: Vec::with_capacity(capacity),
            lookup: HashMap::with_capacity(capacity),
            capacity,
            default_ttl: None,
        }
    }
    
    /// Tạo một LRUCache mới với capacity và default TTL (Time-to-live)
    pub fn with_ttl(capacity: usize, ttl: Duration) -> Self {
        let mut cache = Self::new(capacity);
        cache.default_ttl = Some(ttl);
        cache
    }
    
    /// Thiết lập TTL mặc định cho cache
    pub fn set_default_ttl(&mut self, ttl: Duration) {
        self.default_ttl = Some(ttl);
    }
    
    /// Xóa các entry đã hết hạn
    pub fn cleanup_expired(&mut self) {
        let now = Instant::now();
        
        // Phát hiện các index cần xóa
        let mut to_remove = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if let Some(expiry) = entry.expiry {
                if expiry < now {
                    to_remove.push(index);
                }
            }
        }
        
        // Xóa các entry hết hạn (từ chỉ số lớn nhất đến nhỏ nhất để tránh shift index)
        to_remove.sort_unstable_by(|a, b| b.cmp(a));
        for index in to_remove {
            let entry = self.entries.remove(index);
            self.lookup.remove(&entry.key);
            
            debug!("Xóa entry hết hạn từ cache: {:?}", &entry.key);
        }
    }
    
    /// Thêm entry với TTL cụ thể
    pub fn insert_with_ttl(&mut self, key: K, value: V, ttl: Duration) {
        let now = Instant::now();
        let expiry = Some(now + ttl);
        
        self.cleanup_expired();
        
        if self.lookup.contains_key(&key) {
            // Cập nhật entry đã tồn tại
            match self.lookup.get(&key) {
                Some(&index) => {
                    self.entries[index].value = value;
                    self.entries[index].expiry = expiry;
                    self.entries[index].last_accessed = now;
                },
                None => {
                    eprintln!("[LRUCache] Warning: Index not found for key when updating (insert_with_ttl)");
                    return;
                }
            }
        } else {
            // Nếu đạt giới hạn, xóa entry ít sử dụng nhất
            if self.entries.len() >= self.capacity {
                // Tìm entry ít sử dụng nhất
                let mut oldest_idx = 0;
                let mut oldest_time = self.entries.get(0).map(|e| e.last_accessed).unwrap_or_else(|| Instant::now());
                for (idx, entry) in self.entries.iter().enumerate().skip(1) {
                    if entry.last_accessed < oldest_time {
                        oldest_idx = idx;
                        oldest_time = entry.last_accessed;
                    }
                }
                // Xóa entry ít sử dụng nhất
                let old_entry = self.entries.remove(oldest_idx);
                self.lookup.remove(&old_entry.key);
                debug!("Cache full, removed least used entry: {:?}", &old_entry.key);
                // Cập nhật lại các index trong lookup
                for (_, idx) in self.lookup.iter_mut() {
                    if *idx > oldest_idx {
                        *idx -= 1;
                    }
                }
            }
            // Thêm entry mới
            let index = self.entries.len();
            self.entries.push(LRUEntry {
                key: key.clone(),
                value,
                expiry,
                last_accessed: now,
            });
            self.lookup.insert(key, index);
        }
    }
}

impl<K, V> Cache<K, V> for LRUCache<K, V>
where
    K: Eq + Hash + Clone,
{
    fn insert(&mut self, key: K, value: V) {
        match self.default_ttl {
            Some(ttl) => self.insert_with_ttl(key, value, ttl),
            None => {
                let now = Instant::now();
                
                if self.lookup.contains_key(&key) {
                    // Cập nhật entry đã tồn tại
                    match self.lookup.get(&key) {
                        Some(&index) => {
                            self.entries[index].value = value;
                            self.entries[index].last_accessed = now;
                        },
                        None => {
                            eprintln!("[LRUCache] Warning: Index not found for key when updating (insert)");
                            return;
                        }
                    }
                } else {
                    // Nếu đạt giới hạn, xóa entry ít sử dụng nhất
                    if self.entries.len() >= self.capacity {
                        // Tìm entry ít sử dụng nhất
                        let mut oldest_idx = 0;
                        let mut oldest_time = self.entries.get(0).map(|e| e.last_accessed).unwrap_or_else(|| Instant::now());
                        for (idx, entry) in self.entries.iter().enumerate().skip(1) {
                            if entry.last_accessed < oldest_time {
                                oldest_idx = idx;
                                oldest_time = entry.last_accessed;
                            }
                        }
                        // Xóa entry ít sử dụng nhất
                        let old_entry = self.entries.remove(oldest_idx);
                        self.lookup.remove(&old_entry.key);
                        // Cập nhật lại các index trong lookup
                        for (_, idx) in self.lookup.iter_mut() {
                            if *idx > oldest_idx {
                                *idx -= 1;
                            }
                        }
                    }
                    // Thêm entry mới
                    let index = self.entries.len();
                    self.entries.push(LRUEntry {
                        key: key.clone(),
                        value,
                        expiry: None,
                        last_accessed: now,
                    });
                    self.lookup.insert(key, index);
                }
            }
        }
    }
    
    fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.cleanup_expired();
        
        if let Some(&index) = self.lookup.get(key) {
            let entry = &mut self.entries[index];
            entry.last_accessed = Instant::now();
            
            Some(&entry.value)
        } else {
            None
        }
    }
    
    fn contains<Q>(&self, key: &Q) -> bool 
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.lookup.contains_key(key)
    }
    
    fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if let Some(index) = self.lookup.remove(key) {
            let entry = self.entries.remove(index);
            
            // Cập nhật lại các index trong lookup
            for (_, idx) in self.lookup.iter_mut() {
                if *idx > index {
                    *idx -= 1;
                }
            }
            
            Some(entry.value)
        } else {
            None
        }
    }
    
    fn clear(&mut self) {
        self.entries.clear();
        self.lookup.clear();
    }
    
    fn len(&self) -> usize {
        self.entries.len()
    }
    
    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Cache được bảo vệ bởi RwLock, an toàn khi sử dụng trong môi trường đa luồng
/// Đặc biệt thiết kế cho sử dụng trong async context
pub struct AsyncCache<K, V, C>
where
    C: Cache<K, V>,
{
    cache: Arc<RwLock<C>>,
    _key: PhantomData<K>,
    _value: PhantomData<V>,
}

impl<K, V, C> AsyncCache<K, V, C>
where
    K: Eq + Hash + Clone,
    C: Cache<K, V>,
{
    /// Tạo một AsyncCache mới
    pub fn new(cache: C) -> Self {
        Self {
            cache: Arc::new(RwLock::new(cache)),
            _key: PhantomData,
            _value: PhantomData,
        }
    }
    
    /// Thêm một cặp key-value vào cache
    pub async fn insert(&self, key: K, value: V) {
        let mut cache = self.cache.write().await;
        cache.insert(key, value);
    }
    
    /// Kiểm tra cache trước, nếu không có thì gọi loader và cập nhật cache
    pub async fn get_or_load<F, Fut, Q>(&self, key: K, loader: F) -> Option<V> 
    where
        F: FnOnce(K) -> Fut,
        Fut: std::future::Future<Output = Option<V>>,
        K: Borrow<Q> + Clone,
        Q: Hash + Eq + ?Sized,
        V: Clone,
    {
        // Kiểm tra cache trước
        {
            let mut cache = self.cache.write().await;
            if let Some(value) = cache.get(key.borrow()) {
                return Some(value.clone());
            }
        }
        
        // Nếu không có trong cache, gọi loader
        if let Some(value) = loader(key.clone()).await {
            // Cập nhật cache và trả về giá trị
            let mut cache = self.cache.write().await;
            cache.insert(key, value.clone());
            return Some(value);
        }
        
        None
    }
    
    /// Xóa một entry từ cache
    pub async fn remove<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let mut cache = self.cache.write().await;
        cache.remove(key)
    }
    
    /// Xóa tất cả các entry từ cache
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
    
    /// Kiểm tra một key có tồn tại trong cache không
    pub async fn contains<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let cache = self.cache.read().await;
        cache.contains(key)
    }
}

/// Tạo một async LRU cache với time-to-live
pub fn create_async_lru_cache<K, V>(capacity: usize, ttl_seconds: u64) -> AsyncCache<K, V, LRUCache<K, V>>
where
    K: Eq + Hash + Clone,
{
    let cache = LRUCache::with_ttl(capacity, Duration::from_secs(ttl_seconds));
    AsyncCache::new(cache)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    
    #[test]
    fn test_lru_cache_basic() {
        let mut cache = LRUCache::new(3);
        
        cache.insert("a".to_string(), 1);
        cache.insert("b".to_string(), 2);
        cache.insert("c".to_string(), 3);
        
        assert_eq!(cache.get(&"a".to_string()), Some(&1));
        assert_eq!(cache.get(&"b".to_string()), Some(&2));
        assert_eq!(cache.get(&"c".to_string()), Some(&3));
        
        // Thêm một entry mới, "a" sẽ bị loại bỏ vì đã được truy cập sớm nhất
        // và "b", "c" vừa được truy cập
        cache.insert("d".to_string(), 4);
        
        assert_eq!(cache.get(&"a".to_string()), None);
        assert_eq!(cache.get(&"b".to_string()), Some(&2));
        assert_eq!(cache.get(&"c".to_string()), Some(&3));
        assert_eq!(cache.get(&"d".to_string()), Some(&4));
    }
    
    #[test]
    fn test_lru_cache_ttl() {
        let mut cache = LRUCache::with_ttl(3, Duration::from_millis(100));
        
        cache.insert("a".to_string(), 1);
        cache.insert("b".to_string(), 2);
        
        assert_eq!(cache.get(&"a".to_string()), Some(&1));
        assert_eq!(cache.get(&"b".to_string()), Some(&2));
        
        // Chờ cho entry hết hạn
        sleep(Duration::from_millis(150));
        
        // Khi get, các entry hết hạn sẽ bị xóa
        assert_eq!(cache.get(&"a".to_string()), None);
        assert_eq!(cache.get(&"b".to_string()), None);
    }
    
    #[test]
    fn test_lru_cache_update() {
        let mut cache = LRUCache::new(3);
        
        cache.insert("a".to_string(), 1);
        cache.insert("a".to_string(), 10);
        
        assert_eq!(cache.get(&"a".to_string()), Some(&10));
    }
    
    #[test]
    fn test_lru_cache_remove() {
        let mut cache = LRUCache::new(3);
        
        cache.insert("a".to_string(), 1);
        cache.insert("b".to_string(), 2);
        
        assert_eq!(cache.remove(&"a".to_string()), Some(1));
        assert_eq!(cache.get(&"a".to_string()), None);
        assert_eq!(cache.get(&"b".to_string()), Some(&2));
    }
    
    #[tokio::test]
    async fn test_async_cache() {
        let cache = create_async_lru_cache::<String, i32>(3, 5);
        
        cache.insert("a".to_string(), 1).await;
        cache.insert("b".to_string(), 2).await;
        
        // Test loader function
        let result = cache.get_or_load("c".to_string(), |_| async { Some(3) }).await;
        assert_eq!(result, Some(3));
        
        // Test that c was cached
        let contains = cache.contains(&"c".to_string()).await;
        assert!(contains);
        
        // Test removal
        let removed = cache.remove(&"b".to_string()).await;
        assert_eq!(removed, Some(2));
        
        // Verify b was removed
        let contains = cache.contains(&"b".to_string()).await;
        assert!(!contains);
    }
}

// Các tiện ích cụ thể cho domain wallet

use crate::users::subscription::user_subscription::UserSubscription;
use crate::users::vip_user::VipUserData;
use crate::users::premium_user::PremiumUserData;
use crate::users::subscription::constants::USER_DATA_CACHE_SECONDS;

// Tạo các singleton instance cho các loại cache chung
lazy_static::lazy_static! {
    /// Cache cho UserSubscription
    pub static ref SUBSCRIPTION_CACHE: AsyncCache<String, UserSubscription, LRUCache<String, UserSubscription>> = {
        create_async_lru_cache(500, USER_DATA_CACHE_SECONDS)
    };
    
    /// Cache cho VipUserData
    pub static ref VIP_USER_CACHE: AsyncCache<String, VipUserData, LRUCache<String, VipUserData>> = {
        create_async_lru_cache(200, USER_DATA_CACHE_SECONDS)
    };
    
    /// Cache cho PremiumUserData
    pub static ref PREMIUM_USER_CACHE: AsyncCache<String, PremiumUserData, LRUCache<String, PremiumUserData>> = {
        create_async_lru_cache(300, USER_DATA_CACHE_SECONDS)
    };
}

// Các hàm tiện ích để truy cập cache

/// Lấy thông tin đăng ký từ cache hoặc tải từ database
pub async fn get_subscription<F, Fut>(
    user_id: &str, 
    loader: F
) -> Option<UserSubscription>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Option<UserSubscription>>,
{
    SUBSCRIPTION_CACHE.get_or_load(user_id.to_string(), loader).await
}

/// Lấy thông tin VIP user từ cache hoặc tải từ database
pub async fn get_vip_user<F, Fut>(
    user_id: &str, 
    loader: F
) -> Option<VipUserData>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Option<VipUserData>>,
{
    VIP_USER_CACHE.get_or_load(user_id.to_string(), loader).await
}

/// Lấy thông tin Premium user từ cache hoặc tải từ database
pub async fn get_premium_user<F, Fut>(
    user_id: &str, 
    loader: F
) -> Option<PremiumUserData>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Option<PremiumUserData>>,
{
    PREMIUM_USER_CACHE.get_or_load(user_id.to_string(), loader).await
}

/// Thêm hoặc cập nhật subscription trong cache
pub async fn update_subscription_cache(user_id: &str, subscription: UserSubscription) {
    SUBSCRIPTION_CACHE.insert(user_id.to_string(), subscription).await;
}

/// Thêm hoặc cập nhật VIP user trong cache
pub async fn update_vip_user_cache(user_id: &str, user_data: VipUserData) {
    VIP_USER_CACHE.insert(user_id.to_string(), user_data).await;
}

/// Thêm hoặc cập nhật Premium user trong cache
pub async fn update_premium_user_cache(user_id: &str, user_data: PremiumUserData) {
    PREMIUM_USER_CACHE.insert(user_id.to_string(), user_data).await;
}

/// Xóa cache cho một người dùng cụ thể
pub async fn invalidate_user_cache(user_id: &str) {
    SUBSCRIPTION_CACHE.remove(&user_id.to_string()).await;
    VIP_USER_CACHE.remove(&user_id.to_string()).await;
    PREMIUM_USER_CACHE.remove(&user_id.to_string()).await;
}

/// Xóa toàn bộ cache (sử dụng khi cần refresh hoàn toàn)
pub async fn clear_all_caches() {
    SUBSCRIPTION_CACHE.clear().await;
    VIP_USER_CACHE.clear().await;
    PREMIUM_USER_CACHE.clear().await;
    
    debug!("Đã xóa toàn bộ cache người dùng");
}

/// Hàm helper tổng quát để lấy dữ liệu từ cache hoặc load từ source
/// 
/// # Type Parameters
/// - `K`: Kiểu của key
/// - `V`: Kiểu của value
/// - `C`: Kiểu của cache
/// - `F`: Kiểu của function loader
/// - `Fut`: Kiểu của Future trả về bởi function loader
/// 
/// # Arguments
/// - `cache`: Cache cần sử dụng
/// - `key`: Khóa cần tìm
/// - `loader`: Hàm loader được gọi khi không tìm thấy trong cache
/// 
/// # Returns
/// - `Option<V>`: Dữ liệu được tìm thấy hoặc None
pub async fn get_or_load_with_cache<K, V, C, F, Fut>(
    cache: &AsyncCache<K, V, C>,
    key: &str,
    loader: F
) -> Option<V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    C: Cache<K, V> + Send + Sync + 'static,
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Option<V>>,
{
    cache.get_or_load(key.to_string(), loader).await
}

// Thêm metric tracking
#[derive(Debug, Clone, Default)]
pub struct CacheMetrics {
    /// Số lượng requests vào cache
    pub requests: u64,
    /// Số lượng cache hits
    pub hits: u64,
    /// Số lượng cache misses
    pub misses: u64,
    /// Thời gian truy cập cache trung bình (ms)
    pub avg_access_time_ms: f64,
    /// Thời gian loading trung bình (ms)
    pub avg_load_time_ms: f64,
    /// Số lượng batch loads
    pub batch_loads: u64,
    /// Số lượng single loads
    pub single_loads: u64,
}

impl CacheMetrics {
    /// Tạo metrics mới
    pub fn new() -> Self {
        Self::default()
    }

    /// Cập nhật hit
    pub fn record_hit(&mut self, access_time_ms: f64) {
        self.requests += 1;
        self.hits += 1;
        self.avg_access_time_ms = (self.avg_access_time_ms * (self.hits as f64 - 1.0) + access_time_ms) / self.hits as f64;
    }

    /// Cập nhật miss
    pub fn record_miss(&mut self, access_time_ms: f64, load_time_ms: f64) {
        self.requests += 1;
        self.misses += 1;
        self.avg_access_time_ms = (self.avg_access_time_ms * (self.requests as f64 - 1.0) + access_time_ms) / self.requests as f64;
        self.avg_load_time_ms = (self.avg_load_time_ms * (self.misses as f64 - 1.0) + load_time_ms) / self.misses as f64;
    }

    /// Cập nhật batch load
    pub fn record_batch_load(&mut self) {
        self.batch_loads += 1;
    }

    /// Cập nhật single load
    pub fn record_single_load(&mut self) {
        self.single_loads += 1;
    }

    /// Lấy tỷ lệ hit
    pub fn hit_ratio(&self) -> f64 {
        if self.requests == 0 {
            0.0
        } else {
            self.hits as f64 / self.requests as f64
        }
    }

    /// Log metrics
    pub fn log(&self) {
        info!(
            "Cache Metrics: Requests={}, Hits={}, Misses={}, Hit Ratio={:.2}%, Avg Access Time={:.2}ms, Avg Load Time={:.2}ms, Batch Loads={}, Single Loads={}",
            self.requests,
            self.hits,
            self.misses,
            self.hit_ratio() * 100.0,
            self.avg_access_time_ms,
            self.avg_load_time_ms,
            self.batch_loads,
            self.single_loads
        );
    }
}

/// Cấu hình cho Cache
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Dung lượng tối đa của cache
    pub capacity: usize,
    /// TTL mặc định cho các entries
    pub ttl: Duration,
    /// Kích thước batch tối đa
    pub max_batch_size: usize,
    /// Thời gian chờ tối đa cho batch (ms)
    pub batch_wait_ms: u64,
    /// Có thu thập metrics không
    pub collect_metrics: bool,
    /// Chu kỳ ghi log metrics (số lần truy cập)
    pub metrics_log_interval: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            capacity: 100,
            ttl: Duration::from_secs(3600),
            max_batch_size: 50,
            batch_wait_ms: 50,
            collect_metrics: true,
            metrics_log_interval: 1000,
        }
    }
}

// Thêm vào impl AsyncCache
impl<K, V, C> AsyncCache<K, V, C>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    C: Cache<K, V> + Send + Sync + 'static,
{
    // ... existing methods ...
    
    /// Tạo mới AsyncCache với cấu hình
    pub fn with_config(cache: C, config: CacheConfig) -> Self {
        let metrics = if config.collect_metrics {
            Some(Arc::new(tokio::sync::Mutex::new(CacheMetrics::new())))
        } else {
            None
        };
        
        Self {
            cache: Arc::new(RwLock::new(cache)),
            ongoing_loads: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            batch_queue: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            metrics,
            config: Arc::new(config),
        }
    }
    
    // Thêm batch loading
    /// Tải nhiều keys cùng lúc
    pub async fn batch_get_or_load<F, Fut>(
        &self,
        keys: Vec<K>,
        batch_loader: F,
    ) -> HashMap<K, Option<V>>
    where
        F: FnOnce(Vec<K>) -> Fut,
        Fut: Future<Output = HashMap<K, Option<V>>>,
    {
        if keys.is_empty() {
            return HashMap::new();
        }
        
        if let Some(metrics) = &self.metrics {
            let mut metrics = metrics.lock().await;
            metrics.record_batch_load();
        }
        
        // Tìm các keys đã có trong cache
        let mut result = HashMap::with_capacity(keys.len());
        let mut missing_keys = Vec::new();
        
        {
            let cache = self.cache.read().await;
            for key in keys {
                match cache.get(&key) {
                    Some(value) => {
                        result.insert(key.clone(), Some(value.clone()));
                    }
                    None => {
                        missing_keys.push(key);
                    }
                }
            }
        }
        
        // Nếu có keys chưa có trong cache, gọi batch loader
        if !missing_keys.is_empty() {
            let start = Instant::now();
            let loaded = batch_loader(missing_keys.clone()).await;
            let load_time = start.elapsed().as_secs_f64() * 1000.0;
            
            // Cập nhật cache với các giá trị đã tải
            let mut cache = self.cache.write().await;
            for (key, value_opt) in loaded {
                if let Some(value) = value_opt.clone() {
                    cache.insert(key.clone(), value);
                }
                result.insert(key, value_opt);
            }
            
            // Cập nhật metrics
            if let Some(metrics) = &self.metrics {
                let mut metrics = metrics.lock().await;
                for _ in 0..missing_keys.len() {
                    metrics.record_miss(0.0, load_time);
                }
            }
        }
        
        result
    }
    
    /// Preload dữ liệu vào cache
    pub async fn preload<F, Fut>(&self, keys: Vec<K>, batch_loader: F)
    where
        F: FnOnce(Vec<K>) -> Fut,
        Fut: Future<Output = HashMap<K, Option<V>>>,
    {
        if keys.is_empty() {
            return;
        }
        
        // Lọc các keys đã có trong cache
        let missing_keys = {
            let cache = self.cache.read().await;
            keys.into_iter()
                .filter(|key| !cache.contains(key))
                .collect::<Vec<_>>()
        };
        
        if missing_keys.is_empty() {
            return;
        }
        
        // Tải dữ liệu
        let loaded = batch_loader(missing_keys).await;
        
        // Cập nhật cache
        let mut cache = self.cache.write().await;
        for (key, value_opt) in loaded {
            if let Some(value) = value_opt {
                cache.insert(key, value);
            }
        }
    }
    
    /// Clear các entries hết hạn
    pub async fn clear_expired(&self) -> usize {
        let mut cache = self.cache.write().await;
        if let Some(ttl_cache) = cache.as_ttl_cache_mut() {
            ttl_cache.clear_expired()
        } else {
            0
        }
    }
    
    /// Get metrics
    pub async fn get_metrics(&self) -> Option<CacheMetrics> {
        if let Some(metrics) = &self.metrics {
            let metrics = metrics.lock().await;
            Some(metrics.clone())
        } else {
            None
        }
    }
    
    /// Reset metrics
    pub async fn reset_metrics(&self) {
        if let Some(metrics) = &self.metrics {
            let mut metrics = metrics.lock().await;
            *metrics = CacheMetrics::new();
        }
    }
}

/// Tạo async LRU cache với cấu hình tùy chỉnh
pub fn create_async_lru_cache_with_config<K, V>(config: CacheConfig) -> AsyncCache<K, V, LRUCache<K, V>>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    let cache = LRUCache::with_ttl(config.capacity, config.ttl);
    AsyncCache::with_config(cache, config)
}

// Tạo độ ưu tiên cho cache invalidation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheInvalidationPriority {
    /// Ưu tiên thấp - Chỉ invalidate khi rõ ràng cần thiết
    Low,
    /// Ưu tiên trung bình - Invalidate sau một thời gian
    Medium,
    /// Ưu tiên cao - Invalidate ngay lập tức
    High,
    /// Ưu tiên tối đa - Invalidate tất cả các cache liên quan
    Critical,
}

/// Xóa cache thông minh cho một người dùng cụ thể dựa trên độ ưu tiên
pub async fn invalidate_user_cache_with_priority(user_id: &str, priority: CacheInvalidationPriority) {
    match priority {
        CacheInvalidationPriority::Low => {
            // Ưu tiên thấp: Thêm vào hàng đợi invalidation, không xóa ngay
            debug!("Đã thêm cache user {} vào hàng đợi invalidation với độ ưu tiên thấp", user_id);
        },
        CacheInvalidationPriority::Medium => {
            // Xóa các cache chính
            SUBSCRIPTION_CACHE.remove(&user_id.to_string()).await;
            debug!("Đã xóa cache subscription cho user {} với độ ưu tiên trung bình", user_id);
        },
        CacheInvalidationPriority::High => {
            // Xóa tất cả cache người dùng
            SUBSCRIPTION_CACHE.remove(&user_id.to_string()).await;
            VIP_USER_CACHE.remove(&user_id.to_string()).await;
            PREMIUM_USER_CACHE.remove(&user_id.to_string()).await;
            debug!("Đã xóa toàn bộ cache cho user {} với độ ưu tiên cao", user_id);
        },
        CacheInvalidationPriority::Critical => {
            // Xóa tất cả cache người dùng và các cache liên quan
            SUBSCRIPTION_CACHE.remove(&user_id.to_string()).await;
            VIP_USER_CACHE.remove(&user_id.to_string()).await;
            PREMIUM_USER_CACHE.remove(&user_id.to_string()).await;
            // TODO: Xóa các cache liên quan khác
            info!("Đã xóa tất cả cache cho user {} với độ ưu tiên tối đa", user_id);
        }
    }
}

/// Xóa toàn bộ cache (sử dụng khi cần refresh hoàn toàn)
pub async fn clear_all_caches() {
    SUBSCRIPTION_CACHE.clear().await;
    VIP_USER_CACHE.clear().await;
    PREMIUM_USER_CACHE.clear().await;
    
    debug!("Đã xóa toàn bộ cache người dùng");
}

/// Preload dữ liệu cho các người dùng VIP
pub async fn preload_vip_users<F, Fut>(user_ids: Vec<String>, loader: F)
where
    F: FnOnce(Vec<String>) -> Fut,
    Fut: Future<Output = HashMap<String, Option<VipUserData>>>,
{
    VIP_USER_CACHE.preload(user_ids, loader).await;
}

/// Preload dữ liệu cho các người dùng Premium
pub async fn preload_premium_users<F, Fut>(user_ids: Vec<String>, loader: F)
where
    F: FnOnce(Vec<String>) -> Fut,
    Fut: Future<Output = HashMap<String, Option<PremiumUserData>>>,
{
    PREMIUM_USER_CACHE.preload(user_ids, loader).await;
}

/// Preload dữ liệu cho các đăng ký
pub async fn preload_subscriptions<F, Fut>(user_ids: Vec<String>, loader: F)
where
    F: FnOnce(Vec<String>) -> Fut,
    Fut: Future<Output = HashMap<String, Option<UserSubscription>>>,
{
    SUBSCRIPTION_CACHE.preload(user_ids, loader).await;
}

/// Lấy metrics của cache
pub async fn get_cache_metrics() -> HashMap<String, Option<CacheMetrics>> {
    let mut metrics = HashMap::new();
    
    metrics.insert("subscription".to_string(), SUBSCRIPTION_CACHE.get_metrics().await);
    metrics.insert("vip_user".to_string(), VIP_USER_CACHE.get_metrics().await);
    metrics.insert("premium_user".to_string(), PREMIUM_USER_CACHE.get_metrics().await);
    
    metrics
}

/// Reset metrics của cache
pub async fn reset_cache_metrics() {
    SUBSCRIPTION_CACHE.reset_metrics().await;
    VIP_USER_CACHE.reset_metrics().await;
    PREMIUM_USER_CACHE.reset_metrics().await;
}

/// Hàm helper tổng quát để lấy dữ liệu từ cache hoặc load từ source
/// 
/// # Type Parameters
/// - `K`: Kiểu của key
/// - `V`: Kiểu của value
/// - `C`: Kiểu của cache
/// - `F`: Kiểu của function loader
/// - `Fut`: Kiểu của Future trả về bởi function loader
/// 
/// # Arguments
/// - `cache`: Cache cần sử dụng
/// - `key`: Khóa cần tìm
/// - `loader`: Hàm loader được gọi khi không tìm thấy trong cache
/// 
/// # Returns
/// - `Option<V>`: Dữ liệu được tìm thấy hoặc None
pub async fn get_or_load_with_cache<K, V, C, F, Fut>(
    cache: &AsyncCache<K, V, C>,
    key: &str,
    loader: F
) -> Option<V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    C: Cache<K, V> + Send + Sync + 'static,
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Option<V>>,
{
    cache.get_or_load(key.to_string(), loader).await
}

/// Hàm helper để lấy nhiều dữ liệu từ cache hoặc load từ source cùng lúc
/// 
/// # Type Parameters
/// - `K`: Kiểu của key
/// - `V`: Kiểu của value
/// - `C`: Kiểu của cache
/// - `F`: Kiểu của function batch loader
/// - `Fut`: Kiểu của Future trả về bởi function batch loader
/// 
/// # Arguments
/// - `cache`: Cache cần sử dụng
/// - `keys`: Danh sách các khóa cần tìm
/// - `loader`: Hàm batch loader được gọi với các khóa không tìm thấy trong cache
/// 
/// # Returns
/// - `HashMap<K, Option<V>>`: Map chứa dữ liệu cho mỗi khóa
pub async fn batch_get_or_load_with_cache<K, V, C, F, Fut>(
    cache: &AsyncCache<K, V, C>,
    keys: Vec<String>,
    loader: F
) -> HashMap<String, Option<V>>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    C: Cache<K, V> + Send + Sync + 'static,
    F: FnOnce(Vec<String>) -> Fut,
    Fut: std::future::Future<Output = HashMap<String, Option<V>>>,
{
    cache.batch_get_or_load(keys, loader).await
}
