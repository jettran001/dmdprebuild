use std::error::Error;
use async_trait::async_trait;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use log::{debug, warn, info};
use anyhow::{Context, Result};

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

/// Default base URL for LayerZero API if not provided
const DEFAULT_LAYERZERO_API_URL: &str = "https://api.layerzero.network/v1/";

// Triển khai client cho LayerZero
pub struct LayerZeroClientImpl {
    client: Client,
    api_key: String,
    base_url: Url,
}

impl LayerZeroClientImpl {
    pub fn new(api_key: String) -> Self {
        let client = Client::new();
        let base_url = match Url::parse(DEFAULT_LAYERZERO_API_URL) {
            Ok(url) => {
                debug!("Using default LayerZero API URL: {}", DEFAULT_LAYERZERO_API_URL);
                url
            },
            Err(e) => {
                // Trường hợp này không nên xảy ra vì DEFAULT_LAYERZERO_API_URL là hardcoded
                // nhưng vẫn xử lý để tránh panic
                warn!("Invalid default LayerZero API URL: {} - Error: {}. Falling back to hardcoded URL.",  
                      DEFAULT_LAYERZERO_API_URL, e);
                
                // Hardcoded fallback để tránh panic - không nên xảy ra
                Url::parse("https://api.layerzero.network/v1/").expect("Hardcoded URL should be valid")
            }
        };
        
        info!("Initializing LayerZero client with API URL: {}", base_url);
        
        Self {
            client,
            api_key,
            base_url,
        }
    }
    
    /// Khởi tạo với URL tùy chỉnh - cho phép override default URL
    pub fn with_custom_url(api_key: String, base_url: &str) -> Result<Self> {
        let client = Client::new();
        let base_url = Url::parse(base_url)
            .context(format!("Failed to parse LayerZero API URL: {}", base_url))?;
            
        info!("Initializing LayerZero client with custom API URL: {}", base_url);
        
        Ok(Self {
            client,
            api_key,
            base_url,
        })
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
        debug!("Sending message from chain {} to chain {}", from_chain_id, to_chain_id);
        let url = self.base_url.join("message/send")
            .map_err(|e| format!("Failed to join URL: {}", e))?;
        
        let payload_hex = hex::encode(&payload);
        debug!("Request payload size: {} bytes", payload.len());
        
        let response = self.client
            .post(url.clone())
            .header("x-api-key", &self.api_key)
            .json(&serde_json::json!({
                "srcChainId": from_chain_id,
                "dstChainId": to_chain_id,
                "dstAddress": receiver,
                "payload": payload_hex,
                "useZro": false
            }))
            .send()
            .await
            .map_err(|e| format!("Failed to send request to {}: {}", url, e))?;
        
        let lz_response: LayerZeroResponse<LayerZeroSendResponse> = response.json().await
            .map_err(|e| format!("Failed to parse LayerZero response: {}", e))?;
        
        if !lz_response.success {
            let error_msg = lz_response.error.unwrap_or_else(|| "Unknown error".to_string());
            warn!("LayerZero API error: {}", error_msg);
            return Err(format!("LayerZero API error: {}", error_msg).into());
        }
        
        match lz_response.result {
            Some(result) => {
                info!("Message sent successfully: tx_hash={}", result.tx_hash);
                Ok(result.tx_hash)
            },
            None => {
                warn!("No result from LayerZero API despite success=true");
                Err("No result from LayerZero API despite success=true".into())
            }
        }
    }
    
    async fn get_transaction_status(&self, tx_hash: &str) -> Result<String, Box<dyn Error>> {
        debug!("Getting status for transaction: {}", tx_hash);
        let url = self.base_url.join(&format!("transaction/{}", tx_hash))
            .map_err(|e| format!("Failed to join URL: {}", e))?;
        
        let response = self.client
            .get(url.clone())
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|e| format!("Failed to send request to {}: {}", url, e))?;
        
        let lz_response: LayerZeroResponse<LayerZeroStatusResponse> = response.json().await
            .map_err(|e| format!("Failed to parse LayerZero status response: {}", e))?;
        
        if !lz_response.success {
            let error_msg = lz_response.error.unwrap_or_else(|| "Unknown error".to_string());
            warn!("LayerZero API error when fetching status: {}", error_msg);
            return Err(format!("LayerZero API error: {}", error_msg).into());
        }
        
        match lz_response.result {
            Some(result) => {
                debug!("Transaction status received: status={}, confirmations={}", 
                      result.status, result.confirmations);
                Ok(result.status)
            },
            None => {
                warn!("No result from LayerZero API despite success=true when fetching status");
                Err("No result from LayerZero API despite success=true".into())
            }
        }
    }
    
    async fn estimate_fee(
        &self,
        from_chain_id: u16,
        to_chain_id: u16,
        payload_size: usize,
    ) -> Result<FeeEstimate, Box<dyn Error>> {
        debug!("Estimating fee from chain {} to chain {} for payload size {}", 
              from_chain_id, to_chain_id, payload_size);
              
        let url = self.base_url.join("fee/estimate")
            .map_err(|e| format!("Failed to join URL: {}", e))?;
        
        let response = self.client
            .post(url.clone())
            .header("x-api-key", &self.api_key)
            .json(&serde_json::json!({
                "srcChainId": from_chain_id,
                "dstChainId": to_chain_id,
                "payloadSize": payload_size,
                "useZro": false
            }))
            .send()
            .await
            .map_err(|e| format!("Failed to send request to {}: {}", url, e))?;
        
        let lz_response: LayerZeroResponse<LayerZeroFeeResponse> = response.json().await
            .map_err(|e| format!("Failed to parse LayerZero fee response: {}", e))?;
        
        if !lz_response.success {
            let error_msg = lz_response.error.unwrap_or_else(|| "Unknown error".to_string());
            warn!("LayerZero API error when estimating fee: {}", error_msg);
            return Err(format!("LayerZero API error: {}", error_msg).into());
        }
        
        match lz_response.result {
            Some(result) => {
                debug!("Fee estimated: {} {} (${} USD)", 
                      result.native_fee, result.native_symbol, result.usd_value);
                      
                Ok(FeeEstimate {
                    fee_amount: result.native_fee,
                    fee_token: result.native_symbol,
                    fee_usd: result.usd_value,
                })
            },
            None => {
                warn!("No result from LayerZero API despite success=true when estimating fee");
                Err("No result from LayerZero API despite success=true".into())
            }
        }
    }
} 