/**
 * Gas và pool optimization cho smart trade
 *
 * Module này cung cấp các chức năng để tối ưu hóa gas price và lựa chọn pool
 * nhằm giảm cost và tránh pool bị congestion.
 */

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Result, Context, bail, anyhow};
use tracing::{debug, error, info, warn};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::tradelogic::smart_trade::types::*;

/// Thông tin về liquidity pool
#[derive(Debug, Clone)]
pub struct PoolInfo {
    /// Địa chỉ của pool
    pub pool_address: String,
    
    /// Dex ID (Uniswap, SushiSwap, ...)
    pub dex_id: String,
    
    /// Địa chỉ token base (WETH, WBNB, ...)
    pub base_token_address: String,
    
    /// Tên token base
    pub base_token_name: String,
    
    /// Giá token trong pool (tính bằng base token)
    pub token_price: f64,
    
    /// Tổng thanh khoản trong USD
    pub liquidity_usd: f64,
    
    /// Phần trăm phí (0.3% = 0.3)
    pub fee_percent: f64,
    
    /// Gas price được đề xuất để giao dịch với pool này
    pub suggested_gas_price: f64,
    
    /// Pool có được verify không (Có thể tin tưởng)
    pub is_verified: bool,
}

/// Tham số giao dịch đã được tối ưu
#[derive(Debug, Clone)]
pub struct OptimizedTradeParams {
    /// Địa chỉ pool được chọn
    pub pool_address: String,
    
    /// Gas price tối ưu (gwei)
    pub gas_price: f64,
    
    /// Slippage tolerance tối ưu (%)
    pub slippage_tolerance: f64,
    
    /// Deadline tối ưu (giây)
    pub deadline_seconds: u64,
    
    /// Giá ước tính sau khi tính slippage
    pub estimated_price: f64,
    
    /// Slippage ước tính (%)
    pub estimated_slippage: f64,
    
    /// Thanh khoản của pool (USD)
    pub liquidity_usd: f64,
    
    /// Phí của pool (%)
    pub fee_percent: f64,
    
    /// Ghi chú về quá trình tối ưu
    pub execution_notes: String,
}

/// Thông tin về mempool và gas
#[derive(Debug, Clone, Default)]
pub struct MempoolAnalysis {
    /// Gas price trung bình (gwei)
    pub average_gas_price: f64,
    
    /// Gas price cao nhất (gwei)
    pub max_gas_price: f64,
    
    /// Gas price thấp nhất (gwei)
    pub min_gas_price: f64,
    
    /// Số giao dịch đang chờ xử lý
    pub pending_tx_count: u64,
    
    /// Độ ưu tiên hiện tại (low, medium, high)
    pub current_priority: String,
    
    /// Số block cần để giao dịch được xử lý (ước tính)
    pub estimated_wait_blocks: u64,
    
    /// Thời gian cần để giao dịch được xử lý (ước tính, giây)
    pub estimated_wait_seconds: u64,
    
    /// Gas limit trung bình
    pub average_gas_limit: u64,
    
    /// Mức độ congestion (0-100%)
    pub congestion_level: f64,
    
    /// Loại base fee (EIP-1559 hay legacy)
    pub fee_type: String,
    
    /// Base fee (nếu là EIP-1559)
    pub base_fee: Option<f64>,
    
    /// Độ ưu tiên tip (nếu là EIP-1559)
    pub priority_fee: Option<f64>,
}

/// Gas Optimizer Service
pub struct GasOptimizer {
    /// Lịch sử gas price
    gas_history: RwLock<HashMap<u32, Vec<(u64, f64)>>>, // (timestamp, gas_price)
    
    /// Thông tin mempool cho mỗi chain
    mempool_stats: RwLock<HashMap<u32, MempoolAnalysis>>,
    
    /// Gas price limit tối đa cho mỗi chain
    max_gas_price: RwLock<HashMap<u32, f64>>,
    
    /// Gas price limit tối thiểu cho mỗi chain
    min_gas_price: RwLock<HashMap<u32, f64>>,
}

impl GasOptimizer {
    /// Tạo instance mới của GasOptimizer
    pub fn new() -> Self {
        Self {
            gas_history: RwLock::new(HashMap::new()),
            mempool_stats: RwLock::new(HashMap::new()),
            max_gas_price: RwLock::new(HashMap::new()),
            min_gas_price: RwLock::new(HashMap::new()),
        }
    }
    
    /// Cập nhật thông tin gas price cho chain
    pub async fn update_gas_price(&self, chain_id: u32, gas_price: f64) -> Result<()> {
        // Lấy timestamp hiện tại
        let timestamp = chrono::Utc::now().timestamp() as u64;
        
        // Cập nhật lịch sử
        let mut history = self.gas_history.write().await;
        
        // Thêm vào lịch sử của chain này
        let chain_history = history.entry(chain_id).or_insert_with(Vec::new);
        chain_history.push((timestamp, gas_price));
        
        // Giới hạn lượng lịch sử lưu trữ (giữ 1000 mẫu gần nhất)
        if chain_history.len() > 1000 {
            // Xóa dữ liệu cũ nhất
            chain_history.remove(0);
        }
        
        // Cập nhật gas limits
        let mut max_gas = self.max_gas_price.write().await;
        let mut min_gas = self.min_gas_price.write().await;
        
        // Cập nhật giá trị max nếu cần
        let current_max = max_gas.entry(chain_id).or_insert(gas_price);
        if gas_price > *current_max {
            *current_max = gas_price;
        }
        
        // Cập nhật giá trị min nếu cần
        let current_min = min_gas.entry(chain_id).or_insert(gas_price);
        if gas_price < *current_min {
            *current_min = gas_price;
        }
        
        Ok(())
    }
    
    /// Cập nhật thông tin mempool cho chain
    pub async fn update_mempool_stats(&self, chain_id: u32, stats: MempoolAnalysis) -> Result<()> {
        let mut mempool_stats = self.mempool_stats.write().await;
        mempool_stats.insert(chain_id, stats);
        Ok(())
    }
    
    /// Tính gas price tối ưu dựa trên lịch sử và độ ưu tiên
    ///
    /// # Parameters
    /// * `chain_id` - ID của blockchain
    /// * `priority` - Độ ưu tiên (1-10, 10 là cao nhất)
    /// * `transaction_type` - Loại giao dịch (normal, fast, urgent)
    ///
    /// # Returns
    /// * `f64` - Gas price tối ưu (gwei)
    pub async fn calculate_optimal_gas_price(&self, 
                                           chain_id: u32, 
                                           priority: u8, 
                                           transaction_type: &str) -> Result<f64> {
        // Kiểm tra trong stats mempool
        let mempool_stats = self.mempool_stats.read().await;
        let stats = match mempool_stats.get(&chain_id) {
            Some(stats) => stats.clone(),
            None => {
                debug!("No mempool stats for chain {}, using default approach", chain_id);
                MempoolAnalysis::default()
            }
        };
        drop(mempool_stats); // Giải phóng lock sớm
        
        // Kiểm tra trong lịch sử gas
        let history = self.gas_history.read().await;
        let recent_history = match history.get(&chain_id) {
            Some(h) => h.clone(),
            None => {
                debug!("No gas history for chain {}, using current gas price only", chain_id);
                Vec::new()
            }
        };
        drop(history); // Giải phóng lock sớm
        
        // Tính gas price dựa vào degree of congestion
        let base_gas_price = if stats.average_gas_price > 0.0 {
            stats.average_gas_price
        } else if !recent_history.is_empty() {
            // Nếu không có thông tin mempool, dùng gas price gần nhất
            recent_history.last().map(|(_ts, price)| *price).unwrap_or(30.0) // Mặc định 30 gwei
        } else {
            // Không có dữ liệu, dùng giá trị mặc định an toàn
            match chain_id {
                1 => 40.0, // Ethereum mainnet
                56 => 5.0, // BSC
                137 => 50.0, // Polygon
                43114 => 35.0, // Avalanche
                _ => 30.0, // Chain khác
            }
        };
        
        // Điều chỉnh gas price theo prioritization
        let priority_multiplier = match priority {
            0..=3 => 0.9, // Thấp - chấp nhận chờ lâu hơn để tiết kiệm gas
            4..=6 => 1.0, // Trung bình - gas price thông thường
            7..=8 => 1.2, // Cao - ưu tiên giao dịch nhanh hơn
            9..=10 => 1.5, // Rất cao - giao dịch gấp
            _ => 1.0, // Mặc định
        };
        
        // Điều chỉnh theo loại giao dịch
        let type_multiplier = match transaction_type {
            "normal" => 1.0,
            "fast" => 1.25,
            "urgent" => 1.5,
            "mev_protection" => 2.0, // Bảo vệ chống MEV cần gas cao hơn
            _ => 1.0,
        };
        
        // Điều chỉnh theo mức độ congestion
        let congestion_multiplier = if stats.congestion_level > 0.0 {
            1.0 + (stats.congestion_level / 100.0)
        } else {
            1.0
        };
        
        // Tính giá trị cuối cùng
        let optimal_gas = base_gas_price * priority_multiplier * type_multiplier * congestion_multiplier;
        
        // Đảm bảo gas price nằm trong khoảng hợp lý
        let max_gas = self.max_gas_price.read().await;
        let min_gas = self.min_gas_price.read().await;
        
        // Lấy giá trị min/max cho chain này hoặc dùng mặc định
        let max_limit = max_gas.get(&chain_id).copied().unwrap_or_else(|| {
            match chain_id {
                1 => 500.0, // Ethereum mainnet
                56 => 20.0, // BSC
                137 => 300.0, // Polygon
                _ => 500.0, // Chain khác
            }
        });
        
        let min_limit = min_gas.get(&chain_id).copied().unwrap_or_else(|| {
            match chain_id {
                1 => 10.0, // Ethereum mainnet
                56 => 3.0, // BSC
                137 => 30.0, // Polygon
                _ => 5.0, // Chain khác
            }
        });
        
        // Đảm bảo gas nằm trong khoảng cho phép
        let gas_price = optimal_gas.max(min_limit).min(max_limit);
        
        debug!(
            "Calculated optimal gas price for chain {}: {:.2} gwei (priority: {}, type: {}, congestion: {:.1}%)",
            chain_id, gas_price, priority, transaction_type, stats.congestion_level
        );
        
        Ok(gas_price)
    }
    
    /// Lựa chọn pool tối ưu để giao dịch dựa trên gas, congestion và liquidity
    ///
    /// # Parameters
    /// * `adapter` - EVMAdapter cho blockchain
    /// * `token_address` - Địa chỉ token
    /// * `is_buy` - True nếu là giao dịch mua, false nếu là bán
    ///
    /// # Returns
    /// * `PoolInfo` - Thông tin pool được chọn và các tham số tối ưu
    pub async fn select_optimal_pool(
        &self,
        adapter: Arc<EvmAdapter>, 
        token_address: &str, 
        is_buy: bool
    ) -> Result<PoolInfo> {
        // Lấy tất cả các pool có chứa token này
        let pools = adapter.get_token_pools(token_address).await
            .context("Failed to get token pools")?;
        
        if pools.is_empty() {
            bail!("No liquidity pools found for token {}", token_address);
        }
        
        let chain_id = adapter.get_chain_id();
        debug!("Found {} pools for token {} on chain {}", pools.len(), token_address, chain_id);
        
        // Nếu chỉ có 1 pool, trả về luôn
        if pools.len() == 1 {
            return Ok(pools[0].clone());
        }
        
        // 1. Lấy gas price hiện tại và mức độ congestion của mạng
        let current_gas_price = adapter.get_current_gas_price().await
            .context("Failed to get current gas price")?;
        
        // Tự động sinh mẫu gas price vào lịch sử
        self.update_gas_price(chain_id, current_gas_price).await?;
        
        // Khởi tạo điểm đánh giá cho từng pool
        let mut pool_scores = Vec::new();
        
        // Để đơn giản hóa ví dụ, chúng ta sẽ phân tích tuần tự các pool
        // Trong triển khai thực tế, nên sử dụng phân tích đồng thời như các ví dụ trước
        for pool in &pools {
            // Điểm ban đầu
            let mut score = 0.0;
            
            // Cộng điểm dựa trên thanh khoản (pool với thanh khoản cao được ưu tiên)
            score += pool.liquidity_usd / 1000.0; // Điểm theo thanh khoản
            
            // Trừ điểm nếu pool có fee cao
            score -= pool.fee_percent * 10.0; // Phí 0.3% -> trừ 3 điểm
            
            // Ước tính gas usage
            let gas_usage = adapter.estimate_gas_usage(&pool.pool_address).await.unwrap_or(300000);
            
            // Kiểm tra congestion
            let congestion = adapter.get_pool_congestion(&pool.pool_address).await.unwrap_or(0.0);
            
            // Trừ điểm nếu gas usage cao
            score -= (gas_usage as f64) / 10000.0;
            
            // Trừ điểm dựa vào mức độ congestion (0-100%)
            score -= (congestion * 0.5); // Congestion 100% -> trừ 50 điểm
            
            // Cộng thêm điểm nếu là pool chính (verified)
            if pool.is_verified {
                score += 20.0;
            }
            
            // Thêm vào danh sách đánh giá
            pool_scores.push((pool.clone(), score));
        }
        
        // Sắp xếp các pool theo điểm, từ cao xuống thấp
        pool_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        
        // Chọn pool tốt nhất
        let best_pool = pool_scores[0].0.clone();
        
        // Tính toán gas price tối ưu cho pool này
        let congestion = adapter.get_pool_congestion(&best_pool.pool_address).await.unwrap_or(0.0);
        
        // Điều chỉnh gas price dựa vào mức độ congestion
        let multiplier = 1.0 + (congestion * 0.5); // 0% -> 1x, 100% -> 1.5x
        let optimal_gas_price = current_gas_price * multiplier;
        
        // Tạo thông tin pool tối ưu với gas price đã được điều chỉnh
        let mut optimal_pool = best_pool;
        optimal_pool.suggested_gas_price = optimal_gas_price;
        
        info!(
            "Selected optimal pool for {} token {}: {} (liquidity: ${:.2}, fee: {:.2}%, gas price: {:.2} gwei)",
            if is_buy { "buying" } else { "selling" },
            token_address,
            optimal_pool.pool_address,
            optimal_pool.liquidity_usd,
            optimal_pool.fee_percent,
            optimal_pool.suggested_gas_price
        );
        
        Ok(optimal_pool)
    }
    
    /// Tạo tham số giao dịch tối ưu dựa trên phân tích pool
    ///
    /// # Parameters
    /// * `adapter` - EVMAdapter cho blockchain
    /// * `token_address` - Địa chỉ token
    /// * `amount` - Số lượng token/native coin
    /// * `is_buy` - True nếu là giao dịch mua, false nếu là bán
    ///
    /// # Returns
    /// * `OptimizedTradeParams` - Các tham số giao dịch đã được tối ưu
    pub async fn optimize_trade_parameters(
        &self,
        adapter: Arc<EvmAdapter>,
        token_address: &str,
        amount: f64,
        is_buy: bool
    ) -> Result<OptimizedTradeParams> {
        // Chọn pool tối ưu
        let optimal_pool = self.select_optimal_pool(adapter.clone(), token_address, is_buy).await?;
        
        // Ước tính slippage dựa vào depth của pool và kích thước giao dịch
        let estimated_slippage = adapter.estimate_slippage_for_pool(
            &optimal_pool.pool_address, 
            token_address, 
            amount, 
            is_buy
        ).await.unwrap_or(0.5); // Mặc định 0.5% nếu không ước tính được
        
        // Tính toán slippage tolerance tối ưu
        let slippage_tolerance = if estimated_slippage > 5.0 {
            // Slippage lớn, cần tolerance cao hơn
            estimated_slippage * 1.5
        } else if estimated_slippage > 1.0 {
            // Slippage trung bình
            estimated_slippage * 1.3
        } else {
            // Slippage thấp
            estimated_slippage * 1.2
        }.min(10.0); // Tối đa 10% slippage tolerance
        
        // Lấy thông tin mempool hiện tại
        let mempool_stats = self.mempool_stats.read().await;
        let chain_id = adapter.get_chain_id();
        let stats = mempool_stats.get(&chain_id).cloned().unwrap_or_default();
        drop(mempool_stats);
        
        // Tính toán deadline hợp lý
        let deadline_seconds = if stats.congestion_level > 70.0 {
            // Mạng congested nhiều, cần deadline dài
            300 // 5 phút
        } else if stats.congestion_level > 40.0 {
            // Mạng congested vừa
            180 // 3 phút
        } else {
            // Mạng ít congested
            60 // 1 phút
        };
        
        // Tính giá token ước tính
        let estimated_price = if is_buy {
            // Mua: giá sau slippage sẽ cao hơn
            optimal_pool.token_price * (1.0 + estimated_slippage / 100.0)
        } else {
            // Bán: giá sau slippage sẽ thấp hơn
            optimal_pool.token_price * (1.0 - estimated_slippage / 100.0)
        };
        
        // Tạo tham số giao dịch tối ưu
        let optimized_params = OptimizedTradeParams {
            pool_address: optimal_pool.pool_address,
            gas_price: optimal_pool.suggested_gas_price,
            slippage_tolerance,
            deadline_seconds,
            estimated_price,
            estimated_slippage,
            liquidity_usd: optimal_pool.liquidity_usd,
            fee_percent: optimal_pool.fee_percent,
            execution_notes: format!(
                "Selected pool with ${:.2}M liquidity, {:.2}% fee. Expected slippage: {:.2}%",
                optimal_pool.liquidity_usd / 1_000_000.0,
                optimal_pool.fee_percent,
                estimated_slippage
            ),
        };
        
        debug!(
            "Optimized trade parameters for {} {} token {} on chain {}: gas: {:.2} gwei, slippage: {:.2}%, deadline: {}s",
            if is_buy { "buying" } else { "selling" },
            amount,
            token_address,
            chain_id,
            optimized_params.gas_price,
            optimized_params.slippage_tolerance,
            optimized_params.deadline_seconds
        );
        
        Ok(optimized_params)
    }
} 