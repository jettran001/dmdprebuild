use std::collections::HashMap;
use std::time::SystemTime;

use crate::analys::mempool::types::{MempoolTransaction, SuspiciousPattern};

/// Phát hiện front-running trong mempool
///
/// Phát hiện giao dịch cố gắng chạy trước giao dịch khác với gas price cao hơn
///
/// # Parameters
/// - `transaction`: Giao dịch cần kiểm tra
/// - `pending_txs`: Danh sách giao dịch đang chờ trong mempool
/// - `time_window_ms`: Cửa sổ thời gian (milliseconds) để xem xét
///
/// # Returns
/// - `Option<SuspiciousPattern>`: Pattern được phát hiện nếu có
pub async fn detect_front_running(
    transaction: &MempoolTransaction,
    pending_txs: &HashMap<String, MempoolTransaction>,
    time_window_ms: u64,
) -> Option<SuspiciousPattern> {
    // Kiểm tra gas price cao bất thường
    if transaction.gas_price <= 0.0 {
        return None;
    }

    // Tìm giao dịch tương tự trong cùng thời gian
    let mut similar_txs = Vec::new();
    
    for (_, tx) in pending_txs {
        // Bỏ qua giao dịch hiện tại
        if tx.tx_hash == transaction.tx_hash {
            continue;
        }
        
        // Chỉ xem xét giao dịch trong cùng một cửa sổ thời gian
        if (tx.detected_at as i64 - transaction.detected_at as i64).abs() as u64 > time_window_ms / 1000 {
            continue;
        }
        
        // Kiểm tra liệu giao dịch có cùng mục tiêu
        if let (Some(to1), Some(to2)) = (&transaction.to_address, &tx.to_address) {
            if to1 == to2 && transaction.gas_price > tx.gas_price * 1.5 {
                similar_txs.push(tx);
            }
        }
    }
    
    // Nếu có giao dịch tương tự và gas price cao hơn đáng kể
    if !similar_txs.is_empty() {
        return Some(SuspiciousPattern::FrontRunning);
    }
    
    None
}

/// Phát hiện sandwich attack trong mempool
///
/// Phát hiện cặp giao dịch cố gắng kẹp một giao dịch lớn để hưởng lợi từ slippage
///
/// # Parameters
/// - `transaction`: Giao dịch cần kiểm tra
/// - `pending_txs`: Danh sách giao dịch đang chờ trong mempool
/// - `time_window_ms`: Cửa sổ thời gian (milliseconds) để xem xét
///
/// # Returns
/// - `Option<SuspiciousPattern>`: Pattern được phát hiện nếu có
pub async fn detect_sandwich_attack(
    transaction: &MempoolTransaction,
    pending_txs: &HashMap<String, MempoolTransaction>,
    time_window_ms: u64,
) -> Option<SuspiciousPattern> {
    // Chỉ kiểm tra các giao dịch swap
    let is_swap_function = match &transaction.function_signature {
        Some(sig) => sig.contains("swap") || sig.contains("Swap"),
        None => false,
    };
    
    if !is_swap_function {
        return None;
    }
    
    // Tìm giao dịch từ cùng địa chỉ trong cửa sổ thời gian
    let mut related_txs = Vec::new();
    
    for (_, tx) in pending_txs {
        // Bỏ qua giao dịch hiện tại
        if tx.tx_hash == transaction.tx_hash {
            continue;
        }
        
        // Chỉ xem xét giao dịch trong cùng một cửa sổ thời gian
        if (tx.detected_at as i64 - transaction.detected_at as i64).abs() as u64 > time_window_ms / 1000 {
            continue;
        }
        
        // Kiểm tra nếu cùng người gửi và cũng là swap
        if tx.from_address == transaction.from_address {
            let tx_is_swap = match &tx.function_signature {
                Some(sig) => sig.contains("swap") || sig.contains("Swap"),
                None => false,
            };
            
            if tx_is_swap {
                related_txs.push(tx);
            }
        }
    }
    
    // Nếu cùng địa chỉ có hơn 2 giao dịch swap trong một cửa sổ thời gian ngắn
    if related_txs.len() >= 1 {
        return Some(SuspiciousPattern::SandwichAttack);
    }
    
    None
}

/// Phát hiện giao dịch tần số cao từ cùng địa chỉ
///
/// Phát hiện địa chỉ thực hiện nhiều giao dịch liên tiếp trong thời gian ngắn
///
/// # Parameters
/// - `transaction`: Giao dịch cần kiểm tra
/// - `pending_txs`: Danh sách giao dịch đang chờ trong mempool
///
/// # Returns
/// - `Option<SuspiciousPattern>`: Pattern được phát hiện nếu có
pub async fn detect_high_frequency_trading(
    transaction: &MempoolTransaction,
    pending_txs: &HashMap<String, MempoolTransaction>,
) -> Option<SuspiciousPattern> {
    // Đếm số giao dịch từ cùng địa chỉ trong 5 phút
    let mut transaction_count = 0;
    let from_address = &transaction.from_address;
    
    for (_, tx) in pending_txs {
        // Bỏ qua giao dịch hiện tại
        if tx.tx_hash == transaction.tx_hash {
            continue;
        }
        
        // Kiểm tra nếu trong 5 phút và cùng địa chỉ
        if tx.from_address == *from_address {
            if (transaction.detected_at as i64 - tx.detected_at as i64).abs() as u64 <= 300 { // 5 phút = 300 giây
                transaction_count += 1;
            }
        }
    }
    
    // Nếu có nhiều hơn 5 giao dịch trong 5 phút
    if transaction_count >= 5 {
        return Some(SuspiciousPattern::HighFrequencyTrading);
    }
    
    None
}

/// Phát hiện large pending sells
///
/// Phát hiện các giao dịch bán lớn đang chờ xử lý
///
/// # Parameters
/// - `token_address`: Địa chỉ token cần kiểm tra
/// - `min_value_eth`: Giá trị tối thiểu (ETH) để xem xét là giao dịch lớn
///
/// # Returns
/// - `bool`: True nếu có giao dịch bán lớn đang chờ
pub async fn detect_large_pending_sells(
    token_address: &str,
    pending_txs: &HashMap<String, MempoolTransaction>,
    min_value_eth: f64,
) -> bool {
    for (_, tx) in pending_txs {
        // Xác định nếu đây là giao dịch bán token
        let is_sell = match &tx.function_signature {
            Some(sig) => {
                sig.contains("sell") || sig.contains("Sell") || 
                sig.contains("swapExactTokensForETH") || sig.contains("swapTokensForExactETH")
            },
            None => false,
        };
        
        if !is_sell {
            continue;
        }
        
        // Kiểm tra nếu transaction liên quan đến token đang xem xét
        let token_related = tx.input_data.to_lowercase().contains(&token_address.to_lowercase());
        
        // Nếu là giao dịch bán lớn liên quan đến token
        if token_related && tx.value >= min_value_eth {
            return true;
        }
    }
    
    false
}

/// Phát hiện MEV bots
///
/// Tìm các địa chỉ có dấu hiệu của MEV bot
///
/// # Parameters
/// - `pending_txs`: Danh sách giao dịch đang chờ trong mempool
/// - `high_frequency_threshold`: Ngưỡng số lượng giao dịch để coi là tần số cao
/// - `time_window_sec`: Cửa sổ thời gian (giây) để xem xét
/// - `limit`: Số lượng tối đa MEV bot trả về
///
/// # Returns
/// - `Vec<(String, usize)>`: Danh sách (địa chỉ, số giao dịch) của các MEV bot phát hiện được
pub async fn find_mev_bots(
    pending_txs: &HashMap<String, MempoolTransaction>,
    high_frequency_threshold: usize,
    time_window_sec: u64,
    limit: usize,
) -> Vec<(String, usize)> {
    // Thống kê số lượng giao dịch theo địa chỉ
    let mut tx_count_by_address: HashMap<String, usize> = HashMap::new();
    let mut gas_sum_by_address: HashMap<String, f64> = HashMap::new();
    
    // Thời gian hiện tại
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    for (_, tx) in pending_txs {
        // Chỉ xem xét giao dịch trong cửa sổ thời gian
        if now - tx.detected_at > time_window_sec {
            continue;
        }
        
        // Cập nhật số lượng giao dịch
        *tx_count_by_address.entry(tx.from_address.clone()).or_insert(0) += 1;
        
        // Cập nhật tổng gas price
        *gas_sum_by_address.entry(tx.from_address.clone()).or_insert(0.0) += tx.gas_price;
    }
    
    // Lọc ra các địa chỉ có dấu hiệu của MEV bot
    let mut potential_mev_bots = Vec::new();
    
    for (address, count) in tx_count_by_address {
        if count >= high_frequency_threshold {
            // Gas price trung bình
            let avg_gas = gas_sum_by_address.get(&address).unwrap_or(&0.0) / count as f64;
            
            // MEV bots thường có gas price cao và số lượng giao dịch lớn
            if avg_gas > 0.0 {
                potential_mev_bots.push((address, count));
            }
        }
    }
    
    // Sắp xếp theo số lượng giao dịch giảm dần
    potential_mev_bots.sort_by(|a, b| b.1.cmp(&a.1));
    
    // Giới hạn số lượng kết quả
    potential_mev_bots.truncate(limit);
    
    potential_mev_bots
} 