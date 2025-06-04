/// MEV Opportunity Provider Implementation
///
/// This module implements the MevOpportunityProvider trait to provide
/// standardized access to MEV opportunity functionality.

use std::sync::Arc;
use std::collections::HashMap;
use async_trait::async_trait;
use anyhow::{Result, anyhow};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::tradelogic::traits::{
    MempoolAnalysisProvider, 
    TokenAnalysisProvider, 
    RiskAnalysisProvider,
    MevOpportunityProvider
};
use crate::tradelogic::mev_logic::{
    MevOpportunity,
    MevOpportunityType
};

/// Implementation of the MevOpportunityProvider trait
pub struct MevOpportunityProviderImpl {
    /// Mempool analysis provider for detecting opportunities
    mempool_provider: Arc<dyn MempoolAnalysisProvider>,
    /// Token analysis provider for token safety checks
    token_provider: Arc<dyn TokenAnalysisProvider>,
    /// Risk analysis provider for risk assessment
    risk_provider: Arc<dyn RiskAnalysisProvider>,
    /// Cache of detected opportunities
    opportunities: RwLock<HashMap<u32, Vec<MevOpportunity>>>,
    /// Active opportunity listeners
    listeners: RwLock<HashMap<String, Arc<dyn Fn(MevOpportunity) -> Result<()> + Send + Sync + 'static>>>,
}

impl MevOpportunityProviderImpl {
    /// Create a new MEV opportunity provider
    pub fn new(
        mempool_provider: Arc<dyn MempoolAnalysisProvider>,
        token_provider: Arc<dyn TokenAnalysisProvider>,
        risk_provider: Arc<dyn RiskAnalysisProvider>,
    ) -> Self {
        Self {
            mempool_provider,
            token_provider,
            risk_provider,
            opportunities: RwLock::new(HashMap::new()),
            listeners: RwLock::new(HashMap::new()),
        }
    }
    
    /// Add an opportunity to the cache
    async fn add_opportunity(&self, chain_id: u32, opportunity: MevOpportunity) -> Result<()> {
        let mut opportunities = self.opportunities.write().await;
        
        // Get or create opportunity list for chain
        let chain_opportunities = opportunities.entry(chain_id).or_insert_with(Vec::new);
        
        // Check if opportunity already exists (based on ID)
        if chain_opportunities.iter().any(|opp| opp.id == opportunity.id) {
            return Ok(());
        }
        
        // Add the opportunity
        chain_opportunities.push(opportunity.clone());
        
        // Notify listeners
        drop(opportunities); // Release lock before notifying
        self.notify_listeners(opportunity).await?;
        
        Ok(())
    }
    
    /// Notify all active listeners about a new opportunity
    async fn notify_listeners(&self, opportunity: MevOpportunity) -> Result<()> {
        let listeners = self.listeners.read().await;
        
        for listener in listeners.values() {
            if let Err(e) = listener(opportunity.clone()) {
                // Log error but continue with other listeners
                eprintln!("Error notifying listener: {}", e);
            }
        }
        
        Ok(())
    }
    
    /// Clean up expired opportunities
    async fn cleanup_expired(&self) -> Result<()> {
        let mut opportunities = self.opportunities.write().await;
        
        for chain_opps in opportunities.values_mut() {
            // Keep only valid or executed opportunities
            chain_opps.retain(|opp| opp.is_valid() || opp.executed);
        }
        
        Ok(())
    }
}

#[async_trait]
impl MevOpportunityProvider for MevOpportunityProviderImpl {
    /// Lấy tất cả các cơ hội MEV hiện có
    async fn get_all_opportunities(&self, chain_id: u32) -> Result<Vec<MevOpportunity>> {
        // Clean up expired opportunities first
        self.cleanup_expired().await?;
        
        // Get opportunities for the specified chain
        let opportunities = self.opportunities.read().await;
        
        if let Some(chain_opps) = opportunities.get(&chain_id) {
            Ok(chain_opps.clone())
        } else {
            Ok(Vec::new())
        }
    }
    
    /// Lấy các cơ hội MEV theo loại
    async fn get_opportunities_by_type(
        &self, 
        chain_id: u32, 
        opportunity_type: MevOpportunityType
    ) -> Result<Vec<MevOpportunity>> {
        // Get all opportunities
        let all_opportunities = self.get_all_opportunities(chain_id).await?;
        
        // Filter by type
        let filtered = all_opportunities.into_iter()
            .filter(|opp| opp.opportunity_type == opportunity_type)
            .collect();
        
        Ok(filtered)
    }
    
    /// Đăng ký nhận thông báo cho cơ hội mới
    async fn register_opportunity_listener(
        &self, 
        callback: Arc<dyn Fn(MevOpportunity) -> Result<()> + Send + Sync + 'static>
    ) -> Result<String> {
        // Generate a listener ID
        let listener_id = Uuid::new_v4().to_string();
        
        // Store the listener
        let mut listeners = self.listeners.write().await;
        listeners.insert(listener_id.clone(), callback);
        
        Ok(listener_id)
    }
    
    /// Hủy đăng ký listener
    async fn unregister_listener(&self, listener_id: &str) -> Result<()> {
        let mut listeners = self.listeners.write().await;
        
        if listeners.remove(listener_id).is_none() {
            return Err(anyhow!("Listener not found"));
        }
        
        Ok(())
    }
    
    /// Cập nhật trạng thái cơ hội
    async fn update_opportunity_status(
        &self, 
        opportunity_id: &str, 
        executed: bool, 
        tx_hash: Option<String>
    ) -> Result<()> {
        // Find and update the opportunity
        let mut opportunities = self.opportunities.write().await;
        
        for chain_opps in opportunities.values_mut() {
            if let Some(opp) = chain_opps.iter_mut().find(|o| o.id == opportunity_id) {
                if executed {
                    if let Some(hash) = &tx_hash {
                        opp.mark_as_executed(hash.clone());
                    } else {
                        opp.executed = true;
                    }
                }
                
                return Ok(());
            }
        }
        
        Err(anyhow!("Opportunity not found"))
    }
    
    /// Lọc cơ hội theo điều kiện profit, risk, và loại
    async fn filter_opportunities(
        &self, 
        chain_id: u32,
        min_profit_usd: f64,
        max_risk: u8,
        types: Option<Vec<MevOpportunityType>>
    ) -> Result<Vec<MevOpportunity>> {
        // Get all opportunities for the chain
        let all_opportunities = self.get_all_opportunities(chain_id).await?;
        
        // Apply filters
        let filtered = all_opportunities.into_iter()
            // Filter by profit
            .filter(|opp| opp.estimated_net_profit_usd >= min_profit_usd)
            // Filter by risk
            .filter(|opp| opp.risk_score <= max_risk as u64)
            // Filter by type if specified
            .filter(|opp| {
                if let Some(types) = &types {
                    types.contains(&opp.opportunity_type)
                } else {
                    true
                }
            })
            // Only include valid, unexecuted opportunities
            .filter(|opp| opp.is_valid() && !opp.executed)
            .collect();
        
        Ok(filtered)
    }
} 