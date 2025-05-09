use serde::{Deserialize, Serialize};
use crate::node_manager::NodeRole;

/// Cấu hình profile cho node trong file Cargo.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomlProfile {
    /// Role của node trong mạng
    pub role: NodeRole,
    /// Mạng ưu tiên sử dụng
    pub preferred_network: String,
    /// Có bật GPU không
    pub gpu_enabled: bool,
}

impl Default for TomlProfile {
    fn default() -> Self {
        Self {
            role: NodeRole::Unknown,
            preferred_network: "libp2p".to_string(),
            gpu_enabled: false,
        }
    }
} 