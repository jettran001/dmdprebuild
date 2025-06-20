pub mod discovery;
pub mod execution_adapter;
pub mod master;
pub mod node_classifier;
pub mod scheduler;
pub mod slave;

// Re-export types từ node_manager thay vì import từ crate::core::types
use serde::{Serialize, Deserialize};

/// NodeRole: Vai trò của Node trong hệ thống
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeRole {
    /// Master node điều phối các slave node
    Master,
    /// Slave node thực thi task được giao
    Slave,
    /// Worker node thực hiện các công việc tính toán
    Worker,
    /// Node xử lý AI training
    AiTraining,
    /// Node thực thi Snipebot
    SnipebotExecutor,
    /// Node lưu trữ CDN
    CdnStorageNode,
    /// Node Redis cache
    RedisNode,
    /// Node xử lý wallet
    WalletNode,
    /// Edge computing node
    EdgeCompute,
    /// Vai trò không xác định
    Unknown,
}

/// NodeProfile: Hồ sơ thông tin về node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProfile {
    /// Số lõi CPU
    pub cpu_cores: usize,
    /// Số lượng GPU
    pub gpu_count: usize,
    /// Dung lượng RAM (GB)
    pub ram_gb: usize,
    /// Dung lượng đĩa (GB)
    pub disk_gb: usize,
    /// Có SSD hay không
    pub has_ssd: bool,
    /// Các vai trò của node
    pub roles: Vec<NodeRole>,
}

impl NodeProfile {
    /// Tạo NodeProfile mới
    pub fn new(cpu_cores: usize, gpu_count: usize, ram_gb: usize, disk_gb: usize, has_ssd: bool) -> Self {
        Self {
            cpu_cores,
            gpu_count,
            ram_gb,
            disk_gb,
            has_ssd,
            roles: Vec::new(),
        }
    }
    
    /// Thêm vai trò cho node
    pub fn add_role(&mut self, role: NodeRole) {
        if !self.roles.contains(&role) {
            self.roles.push(role);
        }
    }
    
    /// Kiểm tra node có vai trò nào đó không
    pub fn has_role(&self, role: &NodeRole) -> bool {
        self.roles.contains(role)
    }
    
    /// Lấy số lượng vai trò
    pub fn role_count(&self) -> usize {
        self.roles.len()
    }
}

impl Default for NodeProfile {
    fn default() -> Self {
        Self {
            cpu_cores: 1,
            gpu_count: 0,
            ram_gb: 1,
            disk_gb: 10,
            has_ssd: false,
            roles: Vec::new(),
        }
    }
} 