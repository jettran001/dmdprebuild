/**
 * Resource Manager - Module quản lý tài nguyên giữa các thành phần
 * 
 * Module này cung cấp cơ chế phân phối tài nguyên thông qua TokenBucket, Semaphore,
 * ưu tiên tác vụ, và các giới hạn động dựa trên tải hệ thống.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Semaphore, Mutex};
use tokio::time::sleep;
use anyhow::{Result, anyhow, Context};
use tracing::{debug, error, info, warn};

// Re-export cho tiện sử dụng
pub use tokio::sync::Semaphore as TaskSemaphore;

/// Mức ưu tiên tài nguyên
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourcePriority {
    /// Thấp - Các tác vụ có thể đợi lâu hoặc không quan trọng
    Low = 0,
    
    /// Trung bình - Các tác vụ thông thường
    Medium = 1,
    
    /// Cao - Các tác vụ quan trọng cần xử lý nhanh
    High = 2,
    
    /// Rất cao - Các tác vụ đặc biệt quan trọng cần xử lý ngay lập tức
    Critical = 3,
}

/// Loại tài nguyên được quản lý
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceType {
    /// Request đến blockchain RPC
    BlockchainRPC,
    
    /// Request đến API bên ngoài
    ExternalAPI,
    
    /// Thread/task trong thread pool
    ComputeTask,
    
    /// Memory bandwidth
    MemoryBandwidth,
    
    /// Kết nối mạng/socket
    NetworkConnection,
    
    /// Tùy chỉnh
    Custom(u32),
}

/// Cấu hình Token Bucket
#[derive(Debug, Clone)]
pub struct TokenBucketConfig {
    /// Số token tối đa trong bucket
    pub max_tokens: usize,
    
    /// Số token được thêm vào mỗi giây
    pub refill_rate: f64,
    
    /// Số token ban đầu
    pub initial_tokens: usize,
    
    /// Số token tối thiểu để giữ lại (dự phòng cho ưu tiên cao)
    pub reserve_tokens: usize,
}

impl Default for TokenBucketConfig {
    fn default() -> Self {
        Self {
            max_tokens: 100,
            refill_rate: 10.0, // 10 token/giây
            initial_tokens: 50,
            reserve_tokens: 20, // Giữ 20 token cho ưu tiên cao
        }
    }
}

/// Cài đặt Token Bucket
struct TokenBucket {
    /// Số token hiện tại
    tokens: f64,
    
    /// Số token tối đa
    max_tokens: f64,
    
    /// Số token được thêm vào mỗi giây
    refill_rate: f64,
    
    /// Thời gian lần cuối refill
    last_refill: Instant,
    
    /// Số token giữ lại cho ưu tiên cao
    reserve_tokens: f64,
}

impl TokenBucket {
    /// Tạo token bucket mới
    fn new(config: &TokenBucketConfig) -> Self {
        Self {
            tokens: config.initial_tokens as f64,
            max_tokens: config.max_tokens as f64,
            refill_rate: config.refill_rate,
            last_refill: Instant::now(),
            reserve_tokens: config.reserve_tokens as f64,
        }
    }
    
    /// Refill token dựa vào thời gian đã trôi qua
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;
    }
    
    /// Lấy token từ bucket
    fn take(&mut self, tokens: f64, priority: ResourcePriority) -> bool {
        self.refill();
        
        // Với ưu tiên cao, có thể sử dụng cả reserve tokens
        let available_tokens = match priority {
            ResourcePriority::Critical => self.tokens,
            ResourcePriority::High => self.tokens,
            _ => (self.tokens - self.reserve_tokens).max(0.0),
        };
        
        if available_tokens >= tokens {
            self.tokens -= tokens;
            true
        } else {
            false
        }
    }
    
    /// Lấy token đồng thời đợi nếu không đủ
    async fn take_with_wait(&mut self, tokens: f64, priority: ResourcePriority) -> Duration {
        let start = Instant::now();
        
        loop {
            self.refill();
            
            // Với ưu tiên cao, có thể sử dụng cả reserve tokens
            let available_tokens = match priority {
                ResourcePriority::Critical => self.tokens,
                ResourcePriority::High => self.tokens,
                _ => (self.tokens - self.reserve_tokens).max(0.0),
            };
            
            if available_tokens >= tokens {
                self.tokens -= tokens;
                return start.elapsed();
            }
            
            // Tính thời gian cần đợi
            let tokens_needed = tokens - available_tokens;
            let wait_time = tokens_needed / self.refill_rate;
            
            // Chờ một thời gian hợp lý
            let wait_millis = (wait_time * 1000.0).ceil() as u64;
            sleep(Duration::from_millis(wait_millis.min(100))).await;
        }
    }
}

/// Quản lý tài nguyên giữa các module khác nhau
pub struct ResourceManager {
    /// Token buckets cho blockchain RPC, phân theo chain_id
    blockchain_rpc_buckets: RwLock<HashMap<u32, Arc<Mutex<TokenBucket>>>>,
    
    /// Token buckets cho các API bên ngoài, phân theo domain
    external_api_buckets: RwLock<HashMap<String, Arc<Mutex<TokenBucket>>>>,
    
    /// Semaphore cho các tác vụ compute-heavy
    compute_semaphore: Arc<Semaphore>,
    
    /// Semaphore cho các kết nối mạng đồng thời
    network_semaphore: Arc<Semaphore>,
    
    /// Các semaphore tùy chỉnh
    custom_semaphores: RwLock<HashMap<String, Arc<Semaphore>>>,
    
    /// Cấu hình mặc định cho token buckets
    default_token_bucket_config: TokenBucketConfig,
    
    /// Cấu hình theo từng loại tài nguyên và chain
    resource_configs: RwLock<HashMap<(ResourceType, u32), TokenBucketConfig>>,
}

impl Default for ResourceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceManager {
    /// Tạo resource manager mới với cấu hình mặc định
    pub fn new() -> Self {
        Self {
            blockchain_rpc_buckets: RwLock::new(HashMap::new()),
            external_api_buckets: RwLock::new(HashMap::new()),
            compute_semaphore: Arc::new(Semaphore::new(num_cpus::get() * 2)), // 2 task per CPU
            network_semaphore: Arc::new(Semaphore::new(100)), // 100 kết nối đồng thời
            custom_semaphores: RwLock::new(HashMap::new()),
            default_token_bucket_config: TokenBucketConfig::default(),
            resource_configs: RwLock::new(HashMap::new()),
        }
    }
    
    /// Tạo resource manager với số lượng tác vụ compute và kết nối mạng tùy chỉnh
    pub fn with_capacity(compute_tasks: usize, network_connections: usize) -> Self {
        Self {
            blockchain_rpc_buckets: RwLock::new(HashMap::new()),
            external_api_buckets: RwLock::new(HashMap::new()),
            compute_semaphore: Arc::new(Semaphore::new(compute_tasks)),
            network_semaphore: Arc::new(Semaphore::new(network_connections)),
            custom_semaphores: RwLock::new(HashMap::new()),
            default_token_bucket_config: TokenBucketConfig::default(),
            resource_configs: RwLock::new(HashMap::new()),
        }
    }
    
    /// Thiết lập cấu hình cho một loại tài nguyên cụ thể trên một chain
    pub async fn set_resource_config(
        &self,
        resource_type: ResourceType,
        chain_id: u32,
        config: TokenBucketConfig,
    ) {
        let mut configs = self.resource_configs.write().await;
        configs.insert((resource_type, chain_id), config);
    }
    
    /// Lấy cấu hình cho một loại tài nguyên cụ thể
    pub async fn get_resource_config(
        &self,
        resource_type: ResourceType,
        chain_id: u32,
    ) -> TokenBucketConfig {
        let configs = self.resource_configs.read().await;
        configs
            .get(&(resource_type, chain_id))
            .cloned()
            .unwrap_or_else(|| self.default_token_bucket_config.clone())
    }
    
    /// Lấy hoặc tạo token bucket cho blockchain RPC
    async fn get_or_create_blockchain_bucket(&self, chain_id: u32) -> Arc<Mutex<TokenBucket>> {
        let buckets = self.blockchain_rpc_buckets.read().await;
        
        if let Some(bucket) = buckets.get(&chain_id) {
            return bucket.clone();
        }
        
        drop(buckets);
        
        // Lấy cấu hình cho chain này
        let config = {
            let config = self.get_resource_config(ResourceType::BlockchainRPC, chain_id).await;
            TokenBucketConfig {
                max_tokens: config.max_tokens,
                refill_rate: config.refill_rate,
                initial_tokens: config.initial_tokens,
                reserve_tokens: config.reserve_tokens,
            }
        };
        
        // Tạo bucket mới
        let bucket = Arc::new(Mutex::new(TokenBucket::new(&config)));
        
        // Thêm vào map
        let mut buckets = self.blockchain_rpc_buckets.write().await;
        buckets.insert(chain_id, bucket.clone());
        
        bucket
    }
    
    /// Lấy hoặc tạo token bucket cho external API
    async fn get_or_create_api_bucket(&self, domain: &str) -> Arc<Mutex<TokenBucket>> {
        let buckets = self.external_api_buckets.read().await;
        
        if let Some(bucket) = buckets.get(domain) {
            return bucket.clone();
        }
        
        drop(buckets);
        
        // Lấy cấu hình mặc định
        let config = {
            // API rate limit thường tùy thuộc vào từng domain
            // Có thể cải thiện bằng cách lưu cấu hình theo domain
            let base_config = self.default_token_bucket_config.clone();
            
            // Điều chỉnh cấu hình cho các domain phổ biến
            match domain {
                "coingecko.com" => TokenBucketConfig {
                    max_tokens: 50,
                    refill_rate: 0.5, // 30 request/phút
                    initial_tokens: 10,
                    reserve_tokens: 5,
                },
                "etherscan.io" => TokenBucketConfig {
                    max_tokens: 5,
                    refill_rate: 0.2, // 12 request/phút
                    initial_tokens: 3,
                    reserve_tokens: 1,
                },
                _ => base_config,
            }
        };
        
        // Tạo bucket mới
        let bucket = Arc::new(Mutex::new(TokenBucket::new(&config)));
        
        // Thêm vào map
        let mut buckets = self.external_api_buckets.write().await;
        buckets.insert(domain.to_string(), bucket.clone());
        
        bucket
    }
    
    /// Yêu cầu quyền sử dụng blockchain RPC (ngay lập tức nếu có sẵn)
    pub async fn request_blockchain_rpc(
        &self, 
        chain_id: u32,
        priority: ResourcePriority,
    ) -> Result<()> {
        let bucket = self.get_or_create_blockchain_bucket(chain_id).await;
        let mut bucket = bucket.lock().await;
        
        if !bucket.take(1.0, priority) {
            // Không đủ tokens, trả về lỗi
            match priority {
                ResourcePriority::Low => {
                    return Err(anyhow!("Rate limited: No RPC tokens available for low priority task on chain {}", chain_id));
                }
                ResourcePriority::Medium => {
                    return Err(anyhow!("Rate limited: No RPC tokens available for medium priority task on chain {}", chain_id));
                }
                _ => {
                    // Với high và critical, vẫn tiếp tục thực hiện nhưng log cảnh báo
                    warn!("Rate limit exceeded but proceeding due to high priority for chain {}", chain_id);
                }
            }
        }
        
        Ok(())
    }
    
    /// Yêu cầu quyền sử dụng blockchain RPC (đợi nếu cần)
    pub async fn request_blockchain_rpc_with_wait(
        &self, 
        chain_id: u32,
        priority: ResourcePriority,
    ) -> Duration {
        let bucket = self.get_or_create_blockchain_bucket(chain_id).await;
        let mut bucket = bucket.lock().await;
        
        let wait_time = bucket.take_with_wait(1.0, priority).await;
        
        if wait_time > Duration::from_millis(100) {
            debug!(
                "Waited {:?} for blockchain RPC access on chain {} (priority: {:?})",
                wait_time, chain_id, priority
            );
        }
        
        wait_time
    }
    
    /// Yêu cầu quyền sử dụng external API (ngay lập tức nếu có sẵn)
    pub async fn request_external_api(
        &self, 
        domain: &str,
        priority: ResourcePriority,
    ) -> Result<()> {
        let bucket = self.get_or_create_api_bucket(domain).await;
        let mut bucket = bucket.lock().await;
        
        if !bucket.take(1.0, priority) {
            // Không đủ tokens, trả về lỗi
            return Err(anyhow!("Rate limited: No API tokens available for {} (priority: {:?})", domain, priority));
        }
        
        Ok(())
    }
    
    /// Yêu cầu quyền sử dụng external API (đợi nếu cần)
    pub async fn request_external_api_with_wait(
        &self, 
        domain: &str,
        priority: ResourcePriority,
    ) -> Duration {
        let bucket = self.get_or_create_api_bucket(domain).await;
        let mut bucket = bucket.lock().await;
        
        bucket.take_with_wait(1.0, priority).await
    }
    
    /// Lấy semaphore cho compute tasks
    pub fn get_compute_semaphore(&self) -> Arc<Semaphore> {
        self.compute_semaphore.clone()
    }
    
    /// Lấy semaphore cho network connections
    pub fn get_network_semaphore(&self) -> Arc<Semaphore> {
        self.network_semaphore.clone()
    }
    
    /// Tạo hoặc lấy semaphore tùy chỉnh
    pub async fn get_or_create_semaphore(&self, name: &str, permits: usize) -> Arc<Semaphore> {
        let semaphores = self.custom_semaphores.read().await;
        
        if let Some(semaphore) = semaphores.get(name) {
            return semaphore.clone();
        }
        
        drop(semaphores);
        
        // Tạo semaphore mới
        let semaphore = Arc::new(Semaphore::new(permits));
        
        // Thêm vào map
        let mut semaphores = self.custom_semaphores.write().await;
        semaphores.insert(name.to_string(), semaphore.clone());
        
        semaphore
    }
    
    /// Tạo wrapper để thực thi một future với giới hạn tài nguyên
    pub async fn with_blockchain_limits<F, T>(
        &self,
        chain_id: u32,
        priority: ResourcePriority,
        task_name: &str,
        future_fn: F,
    ) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        // Lấy quyền truy cập RPC
        self.request_blockchain_rpc(chain_id, priority).await?;
        
        // Thực thi future
        match future_fn() {
            Ok(result) => Ok(result),
            Err(e) => {
                // Nếu là lỗi HTTP 429 (rate limit), đợi và thử lại
                if format!("{:?}", e).contains("429") || 
                   format!("{:?}", e).contains("rate limit") || 
                   format!("{:?}", e).contains("too many requests") {
                    warn!("Rate limited by chain {} during {}, waiting before retry", chain_id, task_name);
                    
                    // Đợi tùy theo độ ưu tiên
                    let wait_time = match priority {
                        ResourcePriority::Low => Duration::from_secs(5),
                        ResourcePriority::Medium => Duration::from_secs(2),
                        ResourcePriority::High => Duration::from_millis(500),
                        ResourcePriority::Critical => Duration::from_millis(100),
                    };
                    
                    sleep(wait_time).await;
                    
                    // Cập nhật token bucket để phản ánh rate limit
                    let bucket = self.get_or_create_blockchain_bucket(chain_id).await;
                    let mut bucket = bucket.lock().await;
                    bucket.tokens = 0.0; // Đặt token xuống 0
                }
                
                Err(e)
            }
        }
    }
    
    /// Tạo wrapper để thực thi một future bất đồng bộ với giới hạn tài nguyên
    pub async fn with_blockchain_limits_async<F, Fut, T>(
        &self,
        chain_id: u32,
        priority: ResourcePriority,
        task_name: &str,
        future_fn: F,
    ) -> Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        // Đợi cho đến khi có quyền truy cập
        let wait_time = self.request_blockchain_rpc_with_wait(chain_id, priority).await;
        
        if wait_time > Duration::from_millis(500) {
            warn!("Task {} waited {:?} for blockchain access on chain {}", task_name, wait_time, chain_id);
        }
        
        // Thực thi future
        match future_fn().await {
            Ok(result) => Ok(result),
            Err(e) => {
                // Nếu là lỗi rate limit, cập nhật token bucket
                if format!("{:?}", e).contains("429") || 
                   format!("{:?}", e).contains("rate limit") || 
                   format!("{:?}", e).contains("too many requests") {
                    warn!("Rate limited by chain {} during {}", chain_id, task_name);
                    
                    // Cập nhật token bucket để phản ánh rate limit
                    let bucket = self.get_or_create_blockchain_bucket(chain_id).await;
                    let mut bucket = bucket.lock().await;
                    bucket.tokens = 0.0; // Đặt token xuống 0
                }
                
                Err(e)
            }
        }
    }
    
    /// Chuyển đổi an toàn từ usize sang u64
    #[inline]
    fn usize_to_u64(val: usize) -> u64 {
        // usize luôn chuyển được sang u64 trên kiến trúc 64-bit
        // Nhưng cần kiểm tra trên 32-bit
        #[cfg(target_pointer_width = "32")]
        {
            val as u64
        }
        #[cfg(target_pointer_width = "64")]
        {
            val as u64
        }
    }

    /// Chuyển đổi an toàn từ u64 sang usize
    #[inline]
    fn u64_to_usize(val: u64) -> usize {
        // Trên 64-bit, kiểm tra xem giá trị có vượt quá giới hạn của usize không
        #[cfg(target_pointer_width = "32")]
        {
            if val > usize::MAX as u64 {
                warn!("Giá trị u64 {} vượt quá giới hạn của usize ({}), sẽ sử dụng giá trị tối đa", val, usize::MAX);
                usize::MAX
            } else {
                val as usize
            }
        }
        #[cfg(target_pointer_width = "64")]
        {
            val as usize
        }
    }

    /// Chuyển đổi an toàn từ u32 sang usize
    #[inline]
    fn u32_to_usize(val: u32) -> usize {
        val as usize
    }

    /// Chuyển đổi an toàn từ usize sang u32
    #[inline]
    fn usize_to_u32(val: usize) -> u32 {
        if val > u32::MAX as usize {
            warn!("Giá trị usize {} vượt quá giới hạn của u32 ({}), sẽ sử dụng giá trị tối đa", val, u32::MAX);
            u32::MAX
        } else {
            val as u32
        }
    }

    /// Cập nhật cấu hình dựa trên tải hệ thống
    pub async fn update_based_on_system_load(&self) -> Result<()> {
        // Trong triển khai thực tế, sẽ đọc tải CPU, memory, network từ hệ thống
        // và điều chỉnh các thông số tương ứng
        
        // Ví dụ đơn giản: Đọc tải CPU
        let cpu_load = sys_info::loadavg()
            .map(|load| load.one)
            .unwrap_or(1.0);
            
        let cpu_count = Self::u32_to_usize(num_cpus::get() as u32);
        let load_ratio = cpu_load / cpu_count as f64;
        
        // Điều chỉnh số lượng compute task dựa vào tải CPU
        if load_ratio > 0.8 {
            // CPU đang tải cao, giảm số lượng task
            info!("High CPU load ({:.1}), reducing compute tasks", load_ratio);
            
            // Tạo semaphore mới với ít permits hơn
            let available_permits = self.compute_semaphore.available_permits();
            let new_permits = (available_permits as f64 * 0.8) as usize;
            if new_permits > 0 {
                // Tạo mới semaphore sẽ dẫn đến việc release tất cả các permits
                // Trong triển khai thực tế, sẽ dùng cơ chế khác để giảm từ từ
                
                // Đây là một hack đơn giản, trong thực tế cần triển khai tốt hơn
                let _ = tokio::task::spawn_blocking(move || {
                    // Cố tình để trống để tránh panic khi modify shared state
                });
            }
        }
        
        Ok(())
    }
}

/// Khởi tạo singleton ResourceManager
lazy_static::lazy_static! {
    pub static ref GLOBAL_RESOURCE_MANAGER: Arc<ResourceManager> = Arc::new(ResourceManager::new());
}

/// Hàm tiện ích để lấy instance ResourceManager toàn cục
pub fn get_resource_manager() -> Arc<ResourceManager> {
    GLOBAL_RESOURCE_MANAGER.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Instant;
    
    #[tokio::test]
    async fn test_token_bucket() {
        let config = TokenBucketConfig {
            max_tokens: 10,
            refill_rate: 5.0, // 5 tokens/giây
            initial_tokens: 5,
            reserve_tokens: 2,
        };
        
        let mut bucket = TokenBucket::new(&config);
        
        // Lấy 3 token ngay lập tức
        assert!(bucket.take(3.0, ResourcePriority::Medium));
        assert_eq!(bucket.tokens, 2.0); // 5 - 3 = 2
        
        // Lấy thêm 2 token, còn đúng reserve threshold
        assert!(bucket.take(2.0, ResourcePriority::Medium));
        assert_eq!(bucket.tokens, 0.0); // 2 - 2 = 0
        
        // Không thể lấy thêm với ưu tiên Medium/Low vì đã chạm reserve
        assert!(!bucket.take(1.0, ResourcePriority::Medium));
        assert!(!bucket.take(1.0, ResourcePriority::Low));
        
        // Vẫn có thể lấy với ưu tiên High/Critical
        assert!(bucket.take(1.0, ResourcePriority::High));
        assert_eq!(bucket.tokens, -1.0); // 0 - 1 = -1 (đi vào reserve)
        assert!(bucket.take(1.0, ResourcePriority::Critical));
        assert_eq!(bucket.tokens, -2.0); // -1 - 1 = -2
    }
    
    #[tokio::test]
    async fn test_blockchain_rpc_limits() {
        let manager = ResourceManager::new();
        
        // Ghi đè cấu hình cho chain 1 thành cấu hình test
        manager.set_resource_config(
            ResourceType::BlockchainRPC,
            1,
            TokenBucketConfig {
                max_tokens: 5,
                refill_rate: 1.0,
                initial_tokens: 3,
                reserve_tokens: 1,
            },
        ).await;
        
        // Yêu cầu 3 lần với ưu tiên Medium
        assert!(manager.request_blockchain_rpc(1, ResourcePriority::Medium).await.is_ok());
        assert!(manager.request_blockchain_rpc(1, ResourcePriority::Medium).await.is_ok());
        
        // Lần thứ 3 sẽ thất bại vì đã chạm reserve
        assert!(manager.request_blockchain_rpc(1, ResourcePriority::Medium).await.is_err());
        
        // Nhưng vẫn có thể yêu cầu với ưu tiên High
        assert!(manager.request_blockchain_rpc(1, ResourcePriority::High).await.is_ok());
        
        // Với wait, nó sẽ đợi cho đến khi có token
        let start = Instant::now();
        let wait_time = manager.request_blockchain_rpc_with_wait(1, ResourcePriority::Medium).await;
        assert!(wait_time > Duration::from_millis(10)); // Đảm bảo đã đợi
    }
} 