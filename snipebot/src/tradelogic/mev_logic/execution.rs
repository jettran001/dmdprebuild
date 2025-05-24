/// MEV execution logic
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::OnceCell;
use tracing::{info, warn, error};

use super::opportunity::MevOpportunity;
use super::types::{MevOpportunityType, MevExecutionMethod};
use super::constants::*;
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::tradelogic::common::gas;

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

/// Execute an MEV opportunity
pub async fn execute_mev_opportunity(
    opportunity: &MevOpportunity,
    evm_adapter: Arc<EvmAdapter>,
    dry_run: bool
) -> MevExecutionResult {
    // Calculate optimal gas parameters
    let (gas_limit, gas_price) = match gas::calculate_optimal_gas_for_mev(
        opportunity.chain_id,
        &opportunity.opportunity_type,
        opportunity.estimated_profit_usd,
        &evm_adapter,
    ).await {
        Ok((limit, price)) => (limit, price),
        Err(e) => {
            error!("Failed to calculate optimal gas: {}", e);
            return MevExecutionResult::Failure {
                error: format!("Gas calculation error: {}", e),
                details: None,
            };
        }
    };
    
    info!(
        "Executing MEV opportunity: {:?} on chain {} with gas price {} gwei",
        opportunity.opportunity_type, opportunity.chain_id, gas_price
    );
    
    // Determine execution method
    match opportunity.execution_method {
        MevExecutionMethod::Standard => match opportunity.opportunity_type {
            MevOpportunityType::Arbitrage => {
                execute_arbitrage(opportunity, evm_adapter, gas_limit, gas_price, dry_run).await
            },
            MevOpportunityType::Sandwich => {
                execute_sandwich(opportunity, evm_adapter, gas_limit, gas_price, dry_run).await
            },
            MevOpportunityType::FrontRun => {
                execute_frontrun(opportunity, evm_adapter, gas_limit, gas_price, dry_run).await
            },
            _ => {
                MevExecutionResult::Failure {
                    error: format!("Unsupported opportunity type: {:?}", opportunity.opportunity_type),
                    details: None,
                }
            }
        },
        MevExecutionMethod::Bundle => {
            // Bundle execution not implemented yet
            MevExecutionResult::Failure {
                error: "Bundle execution not implemented".to_string(),
                details: None,
            }
        },
        MevExecutionMethod::FlashBots => {
            // Flashbots execution not implemented yet
            MevExecutionResult::Failure {
                error: "Flashbots execution not implemented".to_string(),
                details: None,
            }
        },
    }
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
    
    // Sử dụng TransactionPriority từ module gas chung để tính toán gas price
    use crate::tradelogic::common::gas::TransactionPriority;
    
    // Gas price cho front transaction (yêu cầu ưu tiên cao hơn)
    let front_gas_price = match gas::calculate_optimal_gas_price(
        opportunity.chain_id,
        TransactionPriority::VeryHigh,  // Cần ưu tiên cao để vượt qua giao dịch của nạn nhân
        &evm_adapter,
        None,
        true,  // Áp dụng MEV protection
    ).await {
        Ok(price) => price,
        Err(e) => {
            error!("Failed to calculate front gas price: {}", e);
            // Fallback: dùng gas price đã tính trước đó với thêm 10%
            gas_price * 1.1
        }
    };
    
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
            
            // Gas price cho back transaction (có thể thấp hơn)
            let back_gas_price = match gas::calculate_optimal_gas_price(
                opportunity.chain_id,
                TransactionPriority::Medium,  // Transaction sau có thể dùng ưu tiên trung bình
                &evm_adapter,
                None,
                false,  // Không cần MEV protection cho transaction sau
            ).await {
                Ok(price) => price,
                Err(e) => {
                    error!("Failed to calculate back gas price: {}", e);
                    // Fallback: dùng gas price thấp hơn front transaction
                    front_gas_price * 0.8
                }
            };
            
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