use std::error::Error;
use async_trait::async_trait;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use log::{debug, warn, info};
use anyhow::{Context, Result};

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

// Default API URL nếu api_url không hợp lệ
const DEFAULT_WORMHOLE_API_URL: &str = "https://api.wormhole.com/v1/";

// Triển khai client cho Wormhole
pub struct WormholeClientImpl {
    client: Client,
    base_url: Url,
}

impl WormholeClientImpl {
    pub fn new(api_url: String) -> Self {
        let client = Client::new();
        let base_url = match Url::parse(&api_url) {
            Ok(url) => {
                debug!("Using provided Wormhole API URL: {}", api_url);
                url
            },
            Err(e) => {
                warn!("Invalid Wormhole API URL: {} - Error: {}. Falling back to default URL.",  
                      api_url, e);
                match Url::parse(DEFAULT_WORMHOLE_API_URL) {
                    Ok(url) => url,
                    Err(_) => {
                        // Fallback hardcoded URL nếu cả hai URL đều không hợp lệ (rất hiếm khi xảy ra)
                        warn!("Default URL also invalid, using hardcoded URL");
                        Url::parse("https://api.wormhole.com/v1/").expect("Hardcoded URL must be valid")
                    }
                }
            }
        };
        
        info!("Initializing Wormhole client with API URL: {}", base_url);
        
        Self {
            client,
            base_url,
        }
    }
    
    /// Khởi tạo với cấu hình chi tiết
    pub fn with_config(api_url: &str, timeout_seconds: Option<u64>) -> Result<Self> {
        let mut client_builder = Client::builder();
        
        // Cấu hình timeout nếu được chỉ định
        if let Some(timeout) = timeout_seconds {
            client_builder = client_builder.timeout(std::time::Duration::from_secs(timeout));
            debug!("Setting custom timeout of {} seconds for Wormhole client", timeout);
        }
        
        let client = client_builder.build()
            .context("Failed to build HTTP client for Wormhole API")?;
            
        let base_url = Url::parse(api_url)
            .context(format!("Failed to parse Wormhole API URL: {}", api_url))?;
            
        info!("Initializing Wormhole client with custom configuration, URL: {}", base_url);
        
        Ok(Self {
            client,
            base_url,
        })
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
        debug!("Sending message from chain {} to chain {}", from_chain, to_chain);
        let url = self.base_url.join("send_message")
            .map_err(|e| format!("Failed to join URL: {}", e))?;
        
        let payload_b64 = base64::encode(&payload);
        debug!("Request payload size: {} bytes (base64 encoded)", payload_b64.len());
        
        let response = self.client
            .post(url.clone())
            .json(&serde_json::json!({
                "source_chain": from_chain,
                "target_chain": to_chain,
                "target_address": receiver,
                "payload": payload_b64,
                "finality": "finalized"
            }))
            .send()
            .await
            .map_err(|e| format!("Failed to send request to {}: {}", url, e))?;
        
        let wh_response: WormholeResponse<WormholeSendResponse> = response.json().await
            .map_err(|e| format!("Failed to parse Wormhole response: {}", e))?;
        
        if !wh_response.success {
            let error_msg = wh_response.error.unwrap_or_else(|| "Unknown error".to_string());
            warn!("Wormhole API error: {}", error_msg);
            return Err(format!("Wormhole API error: {}", error_msg).into());
        }
        
        match wh_response.data {
            Some(result) => {
                info!("Message sent successfully: tx_hash={}, sequence={}", 
                     result.transaction_hash, result.sequence);
                Ok(result.transaction_hash)
            },
            None => {
                warn!("No data from Wormhole API despite success=true");
                Err("No data from Wormhole API despite success=true".into())
            }
        }
    }
    
    async fn get_transaction_status(&self, tx_hash: &str) -> Result<String, Box<dyn Error>> {
        debug!("Getting status for transaction: {}", tx_hash);
        let url = self.base_url.join(&format!("transaction/{}", tx_hash))
            .map_err(|e| format!("Failed to join URL: {}", e))?;
        
        let response = self.client
            .get(url.clone())
            .send()
            .await
            .map_err(|e| format!("Failed to send request to {}: {}", url, e))?;
        
        let wh_response: WormholeResponse<WormholeStatusResponse> = response.json().await
            .map_err(|e| format!("Failed to parse Wormhole status response: {}", e))?;
        
        if !wh_response.success {
            let error_msg = wh_response.error.unwrap_or_else(|| "Unknown error".to_string());
            warn!("Wormhole API error when fetching status: {}", error_msg);
            return Err(format!("Wormhole API error: {}", error_msg).into());
        }
        
        match wh_response.data {
            Some(result) => {
                debug!("Transaction status received: status={}, confirmations={}, target_tx_hash={:?}", 
                      result.status, result.confirmations, result.target_tx_hash);
                Ok(result.status)
            },
            None => {
                warn!("No data from Wormhole API despite success=true when fetching status");
                Err("No data from Wormhole API despite success=true".into())
            }
        }
    }
    
    async fn estimate_fee(
        &self,
        from_chain: &str,
        to_chain: &str,
        payload_size: usize,
    ) -> Result<FeeEstimate, Box<dyn Error>> {
        debug!("Estimating fee from chain {} to chain {} for payload size {}", 
              from_chain, to_chain, payload_size);
              
        let url = self.base_url.join("estimate_fee")
            .map_err(|e| format!("Failed to join URL: {}", e))?;
        
        let response = self.client
            .post(url.clone())
            .json(&serde_json::json!({
                "source_chain": from_chain,
                "target_chain": to_chain,
                "payload_size": payload_size
            }))
            .send()
            .await
            .map_err(|e| format!("Failed to send request to {}: {}", url, e))?;
        
        let wh_response: WormholeResponse<WormholeFeeResponse> = response.json().await
            .map_err(|e| format!("Failed to parse Wormhole fee response: {}", e))?;
        
        if !wh_response.success {
            let error_msg = wh_response.error.unwrap_or_else(|| "Unknown error".to_string());
            warn!("Wormhole API error when estimating fee: {}", error_msg);
            return Err(format!("Wormhole API error: {}", error_msg).into());
        }
        
        match wh_response.data {
            Some(result) => {
                debug!("Fee estimated: {} {} (${} USD)", 
                      result.fee, result.token, result.usd_equivalent);
                      
                Ok(FeeEstimate {
                    fee_amount: result.fee,
                    fee_token: result.token,
                    fee_usd: result.usd_equivalent,
                })
            },
            None => {
                warn!("No data from Wormhole API despite success=true when estimating fee");
                Err("No data from Wormhole API despite success=true".into())
            }
        }
    }
} 