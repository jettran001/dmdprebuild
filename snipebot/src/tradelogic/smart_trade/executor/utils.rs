//! Utils - Các tiện ích phụ trợ
//!
//! Module này chứa các hàm tiện ích được sử dụng bởi các module khác
//! trong executor.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use chrono::Utc;
use rand::{thread_rng, Rng};
use anyhow::{Result, anyhow};
use tokio::time::sleep;
use tracing::{debug, warn};

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::{TokenPair, TradeType};

/// Tạo ID ngẫu nhiên
pub fn generate_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Lấy timestamp hiện tại (Unix epoch)
pub fn get_current_timestamp() -> u64 {
    Utc::now().timestamp() as u64
}

/// Tính số lượng token từ giá trị USD
pub async fn calculate_token_amount_from_usd(
    adapter: &Arc<EvmAdapter>,
    token_address: &str,
    usd_amount: f64,
) -> Result<f64> {
    // Lấy giá token
    let token_price = adapter.get_token_price(token_address).await?;
    
    if token_price <= 0.0 {
        return Err(anyhow!("Invalid token price: {}", token_price));
    }
    
    // Tính số lượng token
    let token_amount = usd_amount / token_price;
    
    Ok(token_amount)
}

/// Tính giá trị USD từ số lượng token
pub async fn calculate_usd_value_from_token(
    adapter: &Arc<EvmAdapter>,
    token_address: &str,
    token_amount: f64,
) -> Result<f64> {
    // Lấy giá token
    let token_price = adapter.get_token_price(token_address).await?;
    
    // Tính giá trị USD
    let usd_value = token_amount * token_price;
    
    Ok(usd_value)
}

/// Kiểm tra nếu giá có chênh lệch quá lớn
pub fn is_price_deviation_too_large(
    expected_price: f64,
    actual_price: f64,
    max_deviation_percent: f64,
) -> bool {
    if expected_price <= 0.0 {
        return false;
    }
    
    let deviation = ((actual_price - expected_price) / expected_price).abs() * 100.0;
    deviation > max_deviation_percent
}

/// Tính giá trị trung bình của danh sách giá
pub fn calculate_average_price(prices: &[f64]) -> f64 {
    if prices.is_empty() {
        return 0.0;
    }
    
    let sum: f64 = prices.iter().sum();
    sum / prices.len() as f64
}

/// Tính slippage từ giá mua/bán
pub fn calculate_slippage(
    expected_price: f64,
    actual_price: f64,
) -> f64 {
    if expected_price <= 0.0 {
        return 0.0;
    }
    
    ((actual_price - expected_price) / expected_price).abs() * 100.0
}

/// Tính toán giá trị slippage tối đa an toàn dựa trên thanh khoản
pub fn calculate_safe_slippage(liquidity_usd: f64, trade_amount_usd: f64) -> f64 {
    if liquidity_usd <= 0.0 || trade_amount_usd <= 0.0 {
        return 0.5; // Default slippage 0.5%
    }
    
    // Tính toán slippage dựa trên tỷ lệ trade_amount / liquidity
    let ratio = trade_amount_usd / liquidity_usd;
    
    // Công thức đơn giản: slippage tăng khi ratio tăng
    let base_slippage = 0.5; // Slippage cơ bản 0.5%
    let slippage = base_slippage + (ratio * 10.0); // Tăng 1% cho mỗi 10% liquidity
    
    // Giới hạn trong khoảng hợp lý
    slippage.max(0.5).min(5.0) // Min 0.5%, max 5%
}

/// Tính toán gas price hợp lý dựa trên mức độ ưu tiên
pub async fn calculate_gas_price(
    adapter: &Arc<EvmAdapter>,
    priority: &str,
) -> Result<u64> {
    // Lấy gas price từ adapter
    let base_gas_price = adapter.get_gas_price().await?;
    
    // Chọn gas price dựa trên mức độ ưu tiên
    let gas_price = match priority {
        "high" => base_gas_price * 15 / 10,  // 1.5x
        "medium" => base_gas_price,          // 1.0x
        "low" => base_gas_price * 8 / 10,    // 0.8x
        _ => base_gas_price,
    };
    
    Ok(gas_price)
}

/// Tạo thông điệp nhật ký (log) từ thông tin giao dịch
pub fn create_trade_log_message(
    trade_id: &str,
    status: &str,
    token_pair: &TokenPair,
    amount: f64,
    price: Option<f64>,
) -> String {
    let price_str = match price {
        Some(p) => format!("at price ${:.6}", p),
        None => "".to_string(),
    };
    
    format!(
        "Trade {} {} - {} {} {} {}",
        trade_id,
        status,
        amount,
        token_pair.token_address,
        if price.is_some() { price_str } else { "".to_string() },
        Utc::now().format("%Y-%m-%d %H:%M:%S")
    )
}

/// Định dạng số thành chuỗi với số lẻ phù hợp
pub fn format_number(value: f64, decimals: usize) -> String {
    format!("{:.*}", decimals, value)
}

/// Kiểm tra nếu token có thanh khoản đủ cho giao dịch
pub async fn has_sufficient_liquidity(
    adapter: &Arc<EvmAdapter>,
    token_address: &str,
    trade_amount_usd: f64,
    min_liquidity_ratio: f64,
) -> Result<bool> {
    // Lấy thông tin thị trường
    let market_info = adapter.get_market_info(token_address).await?;
    
    // Tính tỷ lệ giao dịch/thanh khoản
    let ratio = if market_info.liquidity_usd > 0.0 {
        trade_amount_usd / market_info.liquidity_usd
    } else {
        f64::INFINITY
    };
    
    // Thanh khoản đủ nếu tỷ lệ nhỏ hơn ngưỡng
    Ok(ratio <= min_liquidity_ratio)
}

/// Phân tích mẫu giao dịch và phát hiện anomaly
pub fn detect_trading_pattern_anomaly(
    prices: &[(u64, f64)],
    volumes: &[(u64, f64)],
    threshold: f64,
) -> bool {
    if prices.len() < 10 || volumes.len() < 10 {
        return false;
    }
    
    // Tính toán biến động giá
    let mut price_changes = Vec::new();
    for i in 1..prices.len() {
        let (_, prev_price) = prices[i - 1];
        let (_, curr_price) = prices[i];
        
        if prev_price > 0.0 {
            let change = (curr_price - prev_price) / prev_price;
            price_changes.push(change);
        }
    }
    
    // Tính toán biến động khối lượng
    let mut volume_changes = Vec::new();
    for i in 1..volumes.len() {
        let (_, prev_vol) = volumes[i - 1];
        let (_, curr_vol) = volumes[i];
        
        if prev_vol > 0.0 {
            let change = (curr_vol - prev_vol) / prev_vol;
            volume_changes.push(change);
        }
    }
    
    // Tính standard deviation của biến động giá và khối lượng
    let price_std_dev = calculate_standard_deviation(&price_changes);
    let volume_std_dev = calculate_standard_deviation(&volume_changes);
    
    // Phát hiện anomaly nếu biến động quá lớn
    price_std_dev > threshold || volume_std_dev > threshold
}

/// Tính độ lệch chuẩn (standard deviation)
fn calculate_standard_deviation(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    
    // Tính giá trị trung bình
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    
    // Tính variance
    let variance = values.iter()
        .map(|&x| (x - mean).powi(2))
        .sum::<f64>() / values.len() as f64;
    
    // Standard deviation là căn bậc hai của variance
    variance.sqrt()
}

/// Tìm giá trị tối ưu cho chiến lược giao dịch
pub fn optimize_strategy_parameter(
    values: &[f64],
    target_function: fn(f64) -> f64,
    min_value: f64,
    max_value: f64,
    iterations: usize,
) -> f64 {
    if values.is_empty() {
        return (min_value + max_value) / 2.0;
    }
    
    let mut best_value = min_value;
    let mut best_score = target_function(min_value);
    
    let step = (max_value - min_value) / iterations as f64;
    
    for i in 1..=iterations {
        let current_value = min_value + (i as f64 * step);
        let current_score = target_function(current_value);
        
        if current_score > best_score {
            best_score = current_score;
            best_value = current_value;
        }
    }
    
    best_value
}

/// Enum đại diện cho các phương thức swap khác nhau mà hàm decode có thể xử lý
#[derive(Debug, Clone, PartialEq)]
pub enum SwapMethod {
    /// Uniswap V2 swapExactTokensForTokens
    UniswapV2ExactTokensForTokens,
    /// Uniswap V2 swapTokensForExactTokens
    UniswapV2TokensForExactTokens,
    /// Uniswap V2 swapExactETHForTokens
    UniswapV2ExactETHForTokens,
    /// Uniswap V2 swapTokensForExactETH
    UniswapV2TokensForExactETH,
    /// Uniswap V3 exactInputSingle
    UniswapV3ExactInputSingle,
    /// Uniswap V3 exactInput (path)
    UniswapV3ExactInput,
    /// PancakeSwap V2 swapExactTokensForTokens
    PancakeSwapV2ExactTokensForTokens,
    /// SushiSwap swapExactTokensForTokens
    SushiSwapExactTokensForTokens,
    /// Generic method (không xác định cụ thể)
    Unknown,
}

/// Kết quả decode dữ liệu swap
#[derive(Debug, Clone)]
pub struct SwapData {
    /// Địa chỉ token đầu vào
    pub token_in: String,
    /// Địa chỉ token đầu ra
    pub token_out: String,
    /// Số lượng token (đầu vào hoặc đầu ra tùy method)
    pub amount: f64,
    /// Số lượng token tối thiểu nhận được (nếu có)
    pub amount_min_out: Option<f64>,
    /// Phương thức swap được sử dụng
    pub method: SwapMethod,
    /// Đường dẫn swap (nếu có)
    pub path: Option<Vec<String>>,
    /// Deadline giao dịch (timestamp, nếu có)
    pub deadline: Option<u64>,
    /// Dữ liệu gốc để tham khảo nếu cần
    pub raw_data: Vec<u8>,
}

/// Giải mã dữ liệu giao dịch swap từ calldata
/// 
/// Hỗ trợ nhiều định dạng dữ liệu swap phổ biến từ các DEX:
/// - Uniswap V2/V3
/// - PancakeSwap
/// - SushiSwap
/// và các fork khác
pub fn decode_swap_data(data: &[u8]) -> Result<SwapData> {
    // Kiểm tra độ dài tối thiểu (ít nhất phải có 4 byte function selector)
    if data.len() < 4 {
        return Err(anyhow!("Invalid swap data: too short (< 4 bytes)"));
    }
    
    // Lấy function selector (4 byte đầu tiên)
    let selector = &data[0..4];
    
    // Chuyển selector thành hex để so sánh
    let selector_hex = hex::encode(selector);
    debug!("Function selector: 0x{}", selector_hex);
    
    // Xác định phương thức dựa trên selector
    let method = match selector_hex.as_str() {
        // Uniswap V2 swapExactTokensForTokens
        "38ed1739" => SwapMethod::UniswapV2ExactTokensForTokens,
        // Uniswap V2 swapTokensForExactTokens
        "8803dbee" => SwapMethod::UniswapV2TokensForExactTokens,
        // Uniswap V2 swapExactETHForTokens
        "7ff36ab5" => SwapMethod::UniswapV2ExactETHForTokens,
        // Uniswap V2 swapTokensForExactETH
        "4a25d94a" => SwapMethod::UniswapV2TokensForExactETH,
        // Uniswap V3 exactInputSingle
        "414bf389" => SwapMethod::UniswapV3ExactInputSingle,
        // Uniswap V3 exactInput (path)
        "c04b8d59" => SwapMethod::UniswapV3ExactInput,
        // PancakeSwap swapExactTokensForTokens (cùng selector với Uniswap V2)
        "bca0b292" => SwapMethod::PancakeSwapV2ExactTokensForTokens,
        // SushiSwap swapExactTokensForTokens (cùng selector với Uniswap V2)
        _ => {
            debug!("Unknown function selector: 0x{}", selector_hex);
            SwapMethod::Unknown
        }
    };
    
    // Giải mã dữ liệu dựa trên phương thức
    match method {
        SwapMethod::UniswapV2ExactTokensForTokens | 
        SwapMethod::PancakeSwapV2ExactTokensForTokens | 
        SwapMethod::SushiSwapExactTokensForTokens => {
            decode_uniswap_v2_exact_tokens_for_tokens(data, method)
        },
        SwapMethod::UniswapV2ExactETHForTokens => {
            decode_uniswap_v2_exact_eth_for_tokens(data, method)
        },
        SwapMethod::UniswapV3ExactInputSingle => {
            decode_uniswap_v3_exact_input_single(data, method)
        },
        SwapMethod::UniswapV3ExactInput => {
            decode_uniswap_v3_exact_input(data, method)
        },
        _ => {
            // Fallback: Cố gắng giải mã theo cấu trúc phổ biến nhất
            debug!("Using fallback decoding for unknown method");
            decode_generic_swap(data)
        }
    }
}

/// Giải mã dữ liệu Uniswap V2 swapExactTokensForTokens
/// Format: swapExactTokensForTokens(uint amountIn, uint amountOutMin, address[] path, address to, uint deadline)
fn decode_uniswap_v2_exact_tokens_for_tokens(data: &[u8], method: SwapMethod) -> Result<SwapData> {
    // Kiểm tra độ dài tối thiểu
    if data.len() < 4 + 32 * 5 {
        return Err(anyhow!("Invalid Uniswap V2 swap data: too short"));
    }
    
    // 4 byte đầu là function selector, sau đó là các tham số 32 byte
    let amount_in_offset = 4;
    let amount_out_min_offset = 4 + 32;
    let path_offset_bytes = &data[4 + 32 * 2..4 + 32 * 3];
    let path_offset = u256_from_bytes(path_offset_bytes)? as usize;
    
    // Đọc amount
    let amount_in = u256_from_bytes(&data[amount_in_offset..amount_in_offset + 32])? as f64;
    let amount_out_min = u256_from_bytes(&data[amount_out_min_offset..amount_out_min_offset + 32])? as f64;
    
    // Đọc path
    let path = decode_address_array(data, path_offset)?;
    if path.len() < 2 {
        return Err(anyhow!("Invalid path length: must have at least 2 tokens"));
    }
    
    // Lấy token đầu vào và đầu ra từ path
    let token_in = path.first().unwrap().clone();
    let token_out = path.last().unwrap().clone();
    
    // Đọc deadline nếu có đủ dữ liệu
    let deadline = if data.len() >= 4 + 32 * 5 {
        let deadline_offset = 4 + 32 * 4;
        Some(u256_from_bytes(&data[deadline_offset..deadline_offset + 32])? as u64)
    } else {
        None
    };
    
    Ok(SwapData {
        token_in,
        token_out,
        amount: amount_in,
        amount_min_out: Some(amount_out_min),
        method,
        path: Some(path),
        deadline,
        raw_data: data.to_vec(),
    })
}

/// Giải mã dữ liệu Uniswap V2 swapExactETHForTokens
/// Format: swapExactETHForTokens(uint amountOutMin, address[] path, address to, uint deadline)
fn decode_uniswap_v2_exact_eth_for_tokens(data: &[u8], method: SwapMethod) -> Result<SwapData> {
    // Kiểm tra độ dài tối thiểu
    if data.len() < 4 + 32 * 4 {
        return Err(anyhow!("Invalid Uniswap V2 ETH swap data: too short"));
    }
    
    // 4 byte đầu là function selector, sau đó là các tham số 32 byte
    let amount_out_min_offset = 4;
    let path_offset_bytes = &data[4 + 32..4 + 32 * 2];
    let path_offset = u256_from_bytes(path_offset_bytes)? as usize;
    
    // Đọc amount_out_min
    let amount_out_min = u256_from_bytes(&data[amount_out_min_offset..amount_out_min_offset + 32])? as f64;
    
    // Đọc path
    let path = decode_address_array(data, path_offset)?;
    if path.len() < 2 {
        return Err(anyhow!("Invalid path length: must have at least 2 tokens"));
    }
    
    // Trong trường hợp ETH, token đầu vào là WETH
    let token_in = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string(); // WETH
    let token_out = path.last().unwrap().clone();
    
    // Đọc deadline nếu có đủ dữ liệu
    let deadline = if data.len() >= 4 + 32 * 4 {
        let deadline_offset = 4 + 32 * 3;
        Some(u256_from_bytes(&data[deadline_offset..deadline_offset + 32])? as u64)
    } else {
        None
    };
    
    Ok(SwapData {
        token_in,
        token_out,
        amount: 0.0, // Không có trong calldata, được gửi qua msg.value
        amount_min_out: Some(amount_out_min),
        method,
        path: Some(path),
        deadline,
        raw_data: data.to_vec(),
    })
}

/// Giải mã dữ liệu Uniswap V3 exactInputSingle
/// Format: exactInputSingle((address tokenIn, address tokenOut, uint24 fee, address recipient, uint256 deadline, uint256 amountIn, uint256 amountOutMinimum, uint160 sqrtPriceLimitX96))
fn decode_uniswap_v3_exact_input_single(data: &[u8], method: SwapMethod) -> Result<SwapData> {
    // Kiểm tra độ dài tối thiểu
    if data.len() < 4 + 32 * 7 {
        return Err(anyhow!("Invalid Uniswap V3 exactInputSingle data: too short"));
    }
    
    // 4 byte đầu là function selector, sau đó là struct params
    let params_offset = 4;
    
    // Đọc tokenIn và tokenOut (địa chỉ ethereum 20 bytes)
    let token_in_offset = params_offset;
    let token_out_offset = params_offset + 32;
    
    // Kiểm tra độ dài dữ liệu trước khi truy cập
    if token_in_offset + 32 > data.len() || token_out_offset + 32 > data.len() {
        return Err(anyhow!("Invalid data length for token addresses in Uniswap V3 exactInputSingle"));
    }

    let token_in = format!("0x{}", hex::encode(&data[token_in_offset + 12..token_in_offset + 32]));
    let token_out = format!("0x{}", hex::encode(&data[token_out_offset + 12..token_out_offset + 32]));
    
    // Đọc amountIn và amountOutMinimum
    let amount_in_offset = params_offset + 32 * 5;
    let amount_out_min_offset = params_offset + 32 * 6;
    
    // Kiểm tra độ dài dữ liệu
    if amount_in_offset + 32 > data.len() || amount_out_min_offset + 32 > data.len() {
        return Err(anyhow!("Invalid data length for amounts in Uniswap V3 exactInputSingle"));
    }

    let amount_in = match u256_from_bytes(&data[amount_in_offset..amount_in_offset + 32]) {
        Ok(val) => val as f64,
        Err(e) => {
            warn!("Error parsing amount_in in Uniswap V3 exactInputSingle: {}", e);
            return Err(anyhow!("Failed to parse amount_in: {}", e));
        }
    };

    let amount_out_min = match u256_from_bytes(&data[amount_out_min_offset..amount_out_min_offset + 32]) {
        Ok(val) => val as f64,
        Err(e) => {
            warn!("Error parsing amount_out_min in Uniswap V3 exactInputSingle: {}", e);
            return Err(anyhow!("Failed to parse amount_out_min: {}", e));
        }
    };
    
    // Đọc deadline
    let deadline_offset = params_offset + 32 * 4;
    
    // Kiểm tra độ dài dữ liệu
    if deadline_offset + 32 > data.len() {
        return Err(anyhow!("Invalid data length for deadline in Uniswap V3 exactInputSingle"));
    }

    let deadline = match u256_from_bytes(&data[deadline_offset..deadline_offset + 32]) {
        Ok(val) => val as u64,
        Err(e) => {
            warn!("Error parsing deadline in Uniswap V3 exactInputSingle: {}", e);
            return Err(anyhow!("Failed to parse deadline: {}", e));
        }
    };
    
    Ok(SwapData {
        token_in,
        token_out,
        amount: amount_in,
        amount_min_out: Some(amount_out_min),
        method,
        path: Some(vec![token_in.clone(), token_out.clone()]),
        deadline: Some(deadline),
        raw_data: data.to_vec(),
    })
}

/// Giải mã dữ liệu Uniswap V3 exactInput (với path)
/// Format: exactInput((bytes path, address recipient, uint256 deadline, uint256 amountIn, uint256 amountOutMinimum))
fn decode_uniswap_v3_exact_input(data: &[u8], method: SwapMethod) -> Result<SwapData> {
    // Kiểm tra độ dài tối thiểu
    if data.len() < 4 + 32 * 5 {
        return Err(anyhow!("Invalid Uniswap V3 exactInput data: too short"));
    }
    
    // 4 byte đầu là function selector, sau đó là struct params
    let params_offset = 4;
    
    // Đọc path
    if params_offset + 32 > data.len() {
        return Err(anyhow!("Invalid data length for path offset in Uniswap V3 exactInput"));
    }

    let path_offset_bytes = &data[params_offset..params_offset + 32];
    let path_offset = match u256_from_bytes(path_offset_bytes) {
        Ok(val) => val as usize,
        Err(e) => {
            warn!("Error parsing path_offset in Uniswap V3 exactInput: {}", e);
            return Err(anyhow!("Failed to parse path_offset: {}", e));
        }
    };
    
    // Đọc amountIn và amountOutMinimum
    let amount_in_offset = params_offset + 32 * 3;
    let amount_out_min_offset = params_offset + 32 * 4;
    
    // Kiểm tra độ dài dữ liệu
    if amount_in_offset + 32 > data.len() || amount_out_min_offset + 32 > data.len() {
        return Err(anyhow!("Invalid data length for amounts in Uniswap V3 exactInput"));
    }

    let amount_in = match u256_from_bytes(&data[amount_in_offset..amount_in_offset + 32]) {
        Ok(val) => val as f64,
        Err(e) => {
            warn!("Error parsing amount_in in Uniswap V3 exactInput: {}", e);
            return Err(anyhow!("Failed to parse amount_in: {}", e));
        }
    };

    let amount_out_min = match u256_from_bytes(&data[amount_out_min_offset..amount_out_min_offset + 32]) {
        Ok(val) => val as f64,
        Err(e) => {
            warn!("Error parsing amount_out_min in Uniswap V3 exactInput: {}", e);
            return Err(anyhow!("Failed to parse amount_out_min: {}", e));
        }
    };
    
    // Đọc deadline
    let deadline_offset = params_offset + 32 * 2;
    
    // Kiểm tra độ dài dữ liệu
    if deadline_offset + 32 > data.len() {
        return Err(anyhow!("Invalid data length for deadline in Uniswap V3 exactInput"));
    }

    let deadline = match u256_from_bytes(&data[deadline_offset..deadline_offset + 32]) {
        Ok(val) => val as u64,
        Err(e) => {
            warn!("Error parsing deadline in Uniswap V3 exactInput: {}", e);
            return Err(anyhow!("Failed to parse deadline: {}", e));
        }
    };
    
    // Đọc path bytes
    let path_data = match decode_bytes(data, path_offset) {
        Ok(data) => data,
        Err(e) => {
            warn!("Error decoding path bytes in Uniswap V3 exactInput: {}", e);
            return Err(anyhow!("Failed to decode path bytes: {}", e));
        }
    };

    let mut path = Vec::new();
    
    // Trong Uniswap V3, path có định dạng: [token0, fee, token1, fee, token2, ...]
    let mut i = 0;
    while i < path_data.len() {
        if i + 20 > path_data.len() {
            break;
        }
        
        let token = format!("0x{}", hex::encode(&path_data[i..i + 20]));
        path.push(token);
        
        // Skip token address (20 bytes) và fee (3 bytes)
        i += 23;
    }
    
    if path.is_empty() {
        return Err(anyhow!("Failed to decode path from Uniswap V3 exactInput: path is empty"));
    }
    
    // Token đầu vào và đầu ra từ path
    let token_in = path.first()
        .ok_or_else(|| anyhow!("Path contains no tokens"))?
        .clone();
    let token_out = path.last()
        .ok_or_else(|| anyhow!("Path contains no tokens"))?
        .clone();
    
    Ok(SwapData {
        token_in,
        token_out,
        amount: amount_in,
        amount_min_out: Some(amount_out_min),
        method,
        path: Some(path),
        deadline: Some(deadline),
        raw_data: data.to_vec(),
    })
}

/// Giải mã dữ liệu swap theo cấu trúc chung (generic fallback)
fn decode_generic_swap(data: &[u8]) -> Result<SwapData> {
    // Kiểm tra độ dài tối thiểu
    if data.len() < 4 + 32 * 3 {
        return Err(anyhow!("Invalid swap data length for generic decoding"));
    }
    
    // Thử đọc các tham số phổ biến nhất
    let token_in_offset = 4;
    let token_out_offset = 4 + 32;
    let amount_offset = 4 + 32 * 2;
    
    // Đọc địa chỉ token (ethereum addresses are 20 bytes, padded to 32 bytes)
    let token_in = format!("0x{}", hex::encode(&data[token_in_offset + 12..token_in_offset + 32]));
    let token_out = format!("0x{}", hex::encode(&data[token_out_offset + 12..token_out_offset + 32]));
    
    // Đọc số lượng
    let amount = match u256_from_bytes(&data[amount_offset..amount_offset + 32]) {
        Ok(val) => val as f64,
        Err(_) => {
            // Nếu không đọc được amount, thử đọc từ các vị trí khác
            if data.len() >= 4 + 32 * 4 {
                match u256_from_bytes(&data[4 + 32 * 3..4 + 32 * 4]) {
                    Ok(val) => val as f64,
                    Err(_) => 0.0, // Fallback
                }
            } else {
                0.0 // Fallback
            }
        }
    };
    
    // Kiểm tra xem địa chỉ token có hợp lệ không
    if !is_valid_ethereum_address(&token_in) || !is_valid_ethereum_address(&token_out) {
        warn!("Invalid token addresses detected in generic swap decoding");
        warn!("token_in: {}, token_out: {}", token_in, token_out);
        
        // Thử tìm địa chỉ ethereum trong dữ liệu
        let mut found_addresses = Vec::new();
        for i in 0..data.len() - 20 {
            if is_valid_ethereum_address_bytes(&data[i..i + 20]) {
                found_addresses.push(format!("0x{}", hex::encode(&data[i..i + 20])));
            }
        }
        
        debug!("Found potential ethereum addresses in data: {:?}", found_addresses);
        
        if found_addresses.len() >= 2 {
            return Ok(SwapData {
                token_in: found_addresses[0].clone(),
                token_out: found_addresses[1].clone(),
                amount,
                amount_min_out: None,
                method: SwapMethod::Unknown,
                path: Some(found_addresses),
                deadline: None,
                raw_data: data.to_vec(),
            });
        }
        
        // Nếu không tìm thấy đủ địa chỉ, trả về lỗi
        return Err(anyhow!("Failed to decode swap data: could not identify valid token addresses"));
    }
    
    Ok(SwapData {
        token_in,
        token_out,
        amount,
        amount_min_out: None,
        method: SwapMethod::Unknown,
        path: Some(vec![token_in.clone(), token_out.clone()]),
        deadline: None,
        raw_data: data.to_vec(),
    })
}

/// Chuyển đổi 32 bytes thành u256
fn u256_from_bytes(bytes: &[u8]) -> Result<u128> {
    if bytes.len() != 32 {
        return Err(anyhow!("Invalid byte length for u256: expected 32, got {}", bytes.len()));
    }
    
    // Lấy 16 byte cuối (128 bit) để chuyển thành u128
    // Giả định rằng giá trị không vượt quá u128::MAX
    let mut amount_bytes = [0u8; 16];
    amount_bytes.copy_from_slice(&bytes[16..32]);
    
    Ok(u128::from_be_bytes(amount_bytes))
}

/// Giải mã mảng địa chỉ từ calldata
fn decode_address_array(data: &[u8], offset: usize) -> Result<Vec<String>> {
    if offset + 32 > data.len() {
        return Err(anyhow!("Invalid offset for address array"));
    }
    
    // Đọc độ dài mảng
    let array_length_bytes = &data[offset..offset + 32];
    let array_length = u256_from_bytes(array_length_bytes)? as usize;
    
    let mut addresses = Vec::new();
    for i in 0..array_length {
        let addr_offset = offset + 32 + i * 32;
        if addr_offset + 32 > data.len() {
            return Err(anyhow!("Invalid address offset in array"));
        }
        
        // Địa chỉ ethereum là 20 bytes, được padding thành 32 bytes trong ABI
        let address = format!("0x{}", hex::encode(&data[addr_offset + 12..addr_offset + 32]));
        addresses.push(address);
    }
    
    Ok(addresses)
}

/// Giải mã bytes từ calldata
fn decode_bytes(data: &[u8], offset: usize) -> Result<Vec<u8>> {
    if offset + 32 > data.len() {
        return Err(anyhow!("Invalid offset for bytes"));
    }
    
    // Đọc độ dài bytes
    let bytes_length_bytes = &data[offset..offset + 32];
    let bytes_length = u256_from_bytes(bytes_length_bytes)? as usize;
    
    let bytes_data_offset = offset + 32;
    if bytes_data_offset + bytes_length > data.len() {
        return Err(anyhow!("Invalid bytes length: out of bounds"));
    }
    
    let bytes_data = data[bytes_data_offset..bytes_data_offset + bytes_length].to_vec();
    Ok(bytes_data)
}

/// Kiểm tra xem chuỗi có phải là địa chỉ ethereum hợp lệ không
fn is_valid_ethereum_address(address: &str) -> bool {
    // Địa chỉ ethereum phải bắt đầu bằng 0x và có 42 ký tự (2 cho 0x + 40 cho hex)
    if !address.starts_with("0x") || address.len() != 42 {
        return false;
    }
    
    // Kiểm tra xem tất cả ký tự còn lại có phải hex không
    address[2..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Kiểm tra xem byte array có phải là địa chỉ ethereum hợp lệ không
fn is_valid_ethereum_address_bytes(bytes: &[u8]) -> bool {
    // Địa chỉ ethereum có 20 bytes
    if bytes.len() != 20 {
        return false;
    }
    
    // Kiểm tra xem địa chỉ có phải là địa chỉ 0 không
    let all_zeros = bytes.iter().all(|&b| b == 0);
    if all_zeros {
        return false;
    }
    
    true
}

/// Tính hash của token pair để dùng làm key trong cache
pub fn calculate_token_pair_hash(token_pair: &TokenPair) -> String {
    format!("{}-{}", token_pair.base_token_address, token_pair.token_address)
}

/// Mã hóa tham số cho API request
pub fn encode_api_parameters(params: &HashMap<String, String>) -> String {
    let mut pairs = Vec::new();
    
    for (key, value) in params {
        pairs.push(format!("{}={}", key, urlencoding::encode(value)));
    }
    
    pairs.join("&")
}

/// Rate Limiter để kiểm soát tần suất gọi API
pub struct RateLimiter {
    /// Thời gian giữa các lần gọi API (ms)
    interval_ms: u64,
    
    /// Thời điểm gọi API cuối cùng cho mỗi endpoint
    last_call_time: HashMap<String, Instant>,
    
    /// Giới hạn số lượng gọi API trong một khoảng thời gian
    rate_limit: Option<(u32, Duration)>, // (số lượng tối đa, khoảng thời gian)
    
    /// Số lượng gọi API đã thực hiện trong khoảng thời gian hiện tại
    call_count: HashMap<String, Vec<Instant>>,
}

impl RateLimiter {
    /// Tạo rate limiter mới
    pub fn new(interval_ms: u64) -> Self {
        Self {
            interval_ms,
            last_call_time: HashMap::new(),
            rate_limit: None,
            call_count: HashMap::new(),
        }
    }
    
    /// Tạo rate limiter với giới hạn số lượng gọi
    pub fn with_rate_limit(interval_ms: u64, max_calls: u32, window: Duration) -> Self {
        Self {
            interval_ms,
            last_call_time: HashMap::new(),
            rate_limit: Some((max_calls, window)),
            call_count: HashMap::new(),
        }
    }
    
    /// Chờ nếu cần thiết và ghi nhận lần gọi API
    pub async fn wait_if_needed(&mut self, endpoint: &str) {
        let now = Instant::now();
        
        // Kiểm tra interval
        if let Some(last_time) = self.last_call_time.get(endpoint) {
            let elapsed = now.duration_since(*last_time).as_millis() as u64;
            
            if elapsed < self.interval_ms {
                let wait_time = self.interval_ms - elapsed;
                debug!("Rate limiting: waiting {}ms for endpoint {}", wait_time, endpoint);
                sleep(Duration::from_millis(wait_time)).await;
            }
        }
        
        // Cập nhật thời điểm gọi cuối
        self.last_call_time.insert(endpoint.to_string(), Instant::now());
        
        // Kiểm tra rate limit nếu có
        if let Some((max_calls, window)) = self.rate_limit {
            let call_times = self.call_count.entry(endpoint.to_string()).or_insert_with(Vec::new);
            
            // Loại bỏ các lần gọi cũ hơn window
            let cutoff = now - window;
            call_times.retain(|&time| time > cutoff);
            
            // Nếu đã đạt giới hạn, chờ đến khi có thể gọi tiếp
            if call_times.len() >= max_calls as usize {
                let oldest_call = call_times[0];
                let wait_time = window - now.duration_since(oldest_call);
                
                debug!(
                    "Rate limit reached for {}: {} calls in {:?}, waiting {:?}",
                    endpoint, call_times.len(), window, wait_time
                );
                
                sleep(wait_time).await;
                
                // Cập nhật lại danh sách sau khi chờ
                call_times.remove(0);
            }
            
            // Thêm lần gọi mới
            call_times.push(Instant::now());
        }
    }
}

/// Singleton để quản lý rate limiter cho các endpoint khác nhau
pub struct ApiRateLimiterManager {
    /// Rate limiter cho mỗi loại API
    limiters: HashMap<String, RateLimiter>,
}

impl Default for ApiRateLimiterManager {
    fn default() -> Self {
        let mut limiters = HashMap::new();
        
        // Thiết lập rate limiter mặc định cho các loại API phổ biến
        
        // Ethereum API (Infura, Alchemy) - 5 requests/sec
        limiters.insert(
            "ethereum_rpc".to_string(),
            RateLimiter::with_rate_limit(200, 5, Duration::from_secs(1))
        );
        
        // BSC API - 10 requests/sec
        limiters.insert(
            "bsc_rpc".to_string(),
            RateLimiter::with_rate_limit(100, 10, Duration::from_secs(1))
        );
        
        // CoinGecko API - 50 requests/min
        limiters.insert(
            "coingecko".to_string(),
            RateLimiter::with_rate_limit(1200, 50, Duration::from_secs(60))
        );
        
        // Etherscan API - 5 requests/sec
        limiters.insert(
            "etherscan".to_string(),
            RateLimiter::with_rate_limit(200, 5, Duration::from_secs(1))
        );
        
        // DEX API (generic) - 3 requests/sec
        limiters.insert(
            "dex_api".to_string(),
            RateLimiter::with_rate_limit(333, 3, Duration::from_secs(1))
        );
        
        Self { limiters }
    }
}

impl ApiRateLimiterManager {
    /// Singleton instance
    pub fn global() -> &'static mut Self {
        static mut INSTANCE: Option<ApiRateLimiterManager> = None;
        unsafe {
            INSTANCE.get_or_insert_with(Default::default)
        }
    }
    
    /// Chờ nếu cần thiết trước khi gọi API
    pub async fn wait_for_api(&mut self, api_type: &str, endpoint: &str) {
        let key = format!("{}:{}", api_type, endpoint);
        
        if let Some(limiter) = self.limiters.get_mut(api_type) {
            limiter.wait_if_needed(&key).await;
        } else {
            // Nếu không có rate limiter cụ thể, sử dụng một giá trị mặc định
            let mut default_limiter = RateLimiter::new(100);
            default_limiter.wait_if_needed(&key).await;
        }
    }
    
    /// Thêm hoặc cập nhật rate limiter
    pub fn set_rate_limiter(&mut self, api_type: &str, interval_ms: u64, max_calls: Option<(u32, Duration)>) {
        let limiter = if let Some((max, window)) = max_calls {
            RateLimiter::with_rate_limit(interval_ms, max, window)
        } else {
            RateLimiter::new(interval_ms)
        };
        
        self.limiters.insert(api_type.to_string(), limiter);
    }
}

/// Wrapper cho các cuộc gọi API với rate limiting
pub async fn call_api_with_rate_limit<F, Fut, T, E>(
    api_type: &str,
    endpoint: &str,
    operation: F,
) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, E>>,
    E: std::fmt::Display,
    anyhow::Error: From<E>,
{
    // Chờ rate limit
    ApiRateLimiterManager::global().wait_for_api(api_type, endpoint).await;
    
    // Thực hiện cuộc gọi
    match operation().await {
        Ok(result) => Ok(result),
        Err(e) => Err(anyhow!("API call to {}:{} failed: {}", api_type, endpoint, e)),
    }
}

/// Thực hiện retry cho hàm async với backoff và rate limiting
pub async fn retry_api_call<F, Fut, T, E>(
    operation: F,
    max_retries: usize,
    initial_delay_ms: u64,
    max_delay_ms: u64,
    api_type: &str,
    endpoint: &str,
) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, E>>,
    E: std::fmt::Display,
    anyhow::Error: From<E>,
{
    let mut delay_ms = initial_delay_ms;
    let mut attempt = 0;
    let mut _last_error = None;
    
    loop {
        attempt += 1;
        
        // Áp dụng rate limiting
        ApiRateLimiterManager::global().wait_for_api(api_type, endpoint).await;
        
        match operation().await {
            Ok(value) => {
                if attempt > 1 {
                    debug!("API call to {}:{} succeeded after {} attempts", api_type, endpoint, attempt);
                }
                return Ok(value);
            }
            Err(err) => {
                _last_error = Some(err);
                
                if attempt >= max_retries {
                    let err_msg = format!(
                        "API call to {}:{} failed after {} attempts: {}",
                        api_type, endpoint, attempt, _last_error.as_ref().unwrap()
                    );
                    return Err(anyhow!(err_msg));
                }
                
                // Tính thời gian delay với exponential backoff và jitter
                let jitter = thread_rng().gen_range(0..=100) as u64;
                delay_ms = std::cmp::min(delay_ms * 2, max_delay_ms) + jitter;
                
                debug!(
                    "Attempt {} for {}:{} failed: {}. Retrying in {}ms...",
                    attempt, api_type, endpoint, _last_error.as_ref().unwrap(), delay_ms
                );
                
                sleep(Duration::from_millis(delay_ms)).await;
            }
        }
    }
} 