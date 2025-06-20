// SchedulerService trait: manage task scheduling in the network
use async_trait::async_trait;
use tracing::{info, warn, error};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time;

/// Timeout cho các hoạt động scheduling
const SCHEDULER_TIMEOUT_MS: u64 = 3000; // 3 giây
const CLEANUP_INTERVAL_SEC: u64 = 600; // 10 phút
const CLEANUP_INACTIVE_AFTER_SEC: u64 = 1800; // 30 phút

#[async_trait]
pub trait SchedulerService: Send + Sync {
    /// Schedule a new task for a node (async)
    async fn schedule_task(&self, node_id: &str, task: &str) -> Result<String, String>;
    /// Get all scheduled tasks for a node (async)
    async fn get_scheduled_tasks(&self, node_id: &str) -> Result<Vec<String>, String>;
    /// Cancel a scheduled task for a node (async)
    async fn cancel_task(&self, node_id: &str, task: &str) -> Result<(), String>;
    /// Health check for scheduler service (async)
    async fn health_check(&self) -> Result<bool, String>;
}

/// Default implementation of SchedulerService (async, thread-safe)
pub struct DefaultSchedulerService {
    tasks: Arc<tokio::sync::Mutex<HashMap<String, Vec<String>>>>,
    /// Thời điểm cập nhật cuối cùng của mỗi node/task
    task_last_update: Arc<tokio::sync::Mutex<HashMap<String, Instant>>>,
    /// Flag để dừng cleanup task khi cần
    cleanup_running: Arc<tokio::sync::watch::Sender<bool>>,
}

impl Default for DefaultSchedulerService {
    fn default() -> Self {
        // Tạo channel để gửi tín hiệu dừng cleanup task
        let (cleanup_tx, cleanup_rx) = tokio::sync::watch::channel(false);
        let tasks: Arc<tokio::sync::Mutex<HashMap<String, Vec<String>>>> = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let task_last_update: Arc<tokio::sync::Mutex<HashMap<String, Instant>>> = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        
        // Spawn cleanup task
        let tasks_clone = tasks.clone();
        let task_last_update_clone = task_last_update.clone();
        
        tokio::spawn(async move {
            let mut rx = cleanup_rx;
            
            loop {
                // Kiểm tra có cần dừng không
                if *rx.borrow() {
                    info!("[SchedulerService] Cleanup task terminated");
                    break;
                }
                
                // Sleep và kiểm tra tín hiệu hủy
                tokio::select! {
                    _ = time::sleep(Duration::from_secs(CLEANUP_INTERVAL_SEC)) => {
                        // Thực hiện cleanup
                        let now = Instant::now();
                        let mut tasks_to_remove: Vec<String> = Vec::new();
                        let mut nodes_to_check: Vec<String> = Vec::new();
                        
                        // Lấy lock task_last_update trước
                        let mut last_update = task_last_update_clone.lock().await;
                        
                        // Xác định các task key cần xóa
                        for (task_key, last_active) in last_update.iter() {
                            if now.duration_since(*last_active).as_secs() > CLEANUP_INACTIVE_AFTER_SEC {
                                tasks_to_remove.push(task_key.clone());
                                // Extract node_id from task_key (format: "node_id:task")
                                if let Some(node_id) = task_key.split(':').next() {
                                    if !nodes_to_check.contains(&node_id.to_string()) {
                                        nodes_to_check.push(node_id.to_string());
                                    }
                                }
                            }
                        }
                        
                        // Xóa các task không hoạt động
                        for task_key in &tasks_to_remove {
                            last_update.remove(task_key);
                            warn!("[SchedulerService] Removed inactive task '{}' after {} seconds", 
                                task_key, CLEANUP_INACTIVE_AFTER_SEC);
                        }
                        
                        // Lấy lock tasks
                        if !nodes_to_check.is_empty() {
                            let mut tasks = tasks_clone.lock().await;
                            
                            // Kiểm tra và cập nhật tasks cho mỗi node
                            for node_id in nodes_to_check {
                                if let Some(node_tasks) = tasks.get_mut(&node_id) {
                                    // Lọc các task đã hết hạn
                                    let original_count = node_tasks.len();
                                    node_tasks.retain(|task| {
                                        let task_key = format!("{}:{}", node_id, task);
                                        !tasks_to_remove.contains(&task_key)
                                    });
                                    
                                    // Log nếu có thay đổi
                                    let removed = original_count - node_tasks.len();
                                    if removed > 0 {
                                        info!("[SchedulerService] Removed {} expired tasks for node '{}'", removed, node_id);
                                    }
                                }
                            }
                        }
                        
                        if !tasks_to_remove.is_empty() {
                            info!("[SchedulerService] Cleanup completed, removed {} inactive tasks", 
                                tasks_to_remove.len());
                        }
                    },
                    _ = rx.changed() => {
                        if *rx.borrow() {
                            info!("[SchedulerService] Cleanup task received stop signal");
                            break;
                        }
                    }
                }
            }
        });
        
        Self {
            tasks,
            task_last_update,
            cleanup_running: Arc::new(cleanup_tx),
        }
    }
}

impl Drop for DefaultSchedulerService {
    fn drop(&mut self) {
        // Gửi tín hiệu kết thúc cleanup task khi service bị hủy
        let _ = self.cleanup_running.send(true);
    }
}

#[async_trait]
impl SchedulerService for DefaultSchedulerService {
    async fn schedule_task(&self, node_id: &str, task: &str) -> Result<String, String> {
        if node_id.is_empty() {
            return Err("Node ID cannot be empty".to_string());
        }
        if task.is_empty() {
            return Err("Task description cannot be empty".to_string());
        }
        
        // Kiểm tra task có chứa ký tự không hợp lệ
        if task.contains(':') {
            return Err("Task description cannot contain ':' character".to_string());
        }
        
        // Thêm timeout cho toàn bộ quá trình scheduling
        match time::timeout(
            Duration::from_millis(SCHEDULER_TIMEOUT_MS),
            async {
                // Cập nhật vào danh sách tasks
                {
                    let mut tasks = self.tasks.lock().await;
                    tasks.entry(node_id.to_string())
                        .or_insert_with(Vec::new)
                        .push(task.to_string());
                }
                
                // Cập nhật thời gian hoạt động
                {
                    let mut last_update = self.task_last_update.lock().await;
                    let task_key = format!("{}:{}", node_id, task);
                    last_update.insert(task_key, Instant::now());
                }
                
                // Trả về task ID (đơn giản chỉ là UUID)
                let task_id = format!("task-{}-{}", node_id, uuid::Uuid::new_v4());
                info!("[SchedulerService] Task '{}' scheduled for node '{}'", task, node_id);
                Ok(task_id)
            }
        ).await {
            Ok(result) => result,
            Err(_) => {
                error!("[SchedulerService] Schedule operation timed out after {}ms", SCHEDULER_TIMEOUT_MS);
                Err(format!("Schedule operation timed out after {}ms", SCHEDULER_TIMEOUT_MS))
            }
        }
    }
    
    async fn get_scheduled_tasks(&self, node_id: &str) -> Result<Vec<String>, String> {
        if node_id.is_empty() {
            return Err("Node ID cannot be empty".to_string());
        }
        
        // Thêm timeout cho toàn bộ quá trình get tasks
        match time::timeout(
            Duration::from_millis(SCHEDULER_TIMEOUT_MS),
            async {
                let tasks = self.tasks.lock().await;
                if let Some(node_tasks) = tasks.get(node_id) {
                    // Tạo bản sao để trả về
                    info!("[SchedulerService] Retrieved {} tasks for node '{}'", node_tasks.len(), node_id);
                    Ok(node_tasks.clone())
                } else {
                    info!("[SchedulerService] No tasks found for node '{}'", node_id);
                    Ok(Vec::new())
                }
            }
        ).await {
            Ok(result) => result,
            Err(_) => {
                error!("[SchedulerService] Get tasks operation timed out after {}ms", SCHEDULER_TIMEOUT_MS);
                Err(format!("Get tasks operation timed out after {}ms", SCHEDULER_TIMEOUT_MS))
            }
        }
    }
    
    async fn cancel_task(&self, node_id: &str, task: &str) -> Result<(), String> {
        if node_id.is_empty() {
            return Err("Node ID cannot be empty".to_string());
        }
        if task.is_empty() {
            return Err("Task description cannot be empty".to_string());
        }
        
        // Thêm timeout cho toàn bộ quá trình cancel task
        match time::timeout(
            Duration::from_millis(SCHEDULER_TIMEOUT_MS),
            async {
                // Remove from tasks list
                let mut found = false;
                {
                    let mut tasks = self.tasks.lock().await;
                    if let Some(node_tasks) = tasks.get_mut(node_id) {
                        let original_count = node_tasks.len();
                        node_tasks.retain(|t| t != task);
                        found = original_count > node_tasks.len();
                    }
                }
                
                // Cập nhật last_update để xóa task key
                {
                    let mut last_update = self.task_last_update.lock().await;
                    let task_key = format!("{}:{}", node_id, task);
                    last_update.remove(&task_key);
                }
                
                if found {
                    info!("[SchedulerService] Task '{}' cancelled for node '{}'", task, node_id);
                    Ok(())
                } else {
                    warn!("[SchedulerService] Task '{}' not found for node '{}'", task, node_id);
                    Err(format!("Task '{}' not found for node '{}'", task, node_id))
                }
            }
        ).await {
            Ok(result) => result,
            Err(_) => {
                error!("[SchedulerService] Cancel task operation timed out after {}ms", SCHEDULER_TIMEOUT_MS);
                Err(format!("Cancel task operation timed out after {}ms", SCHEDULER_TIMEOUT_MS))
            }
        }
    }
    
    async fn health_check(&self) -> Result<bool, String> {
        // Cơ bản chỉ kiểm tra khả năng truy cập vào tasks
        match time::timeout(
            Duration::from_millis(SCHEDULER_TIMEOUT_MS / 2),
            async {
                let _ = self.tasks.lock().await;
                true
            }
        ).await {
            Ok(result) => Ok(result),
            Err(_) => {
                error!("[SchedulerService] Health check timed out - service may be unresponsive");
                Ok(false)
            }
        }
    }
}

/// Get total number of task-node pairs in the scheduler service (async version)
pub async fn get_task_node_count(service: &impl SchedulerService) -> Result<usize, String> {
    // Lấy danh sách các node có task
    let node_ids = vec!["node-1".to_string(), "node-2".to_string()];
    
    let mut total_tasks = 0;
    
    // Đếm tổng số task
    for node_id in node_ids {
        match service.get_scheduled_tasks(&node_id).await {
            Ok(tasks) => total_tasks += tasks.len(),
            Err(e) => warn!("[SchedulerService] Error getting tasks for node '{}': {}", node_id, e),
        }
    }
    
    Ok(total_tasks)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_schedule_and_get_tasks() {
        let service = DefaultSchedulerService::default();
        
        // Đăng ký task
        let schedule_result = service.schedule_task("test-node-1", "process_data").await;
        assert!(schedule_result.is_ok(), "Schedule task failed: {:?}", schedule_result.err());
        
        let task_id = schedule_result.unwrap_or_default();
        assert!(!task_id.is_empty(), "Task ID should not be empty");
        assert!(task_id.contains("test-node-1"), "Task ID should contain node ID");
        
        // Lấy danh sách tasks
        let get_result = service.get_scheduled_tasks("test-node-1").await;
        assert!(get_result.is_ok(), "Get scheduled tasks failed: {:?}", get_result.err());
        
        let tasks = get_result.unwrap_or_default();
        assert_eq!(tasks.len(), 1, "Should have exactly 1 task");
        assert_eq!(tasks[0], "process_data", "Task should be 'process_data'");
    }
    
    #[tokio::test]
    async fn test_cancel_task() {
        let service = DefaultSchedulerService::default();
        
        // Đăng ký task
        let schedule_result = service.schedule_task("test-node-2", "process_video").await;
        assert!(schedule_result.is_ok(), "Schedule task failed: {:?}", schedule_result.err());
        
        // Hủy task
        let cancel_result = service.cancel_task("test-node-2", "process_video").await;
        assert!(cancel_result.is_ok(), "Cancel task failed: {:?}", cancel_result);
        
        // Kiểm tra tasks đã trống
        let get_result = service.get_scheduled_tasks("test-node-2").await;
        assert!(get_result.is_ok(), "Get scheduled tasks failed: {:?}", get_result.err());
        
        let tasks = get_result.unwrap_or_default();
        assert_eq!(tasks.len(), 0, "Task list should be empty after cancellation");
    }
    
    #[tokio::test]
    async fn test_health_check() {
        let service = DefaultSchedulerService::default();
        let result = service.health_check().await;
        assert!(result.is_ok(), "Health check failed: {:?}", result.err());
        
        let is_healthy = result.unwrap_or_default();
        assert!(is_healthy, "Health check should return true");
    }
    
    #[tokio::test]
    async fn test_multiple_tasks_per_node() {
        let service = DefaultSchedulerService::default();
        
        // Đăng ký nhiều task
        let results = vec![
            service.schedule_task("multi-node", "task1").await,
            service.schedule_task("multi-node", "task2").await,
            service.schedule_task("multi-node", "task3").await
        ];
        
        // Kiểm tra kết quả đăng ký
        for (i, result) in results.iter().enumerate() {
            assert!(result.is_ok(), "Failed to schedule task{}: {:?}", i+1, result.err());
        }
        
        // Lấy và kiểm tra
        let get_result = service.get_scheduled_tasks("multi-node").await;
        assert!(get_result.is_ok(), "Get scheduled tasks failed: {:?}", get_result.err());
        
        let tasks = get_result.unwrap_or_default();
        assert_eq!(tasks.len(), 3, "Should have exactly 3 tasks");
        assert!(tasks.contains(&"task1".to_string()), "Tasks should contain 'task1'");
        assert!(tasks.contains(&"task2".to_string()), "Tasks should contain 'task2'");
        assert!(tasks.contains(&"task3".to_string()), "Tasks should contain 'task3'");
    }
    
    #[tokio::test]
    async fn test_invalid_input() {
        let service = DefaultSchedulerService::default();
        
        // Test empty node ID
        let result = service.schedule_task("", "task").await;
        assert!(result.is_err(), "Should fail with empty node ID");
        
        // Test empty task
        let result = service.schedule_task("node", "").await;
        assert!(result.is_err(), "Should fail with empty task");
        
        // Test task với ký tự không hợp lệ
        let result = service.schedule_task("node", "invalid:task").await;
        assert!(result.is_err(), "Should fail with invalid task characters");
    }
}
