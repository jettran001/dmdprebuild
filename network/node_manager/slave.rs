// SlaveNodeService trait: manage slave node logic in the network
use async_trait::async_trait;
use tracing::{info, warn, error};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time;
use thiserror::Error;

// Cập nhật import để sử dụng NodeProfile từ node_manager::mod.rs
use crate::node_manager::NodeProfile;

/// Errors specific to SlaveNodeService
#[derive(Error, Debug)]
pub enum SlaveNodeError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    
    #[error("Authentication error: {0}")]
    AuthError(String),
    
    #[error("Registration error: {0}")]
    RegistrationError(String),
    
    #[error("Not registered to master: {0}")]
    NotRegistered(String),
    
    #[error("Task processing error: {0}")]
    TaskError(String),
    
    #[error("Timeout error: {0}")]
    TimeoutError(String),
    
    #[error("Internal error: {0}")]
    InternalError(String),
}

// Implement conversion from SlaveNodeError to ServiceError
impl From<SlaveNodeError> for crate::infra::service_traits::ServiceError {
    fn from(error: SlaveNodeError) -> Self {
        match error {
            SlaveNodeError::InvalidInput(msg) => Self::ValidationError(msg),
            SlaveNodeError::AuthError(msg) => Self::AuthError(msg),
            SlaveNodeError::RegistrationError(msg) => Self::ConnectionError(msg),
            SlaveNodeError::NotRegistered(msg) => Self::NotFoundError(msg),
            SlaveNodeError::TaskError(msg) => Self::InternalError(msg),
            SlaveNodeError::TimeoutError(msg) => Self::TimeoutError(msg),
            SlaveNodeError::InternalError(msg) => Self::InternalError(msg),
        }
    }
}

#[async_trait]
pub trait SlaveNodeService: Send + Sync {
    /// Register this slave node to master (async)
    async fn register_self(&self, master_id: &str, profile: NodeProfile) -> Result<(), SlaveNodeError>;
    /// Get this slave node's info (async)
    async fn get_info(&self) -> Result<Option<NodeProfile>, SlaveNodeError>;
    /// Receive a task from master (async)
    async fn receive_task(&self, task: &str) -> Result<String, SlaveNodeError>;
    /// Report status to master (async)
    async fn report_status(&self, status: &str) -> Result<(), SlaveNodeError>;
    /// Health check for this slave node (async)
    async fn health_check(&self) -> Result<bool, SlaveNodeError>;
}

/// Default implementation of SlaveNodeService (async, thread-safe)
pub struct DefaultSlaveNodeService {
    master_id: Arc<tokio::sync::Mutex<Option<String>>>,
    profile: Arc<tokio::sync::Mutex<Option<NodeProfile>>>,
    last_status: Arc<tokio::sync::Mutex<Option<String>>>,
    last_heartbeat: Arc<tokio::sync::Mutex<Option<Instant>>>,
}

impl Default for DefaultSlaveNodeService {
    fn default() -> Self {
        Self {
            master_id: Arc::new(tokio::sync::Mutex::new(None)),
            profile: Arc::new(tokio::sync::Mutex::new(None)),
            last_status: Arc::new(tokio::sync::Mutex::new(None)),
            last_heartbeat: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }
}

#[async_trait]
impl SlaveNodeService for DefaultSlaveNodeService {
    async fn register_self(&self, master_id: &str, profile: NodeProfile) -> Result<(), SlaveNodeError> {
        if master_id.is_empty() {
            return Err(SlaveNodeError::InvalidInput("Master ID cannot be empty".to_string()));
        }
        
        // Validate profile
        if let Err(e) = crate::node_manager::node_classifier::NodeClassifier::validate_profile(&profile) {
            warn!("[SlaveNodeService] Invalid profile during registration: {}", e);
            return Err(SlaveNodeError::InvalidInput(format!("Invalid profile: {}", e)));
        }
        
        // Update master ID
        {
            let mut master = self.master_id.lock().await;
            *master = Some(master_id.to_string());
        }
        
        // Update profile
        {
            let mut p = self.profile.lock().await;
            *p = Some(profile);
        }
        
        // Update heartbeat
        {
            let mut heartbeat = self.last_heartbeat.lock().await;
            *heartbeat = Some(Instant::now());
        }
        
        info!("[SlaveNodeService] Registered to master '{}'", master_id);
        Ok(())
    }
    
    async fn get_info(&self) -> Result<Option<NodeProfile>, SlaveNodeError> {
        // Lấy profile từ mutex
        let profile_lock = self.profile.lock().await;
        
        // Clone một cách an toàn - chỉ clone khi có dữ liệu
        let result = (*profile_lock).clone();
        
        // Drop mutex lock trước khi return
        drop(profile_lock);
        
        Ok(result)
    }
    
    async fn receive_task(&self, task: &str) -> Result<String, SlaveNodeError> {
        if task.is_empty() {
            return Err(SlaveNodeError::InvalidInput("Task cannot be empty".to_string()));
        }
        
        // Check if registered to a master
        let master_id = {
            let master = self.master_id.lock().await;
            if let Some(id) = master.as_ref() {
                id.clone()
            } else {
                return Err(SlaveNodeError::NotRegistered("Not registered to any master node".to_string()));
            }
        };
        
        // Update heartbeat
        {
            let mut heartbeat = self.last_heartbeat.lock().await;
            *heartbeat = Some(Instant::now());
        }
        
        info!("[SlaveNodeService] Received task '{}' from master", task);
        
        // Thêm timeout cho toàn bộ quá trình xử lý task
        match time::timeout(
            Duration::from_millis(3000),
            async {
                // Giả lập xử lý task
                time::sleep(Duration::from_millis(100)).await;
                Ok(format!("Processed task: {} for master {}", task, master_id))
            }
        ).await {
            Ok(result) => result,
            Err(_) => {
                error!("[SlaveNodeService] Task processing timed out: {}", task);
                Err(SlaveNodeError::TimeoutError(format!("Task processing timed out: {}", task)))
            }
        }
    }
    
    async fn report_status(&self, status: &str) -> Result<(), SlaveNodeError> {
        if status.is_empty() {
            return Err(SlaveNodeError::InvalidInput("Status cannot be empty".to_string()));
        }
        
        // Check if registered to a master
        {
            let master = self.master_id.lock().await;
            if master.is_none() {
                return Err(SlaveNodeError::NotRegistered("Not registered to any master node".to_string()));
            }
        }
        
        // Update status
        {
            let mut last_status = self.last_status.lock().await;
            *last_status = Some(status.to_string());
        }
        
        // Update heartbeat
        {
            let mut heartbeat = self.last_heartbeat.lock().await;
            *heartbeat = Some(Instant::now());
        }
        
        info!("[SlaveNodeService] Reported status: {}", status);
        Ok(())
    }
    
    async fn health_check(&self) -> Result<bool, SlaveNodeError> {
        // Kiểm tra xem có được đăng ký với master nào không
        let has_master = {
            let master = self.master_id.lock().await;
            master.is_some()
        };
        
        if !has_master {
            warn!("[SlaveNodeService] Health check failed: Not registered to any master");
            return Ok(false);
        }
        
        // Kiểm tra heartbeat
        let is_healthy = {
            let heartbeat = self.last_heartbeat.lock().await;
            if let Some(last) = *heartbeat {
                // Slave được coi là healthy nếu heartbeat trong vòng 5 phút
                Instant::now().duration_since(last).as_secs() < 300
            } else {
                false
            }
        };
        
        if !is_healthy {
            warn!("[SlaveNodeService] Health check failed: Heartbeat expired");
        }
        
        Ok(is_healthy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{NodeProfile, NodeRole};
    
    #[tokio::test]
    async fn test_register_and_get_info() {
        let service = DefaultSlaveNodeService::default();
        let mut profile = NodeProfile::default();
        profile.add_role(NodeRole::Worker);
        
        // Đăng ký với master
        let register_result = service.register_self("test-master-1", profile.clone()).await;
        assert!(register_result.is_ok(), "Đăng ký với master thất bại: {:?}", register_result.err());
        
        // Lấy thông tin
        let info_result = service.get_info().await;
        assert!(info_result.is_ok(), "Lấy thông tin thất bại: {:?}", info_result.err());
        
        // Thay panic bằng assertion với skip logic
        let info = if let Ok(info) = info_result {
            info
        } else {
            assert!(false, "Không thể lấy thông tin: {:?}", info_result.err());
            return;
        };
        
        assert!(info.is_some(), "Thông tin phải tồn tại sau khi đăng ký");
        
        // Thay panic bằng assertion với skip logic
        let retrieved_profile = if let Some(profile) = info {
            profile
        } else {
            assert!(false, "Không tìm thấy profile sau khi đã kiểm tra là Some");
            return;
        };
        
        // Kiểm tra thông tin
        assert!(retrieved_profile.has_role(&NodeRole::Worker));
        assert_eq!(retrieved_profile.role_count(), 1);
    }
    
    #[tokio::test]
    async fn test_receive_task() {
        let service = DefaultSlaveNodeService::default();
        let mut profile = NodeProfile::default();
        profile.add_role(NodeRole::Worker);
        
        // Đăng ký với master
        let register_result = service.register_self("test-master-2", profile).await;
        assert!(register_result.is_ok(), "Đăng ký với master thất bại: {:?}", register_result.err());
        
        // Nhận task
        let result = service.receive_task("test-task").await;
        assert!(result.is_ok(), "Nhận task thất bại: {:?}", result.err());
        
        // Thay panic bằng assertion với skip logic
        let output = if let Ok(output) = result {
            output
        } else {
            assert!(false, "Không nhận được task kết quả: {:?}", result.err());
            return;
        };
        
        assert!(output.contains("Processed task: test-task"));
        assert!(output.contains("test-master-2"));
    }
    
    #[tokio::test]
    async fn test_invalid_operations() {
        let service = DefaultSlaveNodeService::default();
        
        // Thử nhận task khi chưa đăng ký
        let result = service.receive_task("test-task").await;
        assert!(result.is_err(), "Nhận task phải thất bại khi chưa đăng ký");
        
        // Kiểm tra nội dung lỗi
        if let Err(error) = result {
            // Thay panic bằng assertion
            assert!(matches!(error, SlaveNodeError::NotRegistered(_)), 
                  "Phải là NotRegistered error, nhưng nhận được: {:?}", error);
            
            if let SlaveNodeError::NotRegistered(msg) = error {
                assert!(msg.contains("Not registered"));
            }
        }
        
        // Thử báo cáo trạng thái khi chưa đăng ký
        let result = service.report_status("healthy").await;
        assert!(result.is_err(), "Báo cáo trạng thái phải thất bại khi chưa đăng ký");
        
        // Kiểm tra nội dung lỗi
        if let Err(error) = result {
            // Thay panic bằng assertion
            assert!(matches!(error, SlaveNodeError::NotRegistered(_)), 
                  "Phải là NotRegistered error, nhưng nhận được: {:?}", error);
            
            if let SlaveNodeError::NotRegistered(msg) = error {
                assert!(msg.contains("Not registered"));
            }
        }
        
        // Đăng ký với master ID trống
        let result = service.register_self("", NodeProfile::default()).await;
        assert!(result.is_err(), "Đăng ký với master ID trống phải thất bại");
        
        // Kiểm tra nội dung lỗi
        if let Err(error) = result {
            // Thay panic bằng assertion
            assert!(matches!(error, SlaveNodeError::InvalidInput(_)), 
                  "Phải là InvalidInput error, nhưng nhận được: {:?}", error);
            
            if let SlaveNodeError::InvalidInput(msg) = error {
                assert!(msg.contains("empty"));
            }
        }
        
        // Thử nhận task trống
        let mut profile = NodeProfile::default();
        profile.add_role(NodeRole::Worker);
        
        // Đăng ký để có thể thử nhận task trống
        let register_result = service.register_self("test-master", profile).await;
        assert!(register_result.is_ok(), "Đăng ký với master thất bại: {:?}", register_result.err());
        
        let result = service.receive_task("").await;
        assert!(result.is_err(), "Nhận task trống phải thất bại");
        
        // Kiểm tra nội dung lỗi
        if let Err(error) = result {
            // Thay panic bằng assertion
            assert!(matches!(error, SlaveNodeError::InvalidInput(_)), 
                  "Phải là InvalidInput error, nhưng nhận được: {:?}", error);
            
            if let SlaveNodeError::InvalidInput(msg) = error {
                assert!(msg.contains("empty"));
            }
        }
    }
    
    #[tokio::test]
    async fn test_health_check() {
        let service = DefaultSlaveNodeService::default();
        
        // Ban đầu health check thất bại (chưa đăng ký)
        let result = service.health_check().await;
        assert!(result.is_ok(), "Health check ban đầu thất bại: {:?}", result.err());
        
        // Thay panic bằng assertion với skip logic
        let is_healthy = if let Ok(status) = result {
            status
        } else {
            assert!(false, "Không thể kiểm tra health: {:?}", result.err());
            return;
        };
        
        assert_eq!(is_healthy, false, "Node chưa đăng ký phải không healthy");
        
        // Đăng ký và health check lại
        let mut profile = NodeProfile::default();
        profile.add_role(NodeRole::Worker);
        
        let register_result = service.register_self("test-master-3", profile).await;
        assert!(register_result.is_ok(), "Đăng ký với master thất bại: {:?}", register_result.err());
        
        let result = service.health_check().await;
        assert!(result.is_ok(), "Health check sau khi đăng ký thất bại: {:?}", result.err());
        
        // Thay panic bằng assertion với skip logic
        let is_healthy = if let Ok(healthy) = result {
            healthy
        } else {
            assert!(false, "Không thể kiểm tra health: {:?}", result.err());
            return;
        };
        
        assert!(is_healthy, "Node phải healthy sau khi vừa đăng ký");
    }
    
    #[tokio::test]
    async fn test_report_status() {
        let service = DefaultSlaveNodeService::default();
        let mut profile = NodeProfile::default();
        profile.add_role(NodeRole::Worker);
        
        // Đăng ký với master
        let register_result = service.register_self("test-master-4", profile).await;
        assert!(register_result.is_ok(), "Đăng ký với master thất bại: {:?}", register_result.err());
        
        // Báo cáo trạng thái
        let result = service.report_status("running").await;
        assert!(result.is_ok());
        
        // Báo cáo trạng thái trống
        let result = service.report_status("").await;
        assert!(result.is_err());
        
        // Kiểm tra loại lỗi
        if let Err(error) = result {
            // Thay panic bằng assertion
            assert!(matches!(error, SlaveNodeError::InvalidInput(_)), 
                  "Phải là InvalidInput error, nhưng nhận được: {:?}", error);
            
            if let SlaveNodeError::InvalidInput(msg) = error {
                assert!(msg.contains("empty"));
            }
        }
    }
}
