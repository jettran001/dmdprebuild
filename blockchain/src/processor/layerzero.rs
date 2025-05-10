use std::error::Error;
use async_trait::async_trait;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};

use crate::processor::bridge_orchestrator::{LayerZeroClient, FeeEstimate};

// Kết quả của Layer Zero API
#[derive(Debug, Serialize, Deserialize)]
struct LayerZeroResponse<T> {
    success: bool,
    result: Option<T>,
    error: Option<String>,
}

// Kết quả ước tính phí
#[derive(Debug, Serialize, Deserialize)]
struct LayerZeroFeeResponse {
    native_fee: String,
    zro_fee: String,
    native_symbol: String,
    usd_value: f64,
}

// Kết quả gửi message
#[derive(Debug, Serialize, Deserialize)]
struct LayerZeroSendResponse {
    tx_hash: String,
    status: String,
}

// Kết quả trạng thái transaction
#[derive(Debug, Serialize, Deserialize)]
struct LayerZeroStatusResponse {
    status: String,
    confirmations: u64,
    destination_tx_hash: Option<String>,
}

// Triển khai client cho LayerZero
pub struct LayerZeroClientImpl {
    client: Client,
    api_key: String,
    base_url: Url,
}

impl LayerZeroClientImpl {
    pub fn new(api_key: String) -> Self {
        let client = Client::new();
        let base_url = Url::parse("https://api.layerzero.network/v1/").unwrap();
        
        Self {
            client,
            api_key,
            base_url,
        }
    }
}

#[async_trait]
impl LayerZeroClient for LayerZeroClientImpl {
    async fn send_message(
        &self,
        from_chain_id: u16,
        to_chain_id: u16,
        receiver: String,
        payload: Vec<u8>,
    ) -> Result<String, Box<dyn Error>> {
        let url = self.base_url.join("message/send")?;
        
        let payload_hex = hex::encode(&payload);
        
        let response = self.client
            .post(url)
            .header("x-api-key", &self.api_key)
            .json(&serde_json::json!({
                "srcChainId": from_chain_id,
                "dstChainId": to_chain_id,
                "dstAddress": receiver,
                "payload": payload_hex,
                "useZro": false
            }))
            .send()
            .await?;
        
        let lz_response: LayerZeroResponse<LayerZeroSendResponse> = response.json().await?;
        
        if !lz_response.success {
            return Err(format!("LayerZero API error: {}", lz_response.error.unwrap_or_default()).into());
        }
        
        let result = lz_response.result.ok_or("No result from LayerZero API")?;
        
        Ok(result.tx_hash)
    }
    
    async fn get_transaction_status(&self, tx_hash: &str) -> Result<String, Box<dyn Error>> {
        let url = self.base_url.join(&format!("transaction/{}", tx_hash))?;
        
        let response = self.client
            .get(url)
            .header("x-api-key", &self.api_key)
            .send()
            .await?;
        
        let lz_response: LayerZeroResponse<LayerZeroStatusResponse> = response.json().await?;
        
        if !lz_response.success {
            return Err(format!("LayerZero API error: {}", lz_response.error.unwrap_or_default()).into());
        }
        
        let result = lz_response.result.ok_or("No result from LayerZero API")?;
        
        Ok(result.status)
    }
    
    async fn estimate_fee(
        &self,
        from_chain_id: u16,
        to_chain_id: u16,
        payload_size: usize,
    ) -> Result<FeeEstimate, Box<dyn Error>> {
        let url = self.base_url.join("fee/estimate")?;
        
        let response = self.client
            .post(url)
            .header("x-api-key", &self.api_key)
            .json(&serde_json::json!({
                "srcChainId": from_chain_id,
                "dstChainId": to_chain_id,
                "payloadSize": payload_size,
                "useZro": false
            }))
            .send()
            .await?;
        
        let lz_response: LayerZeroResponse<LayerZeroFeeResponse> = response.json().await?;
        
        if !lz_response.success {
            return Err(format!("LayerZero API error: {}", lz_response.error.unwrap_or_default()).into());
        }
        
        let result = lz_response.result.ok_or("No result from LayerZero API")?;
        
        Ok(FeeEstimate {
            fee_amount: result.native_fee,
            fee_token: result.native_symbol,
            fee_usd: result.usd_value,
        })
    }
} 