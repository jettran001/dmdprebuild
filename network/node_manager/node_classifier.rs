use crate::node_manager::{NodeProfile, NodeRole};
use crate::dispatcher::task_dispatcher::{Dispatcher, Task};
use rand::Rng;
use tracing::{info, warn};

/// NodeClassifier: Responsible for benchmarking and classifying node hardware
pub struct NodeClassifier;

impl NodeClassifier {
    /// Simulate hardware benchmarking (in real system, gather from sysinfo)
    /// NOTE: This is a simulated benchmark for demo/testing, not real hardware measurement.
    pub fn benchmark_node() -> NodeProfile {
        // Mock: random values for demo, mô phỏng benchmark thực tế hơn
        let mut rng = rand::thread_rng();
        let cpu_cores = rng.gen_range(2..=16);
        let gpu_count = rng.gen_range(0..=2);
        let ram_gb = rng.gen_range(4..=64);
        let disk_gb = rng.gen_range(128..=2048);
        let has_ssd = rng.gen_bool(0.7);
        let net_bandwidth_mbps = rng.gen_range(10..=1000); // Thêm chỉ số network
        info!("[NodeClassifier] Simulated benchmark: CPU {} cores, GPU {}, RAM {} GB, Disk {} GB, SSD {}, Net {} Mbps", cpu_cores, gpu_count, ram_gb, disk_gb, has_ssd, net_bandwidth_mbps);
        NodeProfile::new(cpu_cores, gpu_count, ram_gb, disk_gb, has_ssd)
    }

    /// Validate input profile nếu nhận từ external (mock)
    pub fn validate_profile(profile: &NodeProfile) -> Result<(), String> {
        if profile.cpu_cores == 0 || profile.ram_gb == 0 || profile.disk_gb == 0 {
            return Err("Invalid node profile: zero resource".to_string());
        }
        if profile.cpu_cores > 128 || profile.ram_gb > 2048 {
            warn!("[NodeClassifier] Unusual resource detected: cpu_cores={}, ram_gb={}", profile.cpu_cores, profile.ram_gb);
        }
        Ok(())
    }

    /// Classify node based on hardware profile
    pub fn classify_node(profile: &mut NodeProfile) {
        if let Err(e) = Self::validate_profile(profile) {
            warn!("[NodeClassifier] Profile validation failed: {}", e);
            profile.roles.clear();
            profile.roles.push(NodeRole::Unknown);
            return;
        }
        profile.roles.clear();
        if profile.gpu_count > 0 {
            profile.roles.push(NodeRole::AiTraining);
        }
        if profile.cpu_cores >= 8 && profile.gpu_count == 0 {
            profile.roles.push(NodeRole::SnipebotExecutor);
        }
        if profile.disk_gb >= 1024 {
            profile.roles.push(NodeRole::CdnStorageNode);
        }
        if profile.ram_gb >= 32 {
            profile.roles.push(NodeRole::RedisNode);
        }
        if profile.cpu_cores >= 4 && profile.ram_gb >= 16 {
            profile.roles.push(NodeRole::WalletNode);
        }
        if profile.has_ssd && profile.cpu_cores >= 4 {
            profile.roles.push(NodeRole::EdgeCompute);
        }
        if profile.roles.is_empty() {
            profile.roles.push(NodeRole::Unknown);
        }
        info!("[NodeClassifier] Node classified: roles={:?}", profile.roles);
    }

    /// Classify and dispatch tasks for a node
    pub fn classify_and_dispatch(profile: &mut NodeProfile, dispatcher: &Dispatcher) -> Vec<Task> {
        Self::classify_node(profile);
        dispatcher.map_roles_to_tasks(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_classification() {
        let mut profile = NodeClassifier::benchmark_node();
        NodeClassifier::classify_node(&mut profile);
        assert!(!profile.roles.is_empty());
    }
}
