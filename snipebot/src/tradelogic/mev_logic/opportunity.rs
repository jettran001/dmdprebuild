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

/// Số lượng tối đa cơ hội được lưu trữ trong OpportunityManager
const MAX_OPPORTUNITIES: usize = 1000;

/// Mức độ ưu tiên giữ lại cơ hội khi làm sạch
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

/// Chỉ số hiệu suất và thống kê của OpportunityManager
#[derive(Debug, Default, Clone)]
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
            opportunities: Vec::with_capacity(100), // Pre-allocate space for better performance
            mempool_analyzer,
            token_analyzer,
            risk_analyzer,
            stats: OpportunityManagerStats::default(),
        }
    }
    
    /// Add a new opportunity
    pub fn add_opportunity(&mut self, opportunity: MevOpportunity) {
        // Cập nhật thống kê
        self.stats.total_added += 1;
        self.stats.average_profit_usd = (self.stats.average_profit_usd * (self.stats.total_added - 1) as f64 + 
                                        opportunity.estimated_profit_usd) / self.stats.total_added as f64;
        self.stats.last_opportunity_time = opportunity.detected_at;
        
        // Thêm cơ hội vào danh sách
        self.opportunities.push(opportunity);
        
        // Giới hạn kích thước nếu cần
        if self.opportunities.len() > MAX_OPPORTUNITIES {
            self.prune(OpportunityRetentionPriority::HighProfit);
        }
    }
    
    /// Làm sạch danh sách cơ hội để giữ trong giới hạn kích thước
    /// 
    /// # Parameters
    /// * `priority` - Ưu tiên loại cơ hội nào sẽ được giữ lại
    pub fn prune(&mut self, priority: OpportunityRetentionPriority) {
        if self.opportunities.len() <= MAX_OPPORTUNITIES {
            return;
        }

        // Sắp xếp theo ưu tiên được chọn
        match priority {
            OpportunityRetentionPriority::Executed => {
                // Giữ lại tất cả cơ hội đã thực thi và các cơ hội có lợi nhuận cao nhất
                self.opportunities.sort_by(|a, b| {
                    // Ưu tiên 1: các cơ hội đã thực thi
                    if a.executed && !b.executed {
                        std::cmp::Ordering::Less
                    } else if !a.executed && b.executed {
                        std::cmp::Ordering::Greater
                    } else {
                        // Ưu tiên 2: lợi nhuận cao hơn
                        b.estimated_net_profit_usd.partial_cmp(&a.estimated_net_profit_usd)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }
                });
            },
            OpportunityRetentionPriority::HighProfit => {
                // Sắp xếp theo lợi nhuận từ cao đến thấp
                self.opportunities.sort_by(|a, b| {
                    b.estimated_net_profit_usd.partial_cmp(&a.estimated_net_profit_usd)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            },
            OpportunityRetentionPriority::LowRisk => {
                // Sắp xếp theo rủi ro từ thấp đến cao
                self.opportunities.sort_by(|a, b| {
                    a.risk_score.partial_cmp(&b.risk_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            },
            OpportunityRetentionPriority::Recent => {
                // Sắp xếp theo thời gian phát hiện từ mới đến cũ
                self.opportunities.sort_by(|a, b| {
                    b.detected_at.cmp(&a.detected_at)
                });
            }
        }

        // Giữ lại MAX_OPPORTUNITIES cơ hội đầu tiên, loại bỏ phần còn lại
        let num_to_remove = self.opportunities.len() - MAX_OPPORTUNITIES;
        self.opportunities.truncate(MAX_OPPORTUNITIES);
        
        // Cập nhật thống kê
        self.stats.total_pruned += num_to_remove;
        self.stats.total_cleanups += 1;
        
        // Log thông tin về việc làm sạch
        tracing::info!("Pruned {} opportunities, keeping {}. Total pruned so far: {}", 
                      num_to_remove, MAX_OPPORTUNITIES, self.stats.total_pruned);
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
        // Fetch opportunities from mempool analyzer
        let new_opportunities = self.mempool_analyzer.detect_mev_opportunities(
            chain_id,
            opportunity_types,
        ).await?;
        
        // Update statistics
        self.stats.total_processed += new_opportunities.len();
        
        // Analyze risk for each opportunity and add to collection
        let mut analyzed_opportunities = Vec::new();
        for mut opp in new_opportunities {
            if let Ok(_) = opp.analyze_risk(&*self.risk_analyzer).await {
                // Cập nhật thống kê trước khi thêm
                self.add_opportunity(opp.clone());
                analyzed_opportunities.push(opp);
            }
        }
        
        // Nếu quá nhiều cơ hội được phân tích, chỉ trả về những cơ hội tốt nhất
        if analyzed_opportunities.len() > 50 {
            // Sắp xếp theo lợi nhuận ròng từ cao xuống thấp
            analyzed_opportunities.sort_by(|a, b| {
                b.estimated_net_profit_usd.partial_cmp(&a.estimated_net_profit_usd)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            
            // Chỉ giữ lại 50 cơ hội tốt nhất
            analyzed_opportunities.truncate(50);
        }
        
        Ok(analyzed_opportunities)
    }
    
    /// Clean up expired opportunities
    pub fn cleanup_expired(&mut self) {
        let before_count = self.opportunities.len();
        
        // Chỉ giữ lại các cơ hội còn hiệu lực hoặc đã được thực thi
        self.opportunities.retain(|opp| opp.is_valid() || opp.executed);
        
        // Cập nhật thống kê
        let removed_count = before_count - self.opportunities.len();
        if removed_count > 0 {
            self.stats.total_expired += removed_count;
            self.stats.total_cleanups += 1;
            
            tracing::info!("Cleaned up {} expired opportunities. Total expired: {}", 
                          removed_count, self.stats.total_expired);
        }
    }
    
    /// Get statistics about opportunity management
    pub fn get_stats(&self) -> &OpportunityManagerStats {
        &self.stats
    }
} 