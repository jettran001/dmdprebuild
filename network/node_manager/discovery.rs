// DiscoveryService trait: manage node discovery in the network
use async_trait::async_trait;
use tracing::{info, warn, error};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time;

// Cập nhật import để sử dụng NodeProfile từ node_manager::mod.rs
use crate::node_manager::NodeProfile;

/// Timeout cho các hoạt động discovery
const DISCOVERY_TIMEOUT_MS: u64 = 3000; // 3 giây
const CLEANUP_INTERVAL_SEC: u64 = 600; // 10 phút
const CLEANUP_INACTIVE_AFTER_SEC: u64 = 1800; // 30 phút

#[async_trait]
pub trait DiscoveryService: Send + Sync {
    /// Register a node for discovery (async)
    async fn register_node(&self, node_id: &str, profile: NodeProfile) -> Result<(), String>;
    /// Discover all registered nodes (async)
    async fn discover_nodes(&self) -> Result<Vec<String>, String>;
    /// Get node profile by ID (async)
    async fn get_node_info(&self, node_id: &str) -> Result<Option<NodeProfile>, String>;
    /// Health check for discovery service (async)
    async fn health_check(&self) -> Result<bool, String>;
}

/// Default implementation of DiscoveryService (async, thread-safe)
pub struct DefaultDiscoveryService {
    nodes: Arc<tokio::sync::Mutex<HashMap<String, NodeProfile>>>,
    /// Thời điểm cập nhật cuối cùng của mỗi node
    node_last_update: Arc<tokio::sync::Mutex<HashMap<String, Instant>>>,
    /// Flag để dừng cleanup task khi cần
    cleanup_running: Arc<tokio::sync::watch::Sender<bool>>,
}

impl Default for DefaultDiscoveryService {
    fn default() -> Self {
        // Tạo channel để gửi tín hiệu dừng cleanup task
        let (cleanup_tx, cleanup_rx) = tokio::sync::watch::channel(false);
        let nodes: Arc<tokio::sync::Mutex<HashMap<String, NodeProfile>>> = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let node_last_update: Arc<tokio::sync::Mutex<HashMap<String, Instant>>> = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        
        // Spawn cleanup task
        let nodes_clone = nodes.clone();
        let node_last_update_clone = node_last_update.clone();
        
        tokio::spawn(async move {
            let mut rx = cleanup_rx;
            
            loop {
                // Kiểm tra có cần dừng không
                if *rx.borrow() {
                    info!("[DiscoveryService] Cleanup task terminated");
                    break;
                }
                
                // Sleep và kiểm tra tín hiệu hủy
                tokio::select! {
                    _ = time::sleep(Duration::from_secs(CLEANUP_INTERVAL_SEC)) => {
                        // Thực hiện cleanup
                        let now = Instant::now();
                        let mut nodes_to_remove: Vec<String> = Vec::new();
                        
                        // Lấy lock node_last_update trước
                        let last_update = node_last_update_clone.lock().await;
                        let mut last_update = last_update;
                        
                        // Xác định các node cần xóa
                        for (node_id, last_active) in last_update.iter() {
                            if now.duration_since(*last_active).as_secs() > CLEANUP_INACTIVE_AFTER_SEC {
                                nodes_to_remove.push(node_id.clone());
                            }
                        }
                        
                        // Lấy lock nodes sau khi đã xác định những gì cần xóa
                        if !nodes_to_remove.is_empty() {
                            let nodes = nodes_clone.lock().await;
                            let mut nodes = nodes;
                            
                            // Xóa các node không hoạt động
                            for node_id in &nodes_to_remove {
                                nodes.remove(node_id);
                                last_update.remove(node_id);
                                warn!("[DiscoveryService] Removed inactive node '{}' after {} seconds", 
                                    node_id, CLEANUP_INACTIVE_AFTER_SEC);
                            }
                            
                            info!("[DiscoveryService] Cleanup completed, removed {} inactive nodes", 
                                nodes_to_remove.len());
                        }
                    },
                    _ = rx.changed() => {
                        if *rx.borrow() {
                            info!("[DiscoveryService] Cleanup task received stop signal");
                            break;
                        }
                    }
                }
            }
        });
        
        Self {
            nodes,
            node_last_update,
            cleanup_running: Arc::new(cleanup_tx),
        }
    }
}

impl Drop for DefaultDiscoveryService {
    fn drop(&mut self) {
        // Gửi tín hiệu kết thúc cleanup task khi service bị hủy
        let _ = self.cleanup_running.send(true);
    }
}

#[async_trait]
impl DiscoveryService for DefaultDiscoveryService {
    async fn register_node(&self, node_id: &str, profile: NodeProfile) -> Result<(), String> {
        if node_id.is_empty() {
            return Err("Node ID cannot be empty".to_string());
        }
        
        // Tạo bản sao của profile để kiểm tra
        if profile.roles.is_empty() {
            warn!("[DiscoveryService] Node '{}' registered with empty roles", node_id);
        }
        
        // Kiểm tra các thông số hợp lệ
        if profile.cpu_cores == 0 || profile.ram_gb == 0 {
            return Err("Invalid profile: CPU cores and RAM must be greater than 0".to_string());
        }
        
        // Cập nhật vào danh sách nodes
        {
            let mut nodes = self.nodes.lock().await;
            nodes.insert(node_id.to_string(), profile);
        }
        
        // Cập nhật thời gian hoạt động
        {
            let mut last_update = self.node_last_update.lock().await;
            last_update.insert(node_id.to_string(), Instant::now());
        }
        
        info!("[DiscoveryService] Node '{}' registered successfully", node_id);
        Ok(())
    }
    
    async fn discover_nodes(&self) -> Result<Vec<String>, String> {
        // Thêm timeout cho toàn bộ quy trình discovery
        match time::timeout(
            Duration::from_millis(DISCOVERY_TIMEOUT_MS),
            async {
                let nodes = self.nodes.lock().await;
                nodes.keys().cloned().collect::<Vec<String>>()
            }
        ).await {
            Ok(node_list) => {
                info!("[DiscoveryService] Discovered {} nodes", node_list.len());
                Ok(node_list)
            },
            Err(_) => {
                error!("[DiscoveryService] Discovery operation timed out after {}ms", DISCOVERY_TIMEOUT_MS);
                Err(format!("Discovery operation timed out after {}ms", DISCOVERY_TIMEOUT_MS))
            }
        }
    }
    
    async fn get_node_info(&self, node_id: &str) -> Result<Option<NodeProfile>, String> {
        if node_id.is_empty() {
            return Err("Node ID cannot be empty".to_string());
        }
        
        let nodes = self.nodes.lock().await;
        let result = nodes.get(node_id).cloned();
        
        if result.is_none() {
            warn!("[DiscoveryService] Node info requested for unknown node '{}'", node_id);
        }
        
        Ok(result)
    }
    
    async fn health_check(&self) -> Result<bool, String> {
        // Cơ bản chỉ kiểm tra khả năng truy cập vào nodes
        match time::timeout(
            Duration::from_millis(DISCOVERY_TIMEOUT_MS / 2),
            async {
                let _ = self.nodes.lock().await;
                true
            }
        ).await {
            Ok(result) => Ok(result),
            Err(_) => {
                error!("[DiscoveryService] Health check timed out - service may be unresponsive");
                Ok(false)
            }
        }
    }
}

/// Get total number of nodes in the discovery service (async version)
pub async fn get_discovery_node_count(service: &impl DiscoveryService) -> Result<usize, String> {
    let nodes = service.discover_nodes().await?;
    Ok(nodes.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_register_and_discover() {
        let service = DefaultDiscoveryService::default();
        
        // Tạo profile test
        let profile = NodeProfile {
            cpu_cores: 4,
            gpu_count: 1,
            ram_gb: 16,
            disk_gb: 500,
            has_ssd: true,
            roles: vec![crate::core::types::NodeRole::CdnStorageNode],
        };
        
        // Đăng ký node
        let result = service.register_node("test-node-1", profile.clone()).await;
        assert!(result.is_ok(), "Node registration failed: {:?}", result);
        
        // Discover nodes
        let discover_result = service.discover_nodes().await;
        assert!(discover_result.is_ok(), "Node discovery failed: {:?}", discover_result.err());
        
        let nodes = discover_result.unwrap_or_default();
        assert_eq!(nodes.len(), 1, "Should discover exactly 1 node");
        assert_eq!(nodes[0], "test-node-1", "Node ID should match registered node");
        
        // Get node info
        let info_result = service.get_node_info("test-node-1").await;
        assert!(info_result.is_ok(), "Get node info failed: {:?}", info_result.err());
        
        let node_info = info_result.unwrap_or_default();
        assert!(node_info.is_some(), "Node info should exist");
        
        // Kiểm tra profile chỉ khi node_info có giá trị
        if let Some(retrieved_profile) = node_info {
            assert_eq!(retrieved_profile.cpu_cores, 4, "CPU cores should match");
            assert_eq!(retrieved_profile.ram_gb, 16, "RAM should match");
            assert_eq!(retrieved_profile.gpu_count, 1, "GPU count should match");
            assert_eq!(retrieved_profile.disk_gb, 500, "Disk space should match");
            assert!(retrieved_profile.has_ssd, "SSD flag should match");
        }
    }
    
    #[tokio::test]
    async fn test_get_unknown_node() {
        let service = DefaultDiscoveryService::default();
        let result = service.get_node_info("non-existent").await;
        assert!(result.is_ok(), "Get node info failed: {:?}", result.err());
        
        let node_info = result.unwrap_or_default();
        assert!(node_info.is_none(), "Node info for non-existent node should be None");
    }
    
    #[tokio::test]
    async fn test_health_check() {
        let service = DefaultDiscoveryService::default();
        let result = service.health_check().await;
        assert!(result.is_ok(), "Health check failed: {:?}", result.err());
        
        let is_healthy = result.unwrap_or_default();
        assert!(is_healthy, "Service should be healthy");
    }
}
