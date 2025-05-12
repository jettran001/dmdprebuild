use crate::node_manager::{NodeProfile, NodeRole};
use std::sync::atomic::{AtomicUsize, Ordering};
use once_cell::sync::Lazy;
use std::sync::Arc;
use tracing::{info, warn, error};
use std::time::Duration;
use tokio::time;
use tokio::sync::Mutex;

/// Task enum: Represents a logical task that can be assigned to a node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Task {
    AiTraining,
    SnipebotExecution,
    CdnStorage,
    RedisCache,
    WalletProcessing,
    EdgeComputation,
    MasterNode,
    SlaveNode,
    WorkerNode,
    Unknown,
}

const MAX_CONCURRENT_TASKS: usize = 1000;
const MAX_RETRY_ATTEMPTS: usize = 10; // Số lần thử lại tối đa để tránh vòng lặp vô hạn
const TASK_PROCESSING_TIMEOUT_MS: u64 = 5000; // Timeout xử lý task: 5 giây

pub struct TaskManager {
    pub current_tasks: AtomicUsize,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            current_tasks: AtomicUsize::new(0),
        }
    }
    
    /// Cố gắng bắt đầu một task mới
    /// Trả về true nếu bắt đầu thành công, false nếu đã đạt số lượng task tối đa hoặc thử quá nhiều lần
    pub fn try_start_task(&self) -> bool {
        let mut attempts = 0;
        
        // Thêm số lần thử lại tối đa để tránh vòng lặp vô hạn
        while attempts < MAX_RETRY_ATTEMPTS {
            let current = self.current_tasks.load(Ordering::SeqCst);
            if current >= MAX_CONCURRENT_TASKS {
                return false;
            }
            
            match self.current_tasks.compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => return true,
                Err(_) => {
                    attempts += 1;
                    // Nếu đã thử nhiều lần, log cảnh báo
                    if attempts >= MAX_RETRY_ATTEMPTS / 2 {
                        warn!("[TaskManager] CAS conflict #{} when starting task, current={}", attempts, current);
                    }
                    // Ngủ một chút để tránh cạnh tranh liên tục
                    std::thread::yield_now();
                }
            }
        }
        
        // Nếu thử quá nhiều lần, ghi log lỗi và báo thất bại
        error!("[TaskManager] Failed to start task after {} attempts, possible contention", MAX_RETRY_ATTEMPTS);
        false
    }
    
    pub fn finish_task(&self) {
        let prev = self.current_tasks.fetch_sub(1, Ordering::SeqCst);
        if prev == 0 {
            // Phát hiện lỗi underflow - đã gọi finish_task khi không có task nào đang chạy
            error!("[TaskManager] Underflow detected in finish_task! Called finish_task with no running tasks");
            // Khôi phục về 0 để tránh underflow
            self.current_tasks.store(0, Ordering::SeqCst);
        }
    }
    
    /// Lấy số lượng task hiện tại
    pub fn get_current_tasks(&self) -> usize {
        self.current_tasks.load(Ordering::SeqCst)
    }
}

pub struct Dispatcher {
    pub task_manager: Arc<TaskManager>,
}

/// Task priority order (cao -> thấp)
const TASK_PRIORITY: &[Task] = &[
    Task::AiTraining,
    Task::SnipebotExecution,
    Task::CdnStorage,
    Task::WalletProcessing,
    Task::RedisCache,
    Task::EdgeComputation,
    Task::Unknown,
];

impl Dispatcher {
    pub fn new(task_manager: Arc<TaskManager>) -> Self {
        Self { task_manager }
    }
    
    /// Map node roles to tasks - phiên bản async với timeout
    /// Returns Result với Vec<Task> hoặc error message
    pub async fn map_roles_to_tasks_async(&self, profile: &NodeProfile) -> Result<Vec<Task>, String> {
        // Sử dụng timeout cho toàn bộ quá trình xử lý
        match time::timeout(
            Duration::from_millis(TASK_PROCESSING_TIMEOUT_MS),
            self.map_roles_to_tasks_internal(profile)
        ).await {
            Ok(result) => result,
            Err(_) => {
                error!("[Dispatcher] Timeout after {}ms when mapping roles to tasks", TASK_PROCESSING_TIMEOUT_MS);
                // Đảm bảo giải phóng task nếu timeout
                self.task_manager.finish_task();
                Err(format!("Task mapping timeout after {}ms", TASK_PROCESSING_TIMEOUT_MS))
            }
        }
    }
    
    // Internal implementation - không gọi finish_task ở đây
    async fn map_roles_to_tasks_internal(&self, profile: &NodeProfile) -> Result<Vec<Task>, String> {
        if !self.task_manager.try_start_task() {
            warn!("[Dispatcher] Max concurrent tasks reached ({}), cannot start new task", MAX_CONCURRENT_TASKS);
            return Err("Max concurrent tasks reached, cannot start new task".to_string());
        }
        
        if profile.roles.is_empty() {
            warn!("[Dispatcher] Profile has no roles defined");
            // Không gọi finish_task ở đây - sẽ gọi ở hàm bên ngoài
            return Ok(vec![Task::Unknown]);
        }
        
        let mut tasks: Vec<Task> = Vec::new();
        let mut has_unknown_role = false;
        
        // Trả về option nên có thể xử lý các role không xác định một cách an toàn
        for role in &profile.roles {
            match role {
                NodeRole::AiTraining => tasks.push(Task::AiTraining),
                NodeRole::SnipebotExecutor => tasks.push(Task::SnipebotExecution),
                NodeRole::CdnStorageNode => tasks.push(Task::CdnStorage),
                NodeRole::RedisNode => tasks.push(Task::RedisCache),
                NodeRole::WalletNode => tasks.push(Task::WalletProcessing),
                NodeRole::EdgeCompute => tasks.push(Task::EdgeComputation),
                NodeRole::Master => tasks.push(Task::MasterNode),
                NodeRole::Slave => tasks.push(Task::SlaveNode),
                NodeRole::Worker => tasks.push(Task::WorkerNode),
                NodeRole::Unknown => {
                    has_unknown_role = true;
                    tasks.push(Task::Unknown);
                }
                // Không sử dụng _ => để bắt các role không được xử lý một cách tường minh
                // Thay vào đó, sẽ phát hiện lỗi khi enum NodeRole thay đổi
            }
        }
        
        if has_unknown_role {
            warn!("[Dispatcher] Profile contains unknown role(s), task assignment may not be optimal");
        }
        
        if tasks.is_empty() {
            tasks.push(Task::Unknown);
            warn!("[Dispatcher] No tasks mapped from roles, using Unknown as fallback");
        }
        
        // Không gọi finish_task ở đây - sẽ gọi ở hàm bên ngoài
        Ok(tasks)
    }
    
    /// Map node roles to tasks - phiên bản cũ, vẫn giữ lại để tương thích ngược
    /// Đã sửa để xử lý finish_task đúng cách: chỉ gọi finish_task khi kết thúc tất cả xử lý role
    pub fn map_roles_to_tasks(&self, profile: &NodeProfile) -> Vec<Task> {
        if !self.task_manager.try_start_task() {
            // Log warning and return empty
            warn!("[Dispatcher] Max concurrent tasks reached, cannot start new task");
            return vec![];
        }
        
        // Đặt biến kết quả và finally block để đảm bảo finish_task được gọi
        let mut result: Vec<Task> = Vec::new();
        let _had_error = false;
        
        // Try block để bắt error
        {
            // Xử lý roles
            for role in &profile.roles {
                match role {
                    NodeRole::AiTraining => result.push(Task::AiTraining),
                    NodeRole::SnipebotExecutor => result.push(Task::SnipebotExecution),
                    NodeRole::CdnStorageNode => result.push(Task::CdnStorage),
                    NodeRole::RedisNode => result.push(Task::RedisCache),
                    NodeRole::WalletNode => result.push(Task::WalletProcessing),
                    NodeRole::EdgeCompute => result.push(Task::EdgeComputation),
                    NodeRole::Master => result.push(Task::MasterNode),
                    NodeRole::Slave => result.push(Task::SlaveNode),
                    NodeRole::Worker => result.push(Task::WorkerNode),
                    NodeRole::Unknown => {
                        warn!("[Dispatcher] Unknown role detected, using Task::Unknown");
                        result.push(Task::Unknown)
                    }
                }
            }
            
            if result.is_empty() {
                warn!("[Dispatcher] No task mapped, using Unknown as fallback");
                result.push(Task::Unknown);
            }
        }
        
        // Finally: finish task dù có lỗi hay không
        self.task_manager.finish_task();
        result
    }

    /// Map node roles to tasks with priority (ưu tiên task quan trọng trước)
    pub fn map_roles_to_tasks_with_priority(profile: &NodeProfile) -> Vec<Task> {
        let mut tasks: Vec<Task> = Vec::new();
        let mut has_unknown_role = false;
        
        // Lặp qua mỗi phần tử trong mảng priority, sửa lỗi "Cannot move out of shared reference"
        for priority_index in 0..TASK_PRIORITY.len() {
            let priority_task = TASK_PRIORITY[priority_index];
            for role in &profile.roles {
                let task = match role {
                    NodeRole::AiTraining => Task::AiTraining,
                    NodeRole::SnipebotExecutor => Task::SnipebotExecution,
                    NodeRole::CdnStorageNode => Task::CdnStorage,
                    NodeRole::RedisNode => Task::RedisCache,
                    NodeRole::WalletNode => Task::WalletProcessing,
                    NodeRole::EdgeCompute => Task::EdgeComputation,
                    NodeRole::Master => Task::MasterNode,
                    NodeRole::Slave => Task::SlaveNode,
                    NodeRole::Worker => Task::WorkerNode,
                    NodeRole::Unknown => {
                        has_unknown_role = true;
                        Task::Unknown
                    }
                };
                if task == priority_task && !tasks.contains(&task) {
                    tasks.push(task);
                }
            }
        }
        
        if has_unknown_role {
            warn!("[Dispatcher] Profile contains unknown role(s) in priority mapping");
        }
        
        if tasks.is_empty() {
            warn!("[Dispatcher] No tasks mapped with priority, using Unknown as fallback");
            tasks.push(Task::Unknown);
        }
        
        tasks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_manager::NodeProfile;
    
    #[test]
    fn test_dispatcher_mapping() {
        let task_manager = Arc::new(TaskManager::new());
        let dispatcher = Dispatcher::new(task_manager);
        
        let mut profile = NodeProfile::new(8, 1, 32, 1024, true);
        profile.roles = vec![NodeRole::AiTraining, NodeRole::RedisNode];
        
        let tasks = dispatcher.map_roles_to_tasks(&profile);
        assert!(tasks.contains(&Task::AiTraining));
        assert!(tasks.contains(&Task::RedisCache));
        
        // Kiểm tra số lượng task sau khi mapping xong
        assert_eq!(dispatcher.task_manager.get_current_tasks(), 0, "Task count should be 0 after map_roles_to_tasks");
    }
    
    #[tokio::test]
    async fn test_map_roles_to_tasks_async() {
        let task_manager = Arc::new(TaskManager::new());
        let dispatcher = Dispatcher::new(task_manager);
        
        let mut profile = NodeProfile::new(8, 1, 32, 1024, true);
        profile.roles = vec![NodeRole::AiTraining, NodeRole::RedisNode];
        
        let tasks = dispatcher.map_roles_to_tasks_async(&profile).await.unwrap();
        assert!(tasks.contains(&Task::AiTraining));
        assert!(tasks.contains(&Task::RedisCache));
        
        // Kiểm tra số lượng task sau khi mapping xong
        assert_eq!(dispatcher.task_manager.get_current_tasks(), 1, 
            "Task count should be 1 after map_roles_to_tasks_async because caller is responsible for finish_task");
        
        // Cleanup
        dispatcher.task_manager.finish_task();
    }
    
    #[test]
    fn test_map_roles_to_tasks_with_priority() {
        let mut profile = NodeProfile::new(8, 1, 32, 1024, true);
        profile.roles = vec![NodeRole::RedisNode, NodeRole::AiTraining];
        
        let tasks = Dispatcher::map_roles_to_tasks_with_priority(&profile);
        
        // AiTraining có ưu tiên cao hơn RedisCache nên phải ở đầu danh sách
        assert_eq!(tasks[0], Task::AiTraining);
        assert_eq!(tasks[1], Task::RedisCache);
    }
    
    #[test]
    fn test_empty_profile() {
        let task_manager = Arc::new(TaskManager::new());
        let dispatcher = Dispatcher::new(task_manager);
        
        let profile = NodeProfile::new(8, 1, 32, 1024, true);
        // Không set roles -> empty
        
        let tasks = dispatcher.map_roles_to_tasks(&profile);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0], Task::Unknown);
        
        // Kiểm tra số lượng task sau khi mapping xong
        assert_eq!(dispatcher.task_manager.get_current_tasks(), 0, "Task count should be 0 after map_roles_to_tasks");
    }
    
    #[test]
    fn test_unknown_role() {
        let task_manager = Arc::new(TaskManager::new());
        let dispatcher = Dispatcher::new(task_manager);
        
        let mut profile = NodeProfile::new(8, 1, 32, 1024, true);
        profile.roles = vec![NodeRole::Unknown];
        
        let tasks = dispatcher.map_roles_to_tasks(&profile);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0], Task::Unknown);
        
        // Kiểm tra số lượng task sau khi mapping xong
        assert_eq!(dispatcher.task_manager.get_current_tasks(), 0, "Task count should be 0 after map_roles_to_tasks");
    }
}
