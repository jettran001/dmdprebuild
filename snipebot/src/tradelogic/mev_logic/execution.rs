/// MEV execution logic
use std::sync::Arc;
use tracing::{info, warn, error, debug};

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
                from_token,
                to_token,
                opportunity.amount.to_string(),
                opportunity.chain_id,
                &evm_adapter
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
            
            match build_multi_swap_calldata(token_pairs, opportunity.amount.to_string(), opportunity.chain_id, &evm_adapter).await {
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
            match wait_for_transaction_confirmation(&evm_adapter, &front_tx_hash, 3, 60).await {
                Ok(receipt) => {
                    if !receipt.status {
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

/// Xây dựng calldata cho swap
async fn build_swap_calldata(
    from_token: &str,
    to_token: &str,
    amount: String,
    chain_id: u32,
    evm_adapter: &Arc<EvmAdapter>
) -> anyhow::Result<String> {
    // Xác định router address dựa trên chain
    let router_address = match chain_id {
        1 => "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D", // Uniswap V2 Router
        56 => "0x10ED43C718714eb63d5aA57B78B54704E256024E", // PancakeSwap Router
        137 => "0xa5E0829CaCEd8fFDD4De3c43696c57F7D7A678ff", // QuickSwap Router
        _ => return Err(anyhow::anyhow!("Unsupported chain ID for swap"))
    };
    
    // Kiểm tra xem from_token có phải native token không
    let is_native = from_token.to_lowercase() == "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee" || 
                    from_token.to_lowercase() == "0x0000000000000000000000000000000000000000";
    
    // Xây dựng calldata dựa trên loại token
    let calldata = if is_native {
        // Native to Token: swapExactETHForTokens
        format!(
            "0x7ff36ab5000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000800000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000006553f100000000000000000000000000000000000000000000000000000000000000000002000000000000000000000000{}{}", 
            from_token.trim_start_matches("0x"),
            to_token.trim_start_matches("0x")
        )
    } else {
        // Token to Token: swapExactTokensForTokens
        format!(
            "0x38ed1739{}00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000080000000000000000000000000{}0000000000000000000000000000000000000000000000000000000065d8dca0000000000000000000000000000000000000000000000000000000000000000002000000000000000000000000{}{}", 
            pad_uint256(&amount),
            pad_address(&evm_adapter.get_wallet_address().await?),
            from_token.trim_start_matches("0x"),
            to_token.trim_start_matches("0x")
        )
    };
    
    Ok(calldata)
}

/// Xây dựng calldata cho multi-swap
async fn build_multi_swap_calldata(
    token_pairs: &[crate::types::TokenPair],
    amount: String,
    chain_id: u32,
    evm_adapter: &Arc<EvmAdapter>
) -> anyhow::Result<String> {
    // Xác định router address dựa trên chain
    let router_address = match chain_id {
        1 => "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D", // Uniswap V2 Router
        56 => "0x10ED43C718714eb63d5aA57B78B54704E256024E", // PancakeSwap Router
        137 => "0xa5E0829CaCEd8fFDD4De3c43696c57F7D7A678ff", // QuickSwap Router
        _ => return Err(anyhow::anyhow!("Unsupported chain ID for multi-swap"))
    };
    
    // Tạo path tokens từ các token_pairs
    let mut path = Vec::with_capacity(token_pairs.len() + 1);
    path.push(token_pairs[0].token_a.clone());
    
    for pair in token_pairs {
        path.push(pair.token_b.clone());
    }
    
    // Kiểm tra xem token đầu tiên có phải native token không
    let is_native = path[0].to_lowercase() == "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee" || 
                    path[0].to_lowercase() == "0x0000000000000000000000000000000000000000";
    
    // Tạo path cho calldata
    let mut path_hex = String::new();
    for (i, token) in path.iter().enumerate() {
        if i == 0 && is_native {
            // Thay native token bằng wrapped token
            let wrapped_token = match chain_id {
                1 => "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", // WETH
                56 => "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c", // WBNB
                137 => "0x0d500B1d8E8eF31E21C99d1Db9A6444d3ADf1270", // WMATIC
                _ => return Err(anyhow::anyhow!("Unsupported chain ID for wrapped native token"))
            };
            path_hex.push_str(&wrapped_token.trim_start_matches("0x"));
        } else {
            path_hex.push_str(&token.trim_start_matches("0x"));
        }
    }
    
    // Xây dựng calldata dựa trên loại token
    let calldata = if is_native {
        // Native to Token(s): swapExactETHForTokens
        format!(
            "0x7ff36ab5000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000800000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000006553f100000000000000000000000000000000000000000000000000000000000000000{:02x}{}", 
            path.len(),
            path_hex
        )
    } else {
        // Token to Token(s): swapExactTokensForTokens
        format!(
            "0x38ed1739{}00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000080000000000000000000000000{}0000000000000000000000000000000000000000000000000000000065d8dca0000000000000000000000000000000000000000000000000000000000000000{:02x}{}", 
            pad_uint256(&amount),
            pad_address(&evm_adapter.get_wallet_address().await?),
            path.len(),
            path_hex
        )
    };
    
    Ok(calldata)
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
                    let current_block = match evm_adapter.get_current_block_number().await {
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