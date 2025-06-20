//! MEV opportunity management
//!
//! This module provides functionality for managing MEV opportunities,
//! including tracking, filtering, and prioritizing opportunities.

use std::sync::Arc;
use anyhow::{Result, anyhow, Context};
use serde::{Serialize, Deserialize};
use chrono::Utc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::{info, error, warn};
use uuid::Uuid;

use crate::tradelogic::traits::{
    MempoolAnalysisProvider, 
    TokenAnalysisProvider,
    RiskAnalysisProvider
};
use common::trading_actions::TradeStatus;

// Import MevOpportunity từ types.rs thay vì định nghĩa lại
use super::types::{MevOpportunity, MevOpportunityType, MevExecutionMethod, MevOpportunityStatus};

/// Constants for validation
const MIN_PROFIT_USD: f64 = 0.0;
const MAX_RISK_SCORE: u64 = 100;
const MIN_EXPIRY_SECONDS: u64 = 30;
const MAX_EXPIRY_SECONDS: u64 = 3600; // 1 hour
const MAX_OPPORTUNITIES: usize = 1000;
const PRUNED_OPPORTUNITIES: usize = 500;

/// Opportunity retention priority enum for pruning operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpportunityRetentionPriority {
    /// Giữ lại tất cả các cơ hội đã thực thi
    Executed,
    /// Giữ lại cơ hội lợi nhuận cao
    HighProfit, 
    /// Giữ lại cơ hội rủi ro thấp
    LowRisk,
    /// Giữ lại cơ hội mới nhất
    Recent,
}

/// Stats for OpportunityManager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpportunityManagerStats {
    /// Số cơ hội đã xử lý tổng cộng
    pub total_processed: usize,
    /// Số cơ hội đã thêm vào danh sách
    pub total_added: usize,
    /// Số cơ hội đã loại bỏ do hết hạn
    pub total_expired: usize,
    /// Số cơ hội đã loại bỏ do tràn kích thước
    pub total_pruned: usize,
    /// Số lần làm sạch đã thực hiện
    pub total_cleanups: usize,
    /// Lợi nhuận trung bình của các cơ hội
    pub average_profit_usd: f64,
    /// Thời gian phát hiện cơ hội gần nhất
    pub last_opportunity_time: u64,
}

/// Manager for MEV opportunities
#[derive(Debug)]
pub struct OpportunityManager {
    /// List of active opportunities
    pub opportunities: Vec<MevOpportunity>,
    /// Mempool analyzer reference
    mempool_analyzer: Arc<dyn MempoolAnalysisProvider>,
    /// Token analyzer reference
    token_analyzer: Arc<dyn TokenAnalysisProvider>,
    /// Risk analyzer reference
    risk_analyzer: Arc<dyn RiskAnalysisProvider>,
    /// Chỉ số hiệu suất và thống kê
    stats: OpportunityManagerStats,
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
            stats: OpportunityManagerStats {
                total_processed: 0,
                total_added: 0,
                total_expired: 0,
                total_pruned: 0,
                total_cleanups: 0,
                average_profit_usd: 0.0,
                last_opportunity_time: 0,
            },
        }
    }

    /// Validate opportunity parameters
    fn validate_opportunity(&self, opportunity: &MevOpportunity) -> Result<()> {
        // Validate profit
        if opportunity.estimated_profit_usd < MIN_PROFIT_USD {
            return Err(anyhow!("Estimated profit must be positive"));
        }

        // Validate risk score
        if opportunity.risk_score > MAX_RISK_SCORE {
            return Err(anyhow!("Risk score exceeds maximum allowed value"));
        }

        // Validate expiry time
        let now = Utc::now().timestamp() as u64;
        let expiry_duration = opportunity.expires_at.saturating_sub(now);
        if expiry_duration < MIN_EXPIRY_SECONDS {
            return Err(anyhow!("Opportunity expires too soon"));
        }
        if expiry_duration > MAX_EXPIRY_SECONDS {
            return Err(anyhow!("Opportunity expiry time too far in future"));
        }

        // Validate token pairs
        if opportunity.token_pairs.is_empty() {
            return Err(anyhow!("Opportunity must have at least one token pair"));
        }

        // Validate execution method
        if opportunity.execution_method == MevExecutionMethod::FlashBots && opportunity.chain_id != 1 {
            return Err(anyhow!("FlashBots execution only supported on Ethereum mainnet"));
        }

        Ok(())
    }
    
    /// Add an opportunity to the manager
    pub fn add_opportunity(&mut self, opportunity: MevOpportunity) -> Result<()> {
        // Validate opportunity
        self.validate_opportunity(&opportunity)
            .context("Failed to validate opportunity")?;

        // Check for duplicates
        if self.opportunities.iter().any(|o| o.id == opportunity.id) {
            return Err(anyhow!("Opportunity with this ID already exists"));
        }

        // Update stats
        self.stats.total_added += 1;
        self.stats.total_processed += 1;
        self.stats.last_opportunity_time = opportunity.detected_at;
        
        // Recalculate average profit
        let total_profit = self.stats.average_profit_usd * (self.stats.total_added as f64 - 1.0);
        self.stats.average_profit_usd = (total_profit + opportunity.estimated_profit_usd) / (self.stats.total_added as f64);
        
        // Add to opportunities list
        self.opportunities.push(opportunity);
        
        // Prune if needed
        if self.opportunities.len() > MAX_OPPORTUNITIES {
            self.prune(OpportunityRetentionPriority::HighProfit);
        }

        Ok(())
    }
    
    /// Prune opportunities based on priority
    pub fn prune(&mut self, priority: OpportunityRetentionPriority) {
        let original_count = self.opportunities.len();
        
        // Keep a maximum of PRUNED_OPPORTUNITIES opportunities
        if self.opportunities.len() <= PRUNED_OPPORTUNITIES {
            return;
        }
        
        // Sort opportunities based on priority
        match priority {
            OpportunityRetentionPriority::HighProfit => {
                // Sort by estimated profit (highest first)
                self.opportunities.sort_by(|a, b| {
                    b.estimated_net_profit_usd
                        .partial_cmp(&a.estimated_net_profit_usd)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            },
            OpportunityRetentionPriority::LowRisk => {
                // Sort by risk score (lowest first)
                self.opportunities.sort_by(|a, b| a.risk_score.cmp(&b.risk_score));
            },
            OpportunityRetentionPriority::Recent => {
                // Sort by detection time (newest first)
                self.opportunities.sort_by(|a, b| b.detected_at.cmp(&a.detected_at));
            },
            OpportunityRetentionPriority::Executed => {
                // Keep executed opportunities at the front, then sort by timestamp
                self.opportunities.sort_by(|a, b| {
                    match (a.executed, b.executed) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        _ => b.detected_at.cmp(&a.detected_at),
                    }
                });
            },
        }
        
        // Truncate to PRUNED_OPPORTUNITIES
        self.opportunities.truncate(PRUNED_OPPORTUNITIES);
        
        // Update pruning stats
        let pruned_count = original_count - self.opportunities.len();
        self.stats.total_pruned += pruned_count;
    }
    
    /// Get active (non-expired) opportunities
    pub fn get_active_opportunities(&self) -> Vec<&MevOpportunity> {
        let now = Utc::now().timestamp() as u64;
        self.opportunities.iter()
            .filter(|o| !o.executed && o.expires_at > now)
            .collect()
    }
    
    /// Filter opportunities based on criteria
    pub fn filter_opportunities(
        &self,
        opportunity_type: Option<MevOpportunityType>,
        min_profit_usd: f64,
        max_risk: u64,
    ) -> Result<Vec<&MevOpportunity>> {
        // Validate filter parameters
        if min_profit_usd < MIN_PROFIT_USD {
            return Err(anyhow!("Minimum profit must be positive"));
        }
        if max_risk > MAX_RISK_SCORE {
            return Err(anyhow!("Maximum risk score exceeds allowed value"));
        }

        Ok(self.opportunities.iter()
            .filter(|o| opportunity_type.is_none() || opportunity_type == Some(o.opportunity_type))
            .filter(|o| o.estimated_net_profit_usd >= min_profit_usd)
            .filter(|o| o.risk_score <= max_risk)
            .collect())
    }
    
    /// Get the best opportunity based on criteria
    pub fn get_best_opportunity(
        &self,
        opportunity_type: Option<MevOpportunityType>,
        min_profit_usd: f64,
        max_risk: u64,
    ) -> Result<Option<&MevOpportunity>> {
        let opportunities = self.filter_opportunities(opportunity_type, min_profit_usd, max_risk)?;
        
        // No matching opportunities
        if opportunities.is_empty() {
            return Ok(None);
        }
        
        // Sort by profit-to-risk ratio (descending)
        let mut sorted = opportunities.to_vec();
        sorted.sort_by(|a, b| {
            let ratio_a = a.estimated_net_profit_usd / (a.risk_score as f64 + 1.0);
            let ratio_b = b.estimated_net_profit_usd / (b.risk_score as f64 + 1.0);
            ratio_b.partial_cmp(&ratio_a).unwrap_or(std::cmp::Ordering::Equal)
        });
        
        Ok(sorted.first().copied())
    }

    /// Mark an opportunity as executed
    pub fn mark_as_executed(&mut self, opportunity_id: &str, tx_hash: String) -> Result<()> {
        // Validate inputs
        if tx_hash.is_empty() {
            return Err(anyhow!("Transaction hash cannot be empty"));
        }

        // Find and update opportunity
        let opportunity = self.opportunities.iter_mut()
            .find(|o| o.id == opportunity_id)
            .ok_or_else(|| anyhow!("Opportunity not found"))?;

        // Validate state transition
        if opportunity.executed {
            return Err(anyhow!("Opportunity already executed"));
        }

        opportunity.executed = true;
        opportunity.execution_tx_hash = Some(tx_hash);
        opportunity.mev_status = MevOpportunityStatus::Executed;

        Ok(())
    }

    /// Update opportunity status
    pub fn update_status(&mut self, opportunity_id: &str, status: TradeStatus) -> Result<()> {
        // Find opportunity
        let opportunity = self.opportunities.iter_mut()
            .find(|o| o.id == opportunity_id)
            .ok_or_else(|| anyhow!("Opportunity not found"))?;

        // Validate state transition
        match (opportunity.mev_status, status) {
            (MevOpportunityStatus::Executed, _) => {
                return Err(anyhow!("Cannot update status of executed opportunity"));
            },
            (_, TradeStatus::Failed) => {
                opportunity.mev_status = MevOpportunityStatus::Failed;
            },
            (_, TradeStatus::Success) => {
                opportunity.mev_status = MevOpportunityStatus::Executed;
            },
            _ => {
                return Err(anyhow!("Invalid status transition"));
            }
        }

        // Update trade status
        opportunity.status = Some(status);

        Ok(())
    }
    
    /// Fetch new opportunities from providers
    pub async fn fetch_new_opportunities(
        &mut self,
        chain_id: u32,
        opportunity_types: Option<Vec<MevOpportunityType>>,
    ) -> Result<Vec<MevOpportunity>> {
        let mut new_opportunities = Vec::new();
        
        // Determine which opportunity types to fetch
        let types = opportunity_types.unwrap_or_else(|| {
            vec![
                MevOpportunityType::Arbitrage,
                MevOpportunityType::Sandwich,
                MevOpportunityType::Liquidation,
                MevOpportunityType::FlashLiquidity,
            ]
        });
        
        // Fetch opportunities for each type
        for opp_type in types {
            match opp_type {
                MevOpportunityType::Arbitrage => {
                    let arbs = self.mempool_analyzer.detect_arbitrage_opportunities(chain_id).await?;
                    for arb in arbs {
                        self.add_opportunity(arb.clone())?;
                        new_opportunities.push(arb);
                    }
                },
                MevOpportunityType::Sandwich => {
                    let sandwiches = self.mempool_analyzer.detect_sandwich_opportunities(chain_id).await?;
                    for sandwich in sandwiches {
                        self.add_opportunity(sandwich.clone())?;
                        new_opportunities.push(sandwich);
                    }
                },
                // Additional opportunity types would be fetched here
                _ => {}
            }
        }
        
        Ok(new_opportunities)
    }
    
    /// Clean up expired opportunities
    pub fn cleanup_expired(&mut self) {
        let now = Utc::now().timestamp() as u64;
        let original_count = self.opportunities.len();
        
        // Remove expired opportunities that haven't been executed
        self.opportunities.retain(|o| {
            o.executed || now < o.expires_at
        });
        
        // Update stats
        let expired_count = original_count - self.opportunities.len();
        if expired_count > 0 {
            self.stats.total_expired += expired_count;
            self.stats.total_cleanups += 1;
        }
    }
    
    /// Get statistics about the opportunity manager
    pub fn get_stats(&self) -> &OpportunityManagerStats {
        &self.stats
    }
} 