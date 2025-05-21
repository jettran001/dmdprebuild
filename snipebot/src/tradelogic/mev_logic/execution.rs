/// MEV execution logic
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error};

use super::opportunity::MevOpportunity;
use super::types::{MevOpportunityType, MevExecutionMethod};
use super::constants::*;
use crate::chain_adapters::evm_adapter::EvmAdapter;

/// Result of MEV execution
#[derive(Debug, Clone)]
pub enum MevExecutionResult {
    /// Successfully executed
    Success {
        /// Transaction hash
        tx_hash: String,
        /// Actual profit (USD)
        actual_profit_usd: f64,
        /// Actual gas cost (USD)
        actual_gas_cost_usd: f64,
    },
    /// Failed to execute
    Failure {
        /// Error message
        error: String,
        /// Additional details
        details: Option<String>,
    },
}

/// Lịch sử gas để tối ưu các giao dịch MEV
#[derive(Debug, Clone)]
pub struct GasHistory {
    /// Lịch sử gas price theo chain ID
    gas_prices: std::collections::HashMap<u32, Vec<(u64, f64)>>, // (timestamp, gas_price)
    /// Thống kê tỷ lệ thành công theo gas price
    success_rates: std::collections::HashMap<u32, std::collections::HashMap<u64, (usize, usize)>>, // (gas_tier, (success, total))
}

impl GasHistory {
    /// Tạo mới gas history
    pub fn new() -> Self {
        Self {
            gas_prices: std::collections::HashMap::new(),
            success_rates: std::collections::HashMap::new(),
        }
    }
    
    /// Thêm mức gas vào lịch sử
    pub fn add_gas_price(&mut self, chain_id: u32, gas_price: f64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let entry = self.gas_prices.entry(chain_id).or_insert_with(Vec::new);
        entry.push((now, gas_price));
        
        // Giữ nguyên 100 entry gần nhất
        if entry.len() > 100 {
            entry.sort_by_key(|(timestamp, _)| *timestamp);
            entry.drain(0..(entry.len() - 100));
        }
    }
    
    /// Thêm kết quả thực thi
    pub fn add_execution_result(&mut self, chain_id: u32, gas_tier: u64, success: bool) {
        let chain_stats = self.success_rates.entry(chain_id).or_insert_with(std::collections::HashMap::new);
        let (successes, total) = chain_stats.entry(gas_tier).or_insert((0, 0));
        *total += 1;
        if success {
            *successes += 1;
        }
    }
    
    /// Lấy ước tính gas price tối ưu
    pub fn get_optimal_gas_price(&self, chain_id: u32, priority: u8) -> f64 {
        // Lấy lịch sử gas price cho chain
        let gas_prices = match self.gas_prices.get(&chain_id) {
            Some(prices) => prices,
            None => return 50.0, // Giá trị mặc định nếu không có lịch sử
        };
        
        if gas_prices.is_empty() {
            return 50.0;
        }
        
        // Lọc gas prices trong 10 phút gần nhất
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let recent_prices: Vec<f64> = gas_prices.iter()
            .filter(|(timestamp, _)| now - timestamp < 600) // 10 phút
            .map(|(_, price)| *price)
            .collect();
        
        if recent_prices.is_empty() {
            return 50.0;
        }
        
        // Tính giá gas tối ưu dựa trên mức ưu tiên
        match priority {
            0 => { // Thấp
                let min_price = recent_prices.iter().fold(f64::INFINITY, |a, &b| a.min(b));
                min_price * 0.9 // 90% của giá thấp nhất
            },
            1 => { // Trung bình
                let sum: f64 = recent_prices.iter().sum();
                let avg = sum / recent_prices.len() as f64;
                avg * 1.1 // 110% của giá trung bình
            },
            _ => { // Cao (2+)
                let max_price = recent_prices.iter().fold(0.0, |a, &b| a.max(b));
                max_price * 1.25 // 125% của giá cao nhất
            },
        }
    }
}

/// Độ ưu tiên của giao dịch MEV
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MevPriority {
    /// Thấp - chờ được, không vội
    Low,
    /// Trung bình - nên được xác nhận nhanh
    Medium,
    /// Cao - cần được xác nhận ngay lập tức
    High,
    /// Rất cao - phải được xác nhận trong block tiếp theo
    VeryHigh,
}

impl From<MevPriority> for u8 {
    fn from(priority: MevPriority) -> Self {
        match priority {
            MevPriority::Low => 0,
            MevPriority::Medium => 1,
            MevPriority::High => 2,
            MevPriority::VeryHigh => 3,
        }
    }
}

/// Singleton để theo dõi lịch sử gas
lazy_static::lazy_static! {
    static ref GAS_HISTORY: RwLock<GasHistory> = RwLock::new(GasHistory::new());
}

/// Tính toán gas price tối ưu cho giao dịch MEV
pub async fn calculate_optimal_gas(
    chain_id: u32, 
    opportunity_type: &MevOpportunityType,
    estimated_profit: f64
) -> (u64, f64) {
    // Xác định mức độ ưu tiên dựa trên loại cơ hội và lợi nhuận
    let priority = match opportunity_type {
        MevOpportunityType::Arbitrage => {
            if estimated_profit > 100.0 {
                MevPriority::High
            } else if estimated_profit > 50.0 {
                MevPriority::Medium
            } else {
                MevPriority::Low
            }
        },
        MevOpportunityType::Sandwich => {
            if estimated_profit > 200.0 {
                MevPriority::VeryHigh
            } else if estimated_profit > 100.0 {
                MevPriority::High
            } else {
                MevPriority::Medium
            }
        },
        MevOpportunityType::FrontRun => MevPriority::VeryHigh,
        _ => MevPriority::Medium,
    };
    
    // Lấy gas price từ lịch sử
    let gas_history = GAS_HISTORY.read().await;
    let gas_price = gas_history.get_optimal_gas_price(chain_id, priority.into());
    
    // Xác định gas limit dựa trên loại cơ hội
    let gas_limit = match opportunity_type {
        MevOpportunityType::Arbitrage => ARBITRAGE_GAS_LIMIT,
        MevOpportunityType::Sandwich => SANDWICH_GAS_LIMIT,
        MevOpportunityType::FrontRun => 300000,
        MevOpportunityType::NewToken => 400000,
        _ => 250000,
    };
    
    (gas_limit, gas_price)
}

/// Execute an MEV opportunity
pub async fn execute_mev_opportunity(
    opportunity: &MevOpportunity,
    evm_adapter: Arc<EvmAdapter>,
    dry_run: bool
) -> MevExecutionResult {
    info!(
        "Executing MEV opportunity: {:?}, estimated profit: ${:.2}",
        opportunity.opportunity_type, opportunity.estimated_profit_usd
    );
    
    // Tính toán gas tối ưu
    let (gas_limit, gas_price) = calculate_optimal_gas(
        opportunity.chain_id,
        &opportunity.opportunity_type,
        opportunity.estimated_profit_usd
    ).await;
    
    // Kiểm tra gas price có vượt ngưỡng tối đa không
    if gas_price > MAX_GAS_PRICE_GWEI {
        return MevExecutionResult::Failure {
            error: format!("Gas price too high: {:.2} Gwei > {} Gwei", gas_price, MAX_GAS_PRICE_GWEI),
            details: None,
        };
    }
    
    // Tính chiến lược thực thi dựa trên loại cơ hội
    let execution_result = match opportunity.opportunity_type {
        MevOpportunityType::Arbitrage => {
            execute_arbitrage(opportunity, evm_adapter.clone(), gas_limit, gas_price, dry_run).await
        },
        MevOpportunityType::Sandwich => {
            execute_sandwich(opportunity, evm_adapter.clone(), gas_limit, gas_price, dry_run).await
        },
        MevOpportunityType::FrontRun => {
            execute_frontrun(opportunity, evm_adapter.clone(), gas_limit, gas_price, dry_run).await
        },
        _ => {
            MevExecutionResult::Failure {
                error: format!("Unsupported MEV opportunity type: {:?}", opportunity.opportunity_type),
                details: None,
            }
        }
    };
    
    // Cập nhật lịch sử gas dựa trên kết quả
    let mut gas_history = GAS_HISTORY.write().await;
    gas_history.add_gas_price(opportunity.chain_id, gas_price);
    
    match &execution_result {
        MevExecutionResult::Success { .. } => {
            gas_history.add_execution_result(
                opportunity.chain_id, 
                (gas_price / 5.0).floor() as u64 * 5, // Round to nearest 5 Gwei tier
                true
            );
        },
        MevExecutionResult::Failure { .. } => {
            gas_history.add_execution_result(
                opportunity.chain_id, 
                (gas_price / 5.0).floor() as u64 * 5,
                false
            );
        }
    }
    
    execution_result
}

/// Thực thi cơ hội arbitrage
async fn execute_arbitrage(
    opportunity: &MevOpportunity,
    evm_adapter: Arc<EvmAdapter>,
    gas_limit: u64,
    gas_price: f64,
    dry_run: bool
) -> MevExecutionResult {
    // Lấy thông tin cần thiết từ opportunity params
    let token_pairs = &opportunity.token_pairs;
    if token_pairs.len() < 1 {
        return MevExecutionResult::Failure {
            error: "Missing token pair information".to_string(),
            details: None,
        };
    }
    
    // Xây dựng payload 
    let payload = match &opportunity.execution_method {
        MevExecutionMethod::FlashLoan => {
            // TODO: Implement flash loan execution
            return MevExecutionResult::Failure {
                error: "Flash loan execution not implemented yet".to_string(),
                details: None,
            };
        },
        MevExecutionMethod::StandardTransaction => {
            // Xây dựng calldata cho swap chuẩn
            let first_pair = &token_pairs[0];
            
            let from_token = &first_pair.token_a;
            let to_token = &first_pair.token_b;
            
            // TODO: Build swap calldata
            "0x".to_string()
        },
        MevExecutionMethod::MultiSwap => {
            // TODO: Implement multi-swap
            return MevExecutionResult::Failure {
                error: "Multi-swap execution not implemented yet".to_string(),
                details: None,
            };
        },
        MevExecutionMethod::CustomContract => {
            // Với custom contract, dùng transaction params đã có sẵn
            if let Some(calldata) = opportunity.specific_params.get("calldata") {
                calldata.clone()
            } else {
                return MevExecutionResult::Failure {
                    error: "Missing calldata for custom contract".to_string(),
                    details: None,
                };
            }
        }
    };
    
    // Lấy địa chỉ contract
    let contract_address = match opportunity.specific_params.get("contract_address") {
        Some(address) => address.clone(),
        None => {
            return MevExecutionResult::Failure {
                error: "Missing contract address".to_string(),
                details: None,
            };
        }
    };
    
    // Xác định value (ETH gửi cùng transaction)
    let value = opportunity.specific_params.get("value")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    
    if dry_run {
        // Giả lập thành công cho dry run
        return MevExecutionResult::Success {
            tx_hash: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            actual_profit_usd: opportunity.estimated_profit_usd,
            actual_gas_cost_usd: opportunity.estimated_gas_cost_usd,
        };
    }
    
    // Gửi transaction
    match evm_adapter.send_transaction(
        contract_address.clone(),
        payload,
        value,
        gas_limit,
        gas_price,
    ).await {
        Ok(tx_hash) => {
            info!(
                "Successfully executed arbitrage MEV opportunity. Tx hash: {}", 
                tx_hash
            );
            
            MevExecutionResult::Success {
                tx_hash,
                actual_profit_usd: opportunity.estimated_profit_usd,
                actual_gas_cost_usd: opportunity.estimated_gas_cost_usd,
            }
        },
        Err(e) => {
            error!(
                "Failed to execute arbitrage MEV opportunity: {}", 
                e
            );
            
            MevExecutionResult::Failure {
                error: format!("Transaction failed: {}", e),
                details: None,
            }
        }
    }
}

/// Thực thi sandwich attack
async fn execute_sandwich(
    opportunity: &MevOpportunity,
    evm_adapter: Arc<EvmAdapter>,
    gas_limit: u64,
    gas_price: f64,
    dry_run: bool
) -> MevExecutionResult {
    // Sandwich attack yêu cầu hai giao dịch:
    // 1. Front-transaction: trước giao dịch của nạn nhân
    // 2. Back-transaction: sau giao dịch của nạn nhân
    
    // Lấy thông tin cần thiết
    let victim_tx_hash = match opportunity.specific_params.get("victim_tx_hash") {
        Some(hash) => hash.clone(),
        None => {
            return MevExecutionResult::Failure {
                error: "Missing victim transaction hash".to_string(),
                details: None,
            };
        }
    };
    
    let front_calldata = match opportunity.specific_params.get("front_calldata") {
        Some(data) => data.clone(),
        None => {
            return MevExecutionResult::Failure {
                error: "Missing front calldata".to_string(),
                details: None,
            };
        }
    };
    
    let back_calldata = match opportunity.specific_params.get("back_calldata") {
        Some(data) => data.clone(),
        None => {
            return MevExecutionResult::Failure {
                error: "Missing back calldata".to_string(),
                details: None,
            };
        }
    };
    
    let contract_address = match opportunity.specific_params.get("contract_address") {
        Some(addr) => addr.clone(),
        None => {
            return MevExecutionResult::Failure {
                error: "Missing contract address".to_string(),
                details: None,
            };
        }
    };
    
    if dry_run {
        // Giả lập thành công cho dry run
        return MevExecutionResult::Success {
            tx_hash: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            actual_profit_usd: opportunity.estimated_profit_usd,
            actual_gas_cost_usd: opportunity.estimated_gas_cost_usd,
        };
    }
    
    // Gửi front transaction với gas cao hơn
    let front_gas_price = gas_price * 1.1; // 10% cao hơn
    
    let front_result = evm_adapter.send_transaction(
        contract_address.clone(),
        front_calldata,
        0.0, // No ETH sent
        gas_limit,
        front_gas_price,
    ).await;
    
    match front_result {
        Ok(front_tx_hash) => {
            info!("Front transaction sent. Tx hash: {}", front_tx_hash);
            
            // Chờ xác nhận front transaction
            // TODO: Implement waiting logic
            
            // Gửi back transaction với gas thấp hơn
            let back_gas_price = gas_price * 0.9; // 10% thấp hơn
            
            match evm_adapter.send_transaction(
                contract_address.clone(),
                back_calldata,
                0.0, // No ETH sent
                gas_limit,
                back_gas_price,
            ).await {
                Ok(back_tx_hash) => {
                    info!("Back transaction sent. Tx hash: {}", back_tx_hash);
                    
                    MevExecutionResult::Success {
                        tx_hash: back_tx_hash, // Trả về hash của back transaction
                        actual_profit_usd: opportunity.estimated_profit_usd,
                        actual_gas_cost_usd: opportunity.estimated_gas_cost_usd * 2.0, // Cân nhắc cả 2 tx
                    }
                },
                Err(e) => {
                    error!("Failed to send back transaction: {}", e);
                    
                    MevExecutionResult::Failure {
                        error: format!("Back transaction failed: {}", e),
                        details: Some(format!("Front transaction succeeded: {}", front_tx_hash)),
                    }
                }
            }
        },
        Err(e) => {
            error!("Failed to send front transaction: {}", e);
            
            MevExecutionResult::Failure {
                error: format!("Front transaction failed: {}", e),
                details: None,
            }
        }
    }
}

/// Thực thi front-running
async fn execute_frontrun(
    opportunity: &MevOpportunity,
    evm_adapter: Arc<EvmAdapter>,
    gas_limit: u64,
    gas_price: f64,
    dry_run: bool
) -> MevExecutionResult {
    // Lấy thông tin cần thiết
    let target_tx_hash = match opportunity.specific_params.get("target_tx_hash") {
        Some(hash) => hash.clone(),
        None => {
            return MevExecutionResult::Failure {
                error: "Missing target transaction hash".to_string(),
                details: None,
            };
        }
    };
    
    let calldata = match opportunity.specific_params.get("calldata") {
        Some(data) => data.clone(),
        None => {
            return MevExecutionResult::Failure {
                error: "Missing calldata".to_string(),
                details: None,
            };
        }
    };
    
    let contract_address = match opportunity.specific_params.get("contract_address") {
        Some(addr) => addr.clone(),
        None => {
            return MevExecutionResult::Failure {
                error: "Missing contract address".to_string(),
                details: None,
            };
        }
    };
    
    if dry_run {
        // Giả lập thành công cho dry run
        return MevExecutionResult::Success {
            tx_hash: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            actual_profit_usd: opportunity.estimated_profit_usd,
            actual_gas_cost_usd: opportunity.estimated_gas_cost_usd,
        };
    }
    
    // Bổ sung thêm gas để chắc chắn chạy trước
    let boosted_gas_price = gas_price * FRONT_RUN_GAS_BOOST;
    
    // Gửi transaction
    match evm_adapter.send_transaction(
        contract_address.clone(),
        calldata,
        0.0, // No ETH sent
        gas_limit,
        boosted_gas_price,
    ).await {
        Ok(tx_hash) => {
            info!(
                "Successfully executed frontrun MEV opportunity. Tx hash: {}", 
                tx_hash
            );
            
            MevExecutionResult::Success {
                tx_hash,
                actual_profit_usd: opportunity.estimated_profit_usd,
                actual_gas_cost_usd: opportunity.estimated_gas_cost_usd * (FRONT_RUN_GAS_BOOST as f64),
            }
        },
        Err(e) => {
            error!(
                "Failed to execute frontrun MEV opportunity: {}", 
                e
            );
            
            MevExecutionResult::Failure {
                error: format!("Transaction failed: {}", e),
                details: None,
            }
        }
    }
} 