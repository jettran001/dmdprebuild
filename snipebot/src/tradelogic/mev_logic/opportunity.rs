//! MEV Opportunity Definitions
//!
//! This module defines the structures and types related to MEV opportunities
//! detected and acted upon by the MEV bot.

/// External imports
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use chrono::Utc;
use std::sync::Arc;

/// Internal imports
use crate::types::TokenPair;
use crate::tradelogic::common::types::TradeStatus;
use crate::tradelogic::traits::{MempoolAnalysisProvider, TokenAnalysisProvider, RiskAnalysisProvider};
use crate::analys::token_status::TokenSafety;
use crate::analys::risk_analyzer::TradeRiskAnalysis;
use crate::tradelogic::common::types::RiskScore;
use anyhow::{Result, anyhow};

/// Module imports
use super::types::{MevOpportunityType, MevExecutionMethod};

/// Represents a detected MEV opportunity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MevOpportunity {
    /// Unique identifier
    pub id: String,
    
    /// Type of MEV opportunity
    pub opportunity_type: MevOpportunityType,
    
    /// Chain ID where opportunity was detected
    pub chain_id: u32,
    
    /// Timestamp when opportunity was detected
    pub detected_at: u64,
    
    /// Timestamp when opportunity expires
    pub expires_at: u64,
    
    /// Whether the opportunity has been executed
    pub executed: bool,
    
    /// Estimated profit in USD
    pub estimated_profit_usd: f64,
    
    /// Estimated gas cost in USD
    pub estimated_gas_cost_usd: f64,
    
    /// Estimated net profit (profit - gas cost) in USD
    pub estimated_net_profit_usd: f64,
    
    /// Token pairs involved in the opportunity
    pub token_pairs: Vec<TokenPair>,
    
    /// Risk score (0-100, higher is riskier)
    pub risk_score: u64,
    
    /// Method used for execution
    pub execution_method: MevExecutionMethod,
    
    /// Transaction hash if executed
    pub execution_tx_hash: Option<String>,
    
    /// Timestamp when executed
    pub execution_timestamp: Option<u64>,
    
    /// Execution status
    pub execution_status: TradeStatus,
    
    /// Specific parameters for this opportunity type
    pub specific_params: HashMap<String, String>,
}

impl MevOpportunity {
    /// Create a new MEV opportunity
    pub fn new(
        opportunity_type: MevOpportunityType,
        chain_id: u32,
        estimated_profit_usd: f64,
        estimated_gas_cost_usd: f64,
        token_pairs: Vec<TokenPair>,
        risk_score: u64,
        execution_method: MevExecutionMethod,
    ) -> Self {
        let now = Utc::now().timestamp() as u64;
        let expires_at = now + 300; // Default expiry: 5 minutes
        
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            opportunity_type,
            chain_id,
            detected_at: now,
            expires_at,
            executed: false,
            estimated_profit_usd,
            estimated_gas_cost_usd,
            estimated_net_profit_usd: estimated_profit_usd - estimated_gas_cost_usd,
            token_pairs,
            risk_score,
            execution_method,
            execution_tx_hash: None,
            execution_timestamp: None,
            execution_status: TradeStatus::Pending,
            specific_params: HashMap::new(),
        }
    }
    
    /// Check if the opportunity is still valid (not expired)
    pub fn is_valid(&self) -> bool {
        let now = Utc::now().timestamp() as u64;
        !self.executed && now < self.expires_at
    }
    
    /// Check if the opportunity meets profit threshold
    pub fn meets_profit_threshold(&self, min_profit_usd: f64) -> bool {
        self.estimated_net_profit_usd >= min_profit_usd
    }
    
    /// Check if the opportunity meets risk criteria
    pub fn meets_risk_criteria(&self, max_risk: u64) -> bool {
        self.risk_score <= max_risk
    }
    
    /// Set the opportunity as executed
    pub fn mark_as_executed(&mut self, tx_hash: String) {
        self.executed = true;
        self.execution_tx_hash = Some(tx_hash);
        self.execution_timestamp = Some(Utc::now().timestamp() as u64);
        self.execution_status = TradeStatus::Submitted;
    }
    
    /// Update the execution status
    pub fn update_status(&mut self, status: TradeStatus) {
        self.execution_status = status;
        
        // If completed or failed, mark as executed
        if status == TradeStatus::Completed || status == TradeStatus::Failed {
            self.executed = true;
        }
    }
    
    /// Add a specific parameter for this opportunity
    pub fn add_param(&mut self, key: &str, value: &str) {
        self.specific_params.insert(key.to_string(), value.to_string());
    }
    
    /// Get a specific parameter
    pub fn get_param(&self, key: &str) -> Option<&String> {
        self.specific_params.get(key)
    }
    
    /// Calculate a custom priority score for execution ordering
    /// Higher score = higher priority
    pub fn calculate_priority_score(&self) -> f64 {
        // Base priority on profit and risk
        let profit_factor = self.estimated_net_profit_usd;
        let risk_factor = 1.0 - (self.risk_score as f64 / 100.0);
        let time_factor = {
            let now = Utc::now().timestamp() as u64;
            let time_left = self.expires_at.saturating_sub(now);
            // Prioritize opportunities about to expire
            if time_left < 30 {
                2.0 // Urgency boost
            } else {
                1.0
            }
        };
        
        // Combine factors
        profit_factor * risk_factor * time_factor
    }
    
    /// Verify token safety for all tokens in this opportunity
    pub async fn verify_token_safety(&self, token_analyzer: &dyn TokenAnalysisProvider) -> Result<bool> {
        for pair in &self.token_pairs {
            // Skip wrapped native tokens
            if pair.is_wrapped_native() {
                continue;
            }
            
            // Check each token in the pair
            for token in [&pair.token0, &pair.token1] {
                if token.address == "0x0000000000000000000000000000000000000000" {
                    continue; // Skip native token
                }
                
                match token_analyzer.is_safe_to_trade(self.chain_id, &token.address, 70).await {
                    Ok(false) => return Ok(false), // Token not safe
                    Err(e) => return Err(anyhow!("Failed to verify token safety: {}", e)),
                    _ => continue, // Token is safe, continue checking
                }
            }
        }
        
        Ok(true) // All tokens verified safe
    }
    
    /// Analyze risk for this opportunity using risk analyzer
    pub async fn analyze_risk(&mut self, risk_analyzer: &dyn RiskAnalysisProvider) -> Result<RiskScore> {
        let risk_score = risk_analyzer.calculate_opportunity_risk(self).await?;
        self.risk_score = risk_score.score as u64;
        Ok(risk_score)
    }
    
    /// Check if this opportunity is ready for execution
    pub async fn is_executable(
        &self, 
        min_profit_usd: f64, 
        max_risk: u8, 
        token_analyzer: &dyn TokenAnalysisProvider,
        risk_analyzer: &dyn RiskAnalysisProvider
    ) -> Result<bool> {
        // Check profit threshold
        if !self.meets_profit_threshold(min_profit_usd) {
            return Ok(false);
        }
        
        // Check risk threshold
        if !self.meets_risk_criteria(max_risk as u64) {
            return Ok(false);
        }
        
        // Check expiry
        if !self.is_valid() {
            return Ok(false);
        }
        
        // Verify token safety
        if !self.verify_token_safety(token_analyzer).await? {
            return Ok(false);
        }
        
        // Check if opportunity meets risk criteria using the risk analyzer
        let meets_criteria = risk_analyzer.opportunity_meets_criteria(self, max_risk).await?;
        
        Ok(meets_criteria)
    }
}

/// Opportunity Manager for MEV opportunities
pub struct OpportunityManager {
    /// List of active opportunities
    opportunities: Vec<MevOpportunity>,
    /// Mempool analyzer reference
    mempool_analyzer: Arc<dyn MempoolAnalysisProvider>,
    /// Token analyzer reference
    token_analyzer: Arc<dyn TokenAnalysisProvider>,
    /// Risk analyzer reference
    risk_analyzer: Arc<dyn RiskAnalysisProvider>,
}

impl OpportunityManager {
    /// Create a new opportunity manager
    pub fn new(
        mempool_analyzer: Arc<dyn MempoolAnalysisProvider>,
        token_analyzer: Arc<dyn TokenAnalysisProvider>,
        risk_analyzer: Arc<dyn RiskAnalysisProvider>,
    ) -> Self {
        Self {
            opportunities: Vec::new(),
            mempool_analyzer,
            token_analyzer,
            risk_analyzer,
        }
    }
    
    /// Add a new opportunity
    pub fn add_opportunity(&mut self, opportunity: MevOpportunity) {
        self.opportunities.push(opportunity);
    }
    
    /// Get all active opportunities
    pub fn get_active_opportunities(&self) -> Vec<&MevOpportunity> {
        self.opportunities.iter()
            .filter(|opp| opp.is_valid() && !opp.executed)
            .collect()
    }
    
    /// Filter opportunities by type, minimum profit, and maximum risk
    pub fn filter_opportunities(
        &self,
        opportunity_type: Option<MevOpportunityType>,
        min_profit_usd: f64,
        max_risk: u64,
    ) -> Vec<&MevOpportunity> {
        self.opportunities.iter()
            .filter(|opp| opp.is_valid() && !opp.executed)
            .filter(|opp| opportunity_type.is_none() || opportunity_type.unwrap() == opp.opportunity_type)
            .filter(|opp| opp.estimated_net_profit_usd >= min_profit_usd)
            .filter(|opp| opp.risk_score <= max_risk)
            .collect()
    }
    
    /// Get best opportunity based on priority score
    pub fn get_best_opportunity(
        &self,
        opportunity_type: Option<MevOpportunityType>,
        min_profit_usd: f64,
        max_risk: u64,
    ) -> Option<&MevOpportunity> {
        self.filter_opportunities(opportunity_type, min_profit_usd, max_risk)
            .into_iter()
            .max_by(|a, b| {
                // Handle NaN values safely by treating them as less than any other value
                match a.calculate_priority_score().partial_cmp(&b.calculate_priority_score()) {
                    Some(ordering) => ordering,
                    None => {
                        // One or both scores are NaN
                        // Log this situation
                        tracing::warn!("NaN encountered in priority score comparison");
                        // Treat NaN as less than any value, so the non-NaN value wins
                        if a.calculate_priority_score().is_nan() {
                            std::cmp::Ordering::Less
                        } else {
                            std::cmp::Ordering::Greater
                        }
                    }
                }
            })
    }
    
    /// Mark opportunity as executed
    pub fn mark_as_executed(&mut self, opportunity_id: &str, tx_hash: String) -> Result<()> {
        let opportunity = self.opportunities.iter_mut()
            .find(|opp| opp.id == opportunity_id)
            .ok_or_else(|| anyhow!("Opportunity not found"))?;
        
        opportunity.mark_as_executed(tx_hash);
        Ok(())
    }
    
    /// Update opportunity status
    pub fn update_status(&mut self, opportunity_id: &str, status: TradeStatus) -> Result<()> {
        let opportunity = self.opportunities.iter_mut()
            .find(|opp| opp.id == opportunity_id)
            .ok_or_else(|| anyhow!("Opportunity not found"))?;
        
        opportunity.update_status(status);
        Ok(())
    }
    
    /// Fetch new opportunities from mempool analyzer
    pub async fn fetch_new_opportunities(
        &mut self,
        chain_id: u32,
        opportunity_types: Option<Vec<MevOpportunityType>>,
    ) -> Result<Vec<MevOpportunity>> {
        let new_opportunities = self.mempool_analyzer.detect_mev_opportunities(
            chain_id,
            opportunity_types,
        ).await?;
        
        // Analyze risk for each opportunity
        let mut analyzed_opportunities = Vec::new();
        for mut opp in new_opportunities {
            if let Ok(_) = opp.analyze_risk(&*self.risk_analyzer).await {
                self.add_opportunity(opp.clone());
                analyzed_opportunities.push(opp);
            }
        }
        
        Ok(analyzed_opportunities)
    }
    
    /// Clean up expired opportunities
    pub fn cleanup_expired(&mut self) {
        self.opportunities.retain(|opp| opp.is_valid() || opp.executed);
    }
} 