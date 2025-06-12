use std::sync::Arc;
use tokio::sync::{Semaphore, Mutex, RwLock};
use anyhow::Result;
use std::collections::HashMap;
use std::future::Future;
use std::time::{Duration, Instant};

/// Service quản lý concurrency và các task song song
pub struct ConcurrencyControlService {
    /// Bộ đếm tối đa số task song song
    task_limiter: Arc<Semaphore>,
    
    /// Danh sách các task đang chạy
    running_tasks: Arc<RwLock<HashMap<String, TaskInfo>>>,
    
    /// Các resource lock
    resource_locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
    
    /// Cấu hình
    config: ConcurrencyConfig,
}

/// Cấu hình concurrency
#[derive(Clone)]
pub struct ConcurrencyConfig {
    /// Số lượng task song song tối đa
    pub max_concurrent_tasks: usize,
    
    /// Thời gian timeout mặc định (giây)
    pub default_timeout: u64,
    
    /// Kích thước queue tối đa
    pub max_queue_size: usize,
    
    /// Số lần retry mặc định
    pub default_retries: u32,
}

/// Thông tin về task
struct TaskInfo {
    /// ID task
    _id: String,
    
    /// Thời điểm bắt đầu
    _start_time: Instant,
    
    /// Resource được sử dụng
    _resources: Vec<String>,
    
    /// Mức độ ưu tiên (0-100)
    _priority: u8,
}

impl ConcurrencyControlService {
    /// Tạo mới service quản lý concurrency
    pub fn new(max_concurrent_tasks: usize) -> Self {
        Self {
            task_limiter: Arc::new(Semaphore::new(max_concurrent_tasks)),
            running_tasks: Arc::new(RwLock::new(HashMap::new())),
            resource_locks: Arc::new(RwLock::new(HashMap::new())),
            config: ConcurrencyConfig {
                max_concurrent_tasks,
                default_timeout: 60,
                max_queue_size: 1000,
                default_retries: 3,
            },
        }
    }
    
    /// Thực hiện task với concurrency control
    pub async fn execute<F, T>(&self, task_id: &str, resources: Vec<String>, priority: u8, task: F) -> Result<T>
    where
        F: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        // Lấy semaphore permit
        let _permit = self.task_limiter.acquire().await?;
        
        // Thêm task vào danh sách đang chạy
        {
            let mut tasks = self.running_tasks.write().await;
            tasks.insert(task_id.to_string(), TaskInfo {
                _id: task_id.to_string(),
                _start_time: Instant::now(),
                _resources: resources.clone(),
                _priority: priority,
            });
        }
        
        // Khóa tất cả resources được yêu cầu
        let _resource_guards = self.lock_resources(&resources).await?;
        
        // Thiết lập timeout
        let timeout = Duration::from_secs(self.config.default_timeout);
        
        // Thực hiện task với timeout
        let result = tokio::time::timeout(timeout, task).await;
        
        // Xóa task khỏi danh sách đang chạy
        {
            let mut tasks = self.running_tasks.write().await;
            tasks.remove(task_id);
        }
        
        // Semaphore permit sẽ tự động drop ở đây
        
        // Xử lý kết quả
        match result {
            Ok(res) => res,
            Err(_) => Err(anyhow::anyhow!("Task timed out after {} seconds", self.config.default_timeout)),
        }
    }
    
    /// Khóa các resources
    async fn lock_resources(&self, resources: &[String]) -> Result<Vec<tokio::sync::OwnedMutexGuard<()>>> {
        let mut locks = Vec::new();
        
        // Lấy lock cho từng resource
        for resource in resources {
            let lock = {
                let resource_locks = self.resource_locks.read().await;
                if let Some(lock) = resource_locks.get(resource) {
                    lock.clone()
                } else {
                    // Nếu chưa có lock cho resource này, tạo mới
                    drop(resource_locks);
                    let mut resource_locks = self.resource_locks.write().await;
                    let lock = Arc::new(Mutex::new(()));
                    resource_locks.insert(resource.clone(), lock.clone());
                    lock
                }
            };
            
            // Acquire lock
            let guard = lock.lock_owned().await;
            locks.push(guard);
        }
        
        Ok(locks)
    }
    
    /// Lấy số lượng task đang chạy
    pub async fn running_task_count(&self) -> usize {
        let tasks = self.running_tasks.read().await;
        tasks.len()
    }
    
    /// Lấy số slot trống
    pub fn available_slots(&self) -> usize {
        self.task_limiter.available_permits()
    }
    
    /// Cập nhật cấu hình
    pub fn update_config(&mut self, config: ConcurrencyConfig) {
        self.config = config.clone();
        
        // Cập nhật số lượng semaphore permit
        self.task_limiter = Arc::new(Semaphore::new(self.config.max_concurrent_tasks));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_concurrency_control() {
        let service = ConcurrencyControlService::new(5);
        
        // Tạo task đơn giản
        let task = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(42)
        };
        
        // Thực hiện task
        let result = service.execute("test_task", vec!["resource1".to_string()], 50, task).await.unwrap();
        assert_eq!(result, 42);
        
        // Kiểm tra số lượng task đang chạy
        assert_eq!(service.running_task_count().await, 0);
        
        // Kiểm tra số slot trống
        assert_eq!(service.available_slots(), 5);
    }
} 