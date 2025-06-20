use async_trait::async_trait;
use tracing::{info, error};

#[async_trait]
pub trait ExecutionAdapter: Send + Sync {
    /// Execute a task asynchronously
    async fn execute(&self, task: &str) -> Result<String, String>;
    
    /// Health check for this execution adapter
    async fn health_check(&self) -> Result<bool, String>;
}

/// Default implementation of ExecutionAdapter
#[derive(Default)]
pub struct DefaultExecutionAdapter;

#[async_trait]
impl ExecutionAdapter for DefaultExecutionAdapter {
    async fn execute(&self, task: &str) -> Result<String, String> {
        if task.is_empty() {
            return Err("Task cannot be empty".to_string());
        }
        
        // Giả lập việc thực thi task
        info!("[ExecutionAdapter] Executing task: {}", task);
        
        // Thực hiện công việc với timeout
        match tokio::time::timeout(
            tokio::time::Duration::from_millis(3000),
            async {
                // Giả lập xử lý bất đồng bộ
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                Ok(format!("Executed task: {}", task))
            }
        ).await {
            Ok(result) => result,
            Err(_) => {
                error!("[ExecutionAdapter] Execution timed out for task: {}", task);
                Err(format!("Execution timed out for task: {}", task))
            }
        }
    }
    
    async fn health_check(&self) -> Result<bool, String> {
        // Đơn giản chỉ kiểm tra nếu adapter đang hoạt động
        info!("[ExecutionAdapter] Performing health check");
        Ok(true)
    }
}

/// Execute a task with a specific adapter
pub async fn execute_task_with_adapter(adapter: &impl ExecutionAdapter, task: &str) -> Result<String, String> {
    adapter.execute(task).await
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_execute_task() {
        let adapter = DefaultExecutionAdapter::default();
        
        // Test execution with valid task
        let result = adapter.execute("test_task").await;
        assert!(result.is_ok(), "Task execution failed: {:?}", result);
        
        // Thay thế unwrap bằng match + assertion rõ ràng hơn
        if let Ok(output) = result {
            assert!(output.contains("test_task"), "Output should contain the task name");
        }
        
        // Test execution with empty task
        let result = adapter.execute("").await;
        assert!(result.is_err(), "Empty task should fail");
        
        // Thay thế unwrap bằng if let để xử lý an toàn
        if let Err(error) = result {
            assert!(error.contains("empty"), "Error should mention empty task");
        }
    }
    
    #[tokio::test]
    async fn test_health_check() {
        let adapter = DefaultExecutionAdapter::default();
        
        // Test health check
        let result = adapter.health_check().await;
        assert!(result.is_ok(), "Health check failed: {:?}", result);
        
        // Thay thế unwrap bằng if let để xử lý an toàn
        if let Ok(status) = result {
            assert!(status, "Health check should return true");
        }
    }
    
    #[tokio::test]
    async fn test_execute_task_with_adapter() {
        let adapter = DefaultExecutionAdapter::default();
        
        // Test execute_task_with_adapter function
        let result = execute_task_with_adapter(&adapter, "helper_task").await;
        assert!(result.is_ok(), "Task execution via helper failed: {:?}", result);
        
        // Thay thế unwrap bằng if let để xử lý an toàn
        if let Ok(output) = result {
            assert!(output.contains("helper_task"), "Output should contain the task name");
        }
    }
} 