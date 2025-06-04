/// Common analysis functions shared between trade logic modules
///
/// This module contains analysis utilities that are used by both smart_trade and mev_logic,
/// providing consistent analysis methods and reducing code duplication.

// External imports
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{DateTime, Utc};

// Third-party imports
use anyhow::{Result, Context, anyhow};
use tracing::{debug, error, info, warn};

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::token_status::{
    ContractInfo, TokenStatus, TokenSafety, LiquidityEvent, 
    TokenIssue as TokenStatusIssue
};
use crate::analys::mempool::{MempoolTransaction, TransactionStatus, TransactionType};

// Module imports
use super::types::{
    TokenIssue, SecurityCheckResult, TraderBehaviorAnalysis,
    TraderBehaviorType, TraderExpertiseLevel, GasBehavior
};

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
    transactions: &[MempoolTransaction]
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
fn determine_trader_type(transactions: &[MempoolTransaction]) -> TraderBehaviorType {
    if transactions.is_empty() {
        return TraderBehaviorType::Unknown;
    }
    
    // Count transaction types
    let mut arb_count = 0;
    let mut swap_count = 0;
    let mut high_value_count = 0;
    
    for tx in transactions {
        match tx.tx_type {
            TransactionType::Arbitrage => arb_count += 1,
            TransactionType::Swap => swap_count += 1,
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
fn determine_expertise_level(transactions: &[MempoolTransaction]) -> TraderExpertiseLevel {
    if transactions.is_empty() {
        return TraderExpertiseLevel::Unknown;
    }
    
    // Calculate success rate
    let success_count = transactions.iter()
        .filter(|tx| tx.status == TransactionStatus::Success)
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
fn analyze_gas_behavior(transactions: &[MempoolTransaction]) -> GasBehavior {
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
        .filter(|tx| tx.status == TransactionStatus::Success)
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
fn determine_active_hours(transactions: &[MempoolTransaction]) -> Vec<u8> {
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
fn calculate_prediction_score(transactions: &[MempoolTransaction]) -> f64 {
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

/// Detects common security issues in token contracts
///
/// Analyzes contract source code and bytecode to identify potential security issues
/// such as honeypots, backdoors, and other malicious patterns.
///
/// # Arguments
/// * `contract_info` - Contract information including source code and bytecode
///
/// # Returns
/// * `Vec<TokenIssue>` - List of detected issues
pub fn detect_token_issues(contract_info: &ContractInfo) -> Vec<TokenIssue> {
    let mut issues = Vec::new();
    
    // Check if contract is unverified
    if !contract_info.is_verified {
        issues.push(TokenIssue::UnverifiedContract);
    }
    
    // Check for proxy implementation
    if is_proxy_contract(contract_info) {
        issues.push(TokenIssue::ProxyImplementation);
    }
    
    // Check for blacklist/whitelist functions
    let (has_blacklist, has_whitelist) = check_token_lists(contract_info);
    if has_blacklist {
        issues.push(TokenIssue::HasBlacklist);
    }
    if has_whitelist {
        issues.push(TokenIssue::HasWhitelist);
    }
    
    // Check for excessive owner privileges
    let privileges = check_owner_privileges(contract_info);
    if !privileges.is_empty() {
        issues.push(TokenIssue::ExcessiveOwnerPrivileges);
        
        // Add specific privilege issues
        if privileges.contains("mint") {
            issues.push(TokenIssue::UnlimitedMintAuthority);
        }
        if privileges.contains("selfdestruct") {
            issues.push(TokenIssue::ContractSelfDestruct);
        }
        if privileges.contains("delegatecall") {
            issues.push(TokenIssue::DelegateCall);
        }
    }
    
    issues
}

/// Checks if a token contract is likely a proxy
///
/// # Arguments
/// * `contract_info` - Contract information
///
/// # Returns
/// * `bool` - True if the contract is likely a proxy
pub fn is_proxy_contract(contract_info: &ContractInfo) -> bool {
    // Check for common proxy patterns in bytecode or source code
    let is_proxy_bytecode = if let Some(bytecode) = &contract_info.bytecode {
        // Look for delegatecall signatures (0xF4, 0x45, 0x14, 0xFC)
        bytecode.contains("f4") && 
        bytecode.contains("delegatecall") &&
        bytecode.len() < 5000 // Proxies typically have small bytecode
    } else {
        false
    };
    
    let is_proxy_source = if let Some(source) = &contract_info.source_code {
        // Look for proxy patterns in source code
        source.contains("delegatecall") &&
        (source.contains("implementation") || 
         source.contains("upgradeablility") ||
         source.contains("proxy") ||
         source.contains("delegate"))
    } else {
        false
    };
    
    is_proxy_bytecode || is_proxy_source
}

/// Checks if a token has blacklist or whitelist functionality
///
/// # Arguments
/// * `contract_info` - Contract information
///
/// # Returns
/// * `(bool, bool)` - (has_blacklist, has_whitelist)
pub fn check_token_lists(contract_info: &ContractInfo) -> (bool, bool) {
    let mut has_blacklist = false;
    let mut has_whitelist = false;
    
    if let Some(source) = &contract_info.source_code {
        // Check for blacklist patterns
        has_blacklist = source.contains("blacklist") || 
                        source.contains("blocklist") ||
                        source.contains("banned") ||
                        source.contains("denylist");
        
        // Check for whitelist patterns
        has_whitelist = source.contains("whitelist") ||
                        source.contains("allowlist");
    }
    
    // Check for blacklist in function signatures
    if let Some(abi) = &contract_info.abi {
        has_blacklist = has_blacklist || 
                        abi.contains("blacklist") ||
                        abi.contains("blocklist") ||
                        abi.contains("ban");
        
        has_whitelist = has_whitelist ||
                        abi.contains("whitelist") ||
                        abi.contains("allowlist");
    }
    
    (has_blacklist, has_whitelist)
}

/// Checks if a contract has blacklist functionality
///
/// # Arguments
/// * `contract_info` - Contract information
///
/// # Returns
/// * `bool` - True if the contract has blacklist functionality
pub fn has_blacklist_function(contract_info: &ContractInfo) -> bool {
    let (has_blacklist, _) = check_token_lists(contract_info);
    has_blacklist
}

/// Check owner privileges in a token contract
///
/// # Arguments
/// * `contract_info` - Contract information
///
/// # Returns
/// * `Vec<String>` - List of excessive privileges
pub fn check_owner_privileges(contract_info: &ContractInfo) -> Vec<String> {
    let mut privileges = Vec::new();
    
    if let Some(source) = &contract_info.source_code {
        // Check for unlimited minting
        if source.contains("mint") && 
           (source.contains("onlyOwner") || source.contains("only Owner") || source.contains("onlyAdmin")) {
            privileges.push("mint".to_string());
        }
        
        // Check for ability to pause transfers
        if source.contains("pause") && 
           (source.contains("transfer") || source.contains("trading")) {
            privileges.push("pause".to_string());
        }
        
        // Check for self-destruct capability
        if source.contains("selfdestruct") || source.contains("self-destruct") {
            privileges.push("selfdestruct".to_string());
        }
        
        // Check for delegatecall (can be dangerous)
        if source.contains("delegatecall") {
            privileges.push("delegatecall".to_string());
        }
        
        // Check for fee modification
        if (source.contains("fee") || source.contains("tax")) && 
           source.contains("set") {
            privileges.push("modify_fees".to_string());
        }
        
        // Check for ownership transfer
        if source.contains("transferOwnership") || 
           (source.contains("transfer") && source.contains("ownership")) {
            privileges.push("transfer_ownership".to_string());
        }
    }
    
    privileges
}

/// Convert TokenSafety to SecurityCheckResult
///
/// # Arguments
/// * `token_safety` - Token safety analysis result
/// * `token_address` - Token address
///
/// # Returns
/// * `SecurityCheckResult` - Security check result
pub fn convert_token_safety_to_security_check(
    token_safety: &TokenSafety,
    token_address: &str
) -> SecurityCheckResult {
    let mut issues = Vec::new();
    
    // Convert TokenSafety issues to TokenIssue
    if token_safety.is_honeypot {
        issues.push(TokenIssue::Honeypot);
    }
    
    if token_safety.buy_tax > 10.0 {
        issues.push(TokenIssue::HighBuyTax);
    }
    
    if token_safety.sell_tax > 10.0 {
        issues.push(TokenIssue::HighSellTax);
    }
    
    if token_safety.has_anti_whale {
        issues.push(TokenIssue::MaxWalletLimit);
    }
    
    if token_safety.has_trading_cooldown {
        issues.push(TokenIssue::TradingCooldown);
    }
    
    if token_safety.has_blacklist {
        issues.push(TokenIssue::HasBlacklist);
    }
    
    if token_safety.has_whitelist {
        issues.push(TokenIssue::HasWhitelist);
    }
    
    if token_safety.has_mint_function {
        issues.push(TokenIssue::UnlimitedMintAuthority);
    }
    
    // Calculate risk score based on issues
    let risk_score = if token_safety.is_honeypot {
        100 // Maximum risk for honeypots
    } else {
        let base_score = token_safety.risk_score.unwrap_or(50);
        let issue_penalty = issues.len() as u8 * 5;
        let final_score = base_score + issue_penalty;
        final_score.min(100)
    };
    
    // Create and return the result
    SecurityCheckResult {
        token_address: token_address.to_string(),
        issues,
        risk_score,
        safe_to_trade: risk_score < 70, // Threshold for safety
        extra_info: HashMap::new(),
    }
}

/// Analyze token security comprehensively
///
/// Performs a complete security analysis of a token by combining multiple
/// sources of information including contract analysis and on-chain behavior.
///
/// # Arguments
/// * `token_address` - Address of the token to check
/// * `chain_id` - Chain ID where the token exists
/// * `adapter` - EVM adapter for blockchain interaction
///
/// # Returns
/// * `Result<SecurityCheckResult>` - Security check result with risk score and issues
pub async fn analyze_token_security(
    token_address: &str,
    chain_id: u32,
    adapter: &Arc<EvmAdapter>
) -> Result<SecurityCheckResult> {
    // Get basic token info
    let token_info = adapter.get_token_info(token_address)
        .await
        .context("Failed to get token info")?;
    
    // Get contract source/bytecode for analysis
    let contract_info = adapter.get_contract_info(token_address)
        .await
        .context("Failed to get contract info")?;
    
    // Get token safety analysis
    let token_safety = adapter.analyze_token_safety(token_address)
        .await
        .context("Failed to analyze token safety")?;
    
    // Detect issues from contract info
    let mut contract_issues = detect_token_issues(&contract_info);
    
    // Convert token safety to security check result
    let mut result = convert_token_safety_to_security_check(&token_safety, token_address);
    
    // Merge issues from both sources (avoiding duplicates)
    for issue in contract_issues {
        if !result.issues.contains(&issue) {
            result.issues.push(issue);
        }
    }
    
    // Check token age (new tokens are riskier)
    if let Some(created_at) = token_info.created_at {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
            
        let age_in_days = (now - created_at) / (24 * 60 * 60);
        
        if age_in_days < 1 {
            result.issues.push(TokenIssue::NewToken);
            result.risk_score += 30;
        } else if age_in_days < 7 {
            result.issues.push(TokenIssue::RecentToken);
            result.risk_score += 15;
        }
    }
    
    // Cap risk score at 100
    result.risk_score = result.risk_score.min(100);
    
    // Update safe_to_trade based on final risk score
    result.safe_to_trade = result.risk_score < 70;
    
    Ok(result)
} 