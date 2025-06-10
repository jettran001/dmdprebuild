/// Anti-MEV Module cho Smart Trade System
///
/// Module này triển khai các chiến lược chống MEV để bảo vệ giao dịch
/// khỏi các tấn công như front-running, sandwich attacks, và time-bandit.
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Result, Context, anyhow};
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};

use super::optimizer::GasOptimizer;

/// Mức độ bảo vệ MEV
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MevProtectionLevel {
    /// Không bảo vệ, chấp nhận rủi ro
    None,
    
    /// Bảo vệ cơ bản: gas price cao hơn, slippage thấp
    Basic,
    
    /// Bảo vệ trung bình: random slippage, delay giao dịch
    Medium,
    
    /// Bảo vệ cao: private transactions, bundled transaction
    High,
}

/// Cấu hình bảo vệ MEV
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MevProtectionConfig {
    /// Có bật bảo vệ MEV không
    pub enabled: bool,
    
    /// Mức độ bảo vệ
    pub protection_level: String, // "none", "basic", "medium", "high"
    
    /// Có sử dụng flashbots/private tx không
    pub use_private_tx: bool,
    
    /// Timeout cho việc đợi xác nhận (seconds)
    pub confirmation_timeout: u64,
    
    /// Thời gian delay ngẫu nhiên (ms)
    pub random_delay_ms: Option<u64>,
    
    /// Gas price multiplier tối đa
    pub max_gas_multiplier: f64,
    
    /// Slippage tối đa cho phép
    pub max_slippage: f64,
    
    /// Nonce re-use attempt tối đa
    pub max_nonce_reuse_attempt: u32,
    
    /// Chain IDs hỗ trợ private tx
    pub private_tx_supported_chains: Vec<u32>,
}

impl Default for MevProtectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            protection_level: "basic".to_string(),
            use_private_tx: false,
            confirmation_timeout: 60,
            random_delay_ms: Some(2000),
            max_gas_multiplier: 2.0,
            max_slippage: 5.0,
            max_nonce_reuse_attempt: 3,
            private_tx_supported_chains: vec![1, 5, 42161], // Ethereum mainnet, Goerli, Arbitrum
        }
    }
}

/// Service bảo vệ chống MEV
pub struct AntiMevProtection {
    /// Cấu hình
    config: RwLock<MevProtectionConfig>,
    
    /// Gas optimizer
    gas_optimizer: Arc<GasOptimizer>,
    
    /// Danh sách các MEV bot đã biết
    known_mev_bots: RwLock<HashSet<String>>,
    
    /// Lịch sử mempool
    mempool_history: RwLock<HashMap<u32, Vec<MempoolTransaction>>>,
}

impl AntiMevProtection {
    /// Tạo instance mới của AntiMevProtection
    pub fn new(gas_optimizer: Arc<GasOptimizer>) -> Self {
        Self {
            config: RwLock::new(MevProtectionConfig::default()),
            gas_optimizer,
            known_mev_bots: RwLock::new(HashSet::new()),
            mempool_history: RwLock::new(HashMap::new()),
        }
    }
    
    /// Tạo instance mới với cấu hình tùy chỉnh
    pub fn with_config(gas_optimizer: Arc<GasOptimizer>, config: MevProtectionConfig) -> Self {
        Self {
            config: RwLock::new(config),
            gas_optimizer,
            known_mev_bots: RwLock::new(HashSet::new()),
            mempool_history: RwLock::new(HashMap::new()),
        }
    }
    
    /// Thêm địa chỉ MEV bot đã biết
    pub async fn add_known_mev_bot(&self, address: &str) {
        let mut known_bots = self.known_mev_bots.write().await;
        known_bots.insert(address.to_lowercase());
    }
    
    /// Thêm danh sách địa chỉ MEV bot đã biết
    pub async fn add_known_mev_bots(&self, addresses: &[&str]) {
        let mut known_bots = self.known_mev_bots.write().await;
        for address in addresses {
            known_bots.insert(address.to_lowercase());
        }
    }
    
    /// Kiểm tra xem địa chỉ có phải là MEV bot đã biết không
    pub async fn is_known_mev_bot(&self, address: &str) -> bool {
        let known_bots = self.known_mev_bots.read().await;
        known_bots.contains(&address.to_lowercase())
    }
    
    /// Cập nhật giao dịch trong mempool history
    pub async fn update_mempool_transactions(&self, chain_id: u32, txs: Vec<MempoolTransaction>) -> Result<()> {
        let mut history = self.mempool_history.write().await;
        
        if let Some(chain_txs) = history.get_mut(&chain_id) {
            // Merge transactions, keeping the newer ones
            for tx in txs {
                if !chain_txs.iter().any(|existing| existing.tx_hash == tx.tx_hash) {
                    chain_txs.push(tx);
                }
            }
            
            // Limit history size
            if chain_txs.len() > 1000 {
                chain_txs.sort_by(|a, b| b.detected_at.cmp(&a.detected_at));
                chain_txs.truncate(1000);
            }
        } else {
            // No existing data for this chain
            history.insert(chain_id, txs);
        }
        
        Ok(())
    }
    
    /// Phân tích mempool để phát hiện MEV attacks
    pub async fn analyze_mempool_for_mev(&self, chain_id: u32, token_address: &str) -> Result<MevAnalysisResult> {
        let mempool_txs = {
            let history = self.mempool_history.read().await;
            let chain_txs = history.get(&chain_id)
                .ok_or_else(|| anyhow!("No mempool data available for chain {}", chain_id))?;
                
            // Convert to HashMap for easier lookup
            let mut txs_map = HashMap::new();
            for tx in chain_txs {
                txs_map.insert(tx.tx_hash.clone(), tx.clone());
            }
            txs_map
        };
        
        let known_bots = self.known_mev_bots.read().await;
        
        // Use the shared analysis function
        analyze_mempool_for_mev(&mempool_txs, token_address, chain_id, &known_bots).await
    }
    
    /// Tạo tham số giao dịch chống MEV
    pub async fn create_anti_mev_parameters(
        &self, 
        chain_id: u32, 
        token_address: &str,
        is_buy: bool,
        amount: f64,
    ) -> Result<AntiMevTxParams> {
        // Phân tích MEV activity
        let mev_analysis = self.analyze_mempool_for_mev(chain_id, token_address).await
            .context("Failed to analyze mempool for MEV activity")?;
        
        // Lấy gas price gợi ý
        let base_gas_price = self.gas_optimizer.get_suggested_gas_price(chain_id).await?;
        
        // Lấy cấu hình
        let config = self.config.read().await;
        let base_slippage = if is_buy { 1.0 } else { 0.5 }; // Default slippage
        
        // Generate parameters using shared function
        generate_anti_mev_tx_params(
            &mev_analysis,
            base_gas_price,
            base_slippage,
            is_buy,
            config.use_private_tx,
            &config.private_tx_supported_chains,
            chain_id
        )
    }
} 