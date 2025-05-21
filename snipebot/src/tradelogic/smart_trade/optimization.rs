/// Optimization implementation for SmartTradeExecutor
///
/// This module contains functions for optimizing trades, selecting optimal pools,
/// and implementing anti-MEV protection.

// External imports
use std::collections::{HashMap, BinaryHeap};
use std::cmp::Ordering;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::{TokenPair, ChainType};

// Module imports
use super::types::{TradeTracker, TradeStatus};
use super::constants::*;
use super::executor::SmartTradeExecutor;

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
