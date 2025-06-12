/// Token analysis API implementation
///
/// Module cung cấp API phân tích token sử dụng từ TokenAnalysisProvider
/// Module này đã được cải tiến để sử dụng các hàm chung từ common/analysis.rs
/// thay vì định nghĩa lại logic
///
// External imports
use std::sync::Arc;
use anyhow::{Result, Context, anyhow};
use async_trait::async_trait;
use tracing::debug;

// Internal imports
use crate::analys::token_status::{TokenStatusAnalyzer, TokenSafety};
use crate::tradelogic::common::types::SecurityCheckResult;
use crate::tradelogic::common::analysis::{
    analyze_token_security,
    convert_token_safety_to_security_check
};

/// Provider token analysis API
#[async_trait]
pub trait TokenAnalysisProvider: Send + Sync + 'static {
    /// Analyze token for safety issues
    async fn analyze_token(&self, chain_id: u32, token_address: &str) -> Result<TokenSafety>;
    
    /// Get detailed contract information
    async fn get_contract_details(&self, chain_id: u32, token_address: &str) -> Result<serde_json::Value>;
    
    /// Get token tax information
    async fn get_token_tax_info(&self, chain_id: u32, token_address: &str) -> Result<TokenTaxInfo>;
    
    /// Comprehensive token security check
    async fn check_token_security(&self, chain_id: u32, token_address: &str) -> Result<SecurityCheckResult>;
}

/// Token Analysis Provider Implementation
pub struct TokenAnalysisProviderImpl {
    analyzer: Arc<TokenStatusAnalyzer>,
    // Có thể thêm các dependencies khác như cache, database, blockchain adapter, etc.
}

/// Token tax information structure
#[derive(Debug, Clone)]
pub struct TokenTaxInfo {
    pub token_address: String,
    pub buy_tax: Option<f64>,
    pub sell_tax: Option<f64>,
    pub is_dynamic_tax: Option<bool>,
    pub last_updated: Option<u64>,
}

impl TokenAnalysisProviderImpl {
    /// Create new TokenAnalysisProvider instance
    pub fn new() -> Self {
        Self {
            analyzer: Arc::new(TokenStatusAnalyzer::new()),
        }
    }
    
    /// Get adapter for chain
    fn get_adapter_for_chain(&self, chain_id: u32) -> Result<Arc<crate::chain_adapters::evm_adapter::EvmAdapter>> {
        // Trong thực tế, sẽ lấy adapter từ một registry hoặc từ constructor injection
        let adapter_registry = crate::chain_adapters::get_adapter_registry();
        let adapter = adapter_registry.get_evm_adapter(chain_id)
            .ok_or_else(|| anyhow!("Không tìm thấy adapter cho chain ID {}", chain_id))?;
        
        Ok(adapter)
    }
}

#[async_trait]
impl TokenAnalysisProvider for TokenAnalysisProviderImpl {
    async fn analyze_token(&self, chain_id: u32, token_address: &str) -> Result<TokenSafety> {
        debug!("Analyzing token {} on chain {}", token_address, chain_id);
        
        // Gọi phân tích từ analyzer
        self.analyzer.analyze_token(token_address, chain_id).await
            .context(format!("Failed to analyze token {} on chain {}", token_address, chain_id))
    }
    
    async fn get_contract_details(&self, chain_id: u32, token_address: &str) -> Result<serde_json::Value> {
        debug!("Getting contract details for token {} on chain {}", token_address, chain_id);
        
        // Lấy adapter cho chain
        let adapter = self.get_adapter_for_chain(chain_id)?;
        
        // Gọi API blockchain
        let contract_info = adapter.get_contract_info(token_address).await
            .context(format!("Failed to get contract info for token {} on chain {}", token_address, chain_id))?;
        
        // Convert to JSON value
        let result = serde_json::to_value(contract_info)
            .context("Failed to serialize contract info to JSON")?;
        
        Ok(result)
    }
    
    async fn get_token_tax_info(&self, chain_id: u32, token_address: &str) -> Result<TokenTaxInfo> {
        debug!("Getting tax info for token {} on chain {}", token_address, chain_id);
        
        // Lấy adapter cho chain
        let adapter = self.get_adapter_for_chain(chain_id)?;
        
        // Gọi phân tích từ analyzer
        let (buy_tax, sell_tax, is_dynamic) = self.analyzer.get_token_tax(token_address, chain_id).await
            .context(format!("Failed to get tax info for token {} on chain {}", token_address, chain_id))?;
        
        // Log kết quả
        debug!(
            "Tax info for token {} on chain {}: buy_tax={}, sell_tax={}, dynamic={}",
            token_address, chain_id, buy_tax, sell_tax, is_dynamic
        );
        
        // Trả về kết quả
        Ok(TokenTaxInfo {
            token_address: token_address.to_string(),
            buy_tax: Some(buy_tax),
            sell_tax: Some(sell_tax),
            is_dynamic_tax: Some(is_dynamic),
            last_updated: Some(chrono::Utc::now().timestamp() as u64),
        })
    }
    
    async fn check_token_security(&self, chain_id: u32, token_address: &str) -> Result<SecurityCheckResult> {
        debug!("Performing security check for token {} on chain {}", token_address, chain_id);
        
        // Lấy adapter cho chain
        let adapter = self.get_adapter_for_chain(chain_id)?;
        
        // Sử dụng hàm analyze_token_security từ common/analysis.rs
        let base_result = analyze_token_security(token_address, &adapter).await?;
        
        // Bổ sung phân tích từ analyzer chuyên dụng của TokenAnalysisProvider
        if let Ok(safety) = self.analyze_token(chain_id, token_address).await {
            // Convert và kết hợp với kết quả từ analyze_token_security
            let analyzer_result = convert_token_safety_to_security_check(&safety, token_address);
            
            // Kết hợp các vấn đề từ cả hai nguồn
            let mut combined_issues = base_result.issues.clone();
            for issue in analyzer_result.issues {
                if !combined_issues.contains(&issue) {
                    combined_issues.push(issue);
                }
            }
            
            // Sử dụng risk score cao hơn
            let risk_score = base_result.risk_score.max(analyzer_result.risk_score);
            
            // Tạo kết quả kết hợp cuối cùng
            return Ok(SecurityCheckResult {
                token_address: token_address.to_string(),
                issues: combined_issues,
                risk_score,
                safe_to_trade: risk_score < 70,
                extra_info: base_result.extra_info,
            });
        }
        
        // Nếu không lấy được kết quả từ analyzer, trả về kết quả cơ bản
        Ok(base_result)
    }
} 