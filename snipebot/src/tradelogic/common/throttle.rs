//! Module cung cấp cơ chế throttling cho các giao dịch
//!
//! Module này triển khai các thuật toán throttling để giới hạn
//! số lượng giao dịch được thực thi khi hệ thống quá tải.

use std::sync::Arc;
use std::collections::HashMap;
use std::time::{Instant, Duration};
use tokio::sync::{RwLock, Semaphore, Mutex};
use tracing::{debug, warn};
use once_cell::sync::OnceCell;
use anyhow::{Result, anyhow};
use serde::{Serialize, Deserialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Cấu hình throttling
#[derive(Debug, Clone)]
pub struct ThrottleConfig {
    /// Số lượng giao dịch tối đa mỗi giây
    pub max_tx_per_second: f64,
    
    /// Số lượng giao dịch tối đa mỗi phút
    pub max_tx_per_minute: u32,
    
    /// Số lượng giao dịch tối đa mỗi chain
    pub max_tx_per_chain: HashMap<u32, u32>,
    
    /// Số lượng giao dịch đồng thời tối đa
    pub max_concurrent_tx: u32,
    
    /// Giới hạn CPU/RAM cao để bắt đầu throttling
    pub high_load_threshold: f64,
    
    /// Giới hạn CPU/RAM rất cao để chặn tất cả giao dịch mới
    pub critical_load_threshold: f64,
    
    /// Timeout cho các giao dịch đang thực thi (giây)
    pub tx_timeout_seconds: u64,
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        Self {
            max_tx_per_second: 5.0,
            max_tx_per_minute: 200,
            max_tx_per_chain: HashMap::new(),
            max_concurrent_tx: 50,
            high_load_threshold: 0.8, // 80% CPU/RAM
            critical_load_threshold: 0.95, // 95% CPU/RAM
            tx_timeout_seconds: 60,
        }
    }
}

/// Metric hệ thống
#[derive(Debug, Clone, Copy)]
pub struct SystemMetrics {
    /// Tải CPU (0.0 - 1.0)
    pub cpu_load: f64,
    
    /// Sử dụng RAM (0.0 - 1.0)
    pub ram_usage: f64,
    
    /// Thời gian cập nhật
    pub updated_at: Instant,
}

impl Default for SystemMetrics {
    fn default() -> Self {
        Self {
            cpu_load: 0.0,
            ram_usage: 0.0,
            updated_at: Instant::now(),
        }
    }
}

/// Thông tin throttling cho một chain
#[derive(Debug)]
struct ChainThrottleInfo {
    /// Số lượng giao dịch đã thực thi
    tx_count: u32,
    
    /// Thời gian reset (cho mỗi phút)
    reset_time: Instant,
    
    /// Token bucket cho rate limiting
    token_bucket: TokenBucket,
}

impl ChainThrottleInfo {
    fn new(tx_per_second: f64) -> Self {
        Self {
            tx_count: 0,
            reset_time: Instant::now(),
            token_bucket: TokenBucket::new(tx_per_second, tx_per_second as u32),
        }
    }
    
    /// Reset counter nếu đã hết thời gian
    fn reset_if_needed(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.reset_time).as_secs() >= 60 {
            self.tx_count = 0;
            self.reset_time = now;
        }
    }
    
    /// Kiểm tra xem có thể thực hiện giao dịch không
    fn can_execute(&mut self, max_per_minute: u32) -> bool {
        self.reset_if_needed();
        
        if self.tx_count >= max_per_minute {
            return false;
        }
        
        if !self.token_bucket.try_consume(1) {
            return false;
        }
        
        self.tx_count += 1;
        true
    }
}

/// Token bucket for rate limiting
#[derive(Debug)]
struct TokenBucket {
    /// Tốc độ nạp token (tokens/giây)
    refill_rate: f64,
    
    /// Sức chứa tối đa
    capacity: u32,
    
    /// Số lượng token hiện tại
    tokens: f64,
    
    /// Thời gian cập nhật cuối
    last_refill: Instant,
}

impl TokenBucket {
    fn new(refill_rate: f64, capacity: u32) -> Self {
        Self {
            refill_rate,
            capacity,
            tokens: capacity as f64,
            last_refill: Instant::now(),
        }
    }
    
    /// Refill token bucket dựa trên thời gian đã trôi qua
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + self.refill_rate * elapsed).min(self.capacity as f64);
        self.last_refill = now;
    }
    
    /// Thử tiêu thụ một số lượng token
    fn try_consume(&mut self, tokens: u32) -> bool {
        self.refill();
        
        if self.tokens >= tokens as f64 {
            self.tokens -= tokens as f64;
            true
        } else {
            false
        }
    }
}

/// Thông tin thống kê của throttler cho một chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainThrottlerStats {
    /// Tổng số giao dịch
    pub transaction_count: u64,
    
    /// Số giao dịch đã thực hiện
    pub executed_count: u64,
    
    /// Số giao dịch đã bị từ chối
    pub rejected_count: u64,
    
    /// Số giao dịch đã bị hoãn lại
    pub delayed_count: u64,
    
    /// Tốc độ giao dịch trung bình (tps)
    pub average_tps: f64,
    
    /// Thời gian chờ trung bình (ms)
    pub average_wait_ms: f64,
    
    /// Thời gian batch gần nhất (seconds)
    pub last_batch_time: f64,
}

impl Default for ChainThrottlerStats {
    fn default() -> Self {
        Self {
            transaction_count: 0,
            executed_count: 0,
            rejected_count: 0,
            delayed_count: 0,
            average_tps: 0.0,
            average_wait_ms: 0.0,
            last_batch_time: 0.0,
        }
    }
}

/// Thông tin thống kê của throttler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThrottlerStats {
    /// Tổng số giao dịch
    pub total_transactions: u64,
    
    /// Số giao dịch đã thực hiện
    pub total_executed: u64,
    
    /// Số giao dịch đã bị từ chối
    pub total_rejected: u64,
    
    /// Số giao dịch đang chờ
    pub pending_count: u64,
    
    /// Thống kê theo chain
    pub chain_stats: HashMap<String, ChainThrottlerStats>,
    
    /// CPU usage (%)
    pub cpu_usage: f64,
    
    /// Memory usage (%)
    pub memory_usage: f64,
    
    /// Đang hoạt động
    pub is_active: bool,
    
    /// Thời gian hoạt động (giây)
    pub uptime_seconds: u64,
    
    /// Giới hạn hiện tại
    pub current_limits: HashMap<String, u32>,
}

impl Default for ThrottlerStats {
    fn default() -> Self {
        Self {
            total_transactions: 0,
            total_executed: 0,
            total_rejected: 0,
            pending_count: 0,
            chain_stats: HashMap::new(),
            cpu_usage: 0.0,
            memory_usage: 0.0,
            is_active: false,
            uptime_seconds: 0,
            current_limits: HashMap::new(),
        }
    }
}

/// Transaction Throttler
/// 
/// Throttler giới hạn số lượng giao dịch dựa trên cấu hình
/// và tình trạng hệ thống.
pub struct TransactionThrottler {
    /// Cấu hình throttling
    config: RwLock<ThrottleConfig>,
    
    /// Thông tin throttling cho mỗi chain
    chain_info: RwLock<HashMap<u32, ChainThrottleInfo>>,
    
    /// Semaphore để giới hạn số lượng giao dịch đồng thời
    concurrency_semaphore: Semaphore,
    
    /// Metrics hệ thống
    system_metrics: RwLock<SystemMetrics>,
    
    /// Số lượng giao dịch bị từ chối
    rejected_count: Mutex<u64>,
    
    /// Tổng số giao dịch
    total_transactions: AtomicU64,
    
    /// Số giao dịch đã thực hiện
    executed_transactions: AtomicU64,
    
    /// Số giao dịch đã bị từ chối
    rejected_transactions: AtomicU64,
    
    /// Số giao dịch đang chờ
    pending_transactions: Mutex<Vec<u32>>,
    
    /// Thời gian bắt đầu
    start_time: Instant,
    
    /// Đang hoạt động
    active: AtomicU64,
}

impl TransactionThrottler {
    /// Tạo transaction throttler mới
    pub fn new(config: ThrottleConfig) -> Self {
        Self {
            config: RwLock::new(config.clone()),
            chain_info: RwLock::new(HashMap::new()),
            concurrency_semaphore: Semaphore::new(config.max_concurrent_tx as usize),
            system_metrics: RwLock::new(SystemMetrics::default()),
            rejected_count: Mutex::new(0),
            total_transactions: AtomicU64::new(0),
            executed_transactions: AtomicU64::new(0),
            rejected_transactions: AtomicU64::new(0),
            pending_transactions: Mutex::new(Vec::new()),
            start_time: Instant::now(),
            active: AtomicU64::new(true),
        }
    }
    
    /// Cập nhật metrics hệ thống
    pub async fn update_system_metrics(&self, cpu_load: f64, ram_usage: f64) {
        let mut metrics = self.system_metrics.write().await;
        metrics.cpu_load = cpu_load;
        metrics.ram_usage = ram_usage;
        metrics.updated_at = Instant::now();
    }
    
    /// Cập nhật cấu hình
    pub async fn update_config(&self, config: ThrottleConfig) {
        let mut current_config = self.config.write().await;
        *current_config = config.clone();
        
        // Cập nhật semaphore nếu max_concurrent_tx thay đổi
        let current_permits = self.concurrency_semaphore.available_permits();
        let desired_permits = config.max_concurrent_tx as usize;
        
        if current_permits < desired_permits {
            self.concurrency_semaphore.add_permits(desired_permits - current_permits);
        }
    }
    
    /// Kiểm tra xem có thể thực hiện giao dịch không
    pub async fn can_execute_transaction(&self, chain_id: u32) -> Result<bool> {
        // Kiểm tra tải hệ thống
        let metrics = self.system_metrics.read().await;
        let config = self.config.read().await;
        
        // Chặn tất cả giao dịch nếu tải hệ thống vượt ngưỡng critical
        let system_load = metrics.cpu_load.max(metrics.ram_usage);
        if system_load >= config.critical_load_threshold {
            let mut rejected = self.rejected_count.lock().await;
            *rejected += 1;
            warn!("Hệ thống quá tải nghiêm trọng ({}%), từ chối giao dịch", system_load * 100.0);
            return Ok(false);
        }
        
        // Áp dụng throttling nếu tải hệ thống vượt ngưỡng high
        let apply_throttling = system_load >= config.high_load_threshold;
        
        // Lấy hoặc tạo chain info
        let mut chain_infos = self.chain_info.write().await;
        let chain_info = chain_infos
            .entry(chain_id)
            .or_insert_with(|| ChainThrottleInfo::new(config.max_tx_per_second));
        
        // Lấy giới hạn cho chain
        let max_per_minute = *config.max_tx_per_chain
            .get(&chain_id)
            .unwrap_or(&config.max_tx_per_minute);
        
        // Kiểm tra xem có thể thực hiện giao dịch không
        let can_execute = if apply_throttling {
            // Khi hệ thống có tải cao, áp dụng rate limiting nghiêm ngặt hơn
            let throttled_max = (max_per_minute as f64 * (1.0 - system_load)).max(1.0) as u32;
            chain_info.can_execute(throttled_max)
        } else {
            chain_info.can_execute(max_per_minute)
        };
        
        if !can_execute {
            let mut rejected = self.rejected_count.lock().await;
            *rejected += 1;
            debug!("Throttling giao dịch trên chain {} do vượt giới hạn rate", chain_id);
            return Ok(false);
        }
        
        Ok(true)
    }
    
    /// Thực thi một hàm với throttling
    /// 
    /// Hàm này sẽ đợi cho đến khi có thể thực thi giao dịch,
    /// hoặc trả về lỗi nếu không thể thực thi trong thời gian timeout.
    pub async fn execute_with_throttling<F, T>(&self, chain_id: u32, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        // Kiểm tra xem có thể thực hiện giao dịch không
        if !self.can_execute_transaction(chain_id).await? {
            return Err(anyhow!("Giao dịch bị throttle do hệ thống quá tải hoặc vượt giới hạn rate"));
        }
        
        // Lấy permit từ semaphore để giới hạn concurrency
        let permit = self.concurrency_semaphore.acquire().await.map_err(|e| {
            anyhow!("Lỗi khi acquire semaphore permit: {}", e)
        })?;
        
        // Lấy timeout từ config
        let timeout_duration = {
            let config = self.config.read().await;
            Duration::from_secs(config.tx_timeout_seconds)
        };
        
        // Thực thi hàm với timeout
        let result = tokio::time::timeout(timeout_duration, async {
            f()
        }).await;
        
        // Giải phóng permit
        drop(permit);
        
        // Xử lý kết quả
        match result {
            Ok(inner_result) => {
                // Cập nhật số liệu thống kê
                self.executed_transactions.fetch_add(1, Ordering::Relaxed);
                self.total_transactions.fetch_add(1, Ordering::Relaxed);
                // Trả về kết quả của hàm f()
                inner_result
            },
            Err(_) => {
                self.rejected_transactions.fetch_add(1, Ordering::Relaxed);
                self.total_transactions.fetch_add(1, Ordering::Relaxed);
                Err(anyhow!("Giao dịch timeout sau {} giây", timeout_duration.as_secs()))
            },
        }
    }
    
    /// Lấy thông tin thống kê của throttler
    pub async fn get_stats(&self) -> ThrottlerStats {
        let mut stats = ThrottlerStats::default();
        
        // Tổng số giao dịch
        stats.total_transactions = self.total_transactions.load(Ordering::Relaxed);
        stats.total_executed = self.executed_transactions.load(Ordering::Relaxed);
        stats.total_rejected = self.rejected_transactions.load(Ordering::Relaxed);
        
        // CPU và memory usage
        let metrics = self.system_metrics.read().await;
        stats.cpu_usage = metrics.cpu_load * 100.0;
        stats.memory_usage = metrics.ram_usage * 100.0;
        stats.is_active = self.active.load(Ordering::Relaxed) == 1;
        stats.uptime_seconds = self.start_time.elapsed().as_secs();
        
        // Chờ đợi
        let pending = self.pending_transactions.lock().await;
        stats.pending_count = pending.len() as u64;
        
        // Thống kê theo chain
        let config = self.config.read().await;
        let chain_info = self.chain_info.read().await;
        for (chain_id, info) in chain_info.iter() {
            let max_per_minute = *config.max_tx_per_chain
                .get(chain_id)
                .unwrap_or(&config.max_tx_per_minute);
            
            let chain_stats = ChainThrottlerStats {
                transaction_count: info.tx_count as u64,
                executed_count: 0,
                rejected_count: 0,
                delayed_count: 0,
                average_tps: 0.0,
                average_wait_ms: 0.0,
                last_batch_time: 0.0,
            };
            
            stats.chain_stats.insert(chain_id.to_string(), chain_stats);
        }
        
        // Giới hạn hiện tại
        stats.current_limits.insert("max_transactions_per_second".to_string(), config.max_tx_per_second as u32);
        stats.current_limits.insert("max_gas_per_second".to_string(), config.max_tx_per_second as u32);
        stats.current_limits.insert("max_concurrent".to_string(), config.max_concurrent_tx as u32);
        
        stats
    }
    
    /// Reset thống kê của throttler
    pub async fn reset_stats(&self) {
        // Reset các atomic counters
        self.total_transactions.store(0, Ordering::Relaxed);
        self.executed_transactions.store(0, Ordering::Relaxed);
        self.rejected_transactions.store(0, Ordering::Relaxed);
        
        // Reset các thống kê khác
        let mut metrics = self.system_metrics.write().await;
        metrics.cpu_load = 0.0;
        metrics.ram_usage = 0.0;
        
        // Reset thời gian bắt đầu
        self.start_time.elapsed(); // Chỉ để refresh
    }
    
    /// Đặt trạng thái hoạt động của throttler
    pub fn set_active(&self, active: bool) {
        self.active.store(if active { 1 } else { 0 }, Ordering::Relaxed);
    }
}

/// Global instance của TransactionThrottler
static GLOBAL_THROTTLER: OnceCell<Arc<TransactionThrottler>> = OnceCell::new();

/// Lấy hoặc khởi tạo global throttler
pub fn get_global_throttler() -> Arc<TransactionThrottler> {
    GLOBAL_THROTTLER.get_or_init(|| {
        Arc::new(TransactionThrottler::new(ThrottleConfig::default()))
    }).clone()
}

/// Cập nhật cấu hình global throttler
pub async fn update_global_throttler_config(config: ThrottleConfig) {
    let throttler = get_global_throttler();
    throttler.update_config(config).await;
}

/// Cập nhật metrics hệ thống cho global throttler
pub async fn update_global_system_metrics(cpu_load: f64, ram_usage: f64) {
    let throttler = get_global_throttler();
    throttler.update_system_metrics(cpu_load, ram_usage).await;
}

/// Thực thi một hàm với throttling sử dụng global throttler
pub async fn execute_with_throttling<F, T>(chain_id: u32, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    let throttler = get_global_throttler();
    throttler.execute_with_throttling(chain_id, f).await
}

/// Kiểm tra xem một hành động có bị throttle hay không
pub async fn check_throttle(&self, action_type: &str) -> bool {
    // Kiểm tra xem throttler có được kích hoạt không
    if self.active.load(Ordering::Relaxed) == 0 {
        return false;
    }

    // Kiểm tra tỷ lệ CPU và memory
    let metrics = self.system_metrics.read().await;
    if metrics.cpu_load > self.cpu_threshold || metrics.ram_usage > self.memory_threshold {
        // Tăng bộ đếm giao dịch bị từ chối
        self.rejected_transactions.fetch_add(1, Ordering::Relaxed);
        return true;
    }

    // Kiểm tra giới hạn tốc độ cho loại hành động cụ thể
    let now = Instant::now();
    let mut rate_limits = self.rate_limits.write().await;

    if let Some(limit) = rate_limits.get_mut(action_type) {
        // Cập nhật thời gian cuối cùng và kiểm tra
        let elapsed = now.duration_since(limit.last_action).as_millis() as u64;
        if elapsed < limit.min_interval_ms {
            // Tăng bộ đếm giao dịch bị từ chối
            self.rejected_transactions.fetch_add(1, Ordering::Relaxed);
            return true;
        }

        // Cập nhật thời gian cuối cùng
        limit.last_action = now;
    } else {
        // Thêm giới hạn mới với giá trị mặc định
        rate_limits.insert(action_type.to_string(), RateLimit {
            last_action: now,
            min_interval_ms: self.default_interval_ms,
        });
    }

    // Tăng bộ đếm tổng số giao dịch
    self.total_transactions.fetch_add(1, Ordering::Relaxed);
    // Tăng bộ đếm giao dịch đã thực thi
    self.executed_transactions.fetch_add(1, Ordering::Relaxed);

    false
}

/// Kiểm tra xem một hành động có bị throttle hay không mà không cập nhật bất kỳ trạng thái nào
pub async fn is_throttled(&self, action_type: &str) -> bool {
    // Kiểm tra xem throttler có được kích hoạt không
    if self.active.load(Ordering::Relaxed) == 0 {
        return false;
    }

    // Kiểm tra tỷ lệ CPU và memory
    let metrics = self.system_metrics.read().await;
    if metrics.cpu_load > self.cpu_threshold || metrics.ram_usage > self.memory_threshold {
        return true;
    }

    // Kiểm tra giới hạn tốc độ cho loại hành động cụ thể
    let now = Instant::now();
    let rate_limits = self.rate_limits.read().await;

    if let Some(limit) = rate_limits.get(action_type) {
        // Kiểm tra thời gian
        let elapsed = now.duration_since(limit.last_action).as_millis() as u64;
        if elapsed < limit.min_interval_ms {
            return true;
        }
    }

    false
} 