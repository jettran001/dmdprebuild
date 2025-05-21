use std::collections::{HashMap, HashSet};

use crate::analys::mempool::types::{
    MempoolTransaction, TransactionType, SuspiciousPattern
};

/// Phát hiện cơ hội arbitrage từ các giao dịch mempool
///
/// # Parameters
/// - `transactions`: Danh sách giao dịch mempool
/// - `min_profit_threshold`: Ngưỡng lợi nhuận tối thiểu (%)
/// - `min_value_eth`: Giá trị giao dịch tối thiểu (ETH)
/// - `limit`: Số lượng cơ hội tối đa trả về
///
/// # Returns
/// - `Vec<(MempoolTransaction, f64)>`: Danh sách (giao dịch, lợi nhuận ước tính %)
pub async fn detect_arbitrage_opportunities(
    transactions: &HashMap<String, MempoolTransaction>,
    min_profit_threshold: f64,
    min_value_eth: f64,
    limit: usize,
) -> Vec<(MempoolTransaction, f64)> {
    let mut opportunities = Vec::new();
    
    for (_, tx) in transactions {
        // Chỉ xem xét các giao dịch swap và có giá trị đủ lớn
        if tx.value < min_value_eth {
            continue;
        }
        
        // Phát hiện swap từ function signature
        let is_swap = match &tx.function_signature {
            Some(sig) => {
                sig.contains("swap") || sig.contains("Swap") || tx.transaction_type == TransactionType::Swap
            },
            None => false,
        };
        
        if !is_swap {
            continue;
        }
        
        // Phân tích arbitrage path từ input data
        if let Some(profit) = estimate_arbitrage_profit(tx) {
            if profit >= min_profit_threshold {
                opportunities.push((tx.clone(), profit));
            }
        }
    }
    
    // Sắp xếp theo lợi nhuận giảm dần
    opportunities.sort_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    
    // Giới hạn số lượng kết quả
    opportunities.truncate(limit);
    
    opportunities
}

/// Ước tính lợi nhuận arbitrage từ giao dịch
///
/// # Parameters
/// - `transaction`: Giao dịch cần phân tích
///
/// # Returns
/// - `Option<f64>`: Lợi nhuận ước tính (%) nếu có
fn estimate_arbitrage_profit(transaction: &MempoolTransaction) -> Option<f64> {
    // Placeholder - Cần phân tích input data thực tế để tính toán chính xác
    // Đây chỉ là một ước tính đơn giản dựa trên gas price cao
    
    let gas_price_ratio = transaction.gas_price / 5.0; // Giả sử gas trung bình là 5 Gwei
    if gas_price_ratio > 2.0 {
        // Gas cao gấp đôi trung bình thường chỉ có ý nghĩa khi có lợi nhuận tốt
        Some(gas_price_ratio * 0.5)
    } else {
        None
    }
}

/// Tìm MEV bots dựa trên hành vi giao dịch
///
/// # Parameters
/// - `transactions`: Danh sách giao dịch mempool
/// - `high_frequency_threshold`: Ngưỡng số lượng giao dịch để xem xét là tần số cao
/// - `time_window_sec`: Cửa sổ thời gian (giây) để xem xét
/// - `limit`: Số lượng tối đa MEV bot trả về
///
/// # Returns
/// - `Vec<(String, usize)>`: Danh sách (địa chỉ, số giao dịch) của các MEV bot
pub async fn find_mev_bots(
    transactions: &HashMap<String, MempoolTransaction>,
    high_frequency_threshold: usize,
    time_window_sec: u64,
    limit: usize,
) -> Vec<(String, usize)> {
    // Thống kê số lượng giao dịch theo địa chỉ
    let mut tx_count_by_address: HashMap<String, usize> = HashMap::new();
    let mut swap_count_by_address: HashMap<String, usize> = HashMap::new();
    
    // Thời gian hiện tại là thời gian của giao dịch mới nhất
    let now = transactions.values()
        .map(|tx| tx.detected_at)
        .max()
        .unwrap_or(0);
    
    for (_, tx) in transactions {
        // Chỉ xem xét giao dịch trong cửa sổ thời gian
        if now - tx.detected_at > time_window_sec {
            continue;
        }
        
        // Cập nhật số lượng giao dịch
        *tx_count_by_address.entry(tx.from_address.clone()).or_insert(0) += 1;
        
        // Đếm riêng các giao dịch swap
        let is_swap = match &tx.function_signature {
            Some(sig) => {
                sig.contains("swap") || sig.contains("Swap") || tx.transaction_type == TransactionType::Swap
            },
            None => false,
        };
        
        if is_swap {
            *swap_count_by_address.entry(tx.from_address.clone()).or_insert(0) += 1;
        }
    }
    
    // Lọc ra các địa chỉ có dấu hiệu của MEV bot
    let mut potential_mev_bots = Vec::new();
    
    for (address, count) in tx_count_by_address {
        if count >= high_frequency_threshold {
            let swap_count = swap_count_by_address.get(&address).cloned().unwrap_or(0);
            let swap_ratio = swap_count as f64 / count as f64;
            
            // MEV bots thường có tỉ lệ swap cao
            if swap_ratio >= 0.5 {
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

/// Phát hiện các sandwich attack trong mempool
///
/// # Parameters
/// - `transactions`: Danh sách giao dịch mempool
/// - `min_value_eth`: Giá trị giao dịch tối thiểu (ETH)
/// - `time_window_ms`: Cửa sổ thời gian (milliseconds) để xem xét
/// - `limit`: Số lượng tối đa attack trả về
///
/// # Returns
/// - `Vec<Vec<MempoolTransaction>>`: Danh sách các attack (mỗi attack là 2-3 giao dịch)
pub async fn find_sandwich_attacks(
    transactions: &HashMap<String, MempoolTransaction>,
    min_value_eth: f64,
    time_window_ms: u64,
    limit: usize,
) -> Vec<Vec<MempoolTransaction>> {
    let mut attacks = Vec::new();
    
    // Tổ chức giao dịch theo địa chỉ gửi
    let mut txs_by_sender: HashMap<String, Vec<MempoolTransaction>> = HashMap::new();
    
    for (_, tx) in transactions {
        txs_by_sender.entry(tx.from_address.clone())
            .or_insert_with(Vec::new)
            .push(tx.clone());
    }
    
    // Tìm kiếm các cặp giao dịch sandwich
    for (sender, txs) in txs_by_sender {
        // Bỏ qua nếu số lượng giao dịch < 2
        if txs.len() < 2 {
            continue;
        }
        
        // Sắp xếp theo thời gian phát hiện
        let mut sorted_txs = txs.clone();
        sorted_txs.sort_by_key(|tx| tx.detected_at);
        
        // Tìm chuỗi giao dịch swap từ cùng địa chỉ trong cửa sổ thời gian
        let mut sandwich_candidates = Vec::new();
        
        for i in 0..(sorted_txs.len() - 1) {
            let first_tx = &sorted_txs[i];
            
            // Tìm giao dịch tiếp theo trong cửa sổ thời gian
            for j in (i + 1)..sorted_txs.len() {
                let second_tx = &sorted_txs[j];
                
                // Kiểm tra cửa sổ thời gian
                if second_tx.detected_at - first_tx.detected_at > time_window_ms / 1000 {
                    break;
                }
                
                // Kiểm tra cả hai là swap
                let first_is_swap = is_swap_transaction(first_tx);
                let second_is_swap = is_swap_transaction(second_tx);
                
                if first_is_swap && second_is_swap {
                    // Tìm giao dịch nạn nhân giữa hai giao dịch này
                    let victims = find_potential_victims(
                        transactions,
                        first_tx,
                        second_tx,
                        min_value_eth,
                    );
                    
                    for victim in victims {
                        let mut attack = vec![first_tx.clone(), victim, second_tx.clone()];
                        sandwich_candidates.push(attack);
                        
                        // Giới hạn số lượng kết quả
                        if sandwich_candidates.len() >= limit {
                            return sandwich_candidates;
                        }
                    }
                }
            }
        }
        
        // Thêm các ứng viên vào kết quả
        attacks.extend(sandwich_candidates);
        
        // Giới hạn số lượng kết quả
        if attacks.len() >= limit {
            attacks.truncate(limit);
            break;
        }
    }
    
    attacks
}

/// Tìm các giao dịch tiềm năng bị kẹp giữa hai giao dịch
///
/// # Parameters
/// - `transactions`: Tất cả giao dịch trong mempool
/// - `first_tx`: Giao dịch đầu tiên của kẻ tấn công
/// - `second_tx`: Giao dịch thứ hai của kẻ tấn công
/// - `min_value_eth`: Giá trị giao dịch tối thiểu (ETH)
///
/// # Returns
/// - `Vec<MempoolTransaction>`: Các giao dịch nạn nhân tiềm năng
fn find_potential_victims(
    transactions: &HashMap<String, MempoolTransaction>,
    first_tx: &MempoolTransaction,
    second_tx: &MempoolTransaction,
    min_value_eth: f64,
) -> Vec<MempoolTransaction> {
    let mut victims = Vec::new();
    
    // Tìm các giao dịch nằm giữa first_tx và second_tx
    for (_, tx) in transactions {
        // Bỏ qua nếu là từ cùng địa chỉ với kẻ tấn công
        if tx.from_address == first_tx.from_address {
            continue;
        }
        
        // Kiểm tra thời gian nằm giữa
        if tx.detected_at <= first_tx.detected_at || tx.detected_at >= second_tx.detected_at {
            continue;
        }
        
        // Chỉ xem xét swap có giá trị đủ lớn
        if !is_swap_transaction(tx) || tx.value < min_value_eth {
            continue;
        }
        
        // Kiểm tra nếu giao dịch tương tác với cùng một token
        if has_common_token(first_tx, tx) && has_common_token(second_tx, tx) {
            victims.push(tx.clone());
        }
    }
    
    victims
}

/// Kiểm tra xem giao dịch có phải là swap không
///
/// # Parameters
/// - `tx`: Giao dịch cần kiểm tra
///
/// # Returns
/// - `bool`: True nếu là giao dịch swap
fn is_swap_transaction(tx: &MempoolTransaction) -> bool {
    // Kiểm tra loại giao dịch
    if tx.transaction_type == TransactionType::Swap {
        return true;
    }
    
    // Kiểm tra function signature
    match &tx.function_signature {
        Some(sig) => {
            sig.contains("swap") ||
            sig.contains("Swap") ||
            sig == "0x38ed1739" || // swapExactTokensForTokens
            sig == "0x7ff36ab5" || // swapExactETHForTokens
            sig == "0x18cbafe5"    // swapExactTokensForETH
        },
        None => false,
    }
}

/// Kiểm tra xem hai giao dịch có liên quan đến cùng token không
///
/// # Parameters
/// - `tx1`: Giao dịch thứ nhất
/// - `tx2`: Giao dịch thứ hai
///
/// # Returns
/// - `bool`: True nếu hai giao dịch liên quan đến cùng token
fn has_common_token(tx1: &MempoolTransaction, tx2: &MempoolTransaction) -> bool {
    // Kiểm tra nếu có cùng địa chỉ nhận
    if let (Some(to1), Some(to2)) = (&tx1.to_address, &tx2.to_address) {
        if to1 == to2 {
            return true;
        }
    }
    
    // Kiểm tra input data có chứa thông tin token chung
    // Đây là heuristic đơn giản, thực tế cần phân tích path[] trong input data
    let data1 = tx1.input_data.to_lowercase();
    let data2 = tx2.input_data.to_lowercase();
    
    // Tìm địa chỉ trong input data (đơn giản hóa)
    let potential_addresses1 = extract_potential_addresses(&data1);
    let potential_addresses2 = extract_potential_addresses(&data2);
    
    // Kiểm tra giao điểm
    for addr1 in &potential_addresses1 {
        if potential_addresses2.contains(addr1) {
            return true;
        }
    }
    
    false
}

/// Trích xuất các địa chỉ tiềm năng từ input data
///
/// # Parameters
/// - `input_data`: Dữ liệu đầu vào của giao dịch
///
/// # Returns
/// - `HashSet<String>`: Tập hợp các địa chỉ tiềm năng
fn extract_potential_addresses(input_data: &str) -> HashSet<String> {
    let mut addresses = HashSet::new();
    
    // Tìm tất cả các đoạn 40 ký tự hex có thể là địa chỉ
    let mut i = 0;
    while i + 40 <= input_data.len() {
        let potential_addr = &input_data[i..(i + 40)];
        
        // Kiểm tra nếu là chuỗi hex hợp lệ
        if potential_addr.chars().all(|c| c.is_ascii_hexdigit()) {
            addresses.insert(format!("0x{}", potential_addr));
        }
        
        i += 1;
    }
    
    addresses
} 