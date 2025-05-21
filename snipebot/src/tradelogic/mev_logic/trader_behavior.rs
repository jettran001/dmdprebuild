/// Trader behavior analysis structures and utilities
use std::collections::HashMap;
use crate::tradelogic::common::types::{TraderBehaviorType, TraderExpertiseLevel, GasBehavior, TraderBehaviorAnalysis};
use crate::tradelogic::common::analysis::analyze_trader_behavior;
use crate::analys::mempool::TransactionType;

/// Gas usage behavior of a trader
#[derive(Debug, Clone)]
pub struct GasBehavior {
    /// Average gas price (Gwei)
    pub average_gas_price: f64,
    /// Highest gas price used (Gwei)
    pub highest_gas_price: f64,
    /// Lowest gas price used (Gwei)
    pub lowest_gas_price: f64,
    /// Has gas strategy (e.g. increases gas when needed)
    pub has_gas_strategy: bool,
    /// Transaction success rate
    pub success_rate: f64,
}

/// Trader behavior analysis results
#[derive(Debug, Clone)]
pub struct TraderBehaviorAnalysis {
    /// Trader address
    pub address: String,
    /// Detected behavior type
    pub behavior_type: TraderBehaviorType,
    /// Expertise level
    pub expertise_level: TraderExpertiseLevel,
    /// Transaction frequency (transactions/hour)
    pub transaction_frequency: f64,
    /// Average transaction value (ETH)
    pub average_transaction_value: f64,
    /// Common transaction types
    pub common_transaction_types: Vec<TransactionType>,
    /// Gas usage behavior
    pub gas_behavior: GasBehavior,
    /// Frequently traded tokens
    pub frequently_traded_tokens: Vec<String>,
    /// Preferred DEXes
    pub preferred_dexes: Vec<String>,
    /// Active hours (0-23, UTC hours)
    pub active_hours: Vec<u8>,
    /// Prediction score for next behavior (0-100, higher is more reliable)
    pub prediction_score: f64,
    /// Additional notes
    pub additional_notes: Option<String>,
}

impl TraderBehaviorAnalysis {
    /// Create a new trader behavior analysis
    pub fn new(
        address: String,
        behavior_type: TraderBehaviorType,
        expertise_level: TraderExpertiseLevel,
        transaction_frequency: f64,
        average_transaction_value: f64,
        common_transaction_types: Vec<TransactionType>,
        gas_behavior: GasBehavior,
        frequently_traded_tokens: Vec<String>,
        preferred_dexes: Vec<String>,
        active_hours: Vec<u8>,
        prediction_score: f64,
        additional_notes: Option<String>,
    ) -> Self {
        Self {
            address,
            behavior_type,
            expertise_level,
            transaction_frequency,
            average_transaction_value,
            common_transaction_types,
            gas_behavior,
            frequently_traded_tokens,
            preferred_dexes,
            active_hours,
            prediction_score,
            additional_notes,
        }
    }

    /// Check if trader is likely a bot
    pub fn is_likely_bot(&self) -> bool {
        match self.behavior_type {
            TraderBehaviorType::MevBot | TraderBehaviorType::HighFrequencyTrader => true,
            _ => self.transaction_frequency > 10.0 && self.gas_behavior.has_gas_strategy,
        }
    }

    /// Get trader proficiency score (0-100)
    pub fn get_proficiency_score(&self) -> f64 {
        match self.expertise_level {
            TraderExpertiseLevel::Professional => 90.0,
            TraderExpertiseLevel::Intermediate => 60.0,
            TraderExpertiseLevel::Beginner => 30.0,
            TraderExpertiseLevel::Automated => 80.0,
            TraderExpertiseLevel::Unknown => 50.0,
        }
    }

    /// Predict if the trader will front-run
    pub fn will_front_run(&self) -> bool {
        match self.behavior_type {
            TraderBehaviorType::MevBot => true,
            TraderBehaviorType::Arbitrageur => self.gas_behavior.has_gas_strategy,
            TraderBehaviorType::HighFrequencyTrader => self.gas_behavior.has_gas_strategy,
            _ => false,
        }
    }

    /// Get summary of trader behavior
    pub fn get_summary(&self) -> String {
        let behavior_type_str = match self.behavior_type {
            TraderBehaviorType::Arbitrageur => "Arbitrageur",
            TraderBehaviorType::MevBot => "MEV Bot",
            TraderBehaviorType::Retail => "Retail",
            TraderBehaviorType::Institutional => "Institutional",
            TraderBehaviorType::MarketMaker => "Market Maker",
            TraderBehaviorType::Whale => "Whale",
            TraderBehaviorType::HighFrequencyTrader => "High Frequency Trader",
            TraderBehaviorType::Unknown => "Unknown",
        };

        let expertise_level_str = match self.expertise_level {
            TraderExpertiseLevel::Professional => "Professional",
            TraderExpertiseLevel::Intermediate => "Intermediate",
            TraderExpertiseLevel::Beginner => "Beginner",
            TraderExpertiseLevel::Automated => "Automated (Bot)",
            TraderExpertiseLevel::Unknown => "Unknown",
        };

        format!(
            "Trader {}: {} ({}) - {}tx/h, ${:.2} avg, {}% success rate",
            &self.address[0..10],
            behavior_type_str,
            expertise_level_str,
            self.transaction_frequency,
            self.average_transaction_value,
            self.gas_behavior.success_rate * 100.0
        )
    }
}

// Helper function to analyze the behavior of multiple traders
pub fn analyze_traders_batch(addresses: &[String], transactions_by_address: &HashMap<String, Vec<crate::analys::mempool::MempoolTransaction>>) -> HashMap<String, TraderBehaviorAnalysis> {
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