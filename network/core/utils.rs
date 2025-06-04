//! Utility functions for network engine

use rand::{distributions::Alphanumeric, Rng};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

/// Returns a random node id (mock)
pub fn generate_node_id() -> String {
    let rand_str: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(8)
        .map(char::from)
        .collect();
    format!("node-{}", rand_str)
}

/// Mock: check node health (always ok)
pub fn check_node_health(_node_id: &str) -> bool {
    true
}

/// Get current timestamp (seconds since epoch)
pub fn get_current_timestamp() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(e) => {
            warn!("Error getting current timestamp: {:?}", e);
            0 // Fallback nếu xảy ra lỗi (clock set về quá khứ)
        }
    }
}
