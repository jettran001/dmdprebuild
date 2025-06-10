//! Analyzer cho MEV Logic
//!
//! Module này cung cấp các công cụ phân tích để phát hiện các mẫu giao dịch đáng ngờ,
//! hành vi trader, và tiềm năng các cơ hội MEV trong mempool.

use std::collections::HashMap;

use crate::analys::mempool::{
    MempoolTransaction, TransactionType, TransactionPriority, SuspiciousPattern
};

use super::types::{TraderBehaviorType, TraderExpertiseLevel};

/// Phát hiện mẫu sandwich từ tập các swap
pub fn detect_sandwich_pattern(swaps: &[&MempoolTransaction]) -> bool {
    // Cần ít nhất 3 swap để tạo thành sandwich
    if swaps.len() < 3 {
        return false;
    }
    
    // Kiểm tra các mẫu sandwich
    // Mẫu điển hình: Swap A->B, sau đó B->A (với giá trị lớn), rồi lại A->B
    
    for i in 0..swaps.len() - 2 {
        let first = swaps[i];
        let second = swaps[i + 1];
        let third = swaps[i + 2];
        
        // Kiểm tra nếu cùng token nhưng chiều ngược nhau
        if let (Some(first_from), Some(first_to)) = (&first.from_token, &first.to_token) {
            if let (Some(second_from), Some(second_to)) = (&second.from_token, &second.to_token) {
                if let (Some(third_from), Some(third_to)) = (&third.from_token, &third.to_token) {
                    // Kiểm tra mẫu: A->B, B->A, A->B
                    if first_from.address == second_to.address && 
                       first_to.address == second_from.address &&
                       third_from.address == first_from.address &&
                       third_to.address == first_to.address {
                        // Và kiểm tra giá trị swap giữa lớn hơn
                        if second.value_usd > first.value_usd && second.value_usd > third.value_usd {
                            return true;
                        }
                    }
                }
            }
        }
    }
    
    false
}

/// Phát hiện mẫu front-running từ tập các swap
pub fn detect_front_running_pattern(swaps: &[&MempoolTransaction]) -> bool {
    // Cần ít nhất 2 swap để tạo thành front-running
    if swaps.len() < 2 {
        return false;
    }
    
    // Sắp xếp theo gas price, xử lý NaN một cách an toàn
    let mut sorted_swaps = swaps.to_vec();
    sorted_swaps.sort_by(|a, b| {
        // Lấy gas price, mặc định là 0.0 nếu None
        let a_gas = a.gas_price.unwrap_or(0.0);
        let b_gas = b.gas_price.unwrap_or(0.0);
        
        // Xử lý NaN một cách đặc biệt để tránh bug khi so sánh
        match (a_gas.is_nan(), b_gas.is_nan()) {
            (true, true) => {
                tracing::warn!("NaN gas price cho cả hai giao dịch: {} và {}, coi là bằng nhau", 
                    a.hash, b.hash);
                std::cmp::Ordering::Equal
            },
            (true, false) => {
                tracing::warn!("NaN gas price cho giao dịch: {}, đặt vào cuối danh sách", a.hash);
                std::cmp::Ordering::Less
            },
            (false, true) => {
                tracing::warn!("NaN gas price cho giao dịch: {}, đặt vào cuối danh sách", b.hash);
                std::cmp::Ordering::Greater
            },
            (false, false) => {
                // So sánh bình thường khi cả hai giá trị đều hợp lệ
                b_gas.partial_cmp(&a_gas).unwrap_or_else(|| {
                    tracing::error!(
                        "Lỗi không mong đợi khi so sánh gas price: {} ({:.2}) và {} ({:.2})",
                        a.hash, a_gas, b.hash, b_gas
                    );
                    std::cmp::Ordering::Equal
                })
            }
        }
    });
    
    // Ghi log số lượng giao dịch đã sắp xếp
    tracing::debug!("Đã sắp xếp {} giao dịch swap theo gas price để phân tích front-running", sorted_swaps.len());
    
    // Lọc trước các giao dịch có gas price không hợp lệ
    let valid_sorted_swaps: Vec<&MempoolTransaction> = sorted_swaps.into_iter()
        .filter(|tx| {
            let gas_price = tx.gas_price.unwrap_or(0.0);
            if gas_price.is_nan() || gas_price <= 0.0 {
                tracing::warn!("Bỏ qua giao dịch với gas price không hợp lệ: {} ({:?})", tx.hash, gas_price);
                false
            } else {
                true
            }
        })
        .collect();
    
    // Kiểm tra các swap cùng token nhưng khác gas price đáng kể
    for i in 0..valid_sorted_swaps.len().saturating_sub(1) {
        let high_gas = valid_sorted_swaps[i];
        let low_gas = valid_sorted_swaps[i + 1];
        
        // Lấy gas price, đã được đảm bảo hợp lệ từ bước lọc trước
        let high_gas_price = high_gas.gas_price.unwrap_or(1.0);
        let low_gas_price = low_gas.gas_price.unwrap_or(1.0);
        
        // Kiểm tra nếu cùng token pair nhưng gas price chênh lệch lớn
        if let (Some(high_from), Some(high_to)) = (&high_gas.from_token, &high_gas.to_token) {
            if let (Some(low_from), Some(low_to)) = (&low_gas.from_token, &low_gas.to_token) {
                // Kiểm tra nếu cùng token pair (cùng chiều hoặc ngược chiều)
                let same_tokens = (high_from.address == low_from.address && high_to.address == low_to.address) ||
                                 (high_from.address == low_to.address && high_to.address == low_from.address);
                
                if same_tokens {
                    // Tính tỷ lệ an toàn tránh phép chia cho 0
                    let ratio = if low_gas_price >= 0.000001 { // Ngưỡng nhỏ nhất để tránh chia cho gần 0
                        high_gas_price / low_gas_price
                    } else {
                        // Nếu giá trị quá nhỏ, coi như tỷ lệ rất cao
                        1000.0
                    };
                    
                    // Nếu gas price cao hơn đáng kể
                    if ratio > 1.5 {
                        tracing::info!(
                            "Phát hiện mẫu front-running: gas price cao/thấp = {:.2}x ({:.2} vs {:.2} gwei), tokens: {} -> {}",
                            ratio, high_gas_price, low_gas_price, 
                            high_from.symbol.as_deref().unwrap_or("unknown"),
                            high_to.symbol.as_deref().unwrap_or("unknown")
                        );
                        return true;
                    }
                }
            }
        }
    }
    
    false
}

/// Phát hiện mẫu thanh khoản bất thường từ tập các sự kiện thanh khoản
pub fn detect_abnormal_liquidity_pattern(events: &[&MempoolTransaction]) -> bool {
    // Cần ít nhất 2 event để phát hiện mẫu
    if events.len() < 2 {
        return false;
    }
    
    // Nhóm theo token
    let mut token_to_events: HashMap<String, Vec<&MempoolTransaction>> = HashMap::new();
    
    for event in events {
        if let Some(to_token) = &event.to_token {
            token_to_events
                .entry(to_token.address.clone())
                .or_insert_with(Vec::new)
                .push(event);
        }
    }
    
    // Kiểm tra từng token
    for (_, token_events) in token_to_events {
        if token_events.len() < 2 {
            continue;
        }
        
        // Sắp xếp theo thời gian
        let mut sorted_events = token_events;
        sorted_events.sort_by_key(|&tx| tx.timestamp);
        
        for i in 0..sorted_events.len() - 1 {
            let current = sorted_events[i];
            let next = sorted_events[i + 1];
            
            // Phát hiện thêm rồi rút nhanh chóng (5 phút)
            if current.transaction_type == TransactionType::AddLiquidity &&
               next.transaction_type == TransactionType::RemoveLiquidity &&
               (next.timestamp - current.timestamp) < 300 { // 5 phút
                return true;
            }
        }
    }
    
    false
}

/// Phân tích tần suất giao dịch và phát hiện hành vi từ lịch sử giao dịch
pub fn analyze_transaction_frequency(transactions: &[MempoolTransaction], period_hours: f64) -> f64 {
    if transactions.is_empty() || period_hours <= 0.0 {
        return 0.0;
    }
    
    // Số lượng giao dịch / số giờ = tần suất
    transactions.len() as f64 / period_hours
}

/// Phân tích thời gian hoạt động chính từ lịch sử giao dịch
pub fn analyze_active_hours(transactions: &[MempoolTransaction]) -> Vec<u8> {
    if transactions.is_empty() {
        return Vec::new();
    }
    
    // Đếm giao dịch theo giờ trong ngày
    let mut hour_counts: HashMap<u8, i32> = HashMap::new();
    
    for tx in transactions {
        let timestamp = tx.timestamp;
        // Xử lý an toàn timestamp, tránh unwrap có thể gây panic
        let datetime = match chrono::DateTime::from_timestamp(timestamp as i64, 0) {
            Some(dt) => dt,
            None => {
                tracing::warn!("Không thể chuyển đổi timestamp {} thành DateTime cho giao dịch {}, bỏ qua", 
                               timestamp, tx.hash);
                continue; // Bỏ qua giao dịch này
            }
        };
        
        let hour = datetime.hour() as u8;
        *hour_counts.entry(hour).or_insert(0) += 1;
    }
    
    // Sắp xếp và lấy 5 giờ hoạt động nhiều nhất
    let mut active_hours: Vec<(u8, i32)> = hour_counts.into_iter().collect();
    active_hours.sort_by(|a, b| b.1.cmp(&a.1));
    
    active_hours.into_iter()
        .take(5) // Top 5 active hours
        .map(|(hour, _)| hour)
        .collect()
}

/// Tính điểm đánh giá trader dựa trên hành vi và chuyên môn
pub fn calculate_trader_score(behavior_type: &TraderBehaviorType, expertise_level: &TraderExpertiseLevel) -> f64 {
    // Base score dựa trên loại trader
    let behavior_score = match behavior_type {
        TraderBehaviorType::Arbitrageur => 80.0,
        TraderBehaviorType::MevBot => 85.0,
        TraderBehaviorType::Institutional => 70.0,
        TraderBehaviorType::MarketMaker => 75.0,
        TraderBehaviorType::Whale => 65.0,
        TraderBehaviorType::HighFrequencyTrader => 70.0,
        TraderBehaviorType::Retail => 40.0,
        TraderBehaviorType::Unknown => 30.0,
    };
    
    // Điều chỉnh theo mức độ chuyên môn
    let expertise_adjustment = match expertise_level {
        TraderExpertiseLevel::Professional => 15.0,
        TraderExpertiseLevel::Intermediate => 5.0,
        TraderExpertiseLevel::Beginner => -10.0,
        TraderExpertiseLevel::Automated => 10.0,
        TraderExpertiseLevel::Unknown => 0.0,
    };
    
    // Chuẩn hóa kết quả về thang điểm 0-100
    (behavior_score + expertise_adjustment).max(0.0).min(100.0)
}

/// Phát hiện các mẫu giao dịch đáng ngờ từ một tập các giao dịch
pub fn detect_suspicious_patterns(transactions: &[MempoolTransaction]) -> Vec<SuspiciousPattern> {
    let mut patterns = Vec::new();
    
    // Nhóm giao dịch theo loại
    let mut swaps = Vec::new();
    let mut liquidity_events = Vec::new();
    
    for tx in transactions {
        match tx.transaction_type {
            TransactionType::Swap => swaps.push(tx),
            TransactionType::AddLiquidity | TransactionType::RemoveLiquidity => liquidity_events.push(tx),
            _ => {}
        }
    }
    
    // Tạo tham chiếu cho phân tích
    let swap_refs: Vec<&MempoolTransaction> = swaps.iter().collect();
    let liquidity_refs: Vec<&MempoolTransaction> = liquidity_events.iter().collect();
    
    // Phát hiện mẫu sandwich
    if detect_sandwich_pattern(&swap_refs) {
        patterns.push(SuspiciousPattern::SandwichAttack);
    }
    
    // Phát hiện mẫu front-running
    if detect_front_running_pattern(&swap_refs) {
        patterns.push(SuspiciousPattern::FrontRunning);
    }
    
    // Phát hiện thêm/rút thanh khoản bất thường
    if detect_abnormal_liquidity_pattern(&liquidity_refs) {
        patterns.push(SuspiciousPattern::RugPull);
    }
    
    // Phát hiện whale movement
    if transactions.iter().any(|tx| tx.value_usd > 100000.0) {
        patterns.push(SuspiciousPattern::WhaleMovement);
    }
    
    patterns
}

/// Tìm chu trình có lợi nhuận cho arbitrage trong đồ thị token
pub fn find_profitable_cycles(token_graph: &HashMap<String, HashMap<String, f64>>, max_hops: usize) -> Vec<(Vec<String>, f64)> {
    let mut profitable_paths = Vec::new();
    
    // Lấy danh sách các token
    let tokens: Vec<String> = token_graph.keys().cloned().collect();
    
    // Với mỗi token, thử tìm đường quay lại chính nó
    for start_token in &tokens {
        let mut visited = std::collections::HashSet::new();
        visited.insert(start_token.clone());
        
        let mut path = Vec::new();
        path.push(start_token.clone());
        
        dfs_find_cycles(
            token_graph,
            start_token,
            start_token,
            &mut visited,
            &mut path,
            1.0,
            max_hops,
            &mut profitable_paths
        );
    }
    
    profitable_paths
}

/// DFS helper để tìm chu trình có lợi nhuận trong đồ thị token
fn dfs_find_cycles(
    token_graph: &HashMap<String, HashMap<String, f64>>,
    start_token: &str,
    current_token: &str,
    visited: &mut std::collections::HashSet<String>,
    path: &mut Vec<String>,
    current_ratio: f64,
    max_hops: usize,
    profitable_paths: &mut Vec<(Vec<String>, f64)>
) {
    // Nếu đã đạt đến max_hops và chưa quay lại token ban đầu, dừng
    if path.len() > max_hops && current_token != start_token {
        return;
    }
    
    // Nếu đã quay lại token ban đầu và đường đi có ít nhất 3 token
    if current_token == start_token && path.len() > 2 {
        // Lợi nhuận dương, thêm vào kết quả
        if current_ratio > 1.0 {
            profitable_paths.push((path.clone(), current_ratio));
        }
        return;
    }
    
    // Xem xét các token kế tiếp
    if let Some(edges) = token_graph.get(current_token) {
        for (next_token, rate) in edges {
            // Nếu token tiếp theo là token bắt đầu hoặc chưa thăm
            if next_token == start_token || !visited.contains(next_token) {
                // Nếu token tiếp theo chưa thăm, đánh dấu đã thăm
                if next_token != start_token {
                    visited.insert(next_token.clone());
                }
                
                // Thêm vào đường đi
                path.push(next_token.clone());
                
                // Tiếp tục DFS
                dfs_find_cycles(
                    token_graph,
                    start_token,
                    next_token,
                    visited,
                    path,
                    current_ratio * rate,
                    max_hops,
                    profitable_paths
                );
                
                // Quay lui
                path.pop();
                
                // Nếu token tiếp theo không phải token bắt đầu, bỏ đánh dấu
                if next_token != start_token {
                    visited.remove(next_token);
                }
            }
        }
    }
} 

/// Phát hiện cơ hội backrunning từ tập các giao dịch
pub fn detect_backrunning_opportunity(transactions: &[&MempoolTransaction]) -> bool {
    // Cần ít nhất 2 giao dịch
    if transactions.len() < 2 {
        return false;
    }
    
    // Sắp xếp theo timestamp
    let mut sorted_txs = transactions.to_vec();
    sorted_txs.sort_by_key(|&tx| tx.timestamp);
    
    for i in 0..sorted_txs.len() - 1 {
        let tx1 = sorted_txs[i];
        
        // Tìm giao dịch có giá trị lớn hoặc thay đổi trạng thái quan trọng
        if tx1.value_usd > 50000.0 || tx1.transaction_type == TransactionType::AddLiquidity {
            // Kiểm tra các giao dịch tiếp theo
            for j in i+1..sorted_txs.len() {
                let tx2 = sorted_txs[j];
                
                // Nếu giao dịch này liên quan đến cùng pool/pair và có gas thấp hơn
                if tx1.transaction_type == tx2.transaction_type &&
                   tx1.gas_price > tx2.gas_price &&
                   (tx1.timestamp - tx2.timestamp) < 3 { // Trong vòng 3 giây
                    
                    // Và cùng làm việc với token tương tự
                    if let (Some(tx1_from), Some(tx1_to)) = (&tx1.from_token, &tx1.to_token) {
                        if let (Some(tx2_from), Some(tx2_to)) = (&tx2.from_token, &tx2.to_token) {
                            if (tx1_from.address == tx2_from.address && tx1_to.address == tx2_to.address) ||
                               (tx1_from.address == tx2_to.address && tx1_to.address == tx2_from.address) {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    
    false
}

/// Phát hiện cơ hội MEV liên quan đến vay thanh khoản (flash loan)
pub fn detect_flash_loan_opportunity(
    token_graphs: &HashMap<String, HashMap<String, f64>>,
    token_usd_prices: &HashMap<String, f64>,
    min_profit_usd: f64
) -> Vec<(Vec<String>, f64)> {
    let mut opportunities = Vec::new();
    
    // Tìm các token có thanh khoản lớn để vay flash loan
    let flash_loan_tokens = vec![
        "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2", // WETH
        "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", // USDC
        "0xdac17f958d2ee523a2206206994597c13d831ec7", // USDT
        "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599", // WBTC
    ];
    
    // Kiểm tra mỗi token có thể vay
    for &start_token in &flash_loan_tokens {
        // Tìm đường đi qua 2-3 DEX có lợi nhuận
        let mut visited = HashSet::new();
        visited.insert(start_token.to_string());
        
        let mut path = Vec::new();
        path.push(start_token.to_string());
        
        // Tìm kiếm đệ quy các đường đi có lợi
        dfs_flash_loan(
            token_graphs,
            token_usd_prices,
            start_token,
            start_token,
            &mut visited,
            &mut path,
            1.0, // Bắt đầu với tỷ lệ 1.0
            0,   // Độ sâu 0
            4,   // Độ sâu tối đa 4
            &mut opportunities,
            min_profit_usd
        );
    }
    
    opportunities
}

/// Hỗ trợ tìm kiếm DFS cho cơ hội flash loan
fn dfs_flash_loan(
    token_graphs: &HashMap<String, HashMap<String, f64>>,
    token_usd_prices: &HashMap<String, f64>,
    start_token: &str,
    current_token: &str,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
    current_ratio: f64,
    depth: usize,
    max_depth: usize,
    opportunities: &mut Vec<(Vec<String>, f64)>,
    min_profit_usd: f64
) {
    // Nếu đã quay trở lại token ban đầu và đã qua ít nhất 2 hop
    if current_token == start_token && depth >= 2 {
        // Tính toán lợi nhuận
        let profit_ratio = current_ratio - 1.0;
        
        if profit_ratio > 0.001 { // Ít nhất 0.1% lợi nhuận
            // Ước tính lợi nhuận USD
            let loan_amount_usd = 10000.0; // Giả sử vay $10k
            let profit_usd = loan_amount_usd * profit_ratio;
            
            // Chi phí gas ước tính
            let gas_cost_usd = 20.0 + (path.len() as f64 * 10.0); // Base cost + per hop
            
            // Lợi nhuận thực
            let net_profit_usd = profit_usd - gas_cost_usd;
            
            if net_profit_usd >= min_profit_usd {
                opportunities.push((path.clone(), net_profit_usd));
            }
        }
        return;
    }
    
    // Nếu đã đạt độ sâu tối đa
    if depth >= max_depth {
        return;
    }
    
    // Kiểm tra các đường đi tiếp theo
    if let Some(edges) = token_graphs.get(current_token) {
        for (next_token, &rate) in edges {
            // Bỏ qua token đã thăm, trừ khi là token xuất phát và đã đi qua ít nhất 2 token khác
            if (next_token != start_token && visited.contains(next_token)) ||
               (next_token == start_token && depth < 2) {
                continue;
            }
            
            // Thêm vào đường đi
            path.push(next_token.clone());
            visited.insert(next_token.clone());
            
            // Đệ quy tìm kiếm sâu hơn
            dfs_flash_loan(
                token_graphs,
                token_usd_prices,
                start_token,
                next_token,
                visited,
                path,
                current_ratio * rate,
                depth + 1,
                max_depth,
                opportunities,
                min_profit_usd
            );
            
            // Backtrack
            visited.remove(next_token);
            path.pop();
        }
    }
}

/// Phát hiện cơ hội liquidation trong các giao thức vay mượn
pub fn detect_liquidation_opportunity(
    positions: &[BorrowingPosition],
    current_prices: &HashMap<String, f64>,
    min_profit_usd: f64
) -> Vec<LiquidationOpportunity> {
    let mut opportunities = Vec::new();
    
    for position in positions {
        // Kiểm tra nếu vị thế có nguy cơ bị thanh lý
        if let Some(liquidation_threshold) = position.liquidation_threshold {
            // Tính toán giá trị hiện tại của tài sản thế chấp
            let mut collateral_value = 0.0;
            
            for (token, amount) in &position.collateral {
                if let Some(&price) = current_prices.get(token) {
                    collateral_value += amount * price;
                }
            }
            
            // Tính toán giá trị khoản vay
            let mut borrowed_value = 0.0;
            
            for (token, amount) in &position.borrowed {
                if let Some(&price) = current_prices.get(token) {
                    borrowed_value += amount * price;
                }
            }
            
            // Tính toán tỷ lệ thế chấp hiện tại
            let current_ratio = collateral_value / borrowed_value;
            
            // Kiểm tra nếu vị thế này có thể bị thanh lý
            if current_ratio < liquidation_threshold {
                // Tính toán lợi nhuận từ thanh lý
                let liquidation_incentive = position.liquidation_incentive.unwrap_or(0.08); // 8% mặc định
                let liquidation_profit = collateral_value * liquidation_incentive;
                
                // Chi phí gas giao dịch (ước tính)
                let gas_cost_usd = 50.0; // Giao dịch thanh lý phức tạp
                
                // Lợi nhuận thực
                let net_profit_usd = liquidation_profit - gas_cost_usd;
                
                if net_profit_usd >= min_profit_usd {
                    let opportunity = LiquidationOpportunity {
                        position_id: position.id.clone(),
                        protocol: position.protocol.clone(),
                        owner: position.owner.clone(),
                        collateral_value,
                        borrowed_value,
                        current_ratio,
                        threshold_ratio: liquidation_threshold,
                        estimated_profit_usd: net_profit_usd,
                        liquidation_incentive,
                        risk_score: calculate_liquidation_risk(current_ratio, liquidation_threshold),
                    };
                    
                    opportunities.push(opportunity);
                }
            }
        }
    }
    
    // Sắp xếp theo lợi nhuận ước tính
    opportunities.sort_by(|a, b| match b.estimated_profit_usd.partial_cmp(&a.estimated_profit_usd) {
            Some(ordering) => ordering,
            None => {
                tracing::warn!("NaN encountered in profit comparison");
                std::cmp::Ordering::Equal
            }
        });
    
    opportunities
}

/// Vị thế vay mượn (cho phân tích liquidation)
#[derive(Debug, Clone)]
pub struct BorrowingPosition {
    /// ID vị thế
    pub id: String,
    /// Địa chỉ người sở hữu
    pub owner: String,
    /// Protocol (Aave, Compound, vv)
    pub protocol: String,
    /// Tài sản thế chấp (token -> amount)
    pub collateral: HashMap<String, f64>,
    /// Tài sản đã vay (token -> amount)
    pub borrowed: HashMap<String, f64>,
    /// Ngưỡng thanh lý (collateral/borrow ratio)
    pub liquidation_threshold: Option<f64>,
    /// Phần thưởng thanh lý (%)
    pub liquidation_incentive: Option<f64>,
    /// Thời gian cập nhật gần nhất
    pub updated_at: u64,
}

/// Cơ hội thanh lý
#[derive(Debug, Clone)]
pub struct LiquidationOpportunity {
    /// ID vị thế
    pub position_id: String,
    /// Protocol
    pub protocol: String,
    /// Chủ sở hữu vị thế
    pub owner: String,
    /// Giá trị tài sản thế chấp (USD)
    pub collateral_value: f64,
    /// Giá trị khoản vay (USD)
    pub borrowed_value: f64,
    /// Tỷ lệ hiện tại (collateral/borrow)
    pub current_ratio: f64,
    /// Ngưỡng tỷ lệ thanh lý
    pub threshold_ratio: f64,
    /// Lợi nhuận ước tính (USD)
    pub estimated_profit_usd: f64,
    /// Phần thưởng thanh lý (%)
    pub liquidation_incentive: f64,
    /// Điểm rủi ro (0-100)
    pub risk_score: f64,
}

/// Tính toán điểm rủi ro cho cơ hội thanh lý
fn calculate_liquidation_risk(current_ratio: f64, threshold_ratio: f64) -> f64 {
    // Khoảng cách đến ngưỡng (càng gần ngưỡng càng rủi ro vì người khác có thể thanh lý trước)
    let threshold_distance = (threshold_ratio - current_ratio) / threshold_ratio;
    
    // Chuẩn hóa thành điểm rủi ro
    // Khoảng cách càng nhỏ -> rủi ro càng cao
    let risk = 100.0 - (threshold_distance * 500.0).min(100.0);
    
    risk
}

/// Just-in-time liquidity (JIT) detection from mempool transactions
pub fn detect_jit_opportunities(
    transactions: &[&MempoolTransaction],
    pools: &HashMap<String, PoolInfo>
) -> Vec<JITOpportunity> {
    let mut opportunities = Vec::new();
    
    // Lọc các giao dịch swap có giá trị lớn
    let large_swaps: Vec<_> = transactions.iter()
        .filter(|tx| tx.transaction_type == TransactionType::Swap && tx.value_usd > 50000.0)
        .collect();
    
    for &tx in large_swaps {
        if let Some(pool_address) = &tx.pool_address {
            // Kiểm tra xem có thông tin pool không
            if let Some(pool) = pools.get(pool_address) {
                // Chỉ xem xét các pool V3 với concentrated liquidity
                if pool.dex_name.contains("V3") || pool.dex_name.contains("v3") {
                    // Tạo cơ hội JIT
                    let opportunity = JITOpportunity {
                        swap_tx_hash: tx.hash.clone(),
                        pool_address: pool_address.clone(),
                        swap_value_usd: tx.value_usd,
                        optimal_range_min: 0.0, // Cần tính toán dựa trên swap size và chiều hướng
                        optimal_range_max: 0.0, // Cần tính toán dựa trên swap size và chiều hướng
                        estimated_fee_profit: tx.value_usd * 0.0005, // Ước tính 0.05%
                        gas_cost_estimate: 30.0, // $30 gas
                        net_profit_estimate: (tx.value_usd * 0.0005) - 30.0,
                        priority: tx.priority,
                    };
                    
                    if opportunity.net_profit_estimate > 0.0 {
                        opportunities.push(opportunity);
                    }
                }
            }
        }
    }
    
    // Sắp xếp theo lợi nhuận ước tính
    opportunities.sort_by(|a, b| match b.net_profit_estimate.partial_cmp(&a.net_profit_estimate) {
            Some(ordering) => ordering,
            None => {
                tracing::warn!("NaN encountered in net profit comparison");
                std::cmp::Ordering::Equal
            }
        });
    
    opportunities
}

/// Pool information structure for JIT analysis
#[derive(Debug, Clone)]
pub struct PoolInfo {
    /// Pool address
    pub address: String,
    /// DEX name
    pub dex_name: String,
    /// Token pairs
    pub tokens: (String, String),
    /// Current price
    pub current_price: f64,
    /// Current liquidity (USD)
    pub liquidity: f64,
}

/// Just-in-time liquidity opportunity structure
#[derive(Debug, Clone)]
pub struct JITOpportunity {
    /// Swap transaction hash
    pub swap_tx_hash: String,
    /// Pool address
    pub pool_address: String,
    /// Swap value (USD)
    pub swap_value_usd: f64,
    /// Optimal position range min
    pub optimal_range_min: f64,
    /// Optimal position range max
    pub optimal_range_max: f64,
    /// Estimated fee profit
    pub estimated_fee_profit: f64,
    /// Gas cost estimate
    pub gas_cost_estimate: f64,
    /// Net profit estimate
    pub net_profit_estimate: f64,
    /// Transaction priority
    pub priority: TransactionPriority,
}

/// Phát hiện và phân tích cơ hội order flow
pub fn analyze_order_flow(
    transactions: &[&MempoolTransaction],
    historic_flow: &HashMap<String, TradingStatistics>
) -> Vec<OrderFlowOpportunity> {
    let mut opportunities = Vec::new();
    
    // Nhóm giao dịch theo địa chỉ nguồn
    let mut address_to_txs: HashMap<String, Vec<&MempoolTransaction>> = HashMap::new();
    
    for &tx in transactions {
        address_to_txs
            .entry(tx.from_address.clone())
            .or_insert_with(Vec::new)
            .push(tx);
    }
    
    // Phân tích từng địa chỉ
    for (address, txs) in address_to_txs {
        // Chỉ quan tâm các địa chỉ có nhiều giao dịch
        if txs.len() < 3 {
            continue;
        }
        
        // Kiểm tra lịch sử giao dịch
        if let Some(stats) = historic_flow.get(&address) {
            // Nếu là trader có kỹ năng cao
            if stats.expertise_level >= 8 && stats.success_rate > 0.7 {
                // Tập trung vào các giao dịch swap lớn
                let swaps: Vec<_> = txs.iter()
                    .filter(|&tx| tx.transaction_type == TransactionType::Swap && tx.value_usd > 10000.0)
                    .collect();
                
                if !swaps.is_empty() {
                    // Tạo cơ hội theo dõi order flow
                    let opportunity = OrderFlowOpportunity {
                        trader_address: address,
                        expertise_level: stats.expertise_level,
                        success_rate: stats.success_rate,
                        transaction_count: stats.transaction_count,
                        average_profit: stats.average_profit,
                        current_transactions: swaps.iter().map(|&&tx| tx.hash.clone()).collect(),
                        potential_profit_estimate: stats.average_profit * 0.8, // 80% của trung bình
                        confidence_score: calculate_flow_confidence(stats),
                    };
                    
                    opportunities.push(opportunity);
                }
            }
        }
    }
    
    // Sắp xếp theo mức độ tự tin, xử lý an toàn NaN
    opportunities.sort_by(|a, b| {
        match (a.confidence_score.is_nan(), b.confidence_score.is_nan()) {
            (true, true) => std::cmp::Ordering::Equal, // Cả hai là NaN, coi là bằng nhau
            (true, false) => std::cmp::Ordering::Less, // a là NaN, xếp sau b
            (false, true) => std::cmp::Ordering::Greater, // b là NaN, xếp sau a
            (false, false) => {
                // Cả hai không phải NaN, thực hiện so sánh bình thường
                b.confidence_score.partial_cmp(&a.confidence_score)
                    .unwrap_or_else(|| {
                        tracing::warn!("Không thể so sánh confidence score: {} vs {}, coi là bằng nhau", 
                            a.confidence_score, b.confidence_score);
                        std::cmp::Ordering::Equal
                    })
            }
        }
    });
    
    opportunities
}

/// Thống kê giao dịch
#[derive(Debug, Clone)]
pub struct TradingStatistics {
    /// Số lượng giao dịch đã thực hiện
    pub transaction_count: u64,
    /// Tỷ lệ giao dịch thành công (có lợi nhuận)
    pub success_rate: f64,
    /// Lợi nhuận trung bình mỗi giao dịch thành công
    pub average_profit: f64,
    /// Mức độ chuyên môn (1-10)
    pub expertise_level: u8,
    /// Thời gian giao dịch tích lũy (giờ)
    pub trading_hours: f64,
}

/// Cơ hội theo dõi order flow
#[derive(Debug, Clone)]
pub struct OrderFlowOpportunity {
    /// Địa chỉ trader
    pub trader_address: String,
    /// Mức độ chuyên môn (1-10)
    pub expertise_level: u8,
    /// Tỷ lệ thành công
    pub success_rate: f64,
    /// Số lượng giao dịch
    pub transaction_count: u64,
    /// Lợi nhuận trung bình
    pub average_profit: f64,
    /// Các giao dịch hiện tại đang theo dõi
    pub current_transactions: Vec<String>,
    /// Ước tính lợi nhuận tiềm năng
    pub potential_profit_estimate: f64,
    /// Điểm đánh giá mức độ tự tin (0-100)
    pub confidence_score: f64,
}

/// Tính toán điểm tự tin cho order flow
fn calculate_flow_confidence(stats: &TradingStatistics) -> f64 {
    // Tỷ lệ thành công có trọng số lớn nhất
    let success_component = stats.success_rate * 50.0;
    
    // Mức độ chuyên môn cũng quan trọng
    let expertise_component = stats.expertise_level as f64 * 5.0;
    
    // Số lượng giao dịch phản ánh kinh nghiệm
    let transaction_component = (stats.transaction_count as f64).min(1000.0) / 1000.0 * 20.0;
    
    // Thời gian giao dịch
    let time_component = (stats.trading_hours / 100.0).min(1.0) * 10.0;
    
    // Tổng hợp
    let confidence = success_component + expertise_component + transaction_component + time_component;
    
    // Giới hạn trong khoảng 0-100
    confidence.min(100.0)
}

/// Phân tích các cơ hội đã phát hiện và điều chỉnh theo tình trạng competition
pub fn analyze_opportunities(
    opportunities: &mut Vec<MevOpportunity>,
    current_block_number: u64,
    max_opportunities_per_pool: usize
) -> Result<Vec<MevOpportunity>, anyhow::Error> {
    use tracing::{debug, warn, info};
    
    // Clone danh sách để phân tích và không thay đổi input
    let mut result = opportunities.clone();
    
    // Nhóm các cơ hội theo pool address
    let mut opportunities_by_pool: HashMap<String, Vec<usize>> = HashMap::new();
    
    // Tạo map nhóm cơ hội theo pool address
    for (idx, opportunity) in result.iter().enumerate() {
        let pool_address = match opportunity.parameters.get("pool_address")
            .or_else(|| opportunity.parameters.get("pair_address")) {
                Some(address) => address.clone(),
                None => {
                    // Sử dụng một giá trị mặc định có ý nghĩa và log warning
                    warn!("Không tìm thấy pool_address hoặc pair_address cho opportunity có profit: ${:.2}. Sử dụng unknown_pool.",
                         opportunity.estimated_profit_usd);
                    "unknown_pool".to_string()
                }
            };
        
        opportunities_by_pool
            .entry(pool_address)
            .or_insert_with(Vec::new)
            .push(idx);
    }
    
    // Đánh dấu các cơ hội bị cạnh tranh cao
    for (pool, indices) in opportunities_by_pool.iter() {
        if indices.len() > max_opportunities_per_pool {
            info!("Pool {} có competition cao: {} opportunities targeting cùng pool", 
                 pool, indices.len());
            
            // Sắp xếp theo lợi nhuận ước tính giảm dần
            let mut pool_opportunities: Vec<(usize, &MevOpportunity)> = indices
                .iter()
                .map(|&idx| (idx, &result[idx]))
                .collect();
            
            // Xử lý an toàn sort_by với floating point, tránh panic với NaN
            pool_opportunities.sort_by(|(_, a), (_, b)| {
                // Trước tiên kiểm tra giá trị NaN
                match (a.estimated_profit_usd.is_nan(), b.estimated_profit_usd.is_nan()) {
                    (true, true) => std::cmp::Ordering::Equal, // Cả hai là NaN
                    (true, false) => std::cmp::Ordering::Less, // NaN đặt cuối
                    (false, true) => std::cmp::Ordering::Greater, // NaN đặt cuối
                    (false, false) => {
                        // Thực hiện so sánh bình thường khi cả hai không phải NaN
                        b.estimated_profit_usd.partial_cmp(&a.estimated_profit_usd)
                            .unwrap_or_else(|| {
                                warn!("Không thể so sánh profit: ${:.2} vs ${:.2}, coi là bằng nhau",
                                     a.estimated_profit_usd, b.estimated_profit_usd);
                                std::cmp::Ordering::Equal
                            })
                    }
                }
            });
            
            // Điều chỉnh profit estimate dựa trên competition
            for (i, (idx, _)) in pool_opportunities.iter().enumerate() {
                let competition_factor = if i < max_opportunities_per_pool {
                    1.0 - (i as f64 * 0.1) // Giảm dần đến 10% cho mỗi cơ hội
                } else {
                    // Giảm đáng kể profit estimate cho các cơ hội thấp hơn
                    0.3 - (i as f64 - max_opportunities_per_pool as f64) * 0.1
                };
                
                // Đảm bảo competition factor không âm
                let competition_factor = competition_factor.max(0.1);
                
                // Điều chỉnh lợi nhuận ước tính
                let opportunity = &mut result[*idx];
                let original_profit = opportunity.estimated_profit_usd;
                
                // Kiểm tra NaN trước khi nhân
                if !original_profit.is_nan() {
                    opportunity.estimated_profit_usd *= competition_factor;
                } else {
                    warn!("Phát hiện profit NaN cho opportunity, đặt về 0.0");
                    opportunity.estimated_profit_usd = 0.0;
                }
                
                // Thêm thông tin về competition vào parameters
                opportunity.parameters.insert(
                    "competition_level".to_string(), 
                    indices.len().to_string()
                );
                opportunity.parameters.insert(
                    "competition_factor".to_string(), 
                    competition_factor.to_string()
                );
                
                debug!("Điều chỉnh profit cho opportunity trong pool {}: ${:.2} -> ${:.2} (competition factor: {:.2})",
                     pool, original_profit, opportunity.estimated_profit_usd, competition_factor);
            }
        }
    }
    
    // Lọc lại các cơ hội sau khi điều chỉnh, loại bỏ các cơ hội có lợi nhuận thấp hoặc NaN
    result.retain(|opportunity| {
        let min_profit_threshold = 5.0; // Ngưỡng lợi nhuận tối thiểu sau khi điều chỉnh
        !opportunity.estimated_profit_usd.is_nan() && opportunity.estimated_profit_usd >= min_profit_threshold
    });
    
    // Sắp xếp kết quả theo lợi nhuận ước tính giảm dần, xử lý an toàn NaN
    result.sort_by(|a, b| {
        match (a.estimated_profit_usd.is_nan(), b.estimated_profit_usd.is_nan()) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => {
                b.estimated_profit_usd.partial_cmp(&a.estimated_profit_usd)
                    .unwrap_or_else(|| {
                        warn!("Không thể so sánh profit giữa hai opportunity, coi là bằng nhau");
                        std::cmp::Ordering::Equal
                    })
            }
        }
    });
    
    Ok(result)
} 