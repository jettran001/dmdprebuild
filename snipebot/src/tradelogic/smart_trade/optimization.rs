/// Optimization implementation for SmartTradeExecutor
///
/// This module contains functions for optimizing trades, selecting optimal pools,
/// and implementing anti-MEV protection.

// External imports
use std::collections::{HashMap, BinaryHeap};
use std::cmp::Ordering;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use anyhow::{Result, Context, anyhow};
use serde::{Serialize, Deserialize};

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::{TokenPair, ChainType};

// Module imports
use super::types::{TradeTracker, TradeStatus};
use super::constants::*;
use super::executor::SmartTradeExecutor;
use crate::tradelogic::common::gas::{calculate_optimal_gas_price, GasStrategy};

// Định nghĩa cấu trúc để so sánh các pool
#[derive(Debug, Clone, Eq)]
struct PoolRanking {
    pair_address: String,
    liquidity: f64,
    volume_24h: f64,
    price: f64,
    execution_success_rate: f64,
}

// Implement PartialEq và Ord để so sánh các pool trong BinaryHeap
impl PartialEq for PoolRanking {
    fn eq(&self, other: &Self) -> bool {
        self.calculate_score() == other.calculate_score()
    }
}

impl Ord for PoolRanking {
    fn cmp(&self, other: &Self) -> Ordering {
        self.calculate_score().partial_cmp(&other.calculate_score())
            .unwrap_or(Ordering::Equal)
            .reverse() // Để BinaryHeap trả về phần tử lớn nhất trước
    }
}

impl PartialOrd for PoolRanking {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PoolRanking {
    // Tính điểm cho mỗi pool để xếp hạng
    fn calculate_score(&self) -> f64 {
        // Trọng số cho từng yếu tố
        const LIQUIDITY_WEIGHT: f64 = 0.5;
        const VOLUME_WEIGHT: f64 = 0.3;
        const PRICE_WEIGHT: f64 = 0.1;
        const SUCCESS_RATE_WEIGHT: f64 = 0.1;
        
        // Tính điểm dựa trên trọng số
        (self.liquidity * LIQUIDITY_WEIGHT) +
        (self.volume_24h * VOLUME_WEIGHT) +
        (self.price * PRICE_WEIGHT) +
        (self.execution_success_rate * SUCCESS_RATE_WEIGHT)
    }
}

/// Cấu hình cho tối ưu hóa gas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasOptimizationConfig {
    /// Chiến lược gas mặc định
    pub default_strategy: GasStrategy,
    
    /// Hệ số tăng cho giao dịch mua
    pub buy_multiplier: f64,
    
    /// Hệ số tăng cho giao dịch bán
    pub sell_multiplier: f64,
    
    /// Gas limit cơ bản cho giao dịch swap
    pub base_gas_limit_swap: u64,
    
    /// Gas limit cơ bản cho giao dịch approve
    pub base_gas_limit_approve: u64,
    
    /// Tỷ lệ phần trăm tối đa so với thị trường
    pub max_price_percentage: f64,
    
    /// Tỷ lệ phần trăm tối thiểu so với thị trường
    pub min_price_percentage: f64,
    
    /// Có sử dụng EIP-1559 không
    pub use_eip1559: bool,
    
    /// Có chấp nhận phí ưu tiên cao không
    pub accept_high_priority_fee: bool,
}

impl Default for GasOptimizationConfig {
    fn default() -> Self {
        Self {
            default_strategy: GasStrategy::Balanced,
            buy_multiplier: 1.1,
            sell_multiplier: 1.2,
            base_gas_limit_swap: 250000,
            base_gas_limit_approve: 60000,
            max_price_percentage: 200.0,
            min_price_percentage: 80.0,
            use_eip1559: true,
            accept_high_priority_fee: true,
        }
    }
}

/// Thông tin về pool thanh khoản
#[derive(Debug, Clone)]
pub struct PoolInfo {
    /// Địa chỉ của pool
    pub address: String,
    
    /// Dex sở hữu pool (Uniswap, PancakeSwap, ...)
    pub dex_name: String,
    
    /// Thanh khoản tổng trong pool (USD)
    pub liquidity_usd: f64,
    
    /// Phí giao dịch (%, ví dụ: 0.3%)
    pub fee_percent: f64,
    
    /// Khối lượng giao dịch 24h (USD)
    pub volume_24h: f64,
    
    /// Price impact ước tính cho giao dịch trung bình
    pub estimated_price_impact: f64,
    
    /// Độ tập trung thanh khoản (0-100%)
    pub liquidity_concentration: f64,
    
    /// Điểm an toàn tổng hợp (0-100)
    pub safety_score: u8,
}

/// Tối ưu hóa gas và lựa chọn pool cho giao dịch
pub struct TradeOptimizer {
    /// Cấu hình tối ưu hóa gas
    pub config: GasOptimizationConfig,
    
    /// Cache thông tin pool
    pool_cache: HashMap<String, Vec<PoolInfo>>,
    
    /// Cache gas price gần đây
    gas_price_cache: HashMap<u32, (f64, u64)>, // (price, timestamp)
}

impl TradeOptimizer {
    /// Tạo optimizer mới với cấu hình mặc định
    pub fn new() -> Self {
        Self {
            config: GasOptimizationConfig::default(),
            pool_cache: HashMap::new(),
            gas_price_cache: HashMap::new(),
        }
    }
    
    /// Tạo optimizer với cấu hình tùy chỉnh
    pub fn with_config(config: GasOptimizationConfig) -> Self {
        Self {
            config,
            pool_cache: HashMap::new(),
            gas_price_cache: HashMap::new(),
        }
    }
    
    /// Tính optimal gas price cho giao dịch
    ///
    /// # Tham số
    /// * `chain_id` - ID của blockchain
    /// * `is_buy` - True nếu là giao dịch mua, false nếu là bán
    /// * `priority` - Mức ưu tiên (0-5, càng cao càng ưu tiên)
    /// * `adapter` - EVM adapter để tương tác với blockchain
    ///
    /// # Returns
    /// * `Result<f64>` - Giá gas tối ưu (đơn vị wei)
    pub async fn calculate_optimal_gas(&self, chain_id: u32, is_buy: bool, priority: u8, adapter: &Arc<EvmAdapter>) -> Result<f64> {
        // Kiểm tra cache trước
        let now = chrono::Utc::now().timestamp() as u64;
        if let Some((cached_price, timestamp)) = self.gas_price_cache.get(&chain_id) {
            // Sử dụng cache nếu chưa quá 1 phút
            if now - timestamp < 60 {
                let multiplier = if is_buy {
                    self.config.buy_multiplier
                } else {
                    self.config.sell_multiplier
                };
                
                let priority_factor = 1.0 + (priority as f64 * 0.1);
                return Ok(cached_price * multiplier * priority_factor);
            }
        }
        
        // Chuyển đổi priority sang GasStrategy
        let strategy = match priority {
            0 => GasStrategy::Economical,
            1 => GasStrategy::Balanced,
            2 | 3 => GasStrategy::Fast,
            4 | 5 => GasStrategy::Urgent,
            _ => GasStrategy::Balanced,
        };
        
        // Lấy gas price từ hàm chung
        let gas_price = calculate_optimal_gas_price(chain_id, strategy).await
            .context(format!("Không thể tính gas price tối ưu cho chain {}", chain_id))?;
        
        // Điều chỉnh theo loại giao dịch
        let multiplier = if is_buy {
            self.config.buy_multiplier
        } else {
            self.config.sell_multiplier
        };
        
        // Điều chỉnh theo mức ưu tiên
        let priority_factor = 1.0 + (priority as f64 * 0.1);
        let final_price = gas_price * multiplier * priority_factor;
        
        // Cập nhật cache
        self.gas_price_cache.insert(chain_id, (gas_price, now));
        
        Ok(final_price)
    }
    
    /// Ước tính gas limit cho giao dịch
    ///
    /// # Tham số
    /// * `chain_id` - ID của blockchain
    /// * `is_approve` - True nếu là giao dịch approve, false nếu là swap
    /// * `complex_path` - True nếu là đường dẫn phức tạp (multi-hop)
    ///
    /// # Returns
    /// * `u64` - Ước tính gas limit
    pub fn estimate_gas_limit(&self, chain_id: u32, is_approve: bool, complex_path: bool) -> u64 {
        if is_approve {
            self.config.base_gas_limit_approve
        } else {
            let mut limit = self.config.base_gas_limit_swap;
            
            // Tăng 30% cho đường dẫn phức tạp
            if complex_path {
                limit = (limit as f64 * 1.3) as u64;
            }
            
            // Điều chỉnh theo chain
            match chain_id {
                1 => limit, // Ethereum mainnet, giữ nguyên
                56 => (limit as f64 * 0.8) as u64, // BSC, giảm 20%
                137 => (limit as f64 * 1.2) as u64, // Polygon, tăng 20%
                43114 => (limit as f64 * 1.1) as u64, // Avalanche, tăng 10%
                _ => limit, // Chain khác, giữ nguyên
            }
        }
    }
    
    /// Lấy danh sách pool tối ưu cho cặp token
    ///
    /// # Tham số
    /// * `chain_id` - ID của blockchain
    /// * `token_pair` - Cặp token cần giao dịch
    /// * `adapter` - EVM adapter để tương tác với blockchain
    ///
    /// # Returns
    /// * `Result<Vec<PoolInfo>>` - Danh sách pool được sắp xếp theo mức độ tối ưu
    pub async fn get_optimal_pools(&self, chain_id: u32, token_pair: &TokenPair, adapter: &Arc<EvmAdapter>) -> Result<Vec<PoolInfo>> {
        let cache_key = format!("{}_{}_{}",
            chain_id,
            token_pair.base_token.address,
            token_pair.quote_token.address
        );
        
        // Kiểm tra cache trước
        if let Some(pools) = self.pool_cache.get(&cache_key) {
            return Ok(pools.clone());
        }
        
        // Lấy danh sách pool từ adapter
        let pools = adapter.get_token_pools(
            &token_pair.base_token.address,
            &token_pair.quote_token.address
        ).await.context("Không thể lấy thông tin pool")?;
        
        // Chuyển đổi thành PoolInfo và tính điểm an toàn
        let mut pool_info = Vec::with_capacity(pools.len());
        for pool in pools {
            let safety_score = self.calculate_pool_safety_score(
                &pool,
                token_pair,
                adapter
            ).await;
            
            pool_info.push(PoolInfo {
                address: pool.address,
                dex_name: pool.dex_name,
                liquidity_usd: pool.liquidity_usd,
                fee_percent: pool.fee_percent,
                volume_24h: pool.volume_24h,
                estimated_price_impact: pool.estimated_price_impact,
                liquidity_concentration: pool.liquidity_concentration,
                safety_score,
            });
        }
        
        // Sắp xếp theo điểm tối ưu
        pool_info.sort_by(|a, b| {
            let a_score = self.calculate_pool_optimal_score(a);
            let b_score = self.calculate_pool_optimal_score(b);
            b_score.partial_cmp(&a_score).unwrap_or(std::cmp::Ordering::Equal)
        });
        
        // Cập nhật cache
        self.pool_cache.insert(cache_key, pool_info.clone());
        
        Ok(pool_info)
    }
    
    /// Chọn pool tối ưu cho giao dịch
    ///
    /// # Tham số
    /// * `chain_id` - ID của blockchain
    /// * `token_pair` - Cặp token cần giao dịch
    /// * `amount_usd` - Giá trị giao dịch (USD)
    /// * `adapter` - EVM adapter để tương tác với blockchain
    ///
    /// # Returns
    /// * `Result<PoolInfo>` - Pool tối ưu
    pub async fn select_optimal_pool(&self, chain_id: u32, token_pair: &TokenPair, amount_usd: f64, adapter: &Arc<EvmAdapter>) -> Result<PoolInfo> {
        let pools = self.get_optimal_pools(chain_id, token_pair, adapter).await?;
        
        if pools.is_empty() {
            return Err(anyhow!("Không tìm thấy pool nào cho cặp token này"));
        }
        
        // Tìm pool có điểm số cao nhất dựa trên amount
        let mut best_pool = &pools[0];
        let mut best_score = 0.0;
        
        for pool in &pools {
            let score = self.calculate_pool_score_for_amount(pool, amount_usd);
            if score > best_score {
                best_score = score;
                best_pool = pool;
            }
        }
        
        debug!("Chọn pool {} ({}): liquidity=${:.2}, fee={:.2}%, safety={}/100", 
              best_pool.address, best_pool.dex_name, 
              best_pool.liquidity_usd, best_pool.fee_percent, best_pool.safety_score);
        
        Ok(best_pool.clone())
    }
    
    /// Tính toán điểm số an toàn cho pool
    async fn calculate_pool_safety_score(&self, pool: &crate::chain_adapters::evm_adapter::PoolInfo, token_pair: &TokenPair, adapter: &Arc<EvmAdapter>) -> u8 {
        let mut score = 100;
        
        // 1. Kiểm tra thanh khoản
        if pool.liquidity_usd < 10000.0 {
            // Thanh khoản dưới $10,000
            score -= 30;
        } else if pool.liquidity_usd < 50000.0 {
            // Thanh khoản dưới $50,000
            score -= 15;
        } else if pool.liquidity_usd < 100000.0 {
            // Thanh khoản dưới $100,000
            score -= 5;
        }
        
        // 2. Kiểm tra volume 24h
        if pool.volume_24h < 1000.0 {
            // Volume dưới $1,000
            score -= 20;
        } else if pool.volume_24h < 10000.0 {
            // Volume dưới $10,000
            score -= 10;
        }
        
        // 3. Kiểm tra fee
        if pool.fee_percent > 1.0 {
            // Fee trên 1%
            score -= 15;
        } else if pool.fee_percent > 0.5 {
            // Fee trên 0.5%
            score -= 5;
        }
        
        // 4. Kiểm tra price impact
        if pool.estimated_price_impact > 5.0 {
            // Price impact trên 5%
            score -= 25;
        } else if pool.estimated_price_impact > 1.0 {
            // Price impact trên 1%
            score -= 10;
        }
        
        // 5. Kiểm tra độ tập trung thanh khoản
        if pool.liquidity_concentration > 90.0 {
            // Tập trung > 90%
            score -= 15;
        } else if pool.liquidity_concentration > 75.0 {
            // Tập trung > 75%
            score -= 5;
        }
        
        // Đảm bảo score nằm trong khoảng 0-100
        score = score.max(0).min(100);
        
        score as u8
    }
    
    /// Tính điểm tối ưu cho pool (càng cao càng tốt)
    fn calculate_pool_optimal_score(&self, pool: &PoolInfo) -> f64 {
        // Mô hình tính điểm:
        // - Liquidity cao -> Điểm cao
        // - Fee thấp -> Điểm cao
        // - Volume cao -> Điểm cao
        // - Price impact thấp -> Điểm cao
        // - Safety score cao -> Điểm cao
        
        // Chuẩn hóa các giá trị
        let liquidity_score = (pool.liquidity_usd / 10000.0).min(10.0); // Max 10 điểm cho $100k+
        let fee_score = 5.0 * (1.0 - pool.fee_percent / 1.0).max(0.0); // Max 5 điểm cho fee 0%
        let volume_score = (pool.volume_24h / 10000.0).min(5.0); // Max 5 điểm cho $50k+
        let impact_score = 5.0 * (1.0 - pool.estimated_price_impact / 5.0).max(0.0); // Max 5 điểm cho 0% impact
        let safety_score = pool.safety_score as f64 / 20.0; // Max 5 điểm cho safety 100/100
        
        // Tổng điểm (tối đa 30 điểm)
        liquidity_score + fee_score + volume_score + impact_score + safety_score
    }
    
    /// Tính điểm tối ưu cho pool với amount cụ thể
    fn calculate_pool_score_for_amount(&self, pool: &PoolInfo, amount_usd: f64) -> f64 {
        let mut score = self.calculate_pool_optimal_score(pool);
        
        // Điều chỉnh theo amount
        let amount_ratio = amount_usd / pool.liquidity_usd;
        
        // Phạt nặng nếu amount > 1% liquidity
        if amount_ratio > 0.05 {
            score *= 0.3; // Giảm 70% điểm
        } else if amount_ratio > 0.01 {
            score *= 0.7; // Giảm 30% điểm
        }
        
        // Ưu tiên pool có safety score cao nếu amount lớn
        if amount_usd > 10000.0 && pool.safety_score > 70 {
            score *= 1.2; // Tăng 20% điểm
        }
        
        score
    }
    
    /// Tối ưu hóa đường dẫn swap (path)
    ///
    /// # Tham số
    /// * `chain_id` - ID của blockchain
    /// * `from_token` - Token đầu vào
    /// * `to_token` - Token đầu ra
    /// * `amount` - Giá trị giao dịch
    /// * `adapter` - EVM adapter để tương tác với blockchain
    ///
    /// # Returns
    /// * `Result<Vec<String>>` - Đường dẫn tối ưu (danh sách địa chỉ token)
    pub async fn optimize_swap_path(&self, chain_id: u32, from_token: &str, to_token: &str, amount: f64, adapter: &Arc<EvmAdapter>) -> Result<Vec<String>> {
        // Kiểm tra đường dẫn trực tiếp
        let direct_path = vec![from_token.to_string(), to_token.to_string()];
        
        // Lấy token gốc của chain để thử đường dẫn qua token gốc
        let native_token = Self::get_chain_native_token(chain_id);
        
        let mut potential_paths = vec![
            direct_path.clone(),
        ];
        
        // Thêm đường dẫn qua native token nếu không phải là token gốc
        if from_token != native_token && to_token != native_token {
            potential_paths.push(vec![
                from_token.to_string(),
                native_token.to_string(),
                to_token.to_string(),
            ]);
        }
        
        // Thêm đường dẫn qua USDC nếu cả hai token không phải USDC
        let usdc = Self::get_chain_usdc(chain_id);
        if from_token != usdc && to_token != usdc {
            potential_paths.push(vec![
                from_token.to_string(),
                usdc.to_string(),
                to_token.to_string(),
            ]);
        }
        
        // Thêm đường dẫn qua WETH nếu cả hai token không phải WETH
        let weth = Self::get_chain_wrapped_native(chain_id);
        if from_token != weth && to_token != weth && native_token != weth {
            potential_paths.push(vec![
                from_token.to_string(),
                weth.to_string(),
                to_token.to_string(),
            ]);
        }
        
        // Mô phỏng các đường dẫn và chọn đường dẫn tốt nhất
        let mut best_path = direct_path;
        let mut best_amount_out = 0.0;
        
        for path in potential_paths {
            // Mô phỏng swap
            let amount_out = match adapter.simulate_swap_with_path(&path, amount).await {
                Ok(out) => out,
                Err(e) => {
                    debug!("Không thể mô phỏng path {:?}: {}", path, e);
                    continue;
                }
            };
            
            // Cập nhật best path nếu tốt hơn
            if amount_out > best_amount_out {
                best_amount_out = amount_out;
                best_path = path.clone();
            }
        }
        
        debug!("Đường dẫn tối ưu: {:?}, dự kiến nhận: {:.6}", best_path, best_amount_out);
        
        Ok(best_path)
    }
    
    /// Lấy token gốc của chain
    fn get_chain_native_token(chain_id: u32) -> String {
        match chain_id {
            1 => "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string(), // WETH (Ethereum)
            56 => "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c".to_string(), // WBNB (BSC)
            137 => "0x0d500B1d8E8eF31E21C99d1Db9A6444d3ADf1270".to_string(), // WMATIC (Polygon)
            43114 => "0xB31f66AA3C1e785363F0875A1B74E27b85FD66c7".to_string(), // WAVAX (Avalanche)
            _ => "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string(), // Mặc định WETH
        }
    }
    
    /// Lấy token USDC của chain
    fn get_chain_usdc(chain_id: u32) -> String {
        match chain_id {
            1 => "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(), // USDC (Ethereum)
            56 => "0x8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d".to_string(), // USDC (BSC)
            137 => "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174".to_string(), // USDC (Polygon)
            43114 => "0xB97EF9Ef8734C71904D8002F8b6Bc66Dd9c48a6E".to_string(), // USDC (Avalanche)
            _ => "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(), // Mặc định USDC ETH
        }
    }
    
    /// Lấy token wrapped native của chain
    fn get_chain_wrapped_native(chain_id: u32) -> String {
        Self::get_chain_native_token(chain_id) // Hiện tại giống native token
    }
}

/// Kết quả gom giao dịch batch
#[derive(Debug, Clone)]
pub struct BatchTradeResult {
    /// ID của batch
    pub batch_id: String,
    
    /// Danh sách transaction hash
    pub tx_hashes: Vec<String>,
    
    /// Số lượng giao dịch thành công
    pub successful_trades: usize,
    
    /// Số lượng giao dịch thất bại
    pub failed_trades: usize,
    
    /// Tổng gas đã sử dụng
    pub total_gas_used: f64,
    
    /// Ước tính gas đã tiết kiệm
    pub estimated_gas_saved: f64,
    
    /// Chi tiết từng giao dịch
    pub trade_details: Vec<BatchTradeDetail>,
}

/// Chi tiết giao dịch trong batch
#[derive(Debug, Clone)]
pub struct BatchTradeDetail {
    /// ID giao dịch
    pub trade_id: String,
    
    /// Token address
    pub token_address: String,
    
    /// Amount
    pub amount: f64,
    
    /// Là giao dịch mua hay không
    pub is_buy: bool,
    
    /// Trạng thái giao dịch
    pub status: String,
    
    /// Lỗi nếu có
    pub error: Option<String>,
}

/// Tối ưu hóa thứ tự giao dịch trong batch
///
/// # Tham số
/// * `trades` - Danh sách giao dịch cần thực hiện: (token_address, amount, is_buy)
///
/// # Returns
/// * `Vec<(String, f64, bool)>` - Danh sách giao dịch đã được tối ưu hóa thứ tự
pub fn optimize_trade_order(trades: Vec<(String, f64, bool)>) -> Vec<(String, f64, bool)> {
    // Nguyên tắc tối ưu:
    // 1. Các approve đầu tiên
    // 2. Các giao dịch mua trước, sau đó mới đến giao dịch bán
    // 3. Giao dịch với cùng token gom lại với nhau để tận dụng cache
    
    let mut optimized: Vec<(String, f64, bool)> = Vec::with_capacity(trades.len());
    
    // Nhóm các giao dịch theo token và loại giao dịch
    let mut buys: HashMap<String, f64> = HashMap::new();
    let mut sells: HashMap<String, f64> = HashMap::new();
    
    for (token, amount, is_buy) in trades {
        if is_buy {
            *buys.entry(token).or_default() += amount;
        } else {
            *sells.entry(token).or_default() += amount;
        }
    }
    
    // Đưa các giao dịch mua vào trước
    for (token, amount) in buys {
        optimized.push((token, amount, true));
    }
    
    // Sau đó đến các giao dịch bán
    for (token, amount) in sells {
        optimized.push((token, amount, false));
    }
    
    optimized
}

impl SmartTradeExecutor {
    /// Chọn pool tối ưu để thực hiện giao dịch
    ///
    /// Phân tích và xếp hạng các pool dựa trên thanh khoản, volume, 
    /// và tỷ lệ thành công của giao dịch trước đó
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - `Option<String>`: Địa chỉ của pool tối ưu, hoặc None nếu không tìm thấy
    pub async fn select_optimal_pool(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> Option<String> {
        // Lấy tất cả các pool có token này
        if let Ok(token_pairs) = adapter.get_token_pairs(token_address).await {
            if token_pairs.is_empty() {
                warn!("Không tìm thấy pool nào cho token {}", token_address);
                return None;
            }
            
            // Heap để xếp hạng các pool
            let mut pool_rankings = BinaryHeap::new();
            
            for pair in token_pairs {
                // Lấy thông tin về pool
                let liquidity = adapter.get_pair_liquidity(&pair.pair_address).await.unwrap_or(0.0);
                let volume_24h = adapter.get_pair_volume(&pair.pair_address, 24).await.unwrap_or(0.0);
                let price = adapter.get_token_price_from_pair(token_address, &pair.pair_address).await.unwrap_or(0.0);
                
                // Tính tỷ lệ thành công dựa trên lịch sử giao dịch
                let execution_success_rate = self.calculate_success_rate(chain_id, &pair.pair_address).await;
                
                // Thêm vào heap để xếp hạng
                pool_rankings.push(PoolRanking {
                    pair_address: pair.pair_address.clone(),
                    liquidity,
                    volume_24h,
                    price,
                    execution_success_rate,
                });
                
                debug!(
                    "Pool {}: Liquidity=${:.2}, Volume=${:.2}, Price=${:.10}, Success Rate={}%",
                    pair.pair_address, liquidity, volume_24h, price, execution_success_rate * 100.0
                );
            }
            
            // Lấy top 3 pool để log
            let mut top_pools = Vec::new();
            for _ in 0..3.min(pool_rankings.len()) {
                if let Some(pool) = pool_rankings.pop() {
                    top_pools.push(pool);
                }
            }
            
            // Log top pools
            for (index, pool) in top_pools.iter().enumerate() {
                info!(
                    "Top {} pool: {} (Score: {:.2}, Liquidity: ${:.2}, Volume: ${:.2})",
                    index + 1, 
                    pool.pair_address, 
                    pool.calculate_score(), 
                    pool.liquidity, 
                    pool.volume_24h
                );
            }
            
            // Trả về pool tốt nhất
            if !top_pools.is_empty() {
                return Some(top_pools[0].pair_address.clone());
            }
        }
        
        None
    }
    
    /// Tính tỷ lệ thành công dựa trên lịch sử giao dịch
    async fn calculate_success_rate(&self, chain_id: u32, pair_address: &str) -> f64 {
        if let Some(db) = &self.trade_log_db {
            // Lấy lịch sử giao dịch có liên quan đến pair này
            if let Ok(trade_logs) = db.get_trades_by_pair(pair_address).await {
                if trade_logs.is_empty() {
                    return 0.9; // Default success rate nếu không có dữ liệu
                }
                
                let mut success_count = 0;
                let mut total_count = 0;
                
                for log in trade_logs {
                    if log.chain_id == chain_id {
                        total_count += 1;
                        if log.status == TradeStatus::Completed {
                            success_count += 1;
                        }
                    }
                }
                
                if total_count > 0 {
                    return success_count as f64 / total_count as f64;
                }
            }
        }
        
        0.9 // Default success rate
    }
    
    /// Tạo giao dịch hàng loạt (batch) để tối ưu gas
    ///
    /// Kết hợp nhiều giao dịch vào một để tiết kiệm gas
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_addresses`: Danh sách địa chỉ token
    /// - `amounts`: Danh sách số lượng token tương ứng
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - `Result<String, String>`: Transaction hash nếu thành công, error message nếu thất bại
    pub async fn batch_trade(&self, chain_id: u32, token_addresses: &[String], amounts: &[f64], adapter: &Arc<EvmAdapter>) -> Result<String, String> {
        if token_addresses.len() != amounts.len() {
            return Err("Số lượng token và amount không khớp".to_string());
        }
        
        if token_addresses.is_empty() {
            return Err("Danh sách token trống".to_string());
        }
        
        info!("Bắt đầu giao dịch batch với {} token", token_addresses.len());
        
        let mut trade_params_list = Vec::new();
        
        // Chuẩn bị các tham số giao dịch
        for (i, token_address) in token_addresses.iter().enumerate() {
            // Tìm pool tối ưu cho mỗi token
            let router_address = if let Some(optimal_pool) = self.select_optimal_pool(chain_id, token_address, adapter).await {
                optimal_pool
            } else {
                // Dùng router mặc định nếu không tìm được pool tối ưu
                "".to_string()
            };
            
            // Tạo tham số giao dịch
            let trade_params = crate::types::TradeParams {
                chain_type: ChainType::EVM(chain_id),
                token_address: token_address.clone(),
                amount: amounts[i],
                slippage: 2.0, // 2% slippage mặc định
                trade_type: crate::types::TradeType::Buy,
                deadline_minutes: 5,
                router_address,
            };
            
            trade_params_list.push(trade_params);
        }
        
        // Thực hiện giao dịch batch
        match adapter.execute_batch_trades(&trade_params_list).await {
            Ok(tx_hash) => {
                info!("Giao dịch batch thành công: {}", tx_hash);
                
                // Log kết quả
                for (i, token_address) in token_addresses.iter().enumerate() {
                    self.log_trade_decision(
                        "batch", 
                        token_address, 
                        TradeStatus::Completed, 
                        &format!("Batch trade thành công với {} ETH/BNB", amounts[i]),
                        chain_id
                    ).await;
                }
                
                Ok(tx_hash)
            },
            Err(e) => {
                error!("Giao dịch batch thất bại: {:?}", e);
                
                // Log lỗi
                for token_address in token_addresses {
                    self.log_trade_decision(
                        "batch", 
                        token_address, 
                        TradeStatus::Failed, 
                        &format!("Batch trade thất bại: {:?}", e),
                        chain_id
                    ).await;
                }
                
                Err(format!("Giao dịch batch thất bại: {:?}", e))
            }
        }
    }
    
    /// Bảo vệ chống MEV (Miner Extractable Value)
    ///
    /// Triển khai các kỹ thuật để bảo vệ giao dịch khỏi bị front-run hoặc sandwich attack
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `amount`: Số lượng token
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - `Result<(String, String), String>`: (Tx hash, Tham số bảo vệ) nếu thành công, error nếu thất bại
    pub async fn anti_mev_protection(&self, chain_id: u32, token_address: &str, amount: f64, adapter: &Arc<EvmAdapter>) -> Result<(String, String), String> {
        // Phương pháp bảo vệ chống MEV
        let protection_methods = vec![
            "flashbots", // Gửi trực tiếp đến validator thông qua Flashbots
            "high_gas", // Tăng gas price/priority fee để được mine nhanh
            "low_amount", // Chia thành nhiều giao dịch nhỏ
            "delay", // Gửi vào thời điểm ít người giao dịch
        ];
        
        // Chọn phương pháp bảo vệ phù hợp với chain_id
        let method = match chain_id {
            1 => "flashbots", // Ethereum mainnet
            56 => "high_gas", // BSC
            _ => "high_gas", // Mặc định
        };
        
        info!("Áp dụng bảo vệ chống MEV ({}) cho token {}", method, token_address);
        
        match method {
            "flashbots" => {
                // Tạo bundle Flashbots (chỉ dành cho Ethereum)
                let trade_params = crate::types::TradeParams {
                    chain_type: ChainType::EVM(chain_id),
                    token_address: token_address.to_string(),
                    amount,
                    slippage: 1.0, // Giảm slippage để tránh sandwich
                    trade_type: crate::types::TradeType::Buy,
                    deadline_minutes: 2,
                    router_address: "".to_string(),
                };
                
                match adapter.execute_flashbots_trade(&trade_params).await {
                    Ok(tx_hash) => {
                        self.log_trade_decision(
                            "mev_protected", 
                            token_address, 
                            TradeStatus::Completed, 
                            "Giao dịch được bảo vệ bởi Flashbots",
                            chain_id
                        ).await;
                        
                        Ok((tx_hash, "flashbots".to_string()))
                    },
                    Err(e) => Err(format!("Flashbots bảo vệ thất bại: {:?}", e)),
                }
            },
            "high_gas" => {
                // Tăng gas price để được mine nhanh
                let trade_params = crate::types::TradeParams {
                    chain_type: ChainType::EVM(chain_id),
                    token_address: token_address.to_string(),
                    amount,
                    slippage: 1.0, // Giảm slippage
                    trade_type: crate::types::TradeType::Buy,
                    deadline_minutes: 1, // Giảm deadline
                    router_address: "".to_string(),
                };
                
                // Tính toán gas price phù hợp
                let base_fee = adapter.get_base_fee().await.unwrap_or(5.0);
                let optimal_priority_fee = base_fee * 0.7; // 70% của base fee
                
                match adapter.execute_trade_with_priority(&trade_params, optimal_priority_fee).await {
                    Ok(tx_hash) => {
                        self.log_trade_decision(
                            "mev_protected", 
                            token_address, 
                            TradeStatus::Completed, 
                            &format!("Giao dịch được bảo vệ bởi priority fee cao ({} gwei)", optimal_priority_fee),
                            chain_id
                        ).await;
                        
                        Ok((tx_hash, format!("high_gas_{}", optimal_priority_fee)))
                    },
                    Err(e) => Err(format!("High gas bảo vệ thất bại: {:?}", e)),
                }
            },
            "low_amount" => {
                // Chia thành nhiều giao dịch nhỏ
                let num_txs = 3; // Chia thành 3 giao dịch
                let amount_per_tx = amount / num_txs as f64;
                
                let mut tx_hashes = Vec::new();
                
                for i in 0..num_txs {
                    // Thêm delay nhỏ giữa các giao dịch
                    if i > 0 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                    
                    let trade_params = crate::types::TradeParams {
                        chain_type: ChainType::EVM(chain_id),
                        token_address: token_address.to_string(),
                        amount: amount_per_tx,
                        slippage: 1.0,
                        trade_type: crate::types::TradeType::Buy,
                        deadline_minutes: 2,
                        router_address: "".to_string(),
                    };
                    
                    match adapter.execute_trade(&trade_params).await {
                        Ok(tx_hash) => {
                            tx_hashes.push(tx_hash);
                        },
                        Err(e) => {
                            error!("Phần giao dịch {} thất bại: {:?}", i+1, e);
                        }
                    }
                }
                
                if tx_hashes.is_empty() {
                    Err("Tất cả các giao dịch con đều thất bại".to_string())
                } else {
                    self.log_trade_decision(
                        "mev_protected", 
                        token_address, 
                        TradeStatus::Completed, 
                        &format!("Giao dịch được chia thành {} tx nhỏ", tx_hashes.len()),
                        chain_id
                    ).await;
                    
                    Ok((tx_hashes.join(","), "low_amount".to_string()))
                }
            },
            "delay" => {
                // Chờ đến thời điểm ít người giao dịch
                // Tính toán thời điểm tốt nhất dựa trên phân tích mempool
                if let Some(mempool) = self.mempool_analyzers.get(&chain_id) {
                    let congestion = mempool.get_mempool_congestion().await;
                    
                    // Nếu mempool đông đúc, chờ đợi
                    if congestion > 0.7 { // 70% congestion
                        let wait_time = ((congestion - 0.7) * 10.0) as u64; // Tối đa 3 giây
                        
                        info!("Mempool đông đúc ({}%), chờ {}ms", congestion * 100.0, wait_time * 1000);
                        tokio::time::sleep(tokio::time::Duration::from_secs(wait_time)).await;
                    }
                }
                
                let trade_params = crate::types::TradeParams {
                    chain_type: ChainType::EVM(chain_id),
                    token_address: token_address.to_string(),
                    amount,
                    slippage: 1.5,
                    trade_type: crate::types::TradeType::Buy,
                    deadline_minutes: 3,
                    router_address: "".to_string(),
                };
                
                match adapter.execute_trade(&trade_params).await {
                    Ok(tx_hash) => {
                        self.log_trade_decision(
                            "mev_protected", 
                            token_address, 
                            TradeStatus::Completed, 
                            "Giao dịch được bảo vệ bởi timing tối ưu",
                            chain_id
                        ).await;
                        
                        Ok((tx_hash, "delay".to_string()))
                    },
                    Err(e) => Err(format!("Delay bảo vệ thất bại: {:?}", e)),
                }
            },
            _ => Err("Phương pháp bảo vệ không được hỗ trợ".to_string()),
        }
    }
}
