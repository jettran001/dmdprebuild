/// Blockchain data caching module
///
/// This module provides caching mechanisms for blockchain data
/// to improve performance and reduce RPC calls.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use anyhow::{Result, Context};
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};
use redis::{Client, AsyncCommands, aio::Connection};
use tracing::{debug, error, info, warn};

use ethers::types::{Address, H256, U256};

// Types for cached data
type TokenPrice = f64;
type TokenMetadata = HashMap<String, String>;
type TransactionData = Vec<u8>;

/// Cache provider trait defining the interface for cache implementations
#[async_trait]
pub trait CacheProvider: Send + Sync + 'static {
    /// Get token price from cache
    async fn get_token_price(&self, chain_id: u32, token_address: &str) -> Result<Option<TokenPrice>>;
    
    /// Set token price in cache with TTL
    async fn set_token_price(&self, chain_id: u32, token_address: &str, price: TokenPrice, ttl_seconds: u64) -> Result<()>;
    
    /// Get token metadata from cache
    async fn get_token_metadata(&self, chain_id: u32, token_address: &str) -> Result<Option<TokenMetadata>>;
    
    /// Set token metadata in cache with TTL
    async fn set_token_metadata(&self, chain_id: u32, token_address: &str, metadata: TokenMetadata, ttl_seconds: u64) -> Result<()>;
    
    /// Get transaction data from cache
    async fn get_transaction(&self, tx_hash: &str) -> Result<Option<TransactionData>>;
    
    /// Set transaction data in cache
    async fn set_transaction(&self, tx_hash: &str, data: TransactionData, ttl_seconds: u64) -> Result<()>;
    
    /// Get arbitrary data from cache
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    
    /// Set arbitrary data in cache
    async fn set(&self, key: &str, value: Vec<u8>, ttl_seconds: u64) -> Result<()>;
    
    /// Delete a key from cache
    async fn delete(&self, key: &str) -> Result<()>;
    
    /// Clear all cache data
    async fn clear(&self) -> Result<()>;
    
    /// Check if a key exists in cache
    async fn exists(&self, key: &str) -> Result<bool>;
    
    /// Get cache statistics
    async fn stats(&self) -> Result<CacheStats>;
}

/// In-memory cache implementation
pub struct InMemoryCache {
    /// Memory storage for cache items
    cache: RwLock<HashMap<String, CacheItem>>,
    /// Maximum items to keep in memory
    max_items: usize,
    /// Statistics
    stats: RwLock<CacheStats>,
}

/// Redis cache implementation
pub struct RedisCache {
    /// Redis client
    client: Client,
    /// Statistics
    stats: RwLock<CacheStats>,
}

/// Cache item with TTL
struct CacheItem {
    /// Cached data
    data: Vec<u8>,
    /// Expiration time
    expires_at: Instant,
}

/// Cache statistics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheStats {
    /// Total number of items in cache
    pub item_count: usize,
    /// Total number of cache hits
    pub hits: usize,
    /// Total number of cache misses
    pub misses: usize,
    /// Cache hit ratio (hits / (hits + misses))
    pub hit_ratio: f64,
}

impl InMemoryCache {
    /// Create a new in-memory cache
    pub fn new(max_items: usize) -> Self {
        Self {
            cache: RwLock::new(HashMap::with_capacity(max_items)),
            max_items,
            stats: RwLock::new(CacheStats::default()),
        }
    }
    
    /// Format a cache key with prefix for different data types
    fn format_key(&self, prefix: &str, key: &str) -> String {
        format!("{}:{}", prefix, key)
    }
    
    /// Remove expired items from cache
    async fn cleanup(&self) {
        let mut cache = self.cache.write().await;
        let now = Instant::now();
        
        // Get expired keys
        let expired_keys: Vec<String> = cache.iter()
            .filter(|(_, item)| item.expires_at <= now)
            .map(|(key, _)| key.clone())
            .collect();
        
        // Remove expired items
        for key in expired_keys {
            cache.remove(&key);
        }
    }
    
    /// Evict items if cache is full
    async fn evict_if_needed(&self) {
        let mut cache = self.cache.write().await;
        
        if cache.len() >= self.max_items {
            debug!("Cache is full, evicting oldest items");
            
            // Get all keys sorted by expiration time
            let mut keys_by_expiration: Vec<(String, Instant)> = cache.iter()
                .map(|(key, item)| (key.clone(), item.expires_at))
                .collect();
            
            // Sort by expiration time (oldest first)
            keys_by_expiration.sort_by(|a, b| a.1.cmp(&b.1));
            
            // Remove oldest 10% or at least one item
            let items_to_remove = std::cmp::max(1, self.max_items / 10);
            
            for i in 0..items_to_remove {
                if i < keys_by_expiration.len() {
                    cache.remove(&keys_by_expiration[i].0);
                }
            }
        }
    }
}

impl RedisCache {
    /// Create a new Redis cache
    pub fn new(redis_url: &str) -> Result<Self> {
        let client = Client::open(redis_url)
            .with_context(|| format!("Failed to connect to Redis at {}", redis_url))?;
        
        Ok(Self {
            client,
            stats: RwLock::new(CacheStats::default()),
        })
    }
    
    /// Get a Redis connection
    async fn get_connection(&self) -> Result<Connection> {
        let conn = self.client.get_async_connection().await
            .context("Failed to get Redis connection")?;
        
        Ok(conn)
    }
    
    /// Format a cache key with prefix for different data types
    fn format_key(&self, prefix: &str, key: &str) -> String {
        format!("snipebot:{}:{}", prefix, key)
    }
}

#[async_trait]
impl CacheProvider for InMemoryCache {
    async fn get_token_price(&self, chain_id: u32, token_address: &str) -> Result<Option<TokenPrice>> {
        let key = self.format_key("price", &format!("{}:{}", chain_id, token_address));
        let result = self.get(&key).await?;
        
        if let Some(data) = result {
            let price = bincode::deserialize::<TokenPrice>(&data)
                .context("Failed to deserialize token price")?;
            Ok(Some(price))
        } else {
            Ok(None)
        }
    }
    
    async fn set_token_price(&self, chain_id: u32, token_address: &str, price: TokenPrice, ttl_seconds: u64) -> Result<()> {
        let key = self.format_key("price", &format!("{}:{}", chain_id, token_address));
        let data = bincode::serialize(&price)
            .context("Failed to serialize token price")?;
        
        self.set(&key, data, ttl_seconds).await
    }
    
    async fn get_token_metadata(&self, chain_id: u32, token_address: &str) -> Result<Option<TokenMetadata>> {
        let key = self.format_key("metadata", &format!("{}:{}", chain_id, token_address));
        let result = self.get(&key).await?;
        
        if let Some(data) = result {
            let metadata = bincode::deserialize::<TokenMetadata>(&data)
                .context("Failed to deserialize token metadata")?;
            Ok(Some(metadata))
        } else {
            Ok(None)
        }
    }
    
    async fn set_token_metadata(&self, chain_id: u32, token_address: &str, metadata: TokenMetadata, ttl_seconds: u64) -> Result<()> {
        let key = self.format_key("metadata", &format!("{}:{}", chain_id, token_address));
        let data = bincode::serialize(&metadata)
            .context("Failed to serialize token metadata")?;
        
        self.set(&key, data, ttl_seconds).await
    }
    
    async fn get_transaction(&self, tx_hash: &str) -> Result<Option<TransactionData>> {
        let key = self.format_key("tx", tx_hash);
        self.get(&key).await
    }
    
    async fn set_transaction(&self, tx_hash: &str, data: TransactionData, ttl_seconds: u64) -> Result<()> {
        let key = self.format_key("tx", tx_hash);
        self.set(&key, data, ttl_seconds).await
    }
    
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        // Update stats
        {
            let mut stats = self.stats.write().await;
            
            if self.cache.read().await.contains_key(key) {
                stats.hits += 1;
            } else {
                stats.misses += 1;
            }
            
            let total = stats.hits + stats.misses;
            if total > 0 {
                stats.hit_ratio = stats.hits as f64 / total as f64;
            }
        }
        
        // Clean up expired items occasionally (1% chance)
        if rand::random::<f64>() < 0.01 {
            self.cleanup().await;
        }
        
        let cache = self.cache.read().await;
        if let Some(item) = cache.get(key) {
            if item.expires_at > Instant::now() {
                return Ok(Some(item.data.clone()));
            }
        }
        
        Ok(None)
    }
    
    async fn set(&self, key: &str, value: Vec<u8>, ttl_seconds: u64) -> Result<()> {
        // Ensure cache isn't full
        self.evict_if_needed().await;
        
        let expires_at = Instant::now() + Duration::from_secs(ttl_seconds);
        let item = CacheItem {
            data: value,
            expires_at,
        };
        
        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(key.to_string(), item);
            
            // Update stats
            let mut stats = self.stats.write().await;
            stats.item_count = cache.len();
        }
        
        Ok(())
    }
    
    async fn delete(&self, key: &str) -> Result<()> {
        let mut cache = self.cache.write().await;
        cache.remove(key);
        
        // Update stats
        let mut stats = self.stats.write().await;
        stats.item_count = cache.len();
        
        Ok(())
    }
    
    async fn clear(&self) -> Result<()> {
        let mut cache = self.cache.write().await;
        cache.clear();
        
        // Reset stats
        let mut stats = self.stats.write().await;
        *stats = CacheStats::default();
        
        Ok(())
    }
    
    async fn exists(&self, key: &str) -> Result<bool> {
        let cache = self.cache.read().await;
        Ok(cache.contains_key(key) && cache[key].expires_at > Instant::now())
    }
    
    async fn stats(&self) -> Result<CacheStats> {
        Ok(self.stats.read().await.clone())
    }
}

#[async_trait]
impl CacheProvider for RedisCache {
    async fn get_token_price(&self, chain_id: u32, token_address: &str) -> Result<Option<TokenPrice>> {
        let key = self.format_key("price", &format!("{}:{}", chain_id, token_address));
        let result = self.get(&key).await?;
        
        if let Some(data) = result {
            let price = bincode::deserialize::<TokenPrice>(&data)
                .context("Failed to deserialize token price")?;
            Ok(Some(price))
        } else {
            Ok(None)
        }
    }
    
    async fn set_token_price(&self, chain_id: u32, token_address: &str, price: TokenPrice, ttl_seconds: u64) -> Result<()> {
        let key = self.format_key("price", &format!("{}:{}", chain_id, token_address));
        let data = bincode::serialize(&price)
            .context("Failed to serialize token price")?;
        
        self.set(&key, data, ttl_seconds).await
    }
    
    async fn get_token_metadata(&self, chain_id: u32, token_address: &str) -> Result<Option<TokenMetadata>> {
        let key = self.format_key("metadata", &format!("{}:{}", chain_id, token_address));
        let result = self.get(&key).await?;
        
        if let Some(data) = result {
            let metadata = bincode::deserialize::<TokenMetadata>(&data)
                .context("Failed to deserialize token metadata")?;
            Ok(Some(metadata))
        } else {
            Ok(None)
        }
    }
    
    async fn set_token_metadata(&self, chain_id: u32, token_address: &str, metadata: TokenMetadata, ttl_seconds: u64) -> Result<()> {
        let key = self.format_key("metadata", &format!("{}:{}", chain_id, token_address));
        let data = bincode::serialize(&metadata)
            .context("Failed to serialize token metadata")?;
        
        self.set(&key, data, ttl_seconds).await
    }
    
    async fn get_transaction(&self, tx_hash: &str) -> Result<Option<TransactionData>> {
        let key = self.format_key("tx", tx_hash);
        self.get(&key).await
    }
    
    async fn set_transaction(&self, tx_hash: &str, data: TransactionData, ttl_seconds: u64) -> Result<()> {
        let key = self.format_key("tx", tx_hash);
        self.set(&key, data, ttl_seconds).await
    }
    
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let mut conn = self.get_connection().await?;
        
        let exists: bool = conn.exists(key).await
            .context("Failed to check if key exists in Redis")?;
        
        // Update stats
        {
            let mut stats = self.stats.write().await;
            if exists {
                stats.hits += 1;
            } else {
                stats.misses += 1;
            }
            
            let total = stats.hits + stats.misses;
            if total > 0 {
                stats.hit_ratio = stats.hits as f64 / total as f64;
            }
        }
        
        if !exists {
            return Ok(None);
        }
        
        let data: Vec<u8> = conn.get(key).await
            .context("Failed to get data from Redis")?;
        
        Ok(Some(data))
    }
    
    async fn set(&self, key: &str, value: Vec<u8>, ttl_seconds: u64) -> Result<()> {
        let mut conn = self.get_connection().await?;
        
        conn.set_ex(key, value, ttl_seconds as usize).await
            .context("Failed to set data in Redis")?;
        
        // Update stats
        let mut stats = self.stats.write().await;
        stats.item_count += 1;
        
        Ok(())
    }
    
    async fn delete(&self, key: &str) -> Result<()> {
        let mut conn = self.get_connection().await?;
        
        conn.del(key).await
            .context("Failed to delete key from Redis")?;
        
        // Update stats (approximation)
        let mut stats = self.stats.write().await;
        if stats.item_count > 0 {
            stats.item_count -= 1;
        }
        
        Ok(())
    }
    
    async fn clear(&self) -> Result<()> {
        let mut conn = self.get_connection().await?;
        
        // Delete all keys with pattern snipebot:*
        let pattern = "snipebot:*";
        let keys: Vec<String> = conn.keys(pattern).await
            .context("Failed to get keys from Redis")?;
        
        if !keys.is_empty() {
            conn.del(&keys).await
                .context("Failed to delete keys from Redis")?;
        }
        
        // Reset stats
        let mut stats = self.stats.write().await;
        *stats = CacheStats::default();
        
        Ok(())
    }
    
    async fn exists(&self, key: &str) -> Result<bool> {
        let mut conn = self.get_connection().await?;
        
        let exists: bool = conn.exists(key).await
            .context("Failed to check if key exists in Redis")?;
        
        Ok(exists)
    }
    
    async fn stats(&self) -> Result<CacheStats> {
        let mut conn = self.get_connection().await?;
        
        // Get Redis database size for item count
        let db_size: usize = conn.dbsize().await
            .context("Failed to get Redis database size")?;
        
        // Update stats with actual item count
        {
            let mut stats = self.stats.write().await;
            stats.item_count = db_size;
        }
        
        Ok(self.stats.read().await.clone())
    }
}

/// Factory function to create a cache provider based on configuration
pub fn create_cache_provider(redis_url: Option<&str>) -> Arc<dyn CacheProvider> {
    if let Some(url) = redis_url {
        match RedisCache::new(url) {
            Ok(cache) => {
                info!("Using Redis cache at {}", url);
                Arc::new(cache)
            }
            Err(e) => {
                error!("Failed to create Redis cache: {}. Falling back to in-memory cache", e);
                Arc::new(InMemoryCache::new(10000))
            }
        }
    } else {
        info!("Using in-memory cache");
        Arc::new(InMemoryCache::new(10000))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_in_memory_cache() {
        let cache = InMemoryCache::new(100);
        
        // Test set/get
        cache.set("test_key", vec![1, 2, 3], 60).await.unwrap();
        let result = cache.get("test_key").await.unwrap();
        assert_eq!(result, Some(vec![1, 2, 3]));
        
        // Test exists
        assert!(cache.exists("test_key").await.unwrap());
        assert!(!cache.exists("non_existent").await.unwrap());
        
        // Test delete
        cache.delete("test_key").await.unwrap();
        assert!(!cache.exists("test_key").await.unwrap());
        
        // Test token price
        cache.set_token_price(1, "0x123", 100.5, 60).await.unwrap();
        let price = cache.get_token_price(1, "0x123").await.unwrap();
        assert_eq!(price, Some(100.5));
    }
}
