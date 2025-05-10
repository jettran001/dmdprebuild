use std::error::Error;
use reqwest::{Client, StatusCode};

use crate::processor::bridge_orchestrator::{Chain, BridgeStatus, FeeEstimate};
use crate::sdk::api::{BridgeStatusResponse, FeesResponse, RelayRequest};

/// Client cho bridge API
pub struct BridgeClient {
    client: Client,
    base_url: String,
}

impl BridgeClient {
    /// Tạo client mới
    pub fn new(base_url: &str) -> Self {
        let client = Client::new();
        let base_url = base_url.trim_end_matches('/').to_string();
        
        Self {
            client,
            base_url,
        }
    }
    
    /// Lấy trạng thái của giao dịch bridge
    pub async fn get_bridge_status(&self, tx_hash: &str) -> Result<BridgeStatus, Box<dyn Error>> {
        let url = format!("{}/bridge/status/{}", self.base_url, tx_hash);
        
        let response = self.client.get(&url).send().await?;
        
        let response_body: BridgeStatusResponse = response.json().await?;
        
        if !response_body.success {
            return Err(response_body.error.unwrap_or_else(|| "Unknown error".to_string()).into());
        }
        
        response_body.status.ok_or_else(|| "No status returned".into())
    }
    
    /// Ước tính phí bridge
    pub async fn estimate_fee(
        &self, 
        source_chain: Chain, 
        target_chain: Chain, 
        token_id: u64, 
        amount: &str
    ) -> Result<FeeEstimate, Box<dyn Error>> {
        let url = format!(
            "{}/bridge/estimate-fee?source_chain={:?}&target_chain={:?}&token_id={}&amount={}",
            self.base_url, source_chain, target_chain, token_id, amount
        );
        
        let response = self.client.get(&url).send().await?;
        
        let response_body: FeesResponse = response.json().await?;
        
        if !response_body.success {
            return Err(response_body.error.unwrap_or_else(|| "Unknown error".to_string()).into());
        }
        
        Ok(FeeEstimate {
            fee_amount: response_body.fee_amount.unwrap_or_default(),
            fee_token: response_body.fee_token.unwrap_or_default(),
            fee_usd: 0.0, // Frontend sẽ hiển thị theo đơn vị gốc
        })
    }
    
    /// Thử lại relay cho một giao dịch
    pub async fn retry_relay(&self, tx_hash: &str) -> Result<(), Box<dyn Error>> {
        let url = format!("{}/bridge/relay", self.base_url);
        
        let request = RelayRequest {
            tx_hash: tx_hash.to_string(),
        };
        
        let response = self.client.post(&url)
            .json(&request)
            .send()
            .await?;
        
        if response.status() != StatusCode::OK {
            let error_response: BridgeStatusResponse = response.json().await?;
            return Err(error_response.error.unwrap_or_else(|| "Unknown error".to_string()).into());
        }
        
        Ok(())
    }
} 