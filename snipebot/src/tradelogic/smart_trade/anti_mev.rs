/**
 * Anti-MEV protection module
 * 
 * Module này cung cấp các chức năng để phát hiện và tránh các MEV attacks như
 * front-running, back-running, sandwich attacks. Nó sử dụng các kỹ thuật để giảm
 * thiểu rủi ro MEV và bảo vệ các giao dịch của người dùng.
 */

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Result, Context, bail, anyhow};
use tracing::{debug, error, info, warn};
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};
use chrono::{DateTime, Utc, Duration};

use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::{MempoolTransaction, TransactionType};
use super::optimizer::GasOptimizer;

/// Loại MEV attack
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MevAttackType {
    /// Front-running: Bot thấy giao dịch có lợi nhuận, chèn giao dịch của nó lên trước
    FrontRunning,
    
    /// Back-running: Bot đợi giao dịch lớn, sau đó chèn giao dịch của nó sau
    BackRunning,
    
    /// Sandwich attack: Bot chèn giao dịch trước và sau một giao dịch target
    SandwichAttack,
    
    /// Arbitrage thông thường
    Arbitrage,
    
    /// Liquidation thông thường
    Liquidation,
    
    /// JIT (just-in-time) Liquidity
    JitLiquidity,
    
    /// Loại MEV khác
    Other(String),
}

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

/// Kết quả phân tích MEV
#[derive(Debug, Clone)]
pub struct MevAnalysisResult {
    /// Token address being analyzed
    pub token_address: String,
    
    /// Chain ID
    pub chain_id: u32,
    
    /// Liệu có phát hiện MEV activity không
    pub mev_activity_detected: bool,
    
    /// Loại MEV attack đã phát hiện
    pub attack_types: Vec<MevAttackType>,
    
    /// Địa chỉ của các bot đã phát hiện
    pub mev_bot_addresses: Vec<String>,
    
    /// Mempool transactions liên quan
    pub related_transactions: Vec<MempoolTransaction>,
    
    /// Ước tính profit của MEV bot
    pub estimated_profit: Option<f64>,
    
    /// Risk score (0-100)
    pub risk_score: u8,
    
    /// Đề xuất giao dịch
    pub recommendation: String,
    
    /// Thời gian phân tích
    pub timestamp: DateTime<Utc>,
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
        
        // Lấy hoặc tạo mới lịch sử cho chain này
        let chain_history = history.entry(chain_id).or_insert_with(Vec::new);
        
        // Thêm giao dịch mới
        chain_history.extend(txs);
        
        // Giới hạn kích thước lịch sử (giữ 1000 giao dịch gần nhất)
        if chain_history.len() > 1000 {
            let overflow = chain_history.len() - 1000;
            chain_history.drain(0..overflow);
        }
        
        // Sắp xếp theo timestamp
        chain_history.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        
        Ok(())
    }
    
    /// Phân tích mempool để phát hiện MEV activity
    pub async fn analyze_mempool_for_mev(&self, chain_id: u32, token_address: &str) -> Result<MevAnalysisResult> {
        let history = self.mempool_history.read().await;
        
        // Lấy lịch sử mempool cho chain này
        let chain_history = match history.get(&chain_id) {
            Some(h) => h,
            None => {
                return Ok(MevAnalysisResult {
                    token_address: token_address.to_string(),
                    chain_id,
                    mev_activity_detected: false,
                    attack_types: vec![],
                    mev_bot_addresses: vec![],
                    related_transactions: vec![],
                    estimated_profit: None,
                    risk_score: 0,
                    recommendation: "Not enough mempool data to analyze".to_string(),
                    timestamp: Utc::now(),
                });
            }
        };
        
        // Lọc giao dịch liên quan đến token này
        let token_txs: Vec<MempoolTransaction> = chain_history
            .iter()
            .filter(|tx| {
                tx.token_address.as_ref().map_or(false, |addr| {
                    addr.eq_ignore_ascii_case(token_address)
                })
            })
            .cloned()
            .collect();
        
        if token_txs.is_empty() {
            return Ok(MevAnalysisResult {
                token_address: token_address.to_string(),
                chain_id,
                mev_activity_detected: false,
                attack_types: vec![],
                mev_bot_addresses: vec![],
                related_transactions: vec![],
                estimated_profit: None,
                risk_score: 0,
                recommendation: "No transactions found for this token".to_string(),
                timestamp: Utc::now(),
            });
        }
        
        // Tìm các giao dịch swap/transfer trong khoảng thời gian gần đây (60 giây)
        let now = Utc::now();
        let recent_txs: Vec<&MempoolTransaction> = token_txs
            .iter()
            .filter(|tx| {
                // Chỉ quan tâm đến giao dịch gần đây
                let tx_time = DateTime::from_timestamp(tx.timestamp as i64, 0)
                    .unwrap_or_else(|| now - Duration::seconds(3600));
                
                now.signed_duration_since(tx_time).num_seconds() <= 60
            })
            .collect();
        
        if recent_txs.is_empty() {
            return Ok(MevAnalysisResult {
                token_address: token_address.to_string(),
                chain_id,
                mev_activity_detected: false,
                attack_types: vec![],
                mev_bot_addresses: vec![],
                related_transactions: vec![],
                estimated_profit: None,
                risk_score: 0,
                recommendation: "No recent transactions found for this token".to_string(),
                timestamp: Utc::now(),
            });
        }
        
        // Phát hiện front-running
        let mut attack_types = Vec::new();
        let mut mev_bot_addresses = Vec::new();
        let mut risk_score = 0;
        let mut recommendation = String::new();
        
        // Phân tích front-running (nhiều giao dịch với gas price cao bất thường)
        let high_gas_txs: Vec<&MempoolTransaction> = recent_txs
            .iter()
            .filter(|tx| {
                // Gas price cao gấp 2 lần trung bình
                let avg_gas_price = recent_txs
                    .iter()
                    .map(|t| t.gas_price.unwrap_or(0.0))
                    .sum::<f64>() / recent_txs.len() as f64;
                
                tx.gas_price.unwrap_or(0.0) > avg_gas_price * 2.0
            })
            .copied()
            .collect();
        
        if !high_gas_txs.is_empty() {
            // Kiểm tra xem các giao dịch có phải của bot đã biết không
            for tx in &high_gas_txs {
                if let Some(from) = &tx.from {
                    if self.is_known_mev_bot(from).await {
                        attack_types.push(MevAttackType::FrontRunning);
                        mev_bot_addresses.push(from.clone());
                        risk_score += 30;
                    }
                }
            }
        }
        
        // Phát hiện sandwich attack: tìm các cặp giao dịch từ cùng một địa chỉ
        // mà đầu vào giống nhau nhưng đầu ra khác nhau
        let mut from_addresses = HashMap::new();
        for tx in &recent_txs {
            if let Some(from) = &tx.from {
                from_addresses.entry(from.clone())
                    .or_insert_with(Vec::new)
                    .push(tx);
            }
        }
        
        for (addr, txs) in from_addresses {
            if txs.len() >= 2 {
                // Sắp xếp theo timestamp
                let mut txs_sorted = txs.clone();
                txs_sorted.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
                
                // Nếu thời gian giữa tx đầu và cuối ngắn (< 10s) và có tx khác ở giữa
                // có thể là sandwich attack
                if txs_sorted.len() >= 2 {
                    let first_tx = txs_sorted.first().unwrap();
                    let last_tx = txs_sorted.last().unwrap();
                    
                    let first_time = DateTime::from_timestamp(first_tx.timestamp as i64, 0)
                        .unwrap_or_else(|| now - Duration::seconds(3600));
                    
                    let last_time = DateTime::from_timestamp(last_tx.timestamp as i64, 0)
                        .unwrap_or_else(|| now - Duration::seconds(3600));
                    
                    // Thời gian giữa tx đầu và cuối ngắn (< 10s)
                    if last_time.signed_duration_since(first_time).num_seconds() < 10 {
                        // Và có tx khác ở giữa (từ địa chỉ khác)
                        let middle_txs: Vec<_> = recent_txs.iter()
                            .filter(|tx| {
                                tx.from.as_ref().map_or(false, |f| f != &addr)
                                    && tx.timestamp > first_tx.timestamp
                                    && tx.timestamp < last_tx.timestamp
                            })
                            .collect();
                        
                        if !middle_txs.is_empty() {
                            attack_types.push(MevAttackType::SandwichAttack);
                            mev_bot_addresses.push(addr.clone());
                            risk_score += 50;
                        }
                    }
                }
            }
        }
        
        // Phân tích arbitrage (nhiều giao dịch liên tiếp từ cùng một địa chỉ)
        // với các token khác nhau trong một khoảng thời gian ngắn
        let mut arbitrage_candidates = Vec::new();
        for (addr, txs) in &from_addresses {
            if txs.len() >= 3 {
                // Sắp xếp theo timestamp
                let mut txs_sorted = txs.clone();
                txs_sorted.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
                
                let first_tx = txs_sorted.first().unwrap();
                let last_tx = txs_sorted.last().unwrap();
                
                let first_time = DateTime::from_timestamp(first_tx.timestamp as i64, 0)
                    .unwrap_or_else(|| now - Duration::seconds(3600));
                
                let last_time = DateTime::from_timestamp(last_tx.timestamp as i64, 0)
                    .unwrap_or_else(|| now - Duration::seconds(3600));
                
                // Thời gian giữa tx đầu và cuối ngắn (< 5s) - có thể là arbitrage
                if last_time.signed_duration_since(first_time).num_seconds() < 5 {
                    // Nếu các tx liên quan đến token khác nhau (ít nhất 2 token)
                    let token_addresses: HashSet<_> = txs_sorted
                        .iter()
                        .filter_map(|tx| tx.token_address.as_ref().cloned())
                        .collect();
                    
                    if token_addresses.len() >= 2 {
                        arbitrage_candidates.push((addr.clone(), txs_sorted.clone()));
                    }
                }
            }
        }
        
        if !arbitrage_candidates.is_empty() {
            for (addr, _) in arbitrage_candidates {
                attack_types.push(MevAttackType::Arbitrage);
                mev_bot_addresses.push(addr);
                risk_score += 20; // Arbitrage risk thấp hơn sandwich attack
            }
        }
        
        // Loại bỏ trùng lặp
        mev_bot_addresses.sort();
        mev_bot_addresses.dedup();
        
        attack_types.sort_by_key(|a| format!("{:?}", a));
        attack_types.dedup();
        
        // Xây dựng recommendation dựa vào các attack đã phát hiện
        if !attack_types.is_empty() {
            recommendation = "MEV activity detected. Recommendations: ".to_string();
            
            if attack_types.contains(&MevAttackType::FrontRunning) || attack_types.contains(&MevAttackType::SandwichAttack) {
                recommendation.push_str("Use private transaction if available. ");
                recommendation.push_str("Increase slippage tolerance but set tight deadline. ");
            }
            
            if attack_types.contains(&MevAttackType::SandwichAttack) {
                recommendation.push_str("Split large trades into smaller amounts. ");
            }
            
            recommendation.push_str("Avoid trading during high congestion periods.");
        } else {
            recommendation = "No suspicious MEV activity detected.".to_string();
        }
        
        // Risk score không vượt quá 100
        risk_score = risk_score.min(100);
        
        Ok(MevAnalysisResult {
            token_address: token_address.to_string(),
            chain_id,
            mev_activity_detected: !attack_types.is_empty(),
            attack_types,
            mev_bot_addresses,
            related_transactions: token_txs,
            estimated_profit: None, // Không đủ dữ liệu để ước tính profit
            risk_score,
            recommendation,
            timestamp: Utc::now(),
        })
    }
    
    /// Tạo tham số chống MEV cho giao dịch
    pub async fn create_anti_mev_parameters(
        &self, 
        chain_id: u32, 
        token_address: &str,
        is_buy: bool,
        amount: f64,
    ) -> Result<AntiMevTxParams> {
        // Đọc cấu hình
        let config = self.config.read().await;
        
        // Nếu không bật bảo vệ MEV
        if !config.enabled {
            return Ok(AntiMevTxParams::default());
        }
        
        // Phân tích MEV activity
        let mev_analysis = self.analyze_mempool_for_mev(chain_id, token_address).await?;
        
        // Mức độ bảo vệ dựa vào cấu hình
        let protection_level = match config.protection_level.as_str() {
            "none" => MevProtectionLevel::None,
            "basic" => MevProtectionLevel::Basic,
            "medium" => MevProtectionLevel::Medium,
            "high" => MevProtectionLevel::High,
            _ => MevProtectionLevel::Basic,
        };
        
        // Nếu có MEV activity, tăng mức độ bảo vệ
        let actual_protection = if mev_analysis.mev_activity_detected {
            match protection_level {
                MevProtectionLevel::None => MevProtectionLevel::Basic,
                MevProtectionLevel::Basic => MevProtectionLevel::Medium,
                _ => protection_level,
            }
        } else {
            protection_level
        };
        
        // Tính gas price multiplier dựa trên mức độ risk
        let risk_factor = mev_analysis.risk_score as f64 / 100.0;
        let gas_multiplier = 1.0 + risk_factor * (config.max_gas_multiplier - 1.0);
        
        // Tính tham số của giao dịch dựa trên mức độ bảo vệ
        let (use_private_tx, delay_ms, random_slippage) = match actual_protection {
            MevProtectionLevel::None => (false, 0, false),
            MevProtectionLevel::Basic => (false, 0, false),
            MevProtectionLevel::Medium => (false, config.random_delay_ms.unwrap_or(0), true),
            MevProtectionLevel::High => (
                config.use_private_tx && config.private_tx_supported_chains.contains(&chain_id),
                config.random_delay_ms.unwrap_or(2000),
                true,
            ),
        };
        
        // Nếu là giao dịch mua, cần slippage cao hơn
        let base_slippage = if is_buy { 2.0 } else { 1.0 };
        
        // Tính slippage dựa trên risk và cấu hình
        let slippage = if random_slippage {
            // Ngẫu nhiên hóa slippage trong khoảng [base, max]
            let rand_factor = rand::random::<f64>();
            base_slippage + rand_factor * (config.max_slippage - base_slippage)
        } else {
            base_slippage + risk_factor * (config.max_slippage - base_slippage)
        };
        
        // Tính gas price tối ưu
        let base_gas = self.gas_optimizer.calculate_optimal_gas_price(
            chain_id, 
            7, // Priority cao hơn trung bình
            if mev_analysis.mev_activity_detected { "mev_protection" } else { "normal" }
        ).await?;
        
        // Áp dụng multiplier
        let gas_price = base_gas * gas_multiplier;
        
        // Tính deadline (giây)
        let deadline_seconds = match actual_protection {
            MevProtectionLevel::None => 60,
            MevProtectionLevel::Basic => 30,
            MevProtectionLevel::Medium => 20,
            MevProtectionLevel::High => 10,
        };
        
        Ok(AntiMevTxParams {
            gas_price,
            slippage_tolerance: slippage,
            deadline_seconds,
            use_private_tx,
            delay_ms,
            max_nonce_reuse_attempt: config.max_nonce_reuse_attempt,
            mev_risk_score: mev_analysis.risk_score,
        })
    }
}

/// Tham số giao dịch chống MEV
#[derive(Debug, Clone)]
pub struct AntiMevTxParams {
    /// Gas price tối ưu (gwei)
    pub gas_price: f64,
    
    /// Slippage tolerance (%)
    pub slippage_tolerance: f64,
    
    /// Deadline (giây)
    pub deadline_seconds: u64,
    
    /// Có sử dụng private tx không
    pub use_private_tx: bool,
    
    /// Delay trước khi gửi giao dịch (ms)
    pub delay_ms: u64,
    
    /// Số lần thử lại nếu giao dịch bị thay thế
    pub max_nonce_reuse_attempt: u32,
    
    /// MEV risk score (0-100)
    pub mev_risk_score: u8,
}

impl Default for AntiMevTxParams {
    fn default() -> Self {
        Self {
            gas_price: 0.0,
            slippage_tolerance: 1.0,
            deadline_seconds: 60,
            use_private_tx: false,
            delay_ms: 0,
            max_nonce_reuse_attempt: 0,
            mev_risk_score: 0,
        }
    }
} 