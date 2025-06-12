/// Usage Examples for Analysis API
/// 
/// This file provides examples of how to use the analysis API from tradelogic
/// It is for documentation purposes only and is not used in production code.

use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;

use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::tradelogic::{
    MempoolAnalysisProvider,
    TokenAnalysisProvider,
    RiskAnalysisProvider,
    MevOpportunityProvider,
    create_analysis_providers
};
use crate::tradelogic::mev_logic::MevOpportunityType;
use crate::tradelogic::smart_trade::analys_client::SmartTradeAnalysisClient;

/// Example showing how to initialize and use the analysis providers
pub async fn analysis_api_usage_example() -> Result<()> {
    // In a real application, chain adapters would be created elsewhere
    let chain_adapters: HashMap<u32, Arc<EvmAdapter>> = HashMap::new();
    
    // Method 1: Create all providers at once
    let (mempool_provider, token_provider, risk_provider, mev_provider) = 
        create_analysis_providers(chain_adapters.clone())?;
    
    // Example usage of mempool provider
    let chain_id = 1; // Ethereum
    let mempool_txs = mempool_provider.get_pending_transactions(
        chain_id, 
        None, // All transaction types
        Some(10) // Limit to 10 results
    ).await?;
    println!("Found {} pending transactions", mempool_txs.len());
    
    // Example usage of token provider
    let token_address = "0xTokenAddress";
    let token_safety = token_provider.analyze_token(chain_id, token_address).await?;
    if token_safety.is_valid() {
        println!("Token is safe to trade");
    } else {
        println!("Token has safety issues");
    }
    
    // Example usage of risk provider
    let trade_risk = risk_provider.analyze_trade_risk(
        chain_id, 
        token_address, 
        1000.0 // Trade amount in USD
    ).await?;
    println!("Trade risk score: {}", trade_risk.risk_score);
    
    // Example usage of MEV opportunity provider
    let opportunities = mev_provider.get_opportunities_by_type(
        chain_id,
        MevOpportunityType::Arbitrage
    ).await?;
    println!("Found {} arbitrage opportunities", opportunities.len());
    
    // Method 2: Use the SmartTradeAnalysisClient
    let client = SmartTradeAnalysisClient::new(chain_adapters);
    
    // Verify token safety using the client
    let security_result = client.verify_token_safety(chain_id, token_address).await?;
    if security_result.is_safe {
        println!("Token passed security check");
    } else {
        println!("Token has security issues: {:?}", security_result.issues);
    }
    
    // Analyze risk using the client
    let risk_score = client.analyze_risk(chain_id, token_address).await?;
    println!("Risk level: {:?}", risk_score.risk_level);
    
    Ok(())
} 