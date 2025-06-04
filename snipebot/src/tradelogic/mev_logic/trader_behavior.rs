/// Trader behavior analysis module
/// 
/// This module re-exports types and functionality from the common analysis module.
/// All implementation details are centralized in common/analysis.rs to avoid duplication.
use std::collections::HashMap;
use crate::analys::mempool::{MempoolTransaction, TransactionType};

// Re-export common types
pub use crate::tradelogic::common::types::{
    TraderBehaviorType, TraderExpertiseLevel, GasBehavior, TraderBehaviorAnalysis
};

// Import the canonical implementation from common
use crate::tradelogic::common::analysis::{
    analyze_trader_behavior, determine_trader_type, 
    determine_expertise_level, analyze_gas_behavior
};

// Helper function to analyze the behavior of multiple traders
pub fn analyze_traders_batch(
    addresses: &[String], 
    transactions_by_address: &HashMap<String, Vec<MempoolTransaction>>
) -> HashMap<String, TraderBehaviorAnalysis> {
    let mut results = HashMap::new();
    
    for address in addresses {
        if let Some(transactions) = transactions_by_address.get(address) {
            // Use the shared function from common module
            let analysis = analyze_trader_behavior(address, transactions);
            results.insert(address.clone(), analysis);
        }
    }
    
    results
} 