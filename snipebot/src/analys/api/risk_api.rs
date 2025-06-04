/// Risk Analysis Provider Implementation
///
/// This module implements the RiskAnalysisProvider trait to provide
/// standardized access to risk analysis functionality.

use std::sync::Arc;
use std::collections::HashMap;
use async_trait::async_trait;
use anyhow::{Result, anyhow};

use crate::tradelogic::traits::RiskAnalysisProvider;
use crate::tradelogic::common::types::{
    RiskScore, RiskLevel, RiskFactor
};
use crate::tradelogic::mev_logic::{
    MevOpportunity,
    MevOpportunityType,
    MevExecutionMethod
};
use crate::analys::risk_analyzer::{
    RiskAnalyzer, 
    TradeRiskAnalysis,
    TradeRecommendation
};

/// Implementation of RiskAnalysisProvider trait
pub struct RiskAnalysisProviderImpl {
    /// Risk analyzer instance
    risk_analyzer: Arc<RiskAnalyzer>,
}

impl RiskAnalysisProviderImpl {
    /// Create a new risk analysis provider
    pub fn new(risk_analyzer: Arc<RiskAnalyzer>) -> Self {
        Self {
            risk_analyzer,
        }
    }
    
    /// Convert numeric risk score to RiskLevel
    fn score_to_level(score: u8) -> RiskLevel {
        match score {
            0..=20 => RiskLevel::VeryLow,
            21..=40 => RiskLevel::Low,
            41..=60 => RiskLevel::Medium,
            61..=80 => RiskLevel::High,
            _ => RiskLevel::VeryHigh,
        }
    }
    
    /// Calculate base risk score for opportunity type
    fn calculate_base_risk_for_type(&self, opportunity_type: MevOpportunityType) -> u8 {
        match opportunity_type {
            MevOpportunityType::Arbitrage => 30, // Lower risk
            MevOpportunityType::LiquidityChange => 40,
            MevOpportunityType::BackRunning => 45,
            MevOpportunityType::JitLiquidity => 50,
            MevOpportunityType::CrossDomain => 60,
            MevOpportunityType::FrontRunning => 70,
            MevOpportunityType::Sandwich => 75, // Higher risk
            MevOpportunityType::Liquidation => 55,
            MevOpportunityType::Custom => 65,
        }
    }
    
    /// Calculate risk score for execution method
    fn calculate_risk_for_method(&self, method: MevExecutionMethod) -> u8 {
        match method {
            MevExecutionMethod::Standard => 30, // Lower risk
            MevExecutionMethod::FlashBots => 40,
            MevExecutionMethod::Bundle => 50,
            MevExecutionMethod::PrivateTransaction => 60,
            MevExecutionMethod::CrossDomain => 70, // Higher risk
            MevExecutionMethod::Custom => 65,
        }
    }
    
    /// Calculate opportunity risk score
    fn calculate_opportunity_risk_score(&self, opportunity: &MevOpportunity) -> u8 {
        // Base risk from opportunity type
        let base_risk = self.calculate_base_risk_for_type(opportunity.opportunity_type);
        
        // Risk from execution method
        let method_risk = self.calculate_risk_for_method(opportunity.execution_method);
        
        // Risk from profit/gas ratio
        let profit_risk = if opportunity.estimated_net_profit_usd <= 0.0 {
            25 // Negative profit is risky
        } else if opportunity.estimated_gas_cost_usd > 0.0 {
            let profit_ratio = opportunity.estimated_profit_usd / opportunity.estimated_gas_cost_usd;
            if profit_ratio < 2.0 {
                20 // High risk - low profit ratio
            } else if profit_ratio < 5.0 {
                10 // Medium risk
            } else {
                0 // Low risk - high profit ratio
            }
        } else {
            15 // Can't calculate ratio, medium risk
        };
        
        // Combine risks with weights
        let weighted_risk = (base_risk as f64 * 0.4) + 
                          (method_risk as f64 * 0.3) + 
                          (profit_risk as f64 * 0.3);
        
        weighted_risk.min(100.0) as u8
    }
}

#[async_trait]
impl RiskAnalysisProvider for RiskAnalysisProviderImpl {
    /// Phân tích rủi ro giao dịch
    async fn analyze_trade_risk(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        trade_amount_usd: f64
    ) -> Result<TradeRiskAnalysis> {
        // Create simplified placeholder analysis
        // In a real implementation, this would call risk_analyzer methods with real data
        
        let risk_analysis = TradeRiskAnalysis {
            risk_score: 50.0, // Medium risk
            risk_factors: vec![],
            trade_recommendation: TradeRecommendation::ProceedWithCaution(
                "Token has some moderate risk factors".to_string()
            ),
            analysis_details: "Detailed risk analysis would be provided here".to_string(),
        };
        
        Ok(risk_analysis)
    }
    
    /// Tính toán điểm rủi ro tổng hợp cho MEV opportunity
    async fn calculate_opportunity_risk(
        &self, 
        opportunity: &MevOpportunity
    ) -> Result<RiskScore> {
        // Calculate risk score
        let score = self.calculate_opportunity_risk_score(opportunity);
        let level = Self::score_to_level(score);
        
        // Generate risk factors
        let mut factors = Vec::new();
        
        // Add risk factors based on opportunity type
        match opportunity.opportunity_type {
            MevOpportunityType::Arbitrage => {
                factors.push(RiskFactor {
                    name: "Arbitrage Risk".to_string(),
                    description: "Risk related to price differences between markets".to_string(),
                    impact: 30,
                });
            },
            MevOpportunityType::Sandwich => {
                factors.push(RiskFactor {
                    name: "Sandwich Attack Risk".to_string(),
                    description: "High risk from frontrunning and backrunning a transaction".to_string(),
                    impact: 75,
                });
            },
            MevOpportunityType::FrontRunning => {
                factors.push(RiskFactor {
                    name: "Frontrunning Risk".to_string(),
                    description: "Risk from entering a position before target transaction".to_string(),
                    impact: 70,
                });
            },
            // Add more opportunity type factors
            _ => {
                factors.push(RiskFactor {
                    name: "General MEV Risk".to_string(),
                    description: "Risk related to MEV extraction".to_string(),
                    impact: 50,
                });
            }
        }
        
        // Add risk factor based on execution method
        factors.push(RiskFactor {
            name: format!("{:?} Execution Risk", opportunity.execution_method),
            description: format!("Risk associated with {:?} execution method", opportunity.execution_method),
            impact: self.calculate_risk_for_method(opportunity.execution_method),
        });
        
        // Add profitability risk factor
        if opportunity.estimated_net_profit_usd <= 0.0 {
            factors.push(RiskFactor {
                name: "Negative Profit Risk".to_string(),
                description: "Transaction may result in a loss".to_string(),
                impact: 80,
            });
        } else {
            let profit_ratio = opportunity.estimated_profit_usd / opportunity.estimated_gas_cost_usd.max(0.000001);
            factors.push(RiskFactor {
                name: "Profit/Gas Ratio".to_string(),
                description: format!("Profit to gas cost ratio: {:.2}x", profit_ratio),
                impact: if profit_ratio < 2.0 { 70 } else if profit_ratio < 5.0 { 40 } else { 20 },
            });
        }
        
        // Generate recommendations based on risk score
        let recommendations = match level {
            RiskLevel::VeryLow | RiskLevel::Low => {
                vec!["Opportunity appears safe for execution".to_string()]
            },
            RiskLevel::Medium => {
                vec![
                    "Use appropriate slippage protection".to_string(),
                    "Consider transaction impact in case of failure".to_string(),
                ]
            },
            RiskLevel::High | RiskLevel::VeryHigh => {
                vec![
                    "High risk opportunity - proceed with extreme caution".to_string(),
                    "Consider reducing position size to limit exposure".to_string(),
                    "Use robust error handling and revert protection".to_string(),
                    "Monitor execution closely for unexpected behavior".to_string(),
                ]
            },
        };
        
        Ok(RiskScore {
            score,
            level,
            factors,
            recommendations,
        })
    }
    
    /// Kiểm tra xem cơ hội có đáp ứng tiêu chí rủi ro không
    async fn opportunity_meets_criteria(
        &self, 
        opportunity: &MevOpportunity, 
        max_risk_score: u8
    ) -> Result<bool> {
        let risk_score = self.calculate_opportunity_risk_score(opportunity);
        
        // Check if risk score is within acceptable limit
        Ok(risk_score <= max_risk_score)
    }
    
    /// Đánh giá rủi ro của phương pháp thực thi
    async fn evaluate_execution_method_risk(
        &self, 
        chain_id: u32,
        method: MevExecutionMethod
    ) -> Result<u8> {
        Ok(self.calculate_risk_for_method(method))
    }
    
    /// Thích nghi ngưỡng rủi ro dựa trên biến động thị trường
    async fn adapt_risk_threshold(
        &self, 
        base_threshold: u8, 
        market_volatility: f64
    ) -> Result<u8> {
        // Adjust risk threshold based on market volatility
        // Higher volatility -> lower acceptable risk (more conservative)
        
        let volatility_factor = if market_volatility < 0.01 {
            // Very low volatility
            1.1 // Increase threshold by 10%
        } else if market_volatility < 0.05 {
            // Normal volatility
            1.0 // No change
        } else if market_volatility < 0.1 {
            // High volatility
            0.8 // Reduce threshold by 20%
        } else {
            // Extreme volatility
            0.6 // Reduce threshold by 40%
        };
        
        // Apply adjustment, ensuring result is in range 0-100
        let adjusted = (base_threshold as f64 * volatility_factor).round() as u8;
        Ok(adjusted.min(100))
    }
} 