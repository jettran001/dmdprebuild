/// Analys API Client cho Smart Trade
///
/// Module này triển khai các client để smart trade module có thể sử dụng 
/// các dịch vụ phân tích (mempool, token, risk) từ analys module.
/// Theo nguyên tắc trait-based design, file này tích hợp và sử dụng các
/// provider đã được định nghĩa trong analys/api.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use tracing::{debug, info, warn, error};

// External imports
use async_trait::async_trait;

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::tradelogic::traits::{
    MempoolAnalysisProvider, 
    TokenAnalysisProvider, 
    RiskAnalysisProvider,
    MevOpportunityProvider
};
use crate::tradelogic::common::types::{
    RiskScore, 
    SecurityCheckResult,
    TokenIssue
};
use crate::analys::token_status::TokenSafety;
use crate::analys::api::{
    create_mempool_analysis_provider,
    create_token_analysis_provider,
    create_risk_analysis_provider,
    create_mev_opportunity_provider
};

/// SmartTradeAnalysisClient quản lý tất cả provider từ analys module
pub struct SmartTradeAnalysisClient {
    /// Provider cho phân tích mempool
    mempool_provider: Arc<dyn MempoolAnalysisProvider>,
    
    /// Provider cho phân tích token
    token_provider: Arc<dyn TokenAnalysisProvider>,
    
    /// Provider cho phân tích rủi ro
    risk_provider: Arc<dyn RiskAnalysisProvider>,
    
    /// Provider cho cơ hội MEV
    mev_provider: Arc<dyn MevOpportunityProvider>,
}

impl SmartTradeAnalysisClient {
    /// Tạo client mới với các provider từ analys module
    pub fn new(chain_adapters: HashMap<u32, Arc<EvmAdapter>>) -> Self {
        // Tạo các provider sử dụng factory functions từ tradelogic module
        let (mempool_provider, token_provider, risk_provider, mev_provider) = 
            crate::tradelogic::create_analysis_providers(chain_adapters)
                .expect("Failed to create analysis providers");
        
        Self {
            mempool_provider,
            token_provider,
            risk_provider,
            mev_provider,
        }
    }
    
    /// Kiểm tra an toàn token
    pub async fn verify_token_safety(&self, chain_id: u32, token_address: &str) -> Result<SecurityCheckResult> {
        // Lấy thông tin phân tích token từ provider
        let safety_info = self.token_provider.analyze_token(chain_id, token_address).await
            .context("Không thể phân tích an toàn của token")?;
        
        // Lấy chi tiết contract
        let contract_details = self.token_provider.get_contract_details(chain_id, token_address).await
            .context("Không thể lấy chi tiết contract của token")?;
        
        // Kiểm tra các vấn đề bảo mật
        let is_valid = safety_info.is_valid();
        let issues = if !is_valid {
            let mut token_issues: Vec<TokenIssue> = Vec::new();
            
            // Chuyển đổi các vấn đề từ TokenSafety sang TokenIssue
            if let Some(honeypot_status) = safety_info.is_honeypot {
                if honeypot_status {
                    token_issues.push(TokenIssue::Honeypot { 
                        description: "Token là honeypot, không thể bán".to_string() 
                    });
                }
            }
            
            if let Some(buy_tax) = safety_info.buy_tax {
                if buy_tax > 10.0 {
                    token_issues.push(TokenIssue::HighBuyTax { 
                        tax_percent: buy_tax,
                        threshold: 10.0 
                    });
                }
            }
            
            if let Some(sell_tax) = safety_info.sell_tax {
                if sell_tax > 10.0 {
                    token_issues.push(TokenIssue::HighSellTax { 
                        tax_percent: sell_tax,
                        threshold: 10.0 
                    });
                }
            }
            
            if let Some(has_blacklist) = contract_details.has_blacklist {
                if has_blacklist {
                    token_issues.push(TokenIssue::HasBlacklist);
                }
            }
            
            if let Some(has_whitelist) = contract_details.has_whitelist {
                if has_whitelist {
                    token_issues.push(TokenIssue::HasWhitelist);
                }
            }
            
            if let Some(is_proxy) = contract_details.is_proxy {
                if is_proxy {
                    token_issues.push(TokenIssue::IsProxy);
                }
            }
            
            token_issues
        } else {
            Vec::new()
        };
        
        // Tạo kết quả kiểm tra an toàn
        let security_result = SecurityCheckResult {
            is_safe: is_valid && issues.is_empty(),
            verified_contract: contract_details.is_verified.unwrap_or(false),
            issues,
            owner_renounced: contract_details.is_owner_renounced.unwrap_or(false),
            contract_age_days: contract_details.age_days.unwrap_or(0),
            warning_message: if !is_valid {
                Some("Token có vấn đề bảo mật nghiêm trọng".to_string())
            } else if !issues.is_empty() {
                Some("Token có một số cảnh báo, hãy cẩn trọng".to_string())
            } else {
                None
            },
        };
        
        Ok(security_result)
    }
    
    /// Đánh giá rủi ro cho một token
    pub async fn analyze_risk(&self, chain_id: u32, token_address: &str) -> Result<RiskScore> {
        // Lấy phân tích rủi ro từ provider
        let risk_analysis = self.risk_provider.analyze_token_risk(chain_id, token_address).await
            .context("Không thể phân tích rủi ro token")?;
        
        // Lấy thêm thông tin từ token analysis để làm phong phú kết quả
        let token_safety = self.token_provider.analyze_token(chain_id, token_address).await
            .context("Không thể lấy thông tin an toàn token")?;
        
        let contract_details = self.token_provider.get_contract_details(chain_id, token_address).await
            .context("Không thể lấy chi tiết contract")?;
        
        // Tạo risk score từ kết quả phân tích
        let mut risk_score = RiskScore::new(risk_analysis.risk_score);
        
        // Thêm các yếu tố rủi ro
        for factor in risk_analysis.risk_factors {
            risk_score.add_factor(
                &factor.name,
                &factor.description,
                factor.impact
            );
        }
        
        // Thêm các khuyến nghị
        for recommendation in risk_analysis.recommendations {
            risk_score.add_recommendation(&recommendation);
        }
        
        // Bổ sung thêm một số nhận xét từ phân tích token
        if let Some(buy_tax) = token_safety.buy_tax {
            if buy_tax > 5.0 {
                risk_score.add_factor(
                    "High Buy Tax",
                    &format!("Token có thuế mua cao: {}%", buy_tax),
                    (buy_tax as u8).min(100)
                );
            }
        }
        
        if let Some(sell_tax) = token_safety.sell_tax {
            if sell_tax > 5.0 {
                risk_score.add_factor(
                    "High Sell Tax",
                    &format!("Token có thuế bán cao: {}%", sell_tax),
                    (sell_tax as u8).min(100)
                );
            }
        }
        
        if let Some(is_honeypot) = token_safety.is_honeypot {
            if is_honeypot {
                risk_score.add_factor(
                    "Honeypot",
                    "Token là honeypot, không thể bán",
                    100
                );
                risk_score.add_recommendation("Tránh tuyệt đối token honeypot này");
            }
        }
        
        if let Some(is_verified) = contract_details.is_verified {
            if !is_verified {
                risk_score.add_factor(
                    "Unverified Contract",
                    "Contract chưa được xác minh trên blockchain explorer",
                    60
                );
                risk_score.add_recommendation("Chỉ giao dịch với contract đã được xác minh");
            }
        }
        
        Ok(risk_score)
    }
    
    /// Lấy provider cho phân tích mempool
    pub fn mempool_provider(&self) -> Arc<dyn MempoolAnalysisProvider> {
        self.mempool_provider.clone()
    }
    
    /// Lấy provider cho phân tích token
    pub fn token_provider(&self) -> Arc<dyn TokenAnalysisProvider> {
        self.token_provider.clone()
    }
    
    /// Lấy provider cho phân tích rủi ro
    pub fn risk_provider(&self) -> Arc<dyn RiskAnalysisProvider> {
        self.risk_provider.clone()
    }
    
    /// Lấy provider cho cơ hội MEV
    pub fn mev_provider(&self) -> Arc<dyn MevOpportunityProvider> {
        self.mev_provider.clone()
    }
} 