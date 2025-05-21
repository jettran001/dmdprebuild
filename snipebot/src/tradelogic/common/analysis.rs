/// Common analysis functions shared between trade logic modules
///
/// This module contains analysis utilities that are used by both smart_trade and mev_logic,
/// providing consistent analysis methods and reducing code duplication.

use std::collections::HashMap;
use anyhow::{Result, Context};
use tracing::{debug, error, info, warn};

use crate::analys::token_status::{ContractInfo, TokenStatus};
use crate::chain_adapters::evm_adapter::EvmAdapter;
use super::types::{TokenIssue, TraderBehaviorAnalysis, TraderBehaviorType, TraderExpertiseLevel, GasBehavior};

/// Analyzes trader behavior based on transaction history
///
/// # Arguments
/// * `address` - Trader's address
/// * `transactions` - Transaction history
///
/// # Returns
/// * `TraderBehaviorAnalysis` - Analysis of trader behavior
pub fn analyze_trader_behavior(
    address: &str,
    transactions: &[crate::analys::mempool::MempoolTransaction]
) -> TraderBehaviorAnalysis {
    let behavior_type = determine_trader_type(transactions);
    let expertise_level = determine_expertise_level(transactions);
    
    // Calculate transaction frequency (tx/hour)
    let transaction_frequency = if !transactions.is_empty() {
        transactions.len() as f64 / 24.0 // Simple average per hour over 24h
    } else {
        0.0
    };
    
    // Calculate average transaction value
    let total_value: f64 = transactions.iter().map(|tx| tx.value).sum();
    let average_value = if !transactions.is_empty() {
        total_value / transactions.len() as f64
    } else {
        0.0
    };
    
    // Extract common transaction types
    let mut tx_types = transactions.iter()
        .map(|tx| tx.tx_type.clone())
        .collect::<Vec<_>>();
    tx_types.sort();
    tx_types.dedup();
    
    // Analyze gas behavior
    let gas_behavior = analyze_gas_behavior(transactions);
    
    // Extract frequently traded tokens
    let mut tokens = transactions.iter()
        .filter_map(|tx| tx.to_token.as_ref().map(|t| t.address.clone()))
        .collect::<Vec<_>>();
    tokens.sort();
    tokens.dedup();
    
    // Extract preferred DEXes
    let mut dexes = transactions.iter()
        .filter_map(|tx| tx.dex.clone())
        .collect::<Vec<_>>();
    dexes.sort();
    dexes.dedup();
    
    // Determine active hours
    let active_hours = determine_active_hours(transactions);
    
    // Calculate prediction score based on consistency
    let prediction_score = calculate_prediction_score(transactions);
    
    // Create and return the analysis
    TraderBehaviorAnalysis {
        address: address.to_string(),
        behavior_type,
        expertise_level,
        transaction_frequency,
        average_transaction_value: average_value,
        common_transaction_types: tx_types,
        gas_behavior,
        frequently_traded_tokens: tokens,
        preferred_dexes: dexes,
        active_hours,
        prediction_score,
        additional_notes: None,
    }
}

/// Determines trader type based on transaction patterns
fn determine_trader_type(transactions: &[crate::analys::mempool::MempoolTransaction]) -> TraderBehaviorType {
    if transactions.is_empty() {
        return TraderBehaviorType::Unknown;
    }
    
    // Count transaction types
    let mut arb_count = 0;
    let mut swap_count = 0;
    let mut high_value_count = 0;
    
    for tx in transactions {
        match tx.tx_type {
            crate::analys::mempool::TransactionType::Arbitrage => arb_count += 1,
            crate::analys::mempool::TransactionType::Swap => swap_count += 1,
            _ => {},
        }
        
        if tx.value > 10.0 { // Consider >10 ETH/BNB as high value
            high_value_count += 1;
        }
    }
    
    // Calculate transaction frequency
    let tx_per_hour = transactions.len() as f64 / 24.0;
    
    // Determine type based on patterns
    if arb_count > transactions.len() / 3 {
        TraderBehaviorType::Arbitrageur
    } else if tx_per_hour > 20.0 {
        TraderBehaviorType::HighFrequencyTrader
    } else if high_value_count > transactions.len() / 3 {
        TraderBehaviorType::Whale
    } else if tx_per_hour > 10.0 {
        TraderBehaviorType::MevBot
    } else if swap_count > transactions.len() / 2 {
        TraderBehaviorType::Retail
    } else {
        TraderBehaviorType::Unknown
    }
}

/// Determines trader expertise level
fn determine_expertise_level(transactions: &[crate::analys::mempool::MempoolTransaction]) -> TraderExpertiseLevel {
    if transactions.is_empty() {
        return TraderExpertiseLevel::Unknown;
    }
    
    // Calculate success rate
    let success_count = transactions.iter()
        .filter(|tx| tx.status == crate::analys::mempool::TransactionStatus::Success)
        .count();
    let success_rate = success_count as f64 / transactions.len() as f64;
    
    // Calculate average gas strategy sophistication
    let avg_gas_price: f64 = transactions.iter()
        .map(|tx| tx.gas_price)
        .sum::<f64>() / transactions.len() as f64;
    
    let gas_price_variance: f64 = transactions.iter()
        .map(|tx| (tx.gas_price - avg_gas_price).powi(2))
        .sum::<f64>() / transactions.len() as f64;
    
    // High variance indicates sophisticated gas strategy
    let has_gas_strategy = gas_price_variance > 5.0;
    
    // Determine expertise based on indicators
    if success_rate > 0.95 && has_gas_strategy {
        if transactions.len() > 100 {
            TraderExpertiseLevel::Professional
        } else {
            TraderExpertiseLevel::Intermediate
        }
    } else if success_rate > 0.8 {
        TraderExpertiseLevel::Intermediate
    } else {
        TraderExpertiseLevel::Beginner
    }
}

/// Analyzes gas behavior patterns
fn analyze_gas_behavior(transactions: &[crate::analys::mempool::MempoolTransaction]) -> GasBehavior {
    if transactions.is_empty() {
        return GasBehavior {
            average_gas_price: 0.0,
            highest_gas_price: 0.0,
            lowest_gas_price: 0.0,
            has_gas_strategy: false,
            success_rate: 0.0,
        };
    }
    
    // Calculate gas statistics
    let average_gas_price: f64 = transactions.iter()
        .map(|tx| tx.gas_price)
        .sum::<f64>() / transactions.len() as f64;
    
    let highest_gas_price = transactions.iter()
        .map(|tx| tx.gas_price)
        .fold(0.0, |a, b| a.max(b));
    
    let lowest_gas_price = transactions.iter()
        .map(|tx| tx.gas_price)
        .fold(f64::INFINITY, |a, b| a.min(b));
    
    // Calculate gas price variance to detect strategy
    let gas_price_variance: f64 = transactions.iter()
        .map(|tx| (tx.gas_price - average_gas_price).powi(2))
        .sum::<f64>() / transactions.len() as f64;
    
    // High variance indicates sophisticated gas strategy
    let has_gas_strategy = gas_price_variance > 5.0;
    
    // Calculate success rate
    let success_count = transactions.iter()
        .filter(|tx| tx.status == crate::analys::mempool::TransactionStatus::Success)
        .count();
    let success_rate = success_count as f64 / transactions.len() as f64;
    
    GasBehavior {
        average_gas_price,
        highest_gas_price,
        lowest_gas_price,
        has_gas_strategy,
        success_rate,
    }
}

/// Determines active hours of trader
fn determine_active_hours(transactions: &[crate::analys::mempool::MempoolTransaction]) -> Vec<u8> {
    let mut hour_counts = vec![0; 24];
    
    for tx in transactions {
        let timestamp = tx.timestamp;
        let datetime = chrono::DateTime::from_timestamp(timestamp as i64, 0)
            .unwrap_or_else(|| chrono::Utc::now());
        let hour = datetime.hour() as usize;
        hour_counts[hour] += 1;
    }
    
    // Return hours with significant activity (>10% of max activity)
    let max_count = *hour_counts.iter().max().unwrap_or(&0);
    let threshold = max_count / 10;
    
    hour_counts.iter()
        .enumerate()
        .filter_map(|(hour, &count)| {
            if count > threshold {
                Some(hour as u8)
            } else {
                None
            }
        })
        .collect()
}

/// Calculates prediction score for trader behavior
fn calculate_prediction_score(transactions: &[crate::analys::mempool::MempoolTransaction]) -> f64 {
    if transactions.len() < 5 {
        return 30.0; // Low confidence with few transactions
    }
    
    // Calculate consistency factors
    
    // 1. Consistency in transaction type
    let mut type_counts = HashMap::new();
    for tx in transactions {
        *type_counts.entry(tx.tx_type.clone()).or_insert(0) += 1;
    }
    let max_type_count = type_counts.values().max().unwrap_or(&0);
    let type_consistency = *max_type_count as f64 / transactions.len() as f64;
    
    // 2. Consistency in DEX usage
    let mut dex_counts = HashMap::new();
    for tx in transactions {
        if let Some(dex) = &tx.dex {
            *dex_counts.entry(dex).or_insert(0) += 1;
        }
    }
    let max_dex_count = dex_counts.values().max().unwrap_or(&0);
    let dex_consistency = if dex_counts.is_empty() {
        0.5 // Neutral if no DEX data
    } else {
        *max_dex_count as f64 / transactions.len() as f64
    };
    
    // 3. Consistency in gas price (inverse of variance)
    let avg_gas_price: f64 = transactions.iter()
        .map(|tx| tx.gas_price)
        .sum::<f64>() / transactions.len() as f64;
    
    let gas_price_variance: f64 = transactions.iter()
        .map(|tx| (tx.gas_price - avg_gas_price).powi(2))
        .sum::<f64>() / transactions.len() as f64;
    
    // Normalize gas consistency (lower variance = higher consistency)
    let gas_consistency = 1.0 / (1.0 + gas_price_variance / 100.0);
    
    // 4. Consistency in time patterns
    let hour_consistency = determine_active_hours(transactions).len() as f64 / 24.0;
    let time_consistency = 1.0 - hour_consistency; // Fewer active hours = more consistent
    
    // Combine factors with weights
    let prediction_score = 
        type_consistency * 30.0 +
        dex_consistency * 20.0 +
        gas_consistency * 30.0 +
        time_consistency * 20.0;
    
    // Final score between 0-100
    prediction_score.min(100.0)
}

/// Detects token issues by analyzing contract code and behavior
///
/// # Arguments
/// * `contract_info` - Contract information including bytecode and source
/// * `adapter` - EVM adapter for blockchain interaction
///
/// # Returns
/// * `Result<Vec<TokenIssue>>` - List of detected issues
pub async fn detect_token_issues(
    contract_info: &ContractInfo,
    adapter: &std::sync::Arc<EvmAdapter>
) -> Result<Vec<TokenIssue>> {
    let mut issues = Vec::new();
    let token_status = TokenStatus::from_contract_info(contract_info, &[]);
    
    // Basic checks
    if !contract_info.is_verified {
        issues.push(TokenIssue::UnverifiedContract);
    }
    
    // Check for honeypot
    match adapter.simulate_sell_token(&contract_info.address, 0.01).await {
        Ok(can_sell) => {
            if !can_sell {
                issues.push(TokenIssue::Honeypot);
            }
        },
        Err(_) => {
            // Consider suspicious if can't determine
            issues.push(TokenIssue::Honeypot);
        }
    }
    
    // Check owner privileges
    let owner_analysis = token_status.analyze_owner_privileges(contract_info);
    if !owner_analysis.is_ownership_renounced {
        issues.push(TokenIssue::OwnershipNotRenounced);
        
        if owner_analysis.has_mint_authority {
            issues.push(TokenIssue::UnlimitedMintAuthority);
        }
        
        if owner_analysis.has_pause_authority || 
           owner_analysis.has_blacklist_authority ||
           owner_analysis.has_burn_authority {
            issues.push(TokenIssue::OwnerWithFullControl);
        }
    }
    
    if owner_analysis.can_retrieve_ownership {
        issues.push(TokenIssue::OwnershipRenounceBackdoor);
    }
    
    // Check proxy status
    if owner_analysis.is_proxy {
        issues.push(TokenIssue::ProxyContract);
        issues.push(TokenIssue::UpgradeableLogic);
    }
    
    // Check for trading restrictions
    if token_status.has_max_tx_or_wallet_limit(contract_info) {
        issues.push(TokenIssue::MaxTransactionLimit);
        issues.push(TokenIssue::MaxWalletLimit);
    }
    
    if token_status.has_trading_cooldown(contract_info) {
        issues.push(TokenIssue::TradingCooldown);
    }
    
    // Check for external calls and delegatecall
    if token_status.has_external_delegatecall(contract_info) {
        issues.push(TokenIssue::ExternalCalls);
        issues.push(TokenIssue::DelegateCall);
    }
    
    // Check tax information
    match adapter.get_token_tax_info(&contract_info.address).await {
        Ok((buy_tax, sell_tax)) => {
            if buy_tax > 10.0 || sell_tax > 10.0 {
                issues.push(TokenIssue::HighTax);
            }
            
            if (sell_tax - buy_tax).abs() > 3.0 {
                issues.push(TokenIssue::DynamicTax);
            }
        },
        Err(_) => {
            // Suspicious if can't determine tax
            issues.push(TokenIssue::HiddenFees);
        }
    }
    
    // Check liquidity
    match adapter.get_token_liquidity(&contract_info.address).await {
        Ok(liquidity) => {
            if liquidity < 5000.0 { // $5000 threshold
                issues.push(TokenIssue::LowLiquidity);
            }
        },
        Err(_) => {
            // Suspicious if can't determine liquidity
            issues.push(TokenIssue::LowLiquidity);
        }
    }
    
    Ok(issues)
} 