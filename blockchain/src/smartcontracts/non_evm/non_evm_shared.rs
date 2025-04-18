use anyhow::{anyhow, Result};
use ethers::types::U256;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;
use async_trait::async_trait;
use tracing::{info, warn, error};

use crate::common::chain_types::DmdChain;
use crate::smartcontracts::TransactionStatus;
use crate::smartcontracts::TokenInterface;

/// Cấu trúc dữ liệu cho cấu hình chung của một blockchain Non-EVM
#[derive(Debug, Clone)]
pub struct NonEvmContractConfig {
    /// URL của RPC endpoint
    pub rpc_url: String,
    
    /// Địa chỉ của token contract
    pub token_address: String,
    
    /// Network ID
    pub network_id: String,
    
    /// Timeout cho các API call (milliseconds)
    pub timeout_ms: u64,
    
    /// Cấu hình đặc biệt dành riêng cho từng loại chain
    pub special_config: HashMap<String, String>,
}

impl Default for NonEvmContractConfig {
    fn default() -> Self {
        Self {
            rpc_url: "".to_string(),
            token_address: "".to_string(),
            network_id: "mainnet".to_string(),
            timeout_ms: 30000, // 30 giây
            special_config: HashMap::new(),
        }
    }
}

/// Hàm che dấu private key cho mục đích logging
pub fn mask_private_key(private_key: &str) -> String {
    let key = private_key.trim_start_matches("0x");
    if key.len() <= 8 {
        return "[masked_key]".to_string();
    }
    
    let prefix = &key[0..4];
    let suffix = &key[key.len() - 4..];
    format!("{}....{}", prefix, suffix)
}

/// Hàm kiểm tra tính hợp lệ của địa chỉ Solana
pub fn is_valid_solana_address(address: &str) -> bool {
    // Kiểm tra độ dài địa chỉ Solana
    if address.len() < 32 || address.len() > 44 {
        return false;
    }
    
    // Kiểm tra xem địa chỉ có phải là Base58 không
    const BASE58_CHARS: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    address.chars().all(|c| BASE58_CHARS.contains(c))
}

/// Hàm kiểm tra tính hợp lệ của địa chỉ NEAR
pub fn is_valid_near_address(address: &str) -> bool {
    // Kiểm tra định dạng địa chỉ NEAR (example.near hoặc example.testnet)
    address.ends_with(".near") || address.ends_with(".testnet") || !address.contains(".")
}

/// Hàm kiểm tra tính hợp lệ của địa chỉ TON
pub fn is_valid_ton_address(address: &str) -> bool {
    // Kiểm tra định dạng địa chỉ TON
    if !address.starts_with("EQ") && !address.starts_with("UQ") {
        return false;
    }
    
    // Kiểm tra độ dài và ký tự hợp lệ
    if address.len() != 48 {
        return false;
    }
    
    // Kiểm tra xem địa chỉ có chứa Base64 URL không
    const BASE64_URL_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    address[2..].chars().all(|c| BASE64_URL_CHARS.contains(c))
}

/// Hàm chuyển đổi số dư từ U256 sang f64 cho các blockchain Non-EVM
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