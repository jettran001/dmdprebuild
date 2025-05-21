use sha2::{Sha256, Digest};

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
    let mut addr = address.to_lowercase();
    
    if !addr.starts_with("0x") {
        addr = format!("0x{}", addr);
    }
    
    addr
}

/// Chuyển đổi bội số gas thành chuỗi mô tả
///
/// # Parameters
/// - `multiplier`: Bội số gas so với giá trung bình
///
/// # Returns
/// - `String`: Mô tả bội số gas
pub fn gas_multiplier_to_string(multiplier: f64) -> String {
    if multiplier <= 0.7 {
        "slow".to_string()
    } else if multiplier <= 0.9 {
        "standard".to_string()
    } else if multiplier <= 1.1 {
        "fast".to_string()
    } else if multiplier <= 1.5 {
        "rapid".to_string()
    } else {
        "urgent".to_string()
    }
}

/// Ước tính thời gian xác nhận giao dịch dựa trên priority
///
/// # Parameters
/// - `tx_priority`: Mức độ ưu tiên của giao dịch
/// - `chain_id`: ID của blockchain
///
/// # Returns
/// - `String`: Ước tính thời gian xác nhận
pub fn estimate_confirmation_time(
    tx_priority: &crate::analys::mempool::types::TransactionPriority,
    chain_id: u32,
) -> String {
    use crate::analys::mempool::types::TransactionPriority;
    
    // Thời gian cơ bản theo chain
    let base_time = match chain_id {
        1 => 15.0,  // Ethereum ~15 giây/block
        56 => 3.0,  // BSC ~3 giây/block
        137 => 2.0, // Polygon ~2 giây/block
        _ => 10.0,  // Mặc định 10 giây/block
    };
    
    // Ước tính theo priority
    let (blocks, description) = match tx_priority {
        TransactionPriority::VeryHigh => (1, "very quickly"),
        TransactionPriority::High => (2, "quickly"),
        TransactionPriority::Medium => (5, "in a few minutes"),
        TransactionPriority::Low => (10, "may take some time"),
    };
    
    // Tính toán thời gian dự kiến (giây)
    let time_seconds = base_time * blocks as f64;
    
    if time_seconds < 60.0 {
        format!("~{:.0} seconds ({})", time_seconds, description)
    } else {
        format!("~{:.1} minutes ({})", time_seconds / 60.0, description)
    }
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
    let token_amount = extract_token_amount_from_input(input_data)?;
    
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
fn extract_token_amount_from_input(input_data: &str) -> Option<f64> {
    // Placeholder - triển khai thực sẽ parse ABI data
    // Cần decode function parameters từ input data
    None
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