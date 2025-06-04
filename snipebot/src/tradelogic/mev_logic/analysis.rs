/// MEV analysis utilities
use std::collections::{HashMap, HashSet};
use tracing::{info, debug, warn};
use crate::analys::mempool::{MempoolTransaction, TransactionType, SuspiciousPattern};
use super::types::{TraderBehaviorType, TraderExpertiseLevel, MevOpportunityType};
use super::constants::*;

/// Detect sandwich trading pattern in a set of transactions
pub fn detect_sandwich_pattern(swaps: &[&MempoolTransaction]) -> bool {
    if swaps.len() < 3 {
        return false;
    }
    
    // Look for a pattern where:
    // 1. Small buy of token X
    // 2. Large buy of token X
    // 3. Small sell of token X from same address as #1
    
    // This is a simplified implementation
    for i in 0..swaps.len() - 2 {
        // Check if it's a buy
        if let Some(swap_info) = &swaps[i].swap_info {
            let first_address = &swaps[i].from_address;
            let first_token = &swap_info.to_token.address;
            let first_amount = swap_info.to_amount;
            
            // Look for a large swap in the same token
            if let Some(second_swap) = &swaps[i+1].swap_info {
                if &second_swap.to_token.address == first_token && second_swap.to_amount > first_amount * 5.0 {
                    
                    // Look for a sell from the same address as the first
                    if let Some(third_swap) = &swaps[i+2].swap_info {
                        if &swaps[i+2].from_address == first_address && 
                           &third_swap.from_token.address == first_token {
                            return true;
                        }
                    }
                }
            }
        }
    }
    
    false
}

/// Detect front-running pattern in a set of transactions
pub fn detect_front_running_pattern(swaps: &[&MempoolTransaction]) -> bool {
    if swaps.len() < 2 {
        return false;
    }
    
    // Look for a pattern where:
    // 1. Transaction with high gas price buying the same token as
    // 2. A pending transaction with lower gas price
    
    // This is a simplified implementation
    for i in 0..swaps.len() - 1 {
        if let (Some(first_swap), Some(second_swap)) = (&swaps[i].swap_info, &swaps[i+1].swap_info) {
            // Same target token
            if first_swap.to_token.address == second_swap.to_token.address {
                // First transaction has higher gas price
                if swaps[i].gas_price > swaps[i+1].gas_price * 1.1 {
                    // Different sender addresses
                    if swaps[i].from_address != swaps[i+1].from_address {
                        return true;
                    }
                }
            }
        }
    }
    
    false
}

/// Detect abnormal liquidity pattern in a set of transactions
pub fn detect_abnormal_liquidity_pattern(events: &[&MempoolTransaction]) -> bool {
    if events.len() < 2 {
        return false;
    }
    
    // Track liquidity additions and removals for each token
    let mut token_liquidity_events: HashMap<String, Vec<(&MempoolTransaction, bool)>> = HashMap::new();
    
    // Collect liquidity events by token
    for event in events {
        if let Some(liq_info) = &event.liquidity_info {
            let token_address = &liq_info.token_address;
            let is_add = liq_info.is_addition;
            
            token_liquidity_events
                .entry(token_address.clone())
                .or_insert_with(Vec::new)
                .push((event, is_add));
        }
    }
    
    // Look for suspicious patterns
    for (token, events) in token_liquidity_events {
        if events.len() < 2 {
            continue;
        }
        
        // Sort by timestamp
        let mut sorted_events = events.clone();
        sorted_events.sort_by_key(|(tx, _)| tx.timestamp);
        
        // Check for rapid addition followed by removal
        for i in 0..sorted_events.len() - 1 {
            let (first_tx, first_is_add) = sorted_events[i];
            let (second_tx, second_is_add) = sorted_events[i+1];
            
            // Addition followed by removal
            if *first_is_add && !second_is_add {
                // Time difference in seconds
                let time_diff = second_tx.timestamp - first_tx.timestamp;
                
                // Suspicious if removal happens within 10 minutes of addition
                if time_diff < 600 {
                    // From same address
                    if first_tx.from_address == second_tx.from_address {
                        return true;
                    }
                    
                    // Or if removal is large compared to pool size
                    if let (Some(first_liq), Some(second_liq)) = (&first_tx.liquidity_info, &second_tx.liquidity_info) {
                        if second_liq.value > first_liq.value * 0.7 {
                            return true;
                        }
                    }
                }
            }
        }
    }
    
    false
}

/// Analyze trader behavior from a list of transactions
pub fn analyze_trader_behavior(
    transactions: &[&MempoolTransaction], 
    address: &str
) -> Option<super::types::TraderBehaviorAnalysis> {
    // Filter transactions by the address
    let trader_txs: Vec<_> = transactions.iter()
        .filter(|tx| tx.from_address == address)
        .collect();
    
    if trader_txs.is_empty() {
        return None;
    }
    
    // Analyze gas behavior
    let gas_prices: Vec<f64> = trader_txs.iter()
        .map(|tx| tx.gas_price)
        .collect();
    
    let avg_gas_price = if !gas_prices.is_empty() {
        gas_prices.iter().sum::<f64>() / gas_prices.len() as f64
    } else {
        0.0
    };
    
    let highest_gas_price = gas_prices.iter().fold(0.0, |max, &price| max.max(price));
    let lowest_gas_price = gas_prices.iter().fold(f64::INFINITY, |min, &price| min.min(price));
    
    // Check if trader has a gas strategy by seeing if they adjust gas prices
    let has_gas_strategy = highest_gas_price > lowest_gas_price * 1.5;
    
    // Calculate success rate
    let success_rate = trader_txs.iter()
        .filter(|tx| tx.status.is_some() && tx.status.unwrap())
        .count() as f64 / trader_txs.len() as f64;
    
    // Identify common transaction types
    let mut tx_type_counts: HashMap<TransactionType, usize> = HashMap::new();
    for tx in trader_txs.iter() {
        *tx_type_counts.entry(tx.transaction_type.clone()).or_insert(0) += 1;
    }
    
    let mut common_types = Vec::new();
    for (tx_type, count) in tx_type_counts {
        if count >= trader_txs.len() / 5 { // At least 20% of transactions
            common_types.push(tx_type);
        }
    }
    
    // Identify frequently traded tokens
    let mut token_counts: HashMap<String, usize> = HashMap::new();
    for tx in trader_txs.iter() {
        if let Some(swap) = &tx.swap_info {
            *token_counts.entry(swap.from_token.address.clone()).or_insert(0) += 1;
            *token_counts.entry(swap.to_token.address.clone()).or_insert(0) += 1;
        }
    }
    
    let mut frequent_tokens = Vec::new();
    for (token, count) in token_counts {
        if count >= 2 { // Traded at least twice
            frequent_tokens.push(token);
        }
    }
    
    // Identify preferred DEXes
    let mut dex_counts: HashMap<String, usize> = HashMap::new();
    for tx in trader_txs.iter() {
        if let Some(contract) = &tx.contract_name {
            *dex_counts.entry(contract.clone()).or_insert(0) += 1;
        }
    }
    
    let mut preferred_dexes = Vec::new();
    for (dex, count) in dex_counts {
        if count >= 2 { // Used at least twice
            preferred_dexes.push(dex);
        }
    }
    
    // Calculate activity hours
    let mut hour_counts: HashMap<u8, usize> = HashMap::new();
    for tx in trader_txs.iter() {
        // Convert timestamp to hour (0-23)
        let hour = ((tx.timestamp % 86400) / 3600) as u8;
        *hour_counts.entry(hour).or_insert(0) += 1;
    }
    
    let mut active_hours = Vec::new();
    for (hour, count) in hour_counts {
        if count >= trader_txs.len() / 10 { // At least 10% of activity in this hour
            active_hours.push(hour);
        }
    }
    
    // Determine trader type and expertise
    let trader_type = if has_gas_strategy && avg_gas_price > 100.0 && trader_txs.len() > 10 {
        if common_types.contains(&TransactionType::Swap) {
            TraderBehaviorType::MevBot
        } else {
            TraderBehaviorType::HighFrequencyTrader
        }
    } else if trader_txs.iter().any(|tx| tx.value_usd > HIGH_VALUE_TX_ETH as f64 * 1000.0) {
        TraderBehaviorType::Whale
    } else if preferred_dexes.len() > 3 {
        TraderBehaviorType::Arbitrageur
    } else {
        TraderBehaviorType::Retail
    };
    
    let expertise_level = if avg_gas_price > 100.0 && has_gas_strategy {
        TraderExpertiseLevel::Professional
    } else if avg_gas_price > 50.0 || has_gas_strategy {
        TraderExpertiseLevel::Intermediate
    } else {
        TraderExpertiseLevel::Beginner
    };
    
    // Transaction frequency (transactions per hour)
    let duration_hours = if trader_txs.len() >= 2 {
        let latest = trader_txs.iter().map(|tx| tx.timestamp).max().unwrap_or(0);
        let earliest = trader_txs.iter().map(|tx| tx.timestamp).min().unwrap_or(0);
        let duration_secs = latest - earliest;
        duration_secs as f64 / 3600.0
    } else {
        1.0 // Default to 1 hour if we can't calculate
    };
    
    let tx_frequency = if duration_hours > 0.0 {
        trader_txs.len() as f64 / duration_hours
    } else {
        trader_txs.len() as f64 // All in the same hour
    };
    
    // Average transaction value
    let avg_tx_value = trader_txs.iter()
        .map(|tx| tx.value_usd)
        .sum::<f64>() / trader_txs.len() as f64;
    
    // Create the behavior analysis
    Some(super::types::TraderBehaviorAnalysis {
        address: address.to_string(),
        behavior_type: trader_type,
        expertise_level,
        transaction_frequency: tx_frequency,
        average_transaction_value: avg_tx_value,
        common_transaction_types: common_types,
        gas_behavior: super::types::GasBehavior {
            average_gas_price: avg_gas_price,
            highest_gas_price,
            lowest_gas_price: if lowest_gas_price == f64::INFINITY { 0.0 } else { lowest_gas_price },
            has_gas_strategy,
            success_rate,
        },
        frequently_traded_tokens: frequent_tokens,
        preferred_dexes,
        active_hours,
        prediction_score: 75.0, // Arbitrary score
        additional_notes: None,
    })
}

/// Detect potential arbitrage opportunities between two DEXes
pub fn detect_arbitrage_opportunities(
    transactions: &[&MempoolTransaction],
    current_dex_prices: &HashMap<String, HashMap<String, f64>>,
) -> Vec<(String, String, String, f64)> {
    let mut opportunities = Vec::new();
    
    // Scan for potential price differences created by mempool transactions
    for tx in transactions {
        if let Some(swap_info) = &tx.swap_info {
            let token_in = &swap_info.from_token.address;
            let token_out = &swap_info.to_token.address;
            let dex_name = tx.contract_name.as_ref().unwrap_or(&"unknown".to_string()).clone();
            
            // Skip if we don't have price info
            if !current_dex_prices.contains_key(&dex_name) {
                continue;
            }
            
            // Look for price differences in other DEXs
            for (other_dex, prices) in current_dex_prices {
                if other_dex == &dex_name {
                    continue;
                }
                
                // Check if both DEXs have these tokens
                if let (Some(price_dex1), Some(price_dex2)) = (
                    current_dex_prices.get(&dex_name).and_then(|p| p.get(token_out)),
                    prices.get(token_out)
                ) {
                    // Calculate price difference percentage
                    let price_diff_pct = (price_dex2 - price_dex1).abs() / price_dex1 * 100.0;
                    
                    // If difference is significant
                    if price_diff_pct > MIN_PRICE_DIFFERENCE_PERCENT {
                        opportunities.push((
                            token_in.clone(),
                            token_out.clone(),
                            other_dex.clone(),
                            price_diff_pct
                        ));
                    }
                }
            }
        }
    }
    
    opportunities
}

/// Detect multi-hop arbitrage opportunities across multiple DEXes
pub fn detect_multihop_arbitrage(
    dex_prices: &HashMap<String, HashMap<String, f64>>,
    max_hops: usize,
) -> Vec<(Vec<String>, Vec<String>, f64)> {
    let mut opportunities = Vec::new();
    
    // Get all tokens across all DEXes
    let mut all_tokens = HashSet::new();
    for prices in dex_prices.values() {
        for token in prices.keys() {
            all_tokens.insert(token.clone());
        }
    }
    
    // Convert to vector for iteration
    let tokens: Vec<String> = all_tokens.into_iter().collect();
    
    // For each token, try to find a profitable path that goes back to itself
    for start_token in &tokens {
        find_arbitrage_paths(
            start_token,
            start_token,
            dex_prices,
            &Vec::new(),
            &Vec::new(),
            1.0,
            max_hops,
            &mut opportunities
        );
    }
    
    opportunities
}

/// Recursive helper function to find arbitrage paths
fn find_arbitrage_paths(
    start_token: &str,
    current_token: &str,
    dex_prices: &HashMap<String, HashMap<String, f64>>,
    current_path: &Vec<String>,
    current_dexes: &Vec<String>,
    current_ratio: f64,
    max_hops: usize,
    opportunities: &mut Vec<(Vec<String>, Vec<String>, f64)>
) {
    // Base case: if we've returned to the start token and have at least one hop
    if current_token == start_token && !current_path.is_empty() {
        // Check if profitable (ratio > 1.0 means profit)
        if current_ratio > 1.0 + (MIN_PRICE_DIFFERENCE_PERCENT / 100.0) {
            let profit_percentage = (current_ratio - 1.0) * 100.0;
            opportunities.push((
                current_path.clone(),
                current_dexes.clone(),
                profit_percentage
            ));
        }
        return;
    }
    
    // Stop if we've reached max hops or path is getting too long
    if current_path.len() >= max_hops {
        return;
    }
    
    // Try each DEX for the next hop
    for (dex_name, prices) in dex_prices {
        // Skip if we don't have current token in this DEX
        if !prices.contains_key(current_token) {
            continue;
        }
        
        // Try each token we could swap to
        for (next_token, price) in prices {
            // Skip self-swaps
            if next_token == current_token {
                continue;
            }
            
            // Update the path, dexes, and ratio
            let mut new_path = current_path.clone();
            new_path.push(next_token.clone());
            
            let mut new_dexes = current_dexes.clone();
            new_dexes.push(dex_name.clone());
            
            let new_ratio = current_ratio * price;
            
            // Recursive call
            find_arbitrage_paths(
                start_token,
                next_token,
                dex_prices,
                &new_path,
                &new_dexes,
                new_ratio,
                max_hops,
                opportunities
            );
        }
    }
}

/// Detect new token launches based on mempool transactions
pub fn detect_new_token_launches(
    transactions: &[&MempoolTransaction],
    known_tokens: &HashSet<String>,
) -> Vec<(String, String, f64)> {
    let mut new_tokens = Vec::new();
    
    for tx in transactions {
        // Check for token creation/initial liquidity
        if tx.transaction_type == TransactionType::AddLiquidity {
            if let Some(liq_info) = &tx.liquidity_info {
                let token_address = &liq_info.token_address;
                
                // Skip if already known
                if known_tokens.contains(token_address) {
                    continue;
                }
                
                // New token found, include address, timestamp, and initial liquidity
                new_tokens.push((
                    token_address.clone(),
                    liq_info.pair_address.clone(),
                    liq_info.value
                ));
            }
        }
    }
    
    new_tokens
}

/// Estimate transaction impact on a token's price
pub fn estimate_price_impact(
    tx: &MempoolTransaction,
    pool_reserves: Option<(f64, f64)>,
) -> Option<f64> {
    if let Some(swap_info) = &tx.swap_info {
        if let Some((reserve_in, reserve_out)) = pool_reserves {
            // Simple constant product formula: price impact = 1 - (reserve_in / (reserve_in + amount_in))
            let amount_in = swap_info.from_amount;
            let price_impact = 1.0 - (reserve_in / (reserve_in + amount_in));
            return Some(price_impact * 100.0); // as percentage
        }
    }
    None
}

/// Phân tích giao dịch để tìm cơ hội MEV tiềm năng
pub fn analyze_transaction_for_mev(
    tx: &MempoolTransaction,
    dex_reserves: &HashMap<String, HashMap<String, (f64, f64)>>,
    known_tokens: &HashSet<String>
) -> Vec<(MevOpportunityType, f64, HashMap<String, String>)> {
    let mut opportunities = Vec::new();
    
    // Phát hiện cơ hội arbitrage
    if let Some(swap_info) = &tx.swap_info {
        // Phân tích price impact
        if let Some(contract_name) = &tx.contract_name {
            if let Some(reserves_map) = dex_reserves.get(contract_name) {
                let pool_key = format!("{}-{}", 
                    swap_info.from_token.address, 
                    swap_info.to_token.address
                );
                
                if let Some(reserves) = reserves_map.get(&pool_key) {
                    let impact = estimate_price_impact(tx, Some(*reserves));
                    
                    if let Some(price_impact) = impact {
                        if price_impact > 3.0 {
                            // Có tiềm năng arbitrage khi price impact > 3%
                            let mut params = HashMap::new();
                            params.insert("from_token".to_string(), swap_info.from_token.address.clone());
                            params.insert("to_token".to_string(), swap_info.to_token.address.clone());
                            params.insert("dex".to_string(), contract_name.clone());
                            params.insert("tx_hash".to_string(), tx.hash.clone());
                            
                            opportunities.push((
                                MevOpportunityType::Arbitrage,
                                price_impact,
                                params
                            ));
                        }
                    }
                }
            }
        }
    }
    
    // Phát hiện token mới
    if tx.transaction_type == TransactionType::AddLiquidity {
        if let Some(liq_info) = &tx.liquidity_info {
            if !known_tokens.contains(&liq_info.token_address) {
                let mut params = HashMap::new();
                params.insert("token_address".to_string(), liq_info.token_address.clone());
                params.insert("pair_address".to_string(), liq_info.pair_address.clone());
                params.insert("initial_liquidity".to_string(), liq_info.value.to_string());
                
                opportunities.push((
                    MevOpportunityType::NewToken,
                    liq_info.value,
                    params
                ));
            }
        }
    }
    
    // Phát hiện sandwich opportunity
    if let Some(swap_info) = &tx.swap_info {
        if swap_info.from_amount > MIN_TX_VALUE_ETH * 2.0 { // Giao dịch đủ lớn
            let mut params = HashMap::new();
            params.insert("from_token".to_string(), swap_info.from_token.address.clone());
            params.insert("to_token".to_string(), swap_info.to_token.address.clone());
            params.insert("amount".to_string(), swap_info.from_amount.to_string());
            params.insert("tx_hash".to_string(), tx.hash.clone());
            
            opportunities.push((
                MevOpportunityType::Sandwich,
                swap_info.from_amount,
                params
            ));
        }
    }
    
    opportunities
}

/// Phát hiện MEV bot từ các giao dịch mempool
pub fn detect_mev_bots(
    transactions: &[&MempoolTransaction],
    high_frequency_threshold: usize,
    time_window_sec: u64
) -> Vec<(String, usize, f64)> {
    // Thống kê số lượng giao dịch theo địa chỉ
    let mut tx_count_by_address: HashMap<String, usize> = HashMap::new();
    let mut gas_sum_by_address: HashMap<String, f64> = HashMap::new();
    let mut swap_count_by_address: HashMap<String, usize> = HashMap::new();
    
    // Tìm timestamp lớn nhất làm thời gian hiện tại
    let latest_tx = transactions.iter()
        .max_by_key(|tx| tx.timestamp)
        .map(|tx| tx.timestamp)
        .unwrap_or(0);
    
    for tx in transactions {
        // Chỉ xem xét giao dịch trong cửa sổ thời gian
        if latest_tx - tx.timestamp > time_window_sec {
            continue;
        }
        
        // Cập nhật số lượng giao dịch
        *tx_count_by_address.entry(tx.from_address.clone()).or_insert(0) += 1;
        
        // Cập nhật tổng gas price
        *gas_sum_by_address.entry(tx.from_address.clone()).or_insert(0.0) += tx.gas_price;
        
        // Đếm riêng các giao dịch swap
        if tx.transaction_type == TransactionType::Swap {
            *swap_count_by_address.entry(tx.from_address.clone()).or_insert(0) += 1;
        }
    }
    
    // Lọc ra các địa chỉ có dấu hiệu của MEV bot
    let mut potential_mev_bots = Vec::new();
    
    for (address, count) in tx_count_by_address {
        if count >= high_frequency_threshold {
            let avg_gas = gas_sum_by_address.get(&address).unwrap_or(&0.0) / count as f64;
            let swap_count = swap_count_by_address.get(&address).unwrap_or(&0).clone();
            
            // MEV bots thường có gas price cao và số lượng giao dịch swap lớn
            if avg_gas > 0.0 && (swap_count as f64 / count as f64) > 0.5 {
                potential_mev_bots.push((address, count, avg_gas));
            }
        }
    }
    
    // Sắp xếp theo số lượng giao dịch giảm dần
    potential_mev_bots.sort_by(|a, b| b.1.cmp(&a.1));
    
    potential_mev_bots
}

/// Phát hiện token mới được thêm thanh khoản
pub fn detect_new_tokens(
    transactions: &[&MempoolTransaction],
    known_tokens: &HashSet<String>
) -> Vec<(String, String, f64, u64)> {
    let mut new_tokens = Vec::new();
    
    for tx in transactions {
        if tx.transaction_type == TransactionType::AddLiquidity {
            if let Some(liq_info) = &tx.liquidity_info {
                let token_address = &liq_info.token_address;
                
                // Bỏ qua nếu token đã biết
                if known_tokens.contains(token_address) {
                    continue;
                }
                
                // Thêm thông tin token mới phát hiện được
                new_tokens.push((
                    token_address.clone(),
                    liq_info.pair_address.clone(),
                    liq_info.value,
                    tx.timestamp
                ));
            }
        }
    }
    
    new_tokens
}

/// Tìm các giao dịch liên quan đến một token cụ thể
pub fn find_transactions_for_token(
    token_address: &str,
    transactions: &[&MempoolTransaction]
) -> Vec<&MempoolTransaction> {
    transactions.iter()
        .filter(|tx| {
            // Kiểm tra trong swap_info
            if let Some(swap_info) = &tx.swap_info {
                if swap_info.from_token.address == token_address || 
                   swap_info.to_token.address == token_address {
                    return true;
                }
            }
            
            // Kiểm tra trong liquidity_info
            if let Some(liq_info) = &tx.liquidity_info {
                if liq_info.token_address == token_address {
                    return true;
                }
            }
            
            false
        })
        .copied()
        .collect()
}

/// Phân tích DEX metrics từ các giao dịch
pub fn analyze_dex_metrics(
    transactions: &[&MempoolTransaction],
    window_sec: u64
) -> HashMap<String, HashMap<String, f64>> {
    let mut dex_metrics: HashMap<String, HashMap<String, f64>> = HashMap::new();
    let mut dex_token_volumes: HashMap<String, HashMap<String, f64>> = HashMap::new();
    let mut dex_transaction_counts: HashMap<String, usize> = HashMap::new();
    
    // Tìm timestamp lớn nhất làm thời gian hiện tại
    let latest_tx = transactions.iter()
        .max_by_key(|tx| tx.timestamp)
        .map(|tx| tx.timestamp)
        .unwrap_or(0);
    
    // Lọc giao dịch trong khoảng thời gian
    let recent_txs: Vec<&&MempoolTransaction> = transactions.iter()
        .filter(|tx| latest_tx - tx.timestamp <= window_sec)
        .collect();
    
    // Tính toán các metric
    for tx in recent_txs {
        if let Some(contract_name) = &tx.contract_name {
            // Tổng số giao dịch
            *dex_transaction_counts.entry(contract_name.clone()).or_insert(0) += 1;
            
            // Khối lượng giao dịch theo token
            if let Some(swap_info) = &tx.swap_info {
                let from_token = &swap_info.from_token.address;
                let volume = swap_info.from_amount * swap_info.from_token.price_usd;
                
                dex_token_volumes
                    .entry(contract_name.clone())
                    .or_insert_with(HashMap::new)
                    .entry(from_token.clone())
                    .and_modify(|v| *v += volume)
                    .or_insert(volume);
            }
        }
    }
    
    // Tính trung bình gas price cho mỗi DEX
    let mut dex_gas_prices: HashMap<String, Vec<f64>> = HashMap::new();
    for tx in recent_txs {
        if let Some(contract_name) = &tx.contract_name {
            dex_gas_prices
                .entry(contract_name.clone())
                .or_insert_with(Vec::new)
                .push(tx.gas_price);
        }
    }
    
    // Hoàn thiện metrics
    for (dex, count) in dex_transaction_counts {
        let mut metrics = HashMap::new();
        
        // Tổng giao dịch
        metrics.insert("transaction_count".to_string(), count as f64);
        
        // Tổng khối lượng
        let total_volume = dex_token_volumes
            .get(&dex)
            .map(|volumes| volumes.values().sum::<f64>())
            .unwrap_or(0.0);
        metrics.insert("total_volume_usd".to_string(), total_volume);
        
        // Gas price trung bình
        if let Some(gas_prices) = dex_gas_prices.get(&dex) {
            if !gas_prices.is_empty() {
                let avg_gas = gas_prices.iter().sum::<f64>() / gas_prices.len() as f64;
                metrics.insert("average_gas_price".to_string(), avg_gas);
            }
        }
        
        // Số token đang giao dịch
        if let Some(volumes) = dex_token_volumes.get(&dex) {
            metrics.insert("token_count".to_string(), volumes.len() as f64);
        }
        
        dex_metrics.insert(dex, metrics);
    }
    
    dex_metrics
} 