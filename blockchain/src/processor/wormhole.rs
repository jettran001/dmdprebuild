use std::error::Error;
use async_trait::async_trait;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};

use crate::processor::bridge_orchestrator::{WormholeClient, FeeEstimate};

// Kết quả của Wormhole API
#[derive(Debug, Serialize, Deserialize)]
struct WormholeResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

// Kết quả ước tính phí
#[derive(Debug, Serialize, Deserialize)]
struct WormholeFeeResponse {
    fee: String,
    token: String,
    usd_equivalent: f64,
}

// Kết quả gửi message
#[derive(Debug, Serialize, Deserialize)]
struct WormholeSendResponse {
    sequence: String,
    emitter_chain: String,
    emitter_address: String,
    transaction_hash: String,
}

// Kết quả trạng thái message
#[derive(Debug, Serialize, Deserialize)]
struct WormholeStatusResponse {
    status: String,
    source_tx_hash: String,
    target_tx_hash: Option<String>,
    confirmations: u64,
}

// Triển khai client cho Wormhole
pub struct WormholeClientImpl {
    client: Client,
    base_url: Url,
}

impl WormholeClientImpl {
    pub fn new(api_url: String) -> Self {
        let client = Client::new();
        let base_url = Url::parse(&api_url).unwrap();
        
        Self {
            client,
            base_url,
        }
    }
}

#[async_trait]
impl WormholeClient for WormholeClientImpl {
    async fn send_message(
        &self,
        from_chain: &str,
        to_chain: &str,
        receiver: String,
        payload: Vec<u8>,
    ) -> Result<String, Box<dyn Error>> {
        let url = self.base_url.join("v1/send_message")?;
        
        let payload_b64 = base64::encode(&payload);
        
        let response = self.client
            .post(url)
            .json(&serde_json::json!({
                "source_chain": from_chain,
                "target_chain": to_chain,
                "target_address": receiver,
                "payload": payload_b64,
                "finality": "finalized"
            }))
            .send()
            .await?;
        
        let wh_response: WormholeResponse<WormholeSendResponse> = response.json().await?;
        
        if !wh_response.success {
            return Err(format!("Wormhole API error: {}", wh_response.error.unwrap_or_default()).into());
        }
        
        let result = wh_response.data.ok_or("No result from Wormhole API")?;
        
        Ok(result.transaction_hash)
    }
    
    async fn get_transaction_status(&self, tx_hash: &str) -> Result<String, Box<dyn Error>> {
        let url = self.base_url.join(&format!("v1/transaction/{}", tx_hash))?;
        
        let response = self.client
            .get(url)
            .send()
            .await?;
        
        let wh_response: WormholeResponse<WormholeStatusResponse> = response.json().await?;
        
        if !wh_response.success {
            return Err(format!("Wormhole API error: {}", wh_response.error.unwrap_or_default()).into());
        }
        
        let result = wh_response.data.ok_or("No result from Wormhole API")?;
        
        Ok(result.status)
    }
    
    async fn estimate_fee(
        &self,
        from_chain: &str,
        to_chain: &str,
        payload_size: usize,
    ) -> Result<FeeEstimate, Box<dyn Error>> {
        let url = self.base_url.join("v1/estimate_fee")?;
        
        let response = self.client
            .post(url)
            .json(&serde_json::json!({
                "source_chain": from_chain,
                "target_chain": to_chain,
                "payload_size": payload_size
            }))
            .send()
            .await?;
        
        let wh_response: WormholeResponse<WormholeFeeResponse> = response.json().await?;
        
        if !wh_response.success {
            return Err(format!("Wormhole API error: {}", wh_response.error.unwrap_or_default()).into());
        }
        
        let result = wh_response.data.ok_or("No result from Wormhole API")?;
        
        Ok(FeeEstimate {
            fee_amount: result.fee,
            fee_token: result.token,
            fee_usd: result.usd_equivalent,
        })
    }
} 