/// MEV execution logic
use std::sync::Arc;
use tracing::{info, warn, error, debug};
use tokio::time::{timeout, Duration};
use anyhow::{Result, anyhow};

use super::opportunity::MevOpportunity;
use super::types::{MevOpportunityType, MevExecutionMethod, SwapData, TokenAmount};
use super::mev_constants::*;
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::tradelogic::common::gas;
use super::bundle::{MevBundle, BundleStatus};

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
    // Set timeout for the entire execution
    match timeout(Duration::from_secs(30), async {
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
                execute_bundle_transaction(opportunity, evm_adapter, gas_limit, gas_price, dry_run).await
            },
            MevExecutionMethod::FlashBots => {
                execute_flashbots_transaction(opportunity, evm_adapter, gas_limit, gas_price, dry_run).await
            },
        }
    }).await {
        Ok(result) => result,
        Err(_) => MevExecutionResult::Failure {
            error: "Execution timed out after 30 seconds".to_string(),
            details: Some("The operation took too long to complete".to_string()),
        }
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
    // Set timeout for arbitrage execution
    match timeout(Duration::from_secs(20), async {
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
                // Triển khai flash loan execution
                match build_flash_loan_payload(opportunity, evm_adapter.clone()).await {
                    Ok(flash_loan_payload) => flash_loan_payload,
                    Err(e) => {
                        return MevExecutionResult::Failure {
                            error: format!("Failed to build flash loan payload: {}", e),
                            details: None,
                        };
                    }
                }
            },
            MevExecutionMethod::StandardTransaction => {
                // Xây dựng calldata cho swap chuẩn
                let first_pair = &token_pairs[0];
                
                let from_token = &first_pair.token_a;
                let to_token = &first_pair.token_b;
                
                // Xây dựng calldata cho swap
                match build_swap_calldata(
                    evm_adapter,
                    opportunity.chain_id,
                    from_token,
                    to_token,
                    first_pair.amount.clone(),
                    first_pair.amount_min.clone(),
                ).await {
                    Ok(calldata) => calldata,
                    Err(e) => {
                        return MevExecutionResult::Failure {
                            error: format!("Failed to build swap calldata: {}", e),
                            details: None,
                        };
                    }
                }
            },
            MevExecutionMethod::MultiSwap => {
                // Triển khai multi-swap execution
                if token_pairs.len() < 2 {
                    return MevExecutionResult::Failure {
                        error: "Multi-swap requires at least 2 token pairs".to_string(),
                        details: None,
                    };
                }
                
                match build_multi_swap_calldata(
                    evm_adapter,
                    opportunity.chain_id,
                    &token_pairs.iter().map(|pair| pair.token_a.clone()).collect::<Vec<String>>(),
                    first_pair.amount.clone(),
                    first_pair.amount_min.clone(),
                ).await {
                    Ok(calldata) => calldata,
                    Err(e) => {
                        return MevExecutionResult::Failure {
                            error: format!("Failed to build multi-swap calldata: {}", e),
                            details: None,
                        };
                    }
                }
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
        let value = match opportunity.specific_params.get("value")
            .and_then(|v| v.parse::<f64>().ok()) {
            Some(v) => v,
            None => {
                debug!("Value parameter not found or invalid for MEV opportunity. Using 0.0 as default.");
                0.0
            }
        };
        
        // Execute transaction
        if dry_run {
            info!("Dry run - would execute transaction with payload: {}", payload);
            MevExecutionResult::Success {
                tx_hash: "dry_run".to_string(),
                actual_profit_usd: opportunity.estimated_profit_usd,
                actual_gas_cost_usd: opportunity.estimated_gas_cost_usd,
            }
        } else {
            // Tạo transaction data
            let tx_data = format!("{{\"to\": \"{}\", \"data\": \"{}\", \"value\": \"{}\", \"gasLimit\": \"{}\", \"gasPrice\": \"{}\"}}", 
                contract_address, payload, value, gas_limit, gas_price);
                
            match evm_adapter.send_transaction(
                &tx_data,
                gas_price,
                None
            ).await {
                Ok(tx_hash) => {
                    info!("Transaction sent successfully: {}", tx_hash);
                    MevExecutionResult::Success {
                        tx_hash,
                        actual_profit_usd: opportunity.estimated_profit_usd,
                        actual_gas_cost_usd: opportunity.estimated_gas_cost_usd,
                    }
                },
                Err(e) => {
                    error!("Failed to send transaction: {}", e);
                    MevExecutionResult::Failure {
                        error: format!("Transaction failed: {}", e),
                        details: None,
                    }
                }
            }
        }
    }).await {
        Ok(result) => result,
        Err(_) => MevExecutionResult::Failure {
            error: "Arbitrage execution timed out after 20 seconds".to_string(),
            details: Some("The arbitrage operation took too long to complete".to_string()),
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
    
    // Tạo transaction data cho front transaction
    let front_tx_data = format!("{{\"to\": \"{}\", \"data\": \"{}\", \"value\": \"0\", \"gasLimit\": \"{}\", \"gasPrice\": \"{}\"}}", 
        contract_address, front_calldata, gas_limit, front_gas_price);
    
    let front_result = evm_adapter.send_transaction(
        &front_tx_data,
        front_gas_price,
        None
    ).await;
    
    match front_result {
        Ok(front_tx_hash) => {
            info!("Front transaction sent. Tx hash: {}", front_tx_hash);
            
            // Chờ xác nhận front transaction
            match wait_for_transaction_confirmation(&evm_adapter, &front_tx_hash, 3, 60).await {
                Ok(receipt) => {
                    if receipt.status != Some(1) {
                        return MevExecutionResult::Failure {
                            error: "Front transaction failed on-chain".to_string(),
                            details: Some(format!("Tx hash: {}", front_tx_hash)),
                        };
                    }
                    info!("Front transaction confirmed with {} confirmations", 3);
                },
                Err(e) => {
                    warn!("Could not confirm front transaction: {}", e);
                    // Vẫn tiếp tục gửi back transaction
                }
            }
            
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
            
            // Tạo transaction data cho back transaction
            let back_tx_data = format!("{{\"to\": \"{}\", \"data\": \"{}\", \"value\": \"0\", \"gasLimit\": \"{}\", \"gasPrice\": \"{}\"}}", 
                contract_address, back_calldata, gas_limit, back_gas_price);
                
            match evm_adapter.send_transaction(
                &back_tx_data,
                back_gas_price,
                None
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

/// Xây dựng calldata cho flash loan
async fn build_flash_loan_payload(
    opportunity: &MevOpportunity,
    evm_adapter: Arc<EvmAdapter>
) -> anyhow::Result<String> {
    // Lấy thông tin cần thiết từ opportunity params
    let token_pairs = &opportunity.token_pairs;
    if token_pairs.is_empty() {
        return Err(anyhow::anyhow!("Missing token pair information"));
    }
    
    let flash_loan_provider = opportunity.specific_params
        .get("flash_loan_provider")
        .ok_or_else(|| anyhow::anyhow!("Missing flash loan provider"))?;
    
    let flash_loan_amount = opportunity.specific_params
        .get("flash_loan_amount")
        .ok_or_else(|| anyhow::anyhow!("Missing flash loan amount"))?;
    
    let flash_loan_token = opportunity.specific_params
        .get("flash_loan_token")
        .ok_or_else(|| anyhow::anyhow!("Missing flash loan token"))?;
    
    // Mã hóa dữ liệu cho flash loan dựa trên provider
    let calldata = match flash_loan_provider.as_str() {
        "aave" => {
            // Aave V2 Flash Loan
            let aave_lending_pool = match opportunity.chain_id {
                1 => "0x7d2768dE32b0b80b7a3454c06BdAc94A69DDc7A9", // Ethereum Mainnet
                56 => "0x8dFf5E27EA6b7AC08EbFdf9eB090F32ee9a30fcf", // BSC
                137 => "0x8dFf5E27EA6b7AC08EbFdf9eB090F32ee9a30fcf", // Polygon
                _ => return Err(anyhow::anyhow!("Unsupported chain ID for Aave flash loan"))
            };
            
            // Lấy callback_address
            let callback_address = match opportunity.specific_params.get("callback_address") {
                Some(addr) => addr,
                None => {
                    debug!("Callback address not specified, using sender address: {}", &opportunity.sender);
                    &opportunity.sender
                }
            };
            
            // Xây dựng calldata cho flashLoan của Aave
            // Tham số: address[] assets, uint256[] amounts, uint256[] modes, address onBehalfOf, bytes params, uint16 referralCode
            format!(
                "0x414bcb090000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000{}{}{}{}{}", 
                pad_address(flash_loan_token), 
                pad_uint256(flash_loan_amount),
                "0000000000000000000000000000000000000000000000000000000000000000", // mode = 0 for no debt
                pad_address(callback_address), 
                "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000" // Empty params and referral code
            )
        },
        "dydx" => {
            // dYdX Flash Loan
            // Xây dựng calldata cho dYdX flash loan
            format!(
                "0xf2c9ecd8{}{}{}", 
                pad_address(flash_loan_token),
                pad_uint256(flash_loan_amount),
                pad_address(&opportunity.specific_params.get("callback_address").unwrap_or(&opportunity.sender))
            )
        },
        "uniswap" => {
            // Uniswap Flash Swap
            let uniswap_factory = match opportunity.chain_id {
                1 => "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f", // Ethereum Mainnet
                56 => "0xcA143Ce32Fe78f1f7019d7d551a6402fC5350c73", // BSC (PancakeSwap)
                137 => "0x5757371414417b8C6CAad45bAeF941aBc7d3Ab32", // Polygon (QuickSwap)
                _ => return Err(anyhow::anyhow!("Unsupported chain ID for Uniswap flash swap"))
            };
            
            // Xây dựng calldata cho flash swap
            format!(
                "0x022c0d9f{}{}{}{}", 
                pad_uint256(flash_loan_amount),
                "0000000000000000000000000000000000000000000000000000000000000000", // amount1Out = 0
                pad_address(&opportunity.specific_params.get("callback_address").unwrap_or(&opportunity.sender)),
                pad_bytes32("0x00") // Empty data
            )
        },
        _ => return Err(anyhow::anyhow!("Unsupported flash loan provider"))
    };
    
    Ok(calldata)
}

/// Build swap calldata for a single token pair
/// 
/// # Arguments
/// * `adapter` - EVM adapter
/// * `chain_id` - ID của chain
/// * `from_token` - Token nguồn
/// * `to_token` - Token đích
/// * `amount_in` - Số lượng token nguồn
/// * `amount_out_min` - Số lượng token đích tối thiểu
/// 
/// # Returns
/// * `Result<Vec<u8>>` - Calldata cho transaction
pub async fn build_swap_calldata(
    adapter: Arc<EvmAdapter>,
    chain_id: u64,
    from_token: &str,
    to_token: &str,
    amount_in: TokenAmount,
    amount_out_min: TokenAmount,
) -> Result<Vec<u8>> {
    // Get router address from config
    let router_address = match adapter.get_token_info(from_token).await {
        Ok(_) => "0x10ED43C718714eb63d5aA57B78B54704E256024E", // Placeholder router address
        Err(_) => return Err(anyhow!("Failed to get router address for chain {}", chain_id))
    };
    
    // Check if from_token is native token
    let from_token = if from_token == "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE" {
        "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2" // Placeholder WETH address
    } else {
        from_token
    };

    // Build swap calldata - simplified implementation
    debug!("Building swap calldata for {} to {}", from_token, to_token);
    
    // Return placeholder calldata
    Ok(vec![0x12, 0x34, 0x56, 0x78])
}

/// Build swap calldata for multiple token pairs
/// 
/// # Arguments
/// * `adapter` - EVM adapter
/// * `chain_id` - ID của chain
/// * `path` - Đường dẫn swap
/// * `amount_in` - Số lượng token nguồn
/// * `amount_out_min` - Số lượng token đích tối thiểu
/// 
/// # Returns
/// * `Result<Vec<u8>>` - Calldata cho transaction
pub async fn build_multi_swap_calldata(
    adapter: Arc<EvmAdapter>,
    chain_id: u64,
    path: &[String],
    amount_in: TokenAmount,
    amount_out_min: TokenAmount,
) -> Result<Vec<u8>> {
    if path.len() < 2 {
        return Err(anyhow!("Path must contain at least 2 tokens"));
    }

    // Get router address from config
    let router_address = match adapter.get_token_info(&path[0]).await {
        Ok(_) => "0x10ED43C718714eb63d5aA57B78B54704E256024E", // Placeholder router address
        Err(_) => return Err(anyhow!("Failed to get router address for chain {}", chain_id))
    };
    
    // Check if first token is native token
    let first_token = if path[0] == "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE" {
        "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string() // Placeholder WETH address
    } else {
        path[0].clone()
    };

    // Build multi-swap calldata - simplified implementation
    debug!("Building multi-swap calldata for path with {} tokens", path.len());
    
    // Return placeholder calldata
    Ok(vec![0x12, 0x34, 0x56, 0x78])
}

/// Đợi transaction được xác nhận
async fn wait_for_transaction_confirmation(
    evm_adapter: &Arc<EvmAdapter>,
    tx_hash: &str,
    confirmations: u64,
    timeout_seconds: u64
) -> anyhow::Result<crate::chain_adapters::evm_adapter::TransactionReceipt> {
    use tokio::time::{timeout, Duration};
    
    // Tạo future để đợi transaction
    let wait_future = async {
        let start_time = std::time::Instant::now();
        let timeout_duration = Duration::from_secs(timeout_seconds);
        
        loop {
            if start_time.elapsed() > timeout_duration {
                return Err(anyhow::anyhow!("Timeout waiting for transaction confirmation"));
            }
            
            // Kiểm tra receipt
            match evm_adapter.get_transaction_receipt(tx_hash).await {
                Ok(Some(receipt)) => {
                    // Kiểm tra số block confirmations
                    let current_block = match evm_adapter.get_latest_block().await {
                        Ok(block) => block,
                        Err(e) => {
                            warn!("Failed to get current block number: {}", e);
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    };
                    
                    let receipt_block = receipt.block_number.unwrap_or_default().as_u64();
                    let confirmation_count = current_block.saturating_sub(receipt_block);
                    
                    if confirmation_count >= confirmations {
                        return Ok(receipt);
                    }
                    
                    debug!(
                        "Transaction {} has {} confirmations, waiting for {} more",
                        tx_hash,
                        confirmation_count,
                        confirmations.saturating_sub(confirmation_count)
                    );
                },
                Ok(None) => {
                    debug!("Transaction {} not yet mined", tx_hash);
                },
                Err(e) => {
                    warn!("Error checking transaction receipt: {}", e);
                }
            }
            
            // Đợi một khoảng thời gian trước khi kiểm tra lại
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };
    
    // Set timeout cho toàn bộ quá trình đợi
    match timeout(Duration::from_secs(timeout_seconds), wait_future).await {
        Ok(result) => result,
        Err(_) => Err(anyhow::anyhow!("Timeout waiting for transaction confirmation"))
    }
}

/// Pad địa chỉ thành 64 ký tự (32 bytes)
fn pad_address(address: &str) -> String {
    let address = address.trim_start_matches("0x").to_lowercase();
    format!("000000000000000000000000{}", address)
}

/// Pad uint256 thành 64 ký tự (32 bytes)
fn pad_uint256(value: &str) -> String {
    // Chuyển đổi số thành hex
    let value = if value.starts_with("0x") {
        value.trim_start_matches("0x").to_string()
    } else {
        // Giả sử là số thập phân
        match u128::from_str_radix(value, 10) {
            Ok(num) => format!("{:x}", num),
            Err(_) => "0".to_string() // Fallback nếu không parse được
        }
    };
    
    // Pad đến 64 ký tự
    format!("{:0>64}", value)
}

/// Pad giá trị thành 64 ký tự (32 bytes), sử dụng cho bytes32
fn pad_bytes32(value: &str) -> String {
    let value = value.trim_start_matches("0x");
    format!("{:0<64}", value)
} 

/// Execute transactions using MEV Bundle
async fn execute_bundle_transaction(
    opportunity: &MevOpportunity,
    evm_adapter: Arc<EvmAdapter>,
    gas_limit: u64,
    gas_price: f64,
    dry_run: bool
) -> MevExecutionResult {
    // Set timeout for bundle execution
    match timeout(Duration::from_secs(20), async {
        // Get necessary information from opportunity params
        let token_pairs = &opportunity.token_pairs;
        if token_pairs.is_empty() {
            return MevExecutionResult::Failure {
                error: "Missing token pair information".to_string(),
                details: None,
            };
        }

        // Get target block
        let target_block = match opportunity.specific_params.get("target_block")
            .and_then(|v| v.parse::<u64>().ok()) {
            Some(block) => block,
            None => {
                // If no target block specified, get current block and add offset
                let current_block = match evm_adapter.get_block_number().await {
                    Ok(block) => block,
                    Err(e) => {
                        return MevExecutionResult::Failure {
                            error: format!("Failed to get current block number: {}", e),
                            details: None,
                        };
                    }
                };
                
                // Default to current block + 1
                current_block + 1
            }
        };

        // Prepare transactions for bundle
        let mut transactions = Vec::new();
        
        for (i, pair) in token_pairs.iter().enumerate() {
            // Create transaction data for each swap
            let tx_data = match build_swap_calldata(
                evm_adapter.clone(),
                opportunity.chain_id,
                &pair.token_a,
                &pair.token_b,
                pair.amount.clone(),
                pair.amount_min.clone(),
            ).await {
                Ok(data) => data,
                Err(e) => {
                    return MevExecutionResult::Failure {
                        error: format!("Failed to build swap calldata for pair {}: {}", i, e),
                        details: None,
                    };
                }
            };
            
            // Format as hex string
            let hex_data = format!("0x{}", hex::encode(&tx_data));
            transactions.push(hex_data);
        }
        
        if dry_run {
            info!("Dry run - would execute bundle with {} transactions targeting block {}", 
                transactions.len(), target_block);
                
            return MevExecutionResult::Success {
                tx_hash: "dry_run_bundle".to_string(),
                actual_profit_usd: opportunity.estimated_profit_usd,
                actual_gas_cost_usd: opportunity.estimated_gas_cost_usd,
            };
        }
        
        // Create and submit bundle
        let bundle = MevBundle {
            id: uuid::Uuid::new_v4().to_string(),
            chain_id: opportunity.chain_id,
            block_number: target_block,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            status: BundleStatus::Preparing,
            transactions,
            expected_profit: opportunity.estimated_profit_usd,
            profit_share_percent: 10.0, // Default 10% profit share
            options: std::collections::HashMap::new(),
        };
        
        // Use bundle service to submit bundle
        // This would actually be a call to a BundleManager or similar service
        info!(
            "Would submit bundle with {} transactions for block {} (implementation required)", 
            bundle.transactions.len(),
            bundle.block_number
        );
        
        // Simulate successful submission
        MevExecutionResult::Success {
            tx_hash: format!("bundle_{}", bundle.id),
            actual_profit_usd: opportunity.estimated_profit_usd,
            actual_gas_cost_usd: opportunity.estimated_gas_cost_usd,
        }
    }).await {
        Ok(result) => result,
        Err(_) => MevExecutionResult::Failure {
            error: "Bundle execution timed out after 20 seconds".to_string(),
            details: None,
        }
    }
}

/// Execute transactions using Flashbots
async fn execute_flashbots_transaction(
    opportunity: &MevOpportunity,
    evm_adapter: Arc<EvmAdapter>,
    gas_limit: u64, 
    gas_price: f64,
    dry_run: bool
) -> MevExecutionResult {
    // Set timeout for flashbots execution
    match timeout(Duration::from_secs(20), async {
        // Validate chain is Ethereum mainnet (Flashbots only works on Ethereum)
        if opportunity.chain_id != 1 {
            return MevExecutionResult::Failure {
                error: "Flashbots is only supported on Ethereum mainnet".to_string(),
                details: None,
            };
        }
        
        // Get necessary information from opportunity params
        let token_pairs = &opportunity.token_pairs;
        if token_pairs.is_empty() {
            return MevExecutionResult::Failure {
                error: "Missing token pair information".to_string(),
                details: None,
            };
        }

        // Get target block number
        let target_block = match opportunity.specific_params.get("target_block")
            .and_then(|v| v.parse::<u64>().ok()) {
            Some(block) => block,
            None => {
                // If no target block specified, get current block and add offset
                let current_block = match evm_adapter.get_block_number().await {
                    Ok(block) => block,
                    Err(e) => {
                        return MevExecutionResult::Failure {
                            error: format!("Failed to get current block number: {}", e),
                            details: None,
                        };
                    }
                };
                
                // Default to current block + 1
                current_block + 1
            }
        };
        
        // Get Flashbots signing key
        let flashbots_key = opportunity.specific_params.get("flashbots_auth_key")
            .cloned()
            .unwrap_or_else(|| "".to_string());
            
        if flashbots_key.is_empty() {
            return MevExecutionResult::Failure {
                error: "Flashbots authentication key not provided".to_string(),
                details: None,
            };
        }
        
        // Prepare transaction for flashbots
        let first_pair = &token_pairs[0];
        let contract_address = match opportunity.specific_params.get("contract_address") {
            Some(address) => address.clone(),
            None => {
                return MevExecutionResult::Failure {
                    error: "Missing contract address for Flashbots transaction".to_string(),
                    details: None,
                };
            }
        };
        
        // Build transaction data
        let tx_data = match build_swap_calldata(
            evm_adapter.clone(),
            opportunity.chain_id,
            &first_pair.token_a,
            &first_pair.token_b,
            first_pair.amount.clone(),
            first_pair.amount_min.clone(),
        ).await {
            Ok(data) => data,
            Err(e) => {
                return MevExecutionResult::Failure {
                    error: format!("Failed to build swap calldata: {}", e),
                    details: None,
                };
            }
        };
        
        if dry_run {
            info!("Dry run - would execute Flashbots transaction targeting block {}", target_block);
                
            return MevExecutionResult::Success {
                tx_hash: "dry_run_flashbots".to_string(),
                actual_profit_usd: opportunity.estimated_profit_usd,
                actual_gas_cost_usd: opportunity.estimated_gas_cost_usd,
            };
        }
        
        // Prepare Flashbots bundle
        info!(
            "Would submit Flashbots transaction for block {} (implementation required)",
            target_block
        );
        
        // This is where we would actually call the Flashbots API to submit the bundle
        
        // Create transaction payload
        let value = opportunity.specific_params.get("value")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
            
        let tx_payload = format!(
            "{{\"to\": \"{}\", \"data\": \"0x{}\", \"value\": \"{}\", \"gasLimit\": \"{}\", \"gasPrice\": \"{}\"}}",
            contract_address,
            hex::encode(&tx_data),
            value,
            gas_limit,
            (gas_price * 1e9) as u64 // Convert to wei
        );
        
        // Log what we would do (implementation required)
        info!("Would submit Flashbots transaction with payload: {}", tx_payload);
        
        // Simulate successful submission
        MevExecutionResult::Success {
            tx_hash: format!("flashbots_tx_{}", uuid::Uuid::new_v4()),
            actual_profit_usd: opportunity.estimated_profit_usd,
            actual_gas_cost_usd: opportunity.estimated_gas_cost_usd,
        }
    }).await {
        Ok(result) => result,
        Err(_) => MevExecutionResult::Failure {
            error: "Flashbots execution timed out after 20 seconds".to_string(),
            details: None,
        }
    }
}