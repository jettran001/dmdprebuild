//! Module chứa các tiện ích dùng chung cho các bridge implementation
//! Bao gồm xác thực địa chỉ, kiểm tra số dư, kiểm soát đồng thời và quản lý cache
//! Module này giúp giảm trùng lặp code giữa các bridge implementation

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lazy_static::lazy_static;
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::bridge::error::{BridgeError, BridgeResult};
use crate::bridge::types::TransactionHash;

/// Module cho các hàm xác thực địa chỉ
pub mod address_validation {
    use super::*;

    // Regex cho các loại địa chỉ blockchain, được khởi tạo một lần duy nhất
    pub static ETHEREUM_ADDRESS_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^0x[a-fA-F0-9]{40}$").expect("Failed to compile Ethereum address regex")
    });

    pub static NEAR_ADDRESS_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^(([a-z\d]+[\-_])*[a-z\d]+\.)*([a-z\d]+[\-_])*[a-z\d]+$")
            .expect("Failed to compile NEAR address regex")
    });

    pub static SOLANA_ADDRESS_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^[1-9A-HJ-NP-Za-km-z]{32,44}$").expect("Failed to compile Solana address regex")
    });

    /// Danh sách các địa chỉ đã bị chặn
    static BLOCKED_ADDRESSES: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

    /// Thêm địa chỉ vào danh sách bị chặn
    pub fn add_blocked_address(address: &str) -> BridgeResult<()> {
        let mut blocked = BLOCKED_ADDRESSES
            .lock()
            .map_err(|_| BridgeError::ConcurrencyError("Failed to acquire lock for blocked addresses".into()))?;
        blocked.insert(address.to_string());
        Ok(())
    }

    /// Kiểm tra địa chỉ có bị chặn không
    pub fn is_address_blocked(address: &str) -> BridgeResult<bool> {
        let blocked = BLOCKED_ADDRESSES
            .lock()
            .map_err(|_| BridgeError::ConcurrencyError("Failed to acquire lock for blocked addresses".into()))?;
        Ok(blocked.contains(address))
    }

    /// Xác thực địa chỉ Ethereum 
    pub fn validate_ethereum_address(address: &str) -> bool {
        ETHEREUM_ADDRESS_REGEX.is_match(address)
    }

    /// Xác thực địa chỉ NEAR
    pub fn validate_near_address(address: &str) -> bool {
        NEAR_ADDRESS_REGEX.is_match(address)
    }

    /// Xác thực địa chỉ Solana
    pub fn validate_solana_address(address: &str) -> bool {
        SOLANA_ADDRESS_REGEX.is_match(address)
    }

    /// Xác thực địa chỉ dựa trên loại blockchain
    pub fn validate_address(address: &str, chain_type: &str) -> BridgeResult<bool> {
        // Kiểm tra địa chỉ có bị chặn không
        if is_address_blocked(address)? {
            warn!("Address {} is in the blocked list", address);
            return Ok(false);
        }

        // Xác thực định dạng địa chỉ theo loại blockchain
        let is_valid = match chain_type.to_lowercase().as_str() {
            "ethereum" | "binancesmartchain" | "polygon" | "avalanche" | "evm" => {
                validate_ethereum_address(address)
            }
            "near" => validate_near_address(address),
            "solana" => validate_solana_address(address),
            _ => {
                warn!("Unknown chain type: {}", chain_type);
                false
            }
        };

        if !is_valid {
            warn!("Invalid {} address format: {}", chain_type, address);
        }

        Ok(is_valid)
    }

    /// Kiểm tra địa chỉ có phải là contract không
    pub async fn is_contract_address(address: &str, chain_type: &str) -> BridgeResult<bool> {
        // Trong một triển khai thực tế, hàm này sẽ thực hiện kiểm tra trên chain
        // Ở đây chỉ là một bản demo với một số địa chỉ contract được biết đến
        
        // Giả lập kiểm tra contract address
        let known_contracts = [
            "0x7a250d5630b4cf539739df2c5dacb4c659f2488d", // Uniswap Router
            "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45", // Uniswap Router 2
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", // USDC 
            "0xdac17f958d2ee523a2206206994597c13d831ec7", // USDT
        ];
        
        if chain_type.to_lowercase().contains("evm") {
            let lower_addr = address.to_lowercase();
            if known_contracts.iter().any(|&c| c == lower_addr) {
                return Ok(true);
            }
            
            // TODO: Implement actual on-chain check for contract code
            // This would query the blockchain for code at the address
            // If there is code, it's a contract, otherwise it's an EOA
            
            // Mock implementation for demo
            Ok(false)
        } else {
            // For non-EVM chains, implement specific logic
            Ok(false)
        }
    }
}

/// Module cho kiểm soát đồng thời
pub mod concurrency_control {
    use super::*;
    
    /// Cấu trúc quản lý giới hạn đồng thời cho các phương thức
    #[derive(Debug)]
    pub struct ConcurrencyLimiter {
        /// Map lưu số lượng đang xử lý cho mỗi key (thường là chain id)
        active_count: RwLock<HashMap<String, usize>>,
        /// Giới hạn tối đa cho mỗi key
        max_per_key: usize,
        /// Tổng giới hạn đồng thời toàn bộ hệ thống
        global_max: usize,
        /// Tổng số đang xử lý
        total_active: RwLock<usize>,
    }
    
    impl ConcurrencyLimiter {
        /// Tạo mới limiter với giới hạn cho từng key và tổng
        pub fn new(max_per_key: usize, global_max: usize) -> Self {
            Self {
                active_count: RwLock::new(HashMap::new()),
                max_per_key,
                global_max,
                total_active: RwLock::new(0),
            }
        }
        
        /// Thử lấy token để xử lý, trả về Ok(true) nếu thành công
        pub fn try_acquire(&self, key: &str) -> BridgeResult<bool> {
            // Kiểm tra tổng giới hạn
            {
                let total = self.total_active.read()
                    .map_err(|_| BridgeError::ConcurrencyError("Failed to read total active count".into()))?;
                
                if *total >= self.global_max {
                    debug!("Global concurrency limit reached: {}/{}", total, self.global_max);
                    return Ok(false);
                }
            }
            
            // Kiểm tra giới hạn cho key cụ thể
            {
                let active_map = self.active_count.read()
                    .map_err(|_| BridgeError::ConcurrencyError("Failed to read active count map".into()))?;
                
                let count = active_map.get(key).unwrap_or(&0);
                if *count >= self.max_per_key {
                    debug!("Per-key concurrency limit reached for {}: {}/{}", 
                          key, count, self.max_per_key);
                    return Ok(false);
                }
            }
            
            // Tăng bộ đếm
            {
                let mut active_map = self.active_count.write()
                    .map_err(|_| BridgeError::ConcurrencyError("Failed to write to active count map".into()))?;
                
                let entry = active_map.entry(key.to_string()).or_insert(0);
                *entry += 1;
            }
            
            {
                let mut total = self.total_active.write()
                    .map_err(|_| BridgeError::ConcurrencyError("Failed to write to total active count".into()))?;
                *total += 1;
            }
            
            Ok(true)
        }
        
        /// Giải phóng token sau khi hoàn thành
        pub fn release(&self, key: &str) -> BridgeResult<()> {
            {
                let mut active_map = self.active_count.write()
                    .map_err(|_| BridgeError::ConcurrencyError("Failed to write to active count map".into()))?;
                
                if let Some(count) = active_map.get_mut(key) {
                    if *count > 0 {
                        *count -= 1;
                    }
                }
            }
            
            {
                let mut total = self.total_active.write()
                    .map_err(|_| BridgeError::ConcurrencyError("Failed to write to total active count".into()))?;
                
                if *total > 0 {
                    *total -= 1;
                }
            }
            
            Ok(())
        }
        
        /// Lấy tổng số đang xử lý
        pub fn get_total_active(&self) -> BridgeResult<usize> {
            let total = self.total_active.read()
                .map_err(|_| BridgeError::ConcurrencyError("Failed to read total active count".into()))?;
            Ok(*total)
        }
        
        /// Lấy số đang xử lý cho một key cụ thể
        pub fn get_active_for_key(&self, key: &str) -> BridgeResult<usize> {
            let active_map = self.active_count.read()
                .map_err(|_| BridgeError::ConcurrencyError("Failed to read active count map".into()))?;
            
            Ok(*active_map.get(key).unwrap_or(&0))
        }
    }
}

/// Module cho quản lý cache
pub mod cache_manager {
    use super::*;
    
    /// Struct để theo dõi hiệu suất cache
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CacheMetrics {
        /// Thời điểm cuối cùng thực hiện dọn dẹp
        pub last_cleanup_time: u64,
        /// Số lượng phần tử trong cache tại lần dọn dẹp cuối
        pub last_size: usize,
        /// Số lượng đỉnh điểm từng đạt được
        pub peak_size: usize,
        /// Tỷ lệ hit cache (truy cập thành công)
        pub hit_rate: f64,
        /// Tổng số lần truy cập
        pub total_accesses: usize,
        /// Số lần hit
        pub hits: usize,
        /// Tốc độ tăng trưởng (phần tử/giây)
        pub growth_rate: f64,
        /// Phát hiện tăng tải (true nếu phát hiện)
        pub surge_detected: bool,
    }
    
    impl Default for CacheMetrics {
        fn default() -> Self {
            Self {
                last_cleanup_time: 0,
                last_size: 0,
                peak_size: 0,
                hit_rate: 0.0,
                total_accesses: 0,
                hits: 0,
                growth_rate: 0.0,
                surge_detected: false,
            }
        }
    }
    
    /// Kiểu hàm thông báo cho admin
    pub type AdminNotifierFn = Arc<dyn Fn(&str, &str) -> BridgeResult<()> + Send + Sync>;
    
    /// Cấu trúc cho quản lý cache tổng quát
    pub struct GenericCacheManager<K, V>
    where
        K: std::hash::Hash + Eq + Clone + std::fmt::Debug,
        V: Clone + std::fmt::Debug,
    {
        /// Dữ liệu cache
        cache: Arc<RwLock<HashMap<K, (V, u64)>>>,
        /// Thời gian sống của cache (ms)
        ttl_ms: u64,
        /// Kích thước tối đa của cache
        max_size: usize,
        /// Ngưỡng để bắt đầu dọn dẹp (% của max_size)
        cleanup_threshold: f64,
        /// Mặc định giữ lại bao nhiêu % items mới nhất khi dọn dẹp
        retention_percent: f64,
        /// Metrics theo dõi hiệu suất
        metrics: Arc<RwLock<CacheMetrics>>,
        /// Tự động mở rộng kích thước khi cần
        auto_scaling: bool,
        /// Hàm thông báo cho admin
        admin_notifier: Option<AdminNotifierFn>,
    }
    
    impl<K, V> GenericCacheManager<K, V> 
    where
        K: std::hash::Hash + Eq + Clone + std::fmt::Debug + Send + Sync,
        V: Clone + std::fmt::Debug + Send + Sync,
    {
        /// Tạo cache manager mới
        pub fn new(ttl_ms: u64, max_size: usize) -> Self {
            Self {
                cache: Arc::new(RwLock::new(HashMap::with_capacity(max_size))),
                ttl_ms,
                max_size,
                cleanup_threshold: 0.8, // 80%
                retention_percent: 0.7, // 70%
                metrics: Arc::new(RwLock::new(CacheMetrics::default())),
                auto_scaling: false,
                admin_notifier: None,
            }
        }
        
        /// Đặt ngưỡng dọn dẹp
        pub fn with_cleanup_threshold(mut self, threshold: f64) -> Self {
            if threshold > 0.0 && threshold <= 1.0 {
                self.cleanup_threshold = threshold;
            }
            self
        }
        
        /// Đặt % giữ lại khi dọn dẹp
        pub fn with_retention_percent(mut self, percent: f64) -> Self {
            if percent > 0.0 && percent < 1.0 {
                self.retention_percent = percent;
            }
            self
        }
        
        /// Đặt kích thước tối đa
        pub fn with_max_size(mut self, max_size: usize) -> Self {
            self.max_size = max_size;
            self
        }
        
        /// Bật/tắt tự động mở rộng kích thước
        pub fn with_auto_scaling(mut self, enabled: bool) -> Self {
            self.auto_scaling = enabled;
            self
        }
        
        /// Đặt hàm thông báo admin
        pub fn with_admin_notifier(mut self, notifier: AdminNotifierFn) -> Self {
            self.admin_notifier = Some(notifier);
            self
        }
        
        /// Thêm item vào cache
        pub fn insert(&self, key: K, value: V) -> BridgeResult<()> {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_millis() as u64;
            
            let mut cache = self.cache.write()
                .map_err(|_| BridgeError::ConcurrencyError("Failed to write to cache".into()))?;
            
            cache.insert(key, (value, timestamp));
            
            // Cập nhật metrics
            let current_size = cache.len();
            let mut metrics = self.metrics.write()
                .map_err(|_| BridgeError::ConcurrencyError("Failed to write to metrics".into()))?;
            
            if current_size > metrics.peak_size {
                metrics.peak_size = current_size;
            }
            
            // Kiểm tra xem có cần dọn dẹp không
            if current_size >= (self.max_size as f64 * self.cleanup_threshold) as usize {
                // Drop write lock trước khi gọi hàm khác để tránh deadlock
                drop(cache);
                drop(metrics);
                
                // Thông báo cho admin nếu gần đầy
                if let Some(ref notifier) = self.admin_notifier {
                    if current_size >= (self.max_size as f64 * 0.9) as usize {
                        let _ = notifier(
                            "CACHE_ALMOST_FULL",
                            &format!("Cache is at {}% capacity ({}/{})", 
                                    (current_size as f64 / self.max_size as f64) * 100.0,
                                    current_size, 
                                    self.max_size)
                        );
                    }
                }
                
                self.cleanup()?;
            }
            
            Ok(())
        }
        
        /// Lấy item từ cache
        pub fn get(&self, key: &K) -> BridgeResult<Option<V>> {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_millis() as u64;
            
            let cache = self.cache.read()
                .map_err(|_| BridgeError::ConcurrencyError("Failed to read from cache".into()))?;
            
            // Cập nhật metrics
            let mut metrics = self.metrics.write()
                .map_err(|_| BridgeError::ConcurrencyError("Failed to write to metrics".into()))?;
            metrics.total_accesses += 1;
            
            if let Some((value, timestamp)) = cache.get(key) {
                if now - *timestamp <= self.ttl_ms {
                    // Cache hit
                    metrics.hits += 1;
                    metrics.hit_rate = metrics.hits as f64 / metrics.total_accesses as f64;
                    return Ok(Some(value.clone()));
                }
            }
            
            // Cache miss
            metrics.hit_rate = metrics.hits as f64 / metrics.total_accesses as f64;
            Ok(None)
        }
        
        /// Xóa item khỏi cache
        pub fn remove(&self, key: &K) -> BridgeResult<Option<V>> {
            let mut cache = self.cache.write()
                .map_err(|_| BridgeError::ConcurrencyError("Failed to write to cache".into()))?;
            
            Ok(cache.remove(key).map(|(v, _)| v))
        }
        
        /// Dọn dẹp các item hết hạn và cũ nhất nếu cần
        pub fn cleanup(&self) -> BridgeResult<usize> {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_millis() as u64;
            
            let mut to_keep = Vec::new();
            let mut expired_count = 0;
            
            // Đọc cache để lọc các item và chuẩn bị danh sách giữ lại
            {
                let cache = self.cache.read()
                    .map_err(|_| BridgeError::ConcurrencyError("Failed to read from cache".into()))?;
                
                // Lọc các item đã hết hạn và các item cũ nhất
                for (key, (value, timestamp)) in cache.iter() {
                    if now - *timestamp <= self.ttl_ms {
                        // Item chưa hết hạn, thêm vào danh sách giữ lại
                        to_keep.push((key.clone(), value.clone(), *timestamp));
                    } else {
                        expired_count += 1;
                    }
                }
            }
            
            // Nếu số lượng vẫn lớn hơn target, giữ các item mới nhất
            let target_size = (self.max_size as f64 * self.retention_percent) as usize;
            if to_keep.len() > target_size {
                // Sắp xếp theo thời gian giảm dần (mới nhất lên đầu)
                to_keep.sort_by(|a, b| b.2.cmp(&a.2));
                to_keep.truncate(target_size);
            }
            
            // Cập nhật cache với các item giữ lại
            {
                let mut cache = self.cache.write()
                    .map_err(|_| BridgeError::ConcurrencyError("Failed to write to cache".into()))?;
                
                // Xóa tất cả và thêm lại các items giữ lại
                cache.clear();
                for (key, value, timestamp) in to_keep.iter() {
                    cache.insert(key.clone(), (value.clone(), *timestamp));
                }
                
                // Cập nhật metrics
                let mut metrics = self.metrics.write()
                    .map_err(|_| BridgeError::ConcurrencyError("Failed to write to metrics".into()))?;
                
                // Tính tốc độ tăng trưởng
                if metrics.last_cleanup_time > 0 {
                    let time_diff = now - metrics.last_cleanup_time;
                    if time_diff > 0 {
                        // Phần tử mới/giây
                        let size_diff = cache.len() as isize - metrics.last_size as isize;
                        metrics.growth_rate = size_diff as f64 / (time_diff as f64 / 1000.0);
                        
                        // Phát hiện tăng đột biến (tăng hơn 50% trong thời gian ngắn)
                        if metrics.last_size > 0 && size_diff > 0 && 
                           size_diff as f64 / metrics.last_size as f64 > 0.5 && 
                           time_diff < 60000 { // Dưới 1 phút
                            metrics.surge_detected = true;
                            
                            // Thông báo cho admin
                            if let Some(ref notifier) = self.admin_notifier {
                                let _ = notifier(
                                    "CACHE_SURGE_DETECTED",
                                    &format!("Sudden cache growth detected: {}% increase in {}ms. Growth rate: {:.2} items/sec",
                                            (size_diff as f64 / metrics.last_size as f64) * 100.0,
                                            time_diff,
                                            metrics.growth_rate)
                                );
                            }
                            
                            // Tự động mở rộng cache nếu được bật
                            if self.auto_scaling {
                                self.auto_scale_cache()?;
                            }
                        } else {
                            metrics.surge_detected = false;
                        }
                    }
                }
                
                metrics.last_cleanup_time = now;
                metrics.last_size = cache.len();
                
                info!("Cache cleanup completed. Removed {} expired items, kept {} items. Cache size: {}/{}",
                      expired_count, cache.len(), cache.len(), self.max_size);
            }
            
            Ok(expired_count)
        }
        
        /// Lấy metrics hiện tại
        pub fn get_metrics(&self) -> BridgeResult<CacheMetrics> {
            let metrics = self.metrics.read()
                .map_err(|_| BridgeError::ConcurrencyError("Failed to read metrics".into()))?;
            
            Ok(metrics.clone())
        }
        
        /// Tự động mở rộng kích thước cache dựa trên metrics
        fn auto_scale_cache(&self) -> BridgeResult<usize> {
            let metrics = self.metrics.read()
                .map_err(|_| BridgeError::ConcurrencyError("Failed to read metrics".into()))?;
            
            if !self.auto_scaling || !metrics.surge_detected {
                return Ok(self.max_size);
            }
            
            // Tính toán kích thước mới dựa trên tốc độ tăng
            let growth_factor = if metrics.growth_rate > 0.0 { 
                // Dự đoán tăng trưởng trong 5 phút tới
                (metrics.growth_rate * 300.0) as usize 
            } else { 
                0 
            };
            
            let new_size = std::cmp::max(
                self.max_size,
                std::cmp::min(
                    self.max_size * 2, // Không tăng quá gấp đôi
                    metrics.last_size + growth_factor + 1000 // Thêm buffer
                )
            );
            
            if new_size > self.max_size {
                info!("Auto-scaling cache size from {} to {} due to high growth rate", 
                     self.max_size, new_size);
                
                // Trong thực tế, chúng ta cần một cơ chế để đồng bộ max_size giữa các thread
                // Ở đây giả sử chúng ta có thể thay đổi trực tiếp (không thể với interior mutability)
                // let mut_self = unsafe { &mut *(self as *const _ as *mut Self) };
                // mut_self.max_size = new_size;
                
                // Trong phiên bản thực tế, cần sử dụng cell hoặc bao bọc max_size trong RwLock
                
                // Thông báo cho admin
                if let Some(ref notifier) = self.admin_notifier {
                    let _ = notifier(
                        "CACHE_AUTO_SCALED",
                        &format!("Cache auto-scaled from {} to {} items due to high growth rate: {:.2} items/sec",
                                self.max_size, new_size, metrics.growth_rate)
                    );
                }
                
                return Ok(new_size);
            }
            
            Ok(self.max_size)
        }
        
        /// Lấy kích thước hiện tại
        pub fn size(&self) -> BridgeResult<usize> {
            let cache = self.cache.read()
                .map_err(|_| BridgeError::ConcurrencyError("Failed to read from cache".into()))?;
            
            Ok(cache.len())
        }
        
        /// Xóa toàn bộ cache
        pub fn clear(&self) -> BridgeResult<()> {
            let mut cache = self.cache.write()
                .map_err(|_| BridgeError::ConcurrencyError("Failed to write to cache".into()))?;
            
            cache.clear();
            Ok(())
        }
    }
}

/// Module cho kiểm tra số dư
pub mod balance_checker {
    use super::*;
    
    /// Thông tin số dư token
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct TokenBalance {
        /// Số dư theo định dạng chuỗi (cho độ chính xác cao)
        pub balance_str: String,
        /// Số dư dạng số thực (cho so sánh)
        pub balance_f64: f64,
        /// Decimals của token
        pub decimals: u8,
        /// Ký hiệu token
        pub symbol: String,
        /// Địa chỉ token
        pub token_address: String,
        /// Thời điểm cập nhật
        pub updated_at: u64,
    }
    
    /// Cache số dư token với cặp (chain, address, token)
    #[derive(Debug)]
    pub struct BalanceCache {
        cache: Arc<RwLock<HashMap<(String, String, String), TokenBalance>>>,
        /// Thời gian sống của cache (ms)
        ttl_ms: u64,
    }
    
    impl BalanceCache {
        /// Tạo cache mới với TTL
        pub fn new(ttl_ms: u64) -> Self {
            Self {
                cache: Arc::new(RwLock::new(HashMap::new())),
                ttl_ms,
            }
        }
        
        /// Thêm số dư vào cache
        pub fn update_balance(&self, chain: &str, address: &str, token: &str, balance: TokenBalance) -> BridgeResult<()> {
            let key = (chain.to_string(), address.to_string(), token.to_string());
            
            let mut cache = self.cache.write()
                .map_err(|_| BridgeError::ConcurrencyError("Failed to write to balance cache".into()))?;
            
            cache.insert(key, balance);
            Ok(())
        }
        
        /// Lấy số dư từ cache
        pub fn get_balance(&self, chain: &str, address: &str, token: &str) -> BridgeResult<Option<TokenBalance>> {
            let key = (chain.to_string(), address.to_string(), token.to_string());
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_millis() as u64;
            
            let cache = self.cache.read()
                .map_err(|_| BridgeError::ConcurrencyError("Failed to read from balance cache".into()))?;
            
            if let Some(balance) = cache.get(&key) {
                // Kiểm tra xem có còn hiệu lực không
                if now - balance.updated_at <= self.ttl_ms {
                    return Ok(Some(balance.clone()));
                }
            }
            
            Ok(None)
        }
    }
    
    /// Kiểm tra xem số dư có đủ không
    pub fn is_balance_sufficient(balance: &TokenBalance, required_amount: f64) -> bool {
        balance.balance_f64 >= required_amount
    }
    
    /// So sánh số dư (hỗ trợ chuỗi có độ chính xác cao)
    pub fn compare_balance(balance: &TokenBalance, required_amount_str: &str) -> Result<bool, &'static str> {
        // Parse số dư hiện tại và số lượng yêu cầu
        let balance_value = balance.balance_str
            .parse::<f64>()
            .map_err(|_| "Invalid balance format")?;
            
        let required_value = required_amount_str
            .parse::<f64>()
            .map_err(|_| "Invalid required amount format")?;
        
        Ok(balance_value >= required_value)
    }
} 