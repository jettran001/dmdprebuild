use anyhow::{anyhow, Result};
use ethers::prelude::*;
use ethers::types::{Address, U256, H256};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
use tracing::{info, warn, error};

use crate::common::chain_types::DmdChain;
use crate::smartcontracts::TransactionStatus;
use crate::smartcontracts::TokenInterface;

/// Cấu trúc dữ liệu cho cấu hình chung của một blockchain EVM
#[derive(Debug, Clone)]
pub struct EvmContractConfig {
    /// URL của RPC endpoint
    pub rpc_url: String,
    
    /// Địa chỉ của token contract
    pub token_address: String,
    
    /// Chain ID
    pub chain_id: u64,
    
    /// Timeout cho các API call (milliseconds)
    pub timeout_ms: u64,
    
    /// Gas price (gwei)
    pub gas_price: Option<u64>,
    
    /// Gas limit
    pub gas_limit: Option<u64>,
}

impl Default for EvmContractConfig {
    fn default() -> Self {
        Self {
            rpc_url: "".to_string(),
            token_address: "".to_string(),
            chain_id: 1, // Ethereum mainnet mặc định
            timeout_ms: 30000, // 30 giây
            gas_price: None,
            gas_limit: None,
        }
    }
}

/// Hàm chung để kiểm tra trạng thái giao dịch trên blockchain EVM
pub async fn verify_evm_transaction_status(tx_hash: &str, rpc_url: &str) -> Result<TransactionStatus> {
    // Kiểm tra hash giao dịch
    if !tx_hash.starts_with("0x") {
        return Err(anyhow!("Invalid transaction hash format: {}", tx_hash));
    }
    
    // Tạo provider từ RPC URL
    let provider = Provider::<Http>::try_from(rpc_url)?;
    
    // Parse transaction hash
    let hash = H256::from_str(tx_hash)?;
    
    // Kiểm tra trạng thái giao dịch
    match provider.get_transaction_receipt(hash).await? {
        Some(receipt) => {
            // Kiểm tra xem transaction đã thành công hay chưa
            match receipt.status {
                Some(status) if status == U64::from(1) => Ok(TransactionStatus::Confirmed),
                Some(_) => Ok(TransactionStatus::Failed),
                None => Ok(TransactionStatus::Pending),
            }
        },
        None => {
            // Kiểm tra xem transaction có tồn tại không
            match provider.get_transaction(hash).await? {
                Some(_) => Ok(TransactionStatus::Pending),
                None => Ok(TransactionStatus::NotFound),
            }
        }
    }
}

/// Hàm chung để chuyển đổi private key thành địa chỉ công khai
pub fn private_key_to_public_address(private_key: &str) -> Result<String> {
    // Loại bỏ tiền tố 0x nếu có
    let private_key = private_key.trim_start_matches("0x");
    
    // Tạo ví từ private key
    let wallet = LocalWallet::from_str(private_key)?;
    
    // Lấy địa chỉ
    let address = wallet.address();
    
    Ok(format!("0x{:x}", address))
}

/// Hàm che dấu private key cho mục đích logging
pub fn mask_private_key(private_key: &str) -> String {
    let key = private_key.trim_start_matches("0x");
    if key.len() <= 8 {
        return "[masked_key]".to_string();
    }
    
    let prefix = &key[0..4];
    let suffix = &key[key.len() - 4..];
    format!("0x{}....{}", prefix, suffix)
}

/// Hàm kiểm tra tính hợp lệ của địa chỉ Ethereum
pub fn is_valid_eth_address(address: &str) -> bool {
    if !address.starts_with("0x") || address.len() != 42 {
        return false;
    }
    
    // Kiểm tra xem các ký tự còn lại có phải là hex không
    address[2..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Hàm chuyển đổi số dư từ U256 sang f64
pub fn convert_balance_to_decimal(balance: U256, decimals: u8) -> Result<f64> {
    // Chuyển đổi U256 sang f64
    let balance_str = balance.to_string();
    let balance_f64 = balance_str.parse::<f64>().map_err(|e| {
        anyhow!("Failed to parse balance '{}' to f64: {}", balance_str, e)
    })?;
    
    // Chia cho 10^decimals
    let divisor = 10_f64.powi(decimals as i32);
    Ok(balance_f64 / divisor)
} 