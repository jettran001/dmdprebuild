//! Utility functions for mempool analysis
//!
//! This module contains helper functions for processing mempool transactions,
//! extracting data, and performing common operations on blockchain transactions.

use sha2::{Sha256, Digest};
use std::time::{SystemTime, UNIX_EPOCH};

/// Hash dữ liệu thành chuỗi hex
///
/// # Parameters
/// - `data`: Dữ liệu cần hash
///
/// # Returns
/// - `String`: Chuỗi hex của hash
pub fn hash_data(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    
    format!("0x{}", hex::encode(result))
}

/// Chuẩn hóa địa chỉ Ethereum
///
/// Chuyển đổi địa chỉ thành chữ thường và đảm bảo có tiền tố 0x
///
/// # Parameters
/// - `address`: Địa chỉ cần chuẩn hóa
///
/// # Returns
/// - `String`: Địa chỉ đã chuẩn hóa
pub fn normalize_address(address: &str) -> String {
    let mut clean_address = address.to_lowercase();
    
    // Ensure 0x prefix
    if !clean_address.starts_with("0x") {
        clean_address = format!("0x{}", clean_address);
    }
    
    // Ensure address has correct length
    if clean_address.len() < 42 {
        // Pad with leading zeros to make it valid
        let padding = 42 - clean_address.len();
        clean_address = format!("0x{}{}", "0".repeat(padding), &clean_address[2..]);
    } else if clean_address.len() > 42 {
        // Truncate to valid length
        clean_address = format!("0x{}", &clean_address[clean_address.len() - 40..]);
    }
    
    clean_address
}

/// Phân tích function signature từ input data
///
/// # Parameters
/// - `input_data`: Dữ liệu đầu vào của giao dịch
///
/// # Returns
/// - `Option<String>`: Function signature nếu có
pub fn parse_function_signature(input_data: &str) -> Option<String> {
    if input_data.len() < 10 || !input_data.starts_with("0x") {
        return None;
    }
    
    Some(input_data[0..10].to_string())
}

/// Phát hiện token address từ input data
///
/// # Parameters
/// - `input_data`: Dữ liệu đầu vào của giao dịch
///
/// # Returns
/// - `Option<String>`: Địa chỉ token nếu tìm thấy
pub fn extract_token_address_from_input(input_data: &str) -> Option<String> {
    // Cần tối thiểu 74 ký tự (0x + 4 byte sig + 32 byte param)
    if input_data.len() < 74 || !input_data.starts_with("0x") {
        return None;
    }
    
    // Hàm transfer(address,uint256) thường có token address ở vị trí 34-74
    // Đây chỉ là một heuristic đơn giản, cần được cải tiến
    let potential_address = format!("0x{}", &input_data[34..74]);
    
    // Kiểm tra có phải địa chỉ hợp lệ không
    if potential_address.len() == 42 && potential_address.starts_with("0x") {
        Some(potential_address)
    } else {
        None
    }
}

/// Tính giá token từ giao dịch swap
///
/// # Parameters
/// - `input_data`: Dữ liệu đầu vào của giao dịch
/// - `value`: Giá trị giao dịch (ETH/BNB)
///
/// # Returns
/// - `Option<f64>`: Giá token nếu có thể tính được
pub fn calculate_token_price_from_swap(input_data: &str, value: f64) -> Option<f64> {
    // Phát hiện từ input data số lượng token
    // Đây chỉ là một placeholder, cần triển khai thực tế
    let token_amount = extract_token_amount(input_data)?;
    
    if token_amount > 0.0 && value > 0.0 {
        Some(value / token_amount)
    } else {
        None
    }
}

/// Trích xuất số lượng token từ input data
///
/// # Parameters
/// - `input_data`: Dữ liệu đầu vào của giao dịch
///
/// # Returns
/// - `Option<f64>`: Số lượng token nếu tìm thấy
pub fn extract_token_amount(input_data: &str) -> Option<f64> {
    // Remove 0x prefix if exists
    let data = if input_data.starts_with("0x") {
        &input_data[2..]
    } else {
        input_data
    };
    
    // Check if data is long enough to contain amount
    if data.len() < 64 {
        return None;
    }
    
    // Extract amount from last 32 bytes (64 hex chars)
    let amount_hex = &data[data.len() - 64..];
    if let Ok(amount) = u128::from_str_radix(amount_hex, 16) {
        Some(amount as f64 / 1e18) // Convert from wei to ETH
    } else {
        None
    }
}

/// Ước tính slippage từ giá trị trong input data
///
/// # Parameters
/// - `input_data`: Dữ liệu đầu vào của giao dịch
///
/// # Returns
/// - `Option<f64>`: Phần trăm slippage nếu có thể tính được
pub fn estimate_slippage_from_input(input_data: &str) -> Option<f64> {
    // Chỉ hoạt động với các hàm swap cụ thể
    if !input_data.starts_with("0x38ed1739") && 
       !input_data.starts_with("0x7ff36ab5") &&
       !input_data.starts_with("0x18cbafe5") {
        return None;
    }
    
    // Placeholder - triển khai thực sẽ parse function parameters
    // Đọc amountIn và amountOutMin, tính % slippage
    None
}

/// Phân tích pool address từ input data
///
/// # Parameters
/// - `input_data`: Dữ liệu đầu vào của giao dịch
///
/// # Returns
/// - `Option<String>`: Địa chỉ pool nếu tìm thấy
pub fn extract_pool_address_from_input(input_data: &str) -> Option<String> {
    // Placeholder - triển khai thực sẽ parse path[] parameter
    None
}

/// Calculate liquidity value from input data and token price
pub fn calculate_liquidity_value(input_data: &str, token_price: f64) -> f64 {
    // Parse input data to get token amount
    if let Some(token_amount) = extract_token_amount(input_data) {
        token_amount * token_price
    } else {
        0.0
    }
}

/// Extract related token address from input data
pub fn extract_related_token(input_data: &str) -> Option<String> {
    // Remove 0x prefix if exists
    let data = if input_data.starts_with("0x") {
        &input_data[2..]
    } else {
        input_data
    };
    
    // Check if data is long enough to contain address
    if data.len() < 40 {
        return None;
    }
    
    // Extract address from data (20 bytes = 40 hex chars)
    let address_hex = &data[data.len() - 40..];
    if address_hex.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("0x{}", address_hex))
    } else {
        None
    }
}

/// Get current timestamp in seconds
pub fn current_time_seconds() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(e) => {
            tracing::warn!("Failed to get current time: {}", e);
            0
        }
    }
}

/// Detect DEX from router address
pub fn detect_dex_from_router(router_address: &str) -> String {
    let router = router_address.to_lowercase();
    
    // Common DEX router addresses
    match router.as_str() {
        "0x10ed43c718714eb63d5aa57b78b54704e256024e" => "PancakeSwap".to_string(), // PancakeSwap V2 Router
        "0x7a250d5630b4cf539739df2c5dacb4c659f2488d" => "Uniswap".to_string(),     // Uniswap V2 Router
        "0x1b02da8cb0d097eb8d57a175b88c7d8b47997506" => "SushiSwap".to_string(),   // SushiSwap Router
        "0xcf0febd3f17cef5b47b0cd257acf6025c5bff3b7" => "ApeSwap".to_string(),     // ApeSwap Router
        "0xd99d1c33f9fc3444f8101754abc46c52416550d1" => "PancakeSwap".to_string(), // PancakeSwap Testnet
        "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45" => "Uniswap".to_string(),     // Uniswap V3 Router
        _ => "Unknown DEX".to_string(),
    }
} 