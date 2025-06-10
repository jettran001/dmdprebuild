/*!
 * Network Reliability - Module xử lý độ tin cậy kết nối mạng
 * 
 * Module này cung cấp cơ chế circuit breaker, auto-reconnect và health check
 * để đảm bảo hệ thống vẫn hoạt động ngay cả khi gặp vấn đề kết nối.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Mutex};
use tokio::time::sleep;
use anyhow::{Result, anyhow, Context};
use tracing::{debug, error, info, warn};
use std::future::Future;
use std::pin::Pin;
use rand::Rng;
use once_cell::sync::OnceCell;
use chrono::{DateTime, Utc};
use async_trait::async_trait;

use crate::chain_adapters::evm_adapter::EvmAdapter;

/// Trạng thái kết nối mạng
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkStatus {
    /// Kết nối hoạt động bình thường
    Healthy,
    
    /// Kết nối không ổn định, có lỗi nhưng vẫn có thể sử dụng
    Degraded,
    
    /// Kết nối thất bại, không thể sử dụng
    Failed,
    
    /// Đang thử kết nối lại
    Reconnecting,
}

/// Cấu hình cho circuit breaker
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Số lần lỗi liên tiếp trước khi ngắt mạch
    pub failure_threshold: usize,
    
    /// Thời gian ngắt mạch (giây)
    pub reset_timeout_seconds: u64,
    
    /// Có sử dụng half-open state không
    pub use_half_open: bool,
    
    /// Số lần thử lại tối đa
    pub max_retries: usize,
    
    /// Số lần thành công liên tiếp để đóng mạch
    pub success_threshold: usize,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            reset_timeout_seconds: 30,
            use_half_open: true,
            max_retries: 3,
            success_threshold: 2,
        }
    }
}

/// Trạng thái của circuit breaker
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Mạch đóng, cho phép thực hiện request
    Closed,
    
    /// Mạch mở, chặn tất cả request
    Open,
    
    /// Mạch bán mở, cho phép một số request thử nghiệm
    HalfOpen,
}

/// Circuit breaker để ngăn chặn request khi mạng không ổn định
pub struct CircuitBreaker {
    /// Trạng thái hiện tại của circuit breaker
    state: CircuitState,
    
    /// Số lần lỗi liên tiếp
    failure_count: usize,
    
    /// Số lần thành công liên tiếp (dùng trong half-open state)
    success_count: usize,
    
    /// Thời gian khi chuyển sang open state
    opened_at: Option<Instant>,
    
    /// Cấu hình
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    /// Tạo circuit breaker mới
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            opened_at: None,
            config,
        }
    }
    
    /// Báo cáo thành công
    pub fn record_success(&mut self) {
        match self.state {
            CircuitState::Closed => {
                // Reset failure count
                self.failure_count = 0;
            }
            CircuitState::HalfOpen => {
                // Tăng success count
                self.success_count += 1;
                
                // Nếu đủ số lần thành công liên tiếp, đóng mạch
                if self.success_count >= self.config.success_threshold {
                    info!("Circuit breaker closed after {} consecutive successful requests", self.success_count);
                    self.state = CircuitState::Closed;
                    self.failure_count = 0;
                    self.success_count = 0;
                    self.opened_at = None;
                }
            }
            CircuitState::Open => {
                // Không làm gì nếu đang ở trạng thái open
            }
        }
    }
    
    /// Báo cáo thất bại
    pub fn record_failure(&mut self) {
        match self.state {
            CircuitState::Closed => {
                // Tăng failure count
                self.failure_count += 1;
                
                // Nếu đủ số lần thất bại liên tiếp, mở mạch
                if self.failure_count >= self.config.failure_threshold {
                    warn!("Circuit breaker opened after {} consecutive failures", self.failure_count);
                    self.state = CircuitState::Open;
                    self.opened_at = Some(Instant::now());
                }
            }
            CircuitState::HalfOpen => {
                // Nếu thất bại trong half-open state, trở lại open state
                warn!("Failed request in half-open state, reopening circuit breaker");
                self.state = CircuitState::Open;
                self.success_count = 0;
                self.opened_at = Some(Instant::now());
            }
            CircuitState::Open => {
                // Không làm gì nếu đang ở trạng thái open
            }
        }
    }
    
    /// Kiểm tra xem có thể thực hiện request hay không
    pub fn allow_request(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open => {
                // Kiểm tra xem đã qua thời gian timeout chưa
                if let Some(opened_at) = self.opened_at {
                    let elapsed = opened_at.elapsed();
                    let timeout = Duration::from_secs(self.config.reset_timeout_seconds);
                    
                    if elapsed >= timeout {
                        // Chuyển sang half-open nếu được cấu hình
                        if self.config.use_half_open {
                            debug!("Circuit breaker entering half-open state after timeout");
                            self.state = CircuitState::HalfOpen;
                            self.success_count = 0;
                            true
                        } else {
                            // Hoặc đóng mạch trực tiếp
                            debug!("Circuit breaker closed after timeout");
                            self.state = CircuitState::Closed;
                            self.failure_count = 0;
                            self.opened_at = None;
                            true
                        }
                    } else {
                        false
                    }
                } else {
                    // Trường hợp không có opened_at, đặt lại
                    self.opened_at = Some(Instant::now());
                    false
                }
            }
        }
    }
    
    /// Lấy trạng thái hiện tại
    pub fn state(&self) -> CircuitState {
        self.state
    }
    
    /// Reset circuit breaker về trạng thái ban đầu
    pub fn reset(&mut self) {
        self.state = CircuitState::Closed;
        self.failure_count = 0;
        self.success_count = 0;
        self.opened_at = None;
    }
}

/// Quản lý nhiều circuit breaker cho các dịch vụ khác nhau
pub struct NetworkReliabilityManager {
    /// Các circuit breaker cho blockchain RPC, phân theo chain_id
    circuit_breakers: RwLock<HashMap<String, Arc<Mutex<CircuitBreaker>>>>,
    
    /// Trạng thái kết nối mạng cho từng dịch vụ
    network_status: RwLock<HashMap<String, NetworkStatus>>,
    
    /// Cấu hình mặc định cho circuit breaker
    default_config: CircuitBreakerConfig,
    
    /// Các cấu hình tùy chỉnh cho từng dịch vụ
    custom_configs: RwLock<HashMap<String, CircuitBreakerConfig>>,
    
    /// Thời gian lần cuối cập nhật trạng thái
    last_status_update: RwLock<HashMap<String, Instant>>,

    /// Số lần thử kết nối lại cho từng dịch vụ
    reconnect_attempts: RwLock<HashMap<String, usize>>,
    
    /// Thời gian lần cuối thử kết nối lại
    last_reconnect_attempt: RwLock<HashMap<String, Instant>>,
    
    /// Khoảng thời gian giữa các lần thử kết nối lại (ms)
    reconnect_interval_ms: RwLock<HashMap<String, u64>>,
    
    /// Hàm callback khi kết nối lại thành công
    reconnect_callbacks: RwLock<HashMap<String, Vec<Arc<dyn Fn(&str) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>>>>,
}

impl Default for NetworkReliabilityManager {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkReliabilityManager {
    /// Tạo network reliability manager mới
    pub fn new() -> Self {
        Self {
            circuit_breakers: RwLock::new(HashMap::new()),
            network_status: RwLock::new(HashMap::new()),
            default_config: CircuitBreakerConfig::default(),
            custom_configs: RwLock::new(HashMap::new()),
            last_status_update: RwLock::new(HashMap::new()),
            reconnect_attempts: RwLock::new(HashMap::new()),
            last_reconnect_attempt: RwLock::new(HashMap::new()),
            reconnect_interval_ms: RwLock::new(HashMap::new()),
            reconnect_callbacks: RwLock::new(HashMap::new()),
        }
    }
    
    /// Đặt cấu hình cho một dịch vụ cụ thể
    pub async fn set_config(&self, service_id: &str, config: CircuitBreakerConfig) {
        let mut configs = self.custom_configs.write().await;
        configs.insert(service_id.to_string(), config);
    }
    
    /// Lấy hoặc tạo circuit breaker cho một dịch vụ
    async fn get_or_create_breaker(&self, service_id: &str) -> Arc<Mutex<CircuitBreaker>> {
        let breakers = self.circuit_breakers.read().await;
        
        if let Some(breaker) = breakers.get(service_id) {
            return breaker.clone();
        }
        
        drop(breakers);
        
        // Lấy cấu hình cho dịch vụ này
        let config = {
            let configs = self.custom_configs.read().await;
            configs.get(service_id).cloned().unwrap_or_else(|| self.default_config.clone())
        };
        
        // Tạo circuit breaker mới
        let breaker = Arc::new(Mutex::new(CircuitBreaker::new(config)));
        
        // Thêm vào map
        let mut breakers = self.circuit_breakers.write().await;
        breakers.insert(service_id.to_string(), breaker.clone());
        
        // Khởi tạo trạng thái mạng
        let mut status = self.network_status.write().await;
        status.insert(service_id.to_string(), NetworkStatus::Healthy);
        
        // Khởi tạo thời gian cập nhật
        let mut last_update = self.last_status_update.write().await;
        last_update.insert(service_id.to_string(), Instant::now());
        
        breaker
    }
    
    /// Báo cáo thành công cho một dịch vụ
    pub async fn record_success(&self, service_id: &str) {
        let breaker = self.get_or_create_breaker(service_id).await;
        let mut breaker = breaker.lock().await;
        
        breaker.record_success();
        
        // Cập nhật trạng thái mạng
        let mut status = self.network_status.write().await;
        status.insert(service_id.to_string(), NetworkStatus::Healthy);
        
        // Cập nhật thời gian
        let mut last_update = self.last_status_update.write().await;
        last_update.insert(service_id.to_string(), Instant::now());
    }
    
    /// Báo cáo thất bại cho một dịch vụ
    pub async fn record_failure(&self, service_id: &str) {
        let breaker = self.get_or_create_breaker(service_id).await;
        let mut breaker = breaker.lock().await;
        
        breaker.record_failure();
        
        // Cập nhật trạng thái mạng dựa trên trạng thái circuit breaker
        let mut status = self.network_status.write().await;
        let new_status = match breaker.state() {
            CircuitState::Closed => NetworkStatus::Degraded,
            CircuitState::HalfOpen => NetworkStatus::Reconnecting,
            CircuitState::Open => NetworkStatus::Failed,
        };
        
        status.insert(service_id.to_string(), new_status);
        
        // Cập nhật thời gian
        let mut last_update = self.last_status_update.write().await;
        last_update.insert(service_id.to_string(), Instant::now());
        
        // Log cảnh báo nếu trạng thái là Failed
        if new_status == NetworkStatus::Failed {
            error!("Service {} connection failed, circuit breaker opened", service_id);
        } else if new_status == NetworkStatus::Degraded {
            warn!("Service {} connection degraded", service_id);
        }
    }
    
    /// Kiểm tra xem có thể thực hiện request đến một dịch vụ hay không
    pub async fn allow_request(&self, service_id: &str) -> bool {
        let breaker = self.get_or_create_breaker(service_id).await;
        let mut breaker = breaker.lock().await;
        
        breaker.allow_request()
    }
    
    /// Lấy trạng thái mạng hiện tại của một dịch vụ
    pub async fn get_network_status(&self, service_id: &str) -> NetworkStatus {
        let status = self.network_status.read().await;
        
        status.get(service_id).cloned().unwrap_or(NetworkStatus::Healthy)
    }
    
    /// Đặt lại circuit breaker cho một dịch vụ
    pub async fn reset(&self, service_id: &str) {
        let breaker = self.get_or_create_breaker(service_id).await;
        let mut breaker = breaker.lock().await;
        
        breaker.reset();
        
        // Cập nhật trạng thái mạng
        let mut status = self.network_status.write().await;
        status.insert(service_id.to_string(), NetworkStatus::Healthy);
        
        // Cập nhật thời gian
        let mut last_update = self.last_status_update.write().await;
        last_update.insert(service_id.to_string(), Instant::now());
    }
    
    /// Hàm tiện ích để chạy một future với circuit breaker
    pub async fn with_circuit_breaker<F, Fut, T>(
        &self,
        service_id: &str,
        future_fn: F,
        retry_strategy: Option<RetryStrategy>,
    ) -> Result<T>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        // Lấy retry strategy
        let retry_strategy = retry_strategy.unwrap_or_default();
        
        // Lấy circuit breaker
        let breaker = self.get_or_create_breaker(service_id).await;
        
        // Thử thực hiện request với retry
        for attempt in 0..=retry_strategy.max_retries {
            // Kiểm tra xem có thể thực hiện request không
            let can_proceed = {
                let mut breaker_guard = breaker.lock().await;
                breaker_guard.allow_request()
            };
            
            if !can_proceed {
                // Circuit breaker đang mở, không thực hiện request
                let status = self.get_network_status(service_id).await;
                return Err(anyhow!("Circuit breaker open for service {}, network status: {:?}", service_id, status));
            }
            
            // Thực hiện request
            match future_fn().await {
                Ok(result) => {
                    // Báo cáo thành công
                    self.record_success(service_id).await;
                    return Ok(result);
                }
                Err(e) => {
                    // Báo cáo thất bại
                    self.record_failure(service_id).await;
                    
                    // Nếu đây là lần thử cuối cùng, trả về lỗi
                    if attempt == retry_strategy.max_retries {
                        return Err(e.context(format!("Failed after {} retries for service {}", attempt, service_id)));
                    }
                    
                    // Tính thời gian chờ trước khi thử lại
                    let backoff_ms = retry_strategy.calculate_backoff(attempt);
                    
                    warn!("Request to {} failed (attempt {}/{}), retrying after {}ms: {:?}",
                          service_id, attempt + 1, retry_strategy.max_retries + 1, backoff_ms, e);
                    
                    // Đợi trước khi thử lại
                    sleep(Duration::from_millis(backoff_ms)).await;
                }
            }
        }
        
        // Không bao giờ đến đây do vòng lặp đã xử lý tất cả các trường hợp
        Err(anyhow!("Unexpected error: retry loop exited without result for service {}", service_id))
    }
    
    /// Thực hiện health check cho tất cả các dịch vụ
    pub async fn perform_health_check<F>(
        &self,
        check_fn: F,
    ) -> Result<HashMap<String, NetworkStatus>>
    where
        F: Fn(&str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send>>,
    {
        let mut results = HashMap::new();
        let mut errors = Vec::new();
        
        // Lấy danh sách dịch vụ đã đăng ký
        let service_ids = {
            let status = self.network_status.read().await;
            status.keys().cloned().collect::<Vec<_>>()
        };
        
        // Kiểm tra từng dịch vụ
        for service_id in service_ids {
            // Thực hiện health check với xử lý lỗi
            match check_fn(&service_id).await {
                Ok(is_healthy) => {
                    // Cập nhật trạng thái dựa trên kết quả health check
                    if is_healthy {
                        self.record_success(&service_id).await;
                        results.insert(service_id.clone(), NetworkStatus::Healthy);
                    } else {
                        self.record_failure(&service_id).await;
                        let current_status = self.get_network_status(&service_id).await;
                        results.insert(service_id.clone(), current_status);
                    }
                },
                Err(e) => {
                    // Ghi nhận lỗi và cập nhật trạng thái thất bại
                    error!("Health check for service {} failed with error: {}", service_id, e);
                    self.record_failure(&service_id).await;
                    let current_status = self.get_network_status(&service_id).await;
                    results.insert(service_id.clone(), current_status);
                    errors.push((service_id, e));
                }
            }
        }
        
        // Nếu có lỗi, log thông tin chi tiết nhưng vẫn trả về kết quả
        if !errors.is_empty() {
            warn!("Health check completed with {} errors", errors.len());
            for (service_id, error) in &errors {
                debug!("Health check error detail for {}: {:?}", service_id, error);
            }
        }
        
        Ok(results)
    }
    
    /// Tạo một health check tự động chạy định kỳ
    pub async fn start_auto_health_check<F>(
        self: Arc<Self>,
        check_fn: F,
        interval_seconds: u64,
    )
    where
        F: Fn(&str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send>> + Send + Sync + 'static,
    {
        // Spawn một task riêng để thực hiện health check định kỳ
        tokio::spawn(async move {
            let interval = Duration::from_secs(interval_seconds);
            
            loop {
                // Thực hiện health check với xử lý lỗi
                match self.perform_health_check(&check_fn).await {
                    Ok(results) => {
                        // Log kết quả
                        for (service_id, status) in &results {
                            if *status != NetworkStatus::Healthy {
                                warn!("Health check for service {} returned status: {:?}", service_id, status);
                            } else {
                                debug!("Health check for service {} passed", service_id);
                            }
                        }
                    },
                    Err(e) => {
                        error!("Health check failed with error: {}", e);
                    }
                }
                
                // Đợi đến lần kiểm tra tiếp theo
                sleep(interval).await;
            }
        });
    }

    /// Đăng ký callback khi kết nối lại thành công
    pub async fn register_reconnect_callback<F>(&self, service_id: &str, callback: F)
    where
        F: Fn(&str) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync + 'static,
    {
        let mut callbacks = self.reconnect_callbacks.write().await;
        let service_callbacks = callbacks.entry(service_id.to_string()).or_insert_with(Vec::new);
        service_callbacks.push(Arc::new(callback));
    }

    /// Thử kết nối lại dịch vụ
    pub async fn try_reconnect<F, Fut>(&self, service_id: &str, health_check_fn: F) -> bool
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        // Kiểm tra trạng thái hiện tại
        let current_status = {
            let statuses = self.network_status.read().await;
            statuses.get(service_id).cloned().unwrap_or(NetworkStatus::Failed)
        };

        // Nếu không phải Failed, không cần kết nối lại
        if current_status != NetworkStatus::Failed {
            return true;
        }

        // Cập nhật trạng thái thành Reconnecting
        {
            let mut statuses = self.network_status.write().await;
            statuses.insert(service_id.to_string(), NetworkStatus::Reconnecting);
        }

        // Cập nhật số lần thử kết nối lại
        let attempt = {
            let mut attempts = self.reconnect_attempts.write().await;
            let attempt = attempts.entry(service_id.to_string()).or_insert(0);
            *attempt += 1;
            *attempt
        };

        // Cập nhật thời gian thử kết nối lại
        {
            let mut last_attempts = self.last_reconnect_attempt.write().await;
            last_attempts.insert(service_id.to_string(), Instant::now());
        }

        // Thực hiện health check
        info!("Attempting to reconnect service {} (attempt {})", service_id, attempt);
        let is_healthy = health_check_fn().await;

        // Cập nhật trạng thái
        {
            let mut statuses = self.network_status.write().await;
            let new_status = if is_healthy {
                NetworkStatus::Healthy
            } else {
                NetworkStatus::Failed
            };
            statuses.insert(service_id.to_string(), new_status);
        }

        // Nếu kết nối lại thành công, reset circuit breaker và gọi callback
        if is_healthy {
            info!("Successfully reconnected to service {}", service_id);
            
            // Reset circuit breaker
            self.reset(service_id).await;
            
            // Reset số lần thử kết nối lại
            {
                let mut attempts = self.reconnect_attempts.write().await;
                attempts.insert(service_id.to_string(), 0);
            }
            
            // Execute callbacks
            let callbacks_to_run = {
                let callbacks_map = self.reconnect_callbacks.read().await;
                if let Some(callbacks) = callbacks_map.get(service_id) {
                    callbacks.clone()
                } else {
                    Vec::new()
                }
            };
            
            // Chạy các callbacks sau khi đã giải phóng lock
            for callback in callbacks_to_run {
                let service_id = service_id.to_string();
                tokio::spawn(async move {
                    let fut = callback(&service_id);
                    fut.await;
                });
            }
            
            return true;
        } else {
            warn!("Failed to reconnect to service {} (attempt {})", service_id, attempt);
            return false;
        }
    }

    /// Thiết lập interval cho việc kết nối lại
    pub async fn set_reconnect_interval(&self, service_id: &str, interval_ms: u64) {
        let mut intervals = self.reconnect_interval_ms.write().await;
        intervals.insert(service_id.to_string(), interval_ms);
    }

    /// Lên lịch tự động kết nối lại sau một khoảng thời gian
    pub async fn schedule_reconnect<F, Fut>(
        self: Arc<Self>,
        service_id: &str,
        health_check_fn: F,
        max_attempts: Option<usize>,
    )
    where
        F: Fn() -> Fut + Send + Sync + Clone + 'static,
        Fut: std::future::Future<Output = bool> + Send,
    {
        let service_id = service_id.to_string();
        
        // Lấy interval từ cấu hình
        let base_interval_ms = {
            let intervals = self.reconnect_interval_ms.read().await;
            intervals.get(&service_id).cloned().unwrap_or(5000)
        };
        
        // Khởi động task kết nối lại
        tokio::spawn(async move {
            let mut current_attempts = 0;
            loop {
                // Kiểm tra số lần thử tối đa
                if let Some(max) = max_attempts {
                    if current_attempts >= max {
                        warn!(
                            "Reached maximum reconnect attempts ({}) for service {}",
                            max, service_id
                        );
                        break;
                    }
                }
                
                // Kiểm tra trạng thái hiện tại
                let current_status = {
                    let statuses = self.network_status.read().await;
                    statuses.get(&service_id).cloned().unwrap_or(NetworkStatus::Failed)
                };
                
                // Nếu đã kết nối lại thành công, dừng task
                if current_status == NetworkStatus::Healthy {
                    debug!("Service {} is healthy, stopping reconnect task", service_id);
                    break;
                }
                
                // Thử kết nối lại
                current_attempts += 1;
                let health_check = health_check_fn.clone();
                let success = self.try_reconnect(&service_id, health_check).await;
                
                if success {
                    info!("Successfully reconnected to service {} after {} attempts", service_id, current_attempts);
                    break;
                }
                
                // Tính toán khoảng thời gian chờ với exponential backoff
                let mut interval_ms = base_interval_ms * (1.5_f64.powi(current_attempts as i32 - 1) as u64);
                
                // Giới hạn interval tối đa là 5 phút
                interval_ms = interval_ms.min(300_000);
                
                // Thêm jitter để tránh thundering herd
                let jitter = rand::thread_rng().gen_range(0..=500);
                interval_ms = interval_ms.saturating_add(jitter);
                
                debug!(
                    "Scheduling next reconnect attempt for service {} in {}ms (attempt {})",
                    service_id, interval_ms, current_attempts + 1
                );
                
                // Chờ trước khi thử lại
                sleep(Duration::from_millis(interval_ms)).await;
            }
        });
    }

    /// Phát hiện lỗi kết nối mạng và tự động thử kết nối lại
    pub async fn with_auto_reconnect<F, Fut, G, T>(
        self: Arc<Self>,
        service_id: &str,
        operation_fn: F,
        health_check_fn: G,
        retry_strategy: Option<RetryStrategy>,
    ) -> Result<T>
    where
        F: Fn() -> Fut + Send + Sync + Clone + 'static,
        Fut: std::future::Future<Output = Result<T>> + Send,
        G: Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>> + Send + Sync + Clone + 'static,
    {
        let service_id = service_id.to_string();
        
        // Kiểm tra trạng thái mạng
        let status = self.get_network_status(&service_id).await;
        
        // Nếu đang kết nối lại, chờ một khoảng thời gian
        if status == NetworkStatus::Reconnecting {
            debug!("Service {} is reconnecting, waiting...", service_id);
            sleep(Duration::from_millis(500)).await;
        }
        
        // Nếu đã failed, thử kết nối lại trước khi thực hiện operation
        if status == NetworkStatus::Failed || status == NetworkStatus::Reconnecting {
            debug!("Attempting to reconnect service {} before operation", service_id);
            let health_check = health_check_fn.clone();
            if !self.clone().try_reconnect(&service_id, move || health_check()).await {
                // Nếu không thể kết nối lại, lên lịch kết nối lại và trả về lỗi
                let health_check = health_check_fn.clone();
                self.clone().schedule_reconnect(&service_id, move || health_check(), None).await;
                
                return Err(anyhow!("Network connection to service {} is unavailable", service_id));
            }
        }
        
        // Thực hiện operation với circuit breaker
        let result = self.with_circuit_breaker(
            &service_id,
            operation_fn,
            retry_strategy,
        ).await;
        
        // Nếu operation thất bại và là lỗi liên quan đến mạng, lên lịch kết nối lại
        if let Err(ref e) = result {
            if is_network_error(e) {
                warn!("Network error detected for service {}: {}", service_id, e);
                
                // Cập nhật trạng thái mạng
                {
                    let mut statuses = self.network_status.write().await;
                    statuses.insert(service_id.clone(), NetworkStatus::Failed);
                }
                
                // Lên lịch kết nối lại
                let health_check = health_check_fn.clone();
                self.clone().schedule_reconnect(&service_id, move || health_check(), None).await;
            }
        }
        
        result
    }

    /// Lấy số lần thử kết nối lại cho một dịch vụ
    pub async fn get_reconnect_attempts(&self, service_id: &str) -> usize {
        let attempts = self.reconnect_attempts.read().await;
        attempts.get(service_id).cloned().unwrap_or(0)
    }
}

/// Kiểm tra xem lỗi có phải là lỗi mạng không
fn is_network_error(error: &anyhow::Error) -> bool {
    let error_str = error.to_string().to_lowercase();
    
    // Kiểm tra các pattern lỗi mạng phổ biến
    error_str.contains("network") ||
    error_str.contains("connection") ||
    error_str.contains("timeout") ||
    error_str.contains("timed out") ||
    error_str.contains("connect") ||
    error_str.contains("unreachable") ||
    error_str.contains("refused") ||
    error_str.contains("reset") ||
    error_str.contains("closed") ||
    error_str.contains("eof") ||
    error_str.contains("broken pipe")
}

/// Singleton instance
pub static GLOBAL_NETWORK_MANAGER: once_cell::sync::OnceCell<Arc<NetworkReliabilityManager>> = once_cell::sync::OnceCell::new();

/// Lấy network manager global
pub fn get_network_manager() -> Arc<NetworkReliabilityManager> {
    GLOBAL_NETWORK_MANAGER.get_or_init(|| Arc::new(NetworkReliabilityManager::new()))
}

/// Chiến lược retry
#[derive(Debug, Clone)]
pub struct RetryStrategy {
    /// Số lần thử lại tối đa
    pub max_retries: usize,
    
    /// Thời gian chờ cơ bản (ms)
    pub base_delay_ms: u64,
    
    /// Hệ số nhân cho exponential backoff
    pub backoff_factor: f64,
    
    /// Thời gian chờ tối đa (ms)
    pub max_delay_ms: u64,
    
    /// Có thêm jitter không
    pub add_jitter: bool,
}

impl Default for RetryStrategy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 100,
            backoff_factor: 2.0,
            max_delay_ms: 10000, // 10 giây
            add_jitter: true,
        }
    }
}

impl RetryStrategy {
    /// Tạo chiến lược retry mới
    pub fn new(max_retries: usize, base_delay_ms: u64, backoff_factor: f64, max_delay_ms: u64, add_jitter: bool) -> Self {
        Self {
            max_retries,
            base_delay_ms,
            backoff_factor,
            max_delay_ms,
            add_jitter,
        }
    }
    
    /// Tạo chiến lược retry với backoff nhanh
    pub fn fast() -> Self {
        Self {
            max_retries: 2,
            base_delay_ms: 50,
            backoff_factor: 1.5,
            max_delay_ms: 1000, // 1 giây
            add_jitter: true,
        }
    }
    
    /// Tạo chiến lược retry với backoff chậm
    pub fn slow() -> Self {
        Self {
            max_retries: 5,
            base_delay_ms: 500,
            backoff_factor: 2.0,
            max_delay_ms: 30000, // 30 giây
            add_jitter: true,
        }
    }
    
    /// Tính thời gian chờ cho lần thử tiếp theo
    pub fn calculate_backoff(&self, attempt: usize) -> u64 {
        // Tính thời gian chờ theo exponential backoff
        let backoff = self.base_delay_ms * (self.backoff_factor.powi(attempt as i32) as u64);
        
        // Giới hạn theo max_delay_ms
        let backoff = std::cmp::min(backoff, self.max_delay_ms);
        
        // Thêm jitter nếu cần
        if self.add_jitter {
            // Thêm jitter trong khoảng ±20%
            let jitter_factor = 0.8 + (rand::random::<f64>() * 0.4);
            (backoff as f64 * jitter_factor) as u64
        } else {
            backoff
        }
    }
}

/// CallbackWrapper cho phép clone callback function an toàn
struct CallbackWrapper {
    callback: Arc<dyn Fn(&str) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>,
}

impl CallbackWrapper {
    fn new<F>(callback: F) -> Self 
    where 
        F: Fn(&str) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }
    
    fn call(&self, service_id: &str) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        (self.callback)(service_id)
    }
}

impl Clone for CallbackWrapper {
    fn clone(&self) -> Self {
        Self {
            callback: self.callback.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_circuit_breaker() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            reset_timeout_seconds: 1,
            use_half_open: true,
            max_retries: 2,
            success_threshold: 2,
        };
        
        let mut breaker = CircuitBreaker::new(config);
        
        // Kiểm tra ban đầu
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.allow_request());
        
        // Báo cáo 2 lần thất bại, vẫn còn closed
        breaker.record_failure();
        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.allow_request());
        
        // Báo cáo lần thất bại thứ 3, chuyển sang open
        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.allow_request());
        
        // Đợi quá timeout, chuyển sang half-open
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(breaker.allow_request());
        assert_eq!(breaker.state(), CircuitState::HalfOpen);
        
        // Báo cáo thành công trong half-open
        breaker.record_success();
        assert_eq!(breaker.state(), CircuitState::HalfOpen);
        
        // Báo cáo thành công lần 2, chuyển về closed
        breaker.record_success();
        assert_eq!(breaker.state(), CircuitState::Closed);
        
        // Reset
        breaker.reset();
        assert_eq!(breaker.state(), CircuitState::Closed);
    }
    
    #[tokio::test]
    async fn test_retry_strategy() {
        let strategy = RetryStrategy {
            max_retries: 3,
            base_delay_ms: 100,
            backoff_factor: 2.0,
            max_delay_ms: 10000,
            add_jitter: false, // Tắt jitter để dễ test
        };
        
        // Kiểm tra backoff
        assert_eq!(strategy.calculate_backoff(0), 100);
        assert_eq!(strategy.calculate_backoff(1), 200);
        assert_eq!(strategy.calculate_backoff(2), 400);
        assert_eq!(strategy.calculate_backoff(3), 800);
    }
} 