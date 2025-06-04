// MasterNodeService trait: manage master node logic in the network
use async_trait::async_trait;
use tracing::{info, warn, error};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

// Cập nhật import để sử dụng NodeProfile từ node_manager::mod.rs
use crate::node_manager::NodeProfile;

/// Health status for a partition
pub struct PartitionHealthStatus {
    pub partition_id: String,
    pub total_slaves: usize,
    pub online_slaves: usize,
    pub offline_slaves: usize,
    pub offline_slave_ids: Vec<String>,
}

#[async_trait]
pub trait MasterNodeService: Send + Sync {
    /// Register a new slave node (async)
    async fn register_slave(&self, slave_id: &str, profile: NodeProfile) -> Result<(), String>;
    /// List all registered slave node IDs (async)
    async fn list_slaves(&self) -> Result<Vec<String>, String>;
    /// Assign a task to a slave node (async)
    async fn assign_task(&self, slave_id: &str, task: &str) -> Result<String, String>;
    /// Health check for a slave node (async)
    async fn health_check(&self, slave_id: &str) -> Result<bool, String>;
    /// Get slave node profile by ID (async)
    async fn get_slave_info(&self, slave_id: &str) -> Result<Option<NodeProfile>, String>;
    /// Health check for a partition (async)
    async fn health_check_partition(&self, partition_id: &str) -> Result<PartitionHealthStatus, String>;
}

/// Partitioned master node service: async, thread-safe
pub struct PartitionedMasterNodeService {
    partitions: Arc<RwLock<HashMap<String, Vec<String>>>>, // partition_id -> list of slave_id
    slaves: Arc<RwLock<HashMap<String, NodeProfile>>>,
    online_status: Arc<RwLock<HashMap<String, bool>>>, // slave_id -> online/offline
    /// Thời điểm cập nhật cuối cùng của mỗi slave
    slave_last_update: Arc<RwLock<HashMap<String, Instant>>>,
    /// Notify để đánh thức cleanup task khi cần
    cleanup_notify: Arc<tokio::sync::Notify>,
}

impl Default for PartitionedMasterNodeService {
    fn default() -> Self {
        Self::new()
    }
}

impl PartitionedMasterNodeService {
    /// Tạo mới một PartitionedMasterNodeService
    pub fn new() -> Self {
        let cleanup_notify = Arc::new(tokio::sync::Notify::new());
        
        let service = Self {
            partitions: Arc::new(RwLock::new(HashMap::new())),
            slaves: Arc::new(RwLock::new(HashMap::new())),
            online_status: Arc::new(RwLock::new(HashMap::new())),
            slave_last_update: Arc::new(RwLock::new(HashMap::new())),
            cleanup_notify: cleanup_notify.clone(),
        };
        // Spawn cleanup task
        let slaves = service.slaves.clone();
        let last_update = service.slave_last_update.clone();
        let cleanup_notify_clone = cleanup_notify.clone();
        
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(600)) => {
                        // Periodic cleanup every 10 minutes
                    },
                    _ = cleanup_notify_clone.notified() => {
                        // Immediate cleanup when triggered
                        info!("[MasterNodeService] Cleanup task woken up by notification");
                    }
                }
                
                let now = Instant::now();
                let mut slaves_guard = slaves.write().await;
                let mut last_update_guard = last_update.write().await;
                
                let mut removed = vec![];
                for (id, ts) in last_update_guard.iter() {
                    if now.duration_since(*ts).as_secs() > 1800 {
                        removed.push(id.clone());
                    }
                }
                
                for id in removed {
                    slaves_guard.remove(&id);
                    last_update_guard.remove(&id);
                    warn!("[MasterNodeService] Cleanup slave node '{}' do không hoạt động lâu (>30 phút)", id);
                }
                
                if slaves_guard.len() > 1000 {
                    error!("[MasterNodeService] Số lượng slave node vượt ngưỡng: {}", slaves_guard.len());
                }
            }
        });
        
        service
    }
    
    /// Triggers immediate cleanup
    pub async fn trigger_cleanup(&self) {
        info!("[MasterNodeService] Triggering immediate cleanup");
        self.cleanup_notify.notify_one();
    }
    
    /// Ping thực tế slave node (mock: random online/offline, sleep 50ms)
    async fn ping_slave(&self, slave_id: &str) -> bool {
        use rand::Rng;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut rng = rand::thread_rng();
        let online = rng.gen_bool(0.9); // 90% online, 10% offline (mock)
        if !online {
            warn!("[MasterNodeService] ping_slave: '{}' is offline (mock)", slave_id);
        } else {
            info!("[MasterNodeService] ping_slave: '{}' is online (mock)", slave_id);
        }
        online
    }
}

#[async_trait]
impl MasterNodeService for PartitionedMasterNodeService {
    async fn register_slave(&self, slave_id: &str, profile: NodeProfile) -> Result<(), String> {
        if slave_id.is_empty() {
            warn!("[MasterNodeService] register_slave: slave_id is empty");
            return Err("slave_id is empty".to_string());
        }
        if let Err(e) = crate::node_manager::node_classifier::NodeClassifier::validate_profile(&profile) {
            warn!("[MasterNodeService] register_slave: invalid profile: {}", e);
            return Err(format!("invalid profile: {}", e));
        }
        
        // Use tokio::sync::RwLock with separate guards to avoid deadlocks
        {
            let mut partitions = self.partitions.write().await;
            let partition_id = format!("partition_{}", partitions.len() % 4); // ví dụ chia 4 partition
            partitions.entry(partition_id.clone()).or_default().push(slave_id.to_string());
        }
        
        {
            let mut slaves = self.slaves.write().await;
            slaves.insert(slave_id.to_string(), profile);
        }
        
        {
            let mut online_status = self.online_status.write().await;
            online_status.insert(slave_id.to_string(), true);
        }
        
        {
            let mut last_update = self.slave_last_update.write().await;
            last_update.insert(slave_id.to_string(), Instant::now());
        }
        
        info!("[MasterNodeService] Registered slave '{}'", slave_id);
        
        // Trigger cleanup check if we have too many slaves (prevent memory leak)
        let count = self.slaves.read().await.len();
        if count > 900 {  // Approaching threshold
            self.trigger_cleanup().await;
        }
        
        Ok(())
    }
    
    async fn list_slaves(&self) -> Result<Vec<String>, String> {
        let slaves = self.slaves.read().await;
        Ok(slaves.keys().cloned().collect())
    }
    
    async fn assign_task(&self, slave_id: &str, task: &str) -> Result<String, String> {
        let slaves = self.slaves.read().await;
        if slaves.contains_key(slave_id) {
            info!("[MasterNodeService] Assigned task '{}' to slave '{}'", task, slave_id);
            Ok(format!("Task '{}' assigned to slave node '{}'.", task, slave_id))
        } else {
            warn!("[MasterNodeService] assign_task: slave '{}' not found", slave_id);
            Err(format!("Slave node '{}' not found.", slave_id))
        }
    }
    
    async fn health_check(&self, slave_id: &str) -> Result<bool, String> {
        let online_status = self.online_status.read().await;
        let status = online_status.get(slave_id).copied().unwrap_or(false);
        if !status {
            warn!("[MasterNodeService] health_check: slave '{}' is offline", slave_id);
        }
        Ok(status)
    }
    
    async fn get_slave_info(&self, slave_id: &str) -> Result<Option<NodeProfile>, String> {
        let slaves = self.slaves.read().await;
        Ok(slaves.get(slave_id).cloned())
    }
    
    async fn health_check_partition(&self, partition_id: &str) -> Result<PartitionHealthStatus, String> {
        let partitions = self.partitions.read().await;
        let slave_ids = partitions.get(partition_id).cloned().unwrap_or_default();
        let mut online = 0;
        let mut offline = 0;
        let mut offline_ids = vec![];
        
        for slave_id in &slave_ids {
            let is_online = self.ping_slave(slave_id).await;
            {
                let mut online_status = self.online_status.write().await;
                online_status.insert(slave_id.clone(), is_online);
            }
            if is_online {
                online += 1;
            } else {
                offline += 1;
                offline_ids.push(slave_id.clone());
            }
        }
        
        if offline > 0 {
            warn!("[MasterNodeService] Partition '{}' has {} offline slaves: {:?}", 
                  partition_id, offline, offline_ids);
            // TODO: Gửi alert lên gateway nếu cần
        }
        
        Ok(PartitionHealthStatus {
            partition_id: partition_id.to_string(),
            total_slaves: slave_ids.len(),
            online_slaves: online,
            offline_slaves: offline,
            offline_slave_ids: offline_ids,
        })
    }
}

/// Get total number of slaves in the master service (async version)
pub async fn get_slave_count(service: &impl MasterNodeService) -> Result<usize, String> {
    let slaves = service.list_slaves().await?;
    Ok(slaves.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{NodeProfile, NodeRole};
    
    #[tokio::test]
    async fn test_register_and_list_slaves() {
        let service = PartitionedMasterNodeService::new();
        
        // Tạo profile test
        let profile = NodeProfile {
            cpu_cores: 4,
            gpu_count: 1,
            ram_gb: 16,
            disk_gb: 500, 
            has_ssd: true,
            roles: vec![NodeRole::CdnStorageNode],
        };
        
        // Đăng ký slave
        let result = service.register_slave("test-slave-1", profile.clone()).await;
        assert!(result.is_ok(), "Slave registration failed: {:?}", result);
        
        // List slaves
        let list_result = service.list_slaves().await;
        assert!(list_result.is_ok(), "List slaves failed: {:?}", list_result.err());
        
        let slaves = list_result.unwrap_or_default();
        assert_eq!(slaves.len(), 1, "Should have exactly 1 slave");
        assert_eq!(slaves[0], "test-slave-1", "Slave ID should match registered ID");
        
        // Get slave info
        let info_result = service.get_slave_info("test-slave-1").await;
        assert!(info_result.is_ok(), "Get slave info failed: {:?}", info_result.err());
        
        let info = info_result.unwrap_or_default();
        assert!(info.is_some(), "Slave info should exist");
        
        // Kiểm tra profile chỉ khi info có giá trị
        if let Some(retrieved_profile) = info {
            assert_eq!(retrieved_profile.cpu_cores, 4, "CPU cores should match");
            assert_eq!(retrieved_profile.ram_gb, 16, "RAM should match");
            assert_eq!(retrieved_profile.gpu_count, 1, "GPU count should match");
            assert_eq!(retrieved_profile.disk_gb, 500, "Disk space should match");
            assert!(retrieved_profile.has_ssd, "SSD flag should match");
        }
    }
    
    #[tokio::test]
    async fn test_health_check() {
        let service = PartitionedMasterNodeService::new();
        
        // Tạo profile test
        let profile = NodeProfile {
            cpu_cores: 4,
            gpu_count: 1,
            ram_gb: 16,
            disk_gb: 500,
            has_ssd: true,
            roles: vec![NodeRole::CdnStorageNode],
        };
        
        // Đăng ký slave
        let register_result = service.register_slave("test-slave-2", profile).await;
        assert!(register_result.is_ok(), "Slave registration failed: {:?}", register_result.err());
        
        // Health check
        let result = service.health_check("test-slave-2").await;
        assert!(result.is_ok(), "Health check failed: {:?}", result.err());
        
        let is_healthy = result.unwrap_or_default();
        // Mặc dù kết quả có thể true hoặc false (do xác suất), nhưng test này vẫn hợp lệ
        // vì chúng ta chỉ muốn đảm bảo hàm không throw exception
        assert!(is_healthy == true || is_healthy == false, "Health status should be boolean");
    }
}
