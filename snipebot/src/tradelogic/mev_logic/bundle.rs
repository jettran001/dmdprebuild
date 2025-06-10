//! MEV Bundle management
//!
//! Module này chịu trách nhiệm tạo, quản lý và gửi MEV bundles thông qua
//! các dịch vụ như Flashbots, Blocknative, Eden Network, và các relay MEV-Boost.

use std::sync::Arc;
use anyhow::{Result, anyhow};
use tracing::error;
use ethers::types::{TransactionRequest, U256, Address, H160};

use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::mempool::MempoolTransaction;
use super::types::{MevOpportunity, MevExecutionMethod};
use super::types::MevConfig;
use super::constants::*;

/// Trạng thái của MEV bundle
#[derive(Debug, Clone, PartialEq)]
pub enum BundleStatus {
    /// Đang chuẩn bị
    Preparing,
    /// Đã gửi, đang chờ xác nhận
    Submitted,
    /// Đã được chấp nhận
    Accepted,
    /// Bị từ chối
    Rejected,
    /// Đã lỗi thời (không được gửi hoặc xử lý)
    Expired,
}

/// Thông tin về MEV bundle
#[derive(Debug, Clone)]
pub struct MevBundle {
    /// ID bundle
    pub id: String,
    /// Chain ID
    pub chain_id: u64,
    /// Block target
    pub block_number: u64,
    /// Thời điểm tạo
    pub created_at: u64,
    /// Trạng thái hiện tại
    pub status: BundleStatus,
    /// Các raw transaction (hex)
    pub transactions: Vec<String>,
    /// Lợi nhuận dự kiến (ETH)
    pub expected_profit: f64,
    /// Tỷ lệ chia lợi nhuận cho validator (%)
    pub profit_share_percent: f64,
    /// Tùy chọn cấu hình (key-value)
    pub options: std::collections::HashMap<String, String>,
}

/// Kết quả mô phỏng bundle
#[derive(Debug, Clone)]
pub struct BundleSimulationResult {
    /// Có thành công không
    pub success: bool,
    /// Gas đã sử dụng
    pub gas_used: u64,
    /// Lợi nhuận thực tế (ETH)
    pub actual_profit: f64,
    /// Thông báo lỗi nếu có
    pub error: Option<String>,
    /// Dữ liệu bổ sung (key-value)
    pub extra_data: std::collections::HashMap<String, String>,
}

/// Bundle provider (Flashbots, Blocknative, vv)
#[async_trait]
pub trait BundleProvider: Send + Sync + 'static {
    /// Gửi bundle đến provider
    async fn submit_bundle(&self, bundle: &MevBundle) -> Result<String, String>;
    
    /// Kiểm tra trạng thái bundle
    async fn check_bundle_status(&self, bundle_id: &str) -> Result<BundleStatus, String>;
    
    /// Mô phỏng bundle trước khi gửi
    async fn simulate_bundle(&self, bundle: &MevBundle) -> Result<BundleSimulationResult, String>;
    
    /// Hủy bundle đã gửi
    async fn cancel_bundle(&self, bundle_id: &str) -> Result<bool, String>;
    
    /// Lấy thông tin provider
    fn get_provider_info(&self) -> (String, String);
}

/// Flashbots bundle provider
pub struct FlashbotsProvider {
    /// EVM adapter
    evm_adapter: Arc<EvmAdapter>,
    /// Signing wallet cho Flashbots auth
    signing_wallet: LocalWallet,
    /// Flashbots endpoint (mainnet/testnet)
    endpoint: String,
}

impl FlashbotsProvider {
    /// Tạo mới Flashbots provider
    pub fn new(evm_adapter: Arc<EvmAdapter>, signing_key: &str, is_mainnet: bool) -> Result<Self, String> {
        // Parse private key cho signing wallet
        let wallet = match signing_key.parse::<LocalWallet>() {
            Ok(wallet) => wallet,
            Err(e) => return Err(format!("Failed to create signing wallet: {}", e)),
        };
        
        // Chọn endpoint phù hợp
        let endpoint = if is_mainnet {
            "https://relay.flashbots.net".to_string()
        } else {
            "https://relay-goerli.flashbots.net".to_string()
        };
        
        Ok(Self {
            evm_adapter,
            signing_wallet: wallet,
            endpoint,
        })
    }
    
    /// Helper method to convert hex string to transaction
    fn hex_to_transaction(&self, hex_data: &str) -> Result<TransactionRequest, String> {
        let bytes = match hex::decode(&hex_data[2..]) {
            Ok(b) => b,
            Err(e) => return Err(format!("Invalid hex data: {}", e)),
        };
        
        let tx = match rlp::decode::<TransactionRequest>(&bytes) {
            Ok(tx) => tx,
            Err(e) => return Err(format!("Failed to decode transaction: {}", e)),
        };
        
        Ok(tx)
    }
}

#[async_trait]
impl BundleProvider for FlashbotsProvider {
    async fn submit_bundle(&self, bundle: &MevBundle) -> Result<String, String> {
        // Xây dựng params cho request
        let mut transactions = Vec::new();
        for tx_hex in &bundle.transactions {
            let tx = match self.hex_to_transaction(tx_hex) {
                Ok(tx) => tx,
                Err(e) => return Err(format!("Invalid transaction: {}", e)),
            };
            transactions.push(tx);
        }
        
        // Construct the payload
        let target_block = U256::from(bundle.block_number);
        
        // Build provider URL
        let provider_url = format!("{}/relay/v1/sendBundle", self.endpoint);
        
        // Prepare client with authentication
        let client = reqwest::Client::new();
        
        // Prepare request body
        // Note: This is simplified - in a real implementation you would need
        // to properly format the Flashbots bundle payload
        let payload = serde_json::json!({
            "txs": bundle.transactions,
            "blockNumber": format!("0x{:x}", bundle.block_number),
            "minTimestamp": 0,
            "maxTimestamp": 0, // 0 means no limit
            "revertingTxHashes": []
        });
        
        // Sign the payload with the Flashbots auth key
        let signature = self.signing_wallet
            .sign_message(format!("{:?}", payload))
            .await
            .map_err(|e| format!("Failed to sign payload: {}", e))?;
        
        // Send request
        let response = client.post(&provider_url)
            .header("X-Flashbots-Signature", format!("{}", signature))
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("Bundle submission failed: {}", error_text));
        }
        
        // Parse response
        let response_json: serde_json::Value = response.json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;
        
        // Extract bundle hash (this is the ID in Flashbots)
        let bundle_hash = response_json.get("bundleHash")
            .and_then(|v| v.as_str())
            .ok_or("No bundle hash in response")?
            .to_string();
        
        Ok(bundle_hash)
    }
    
    async fn check_bundle_status(&self, bundle_id: &str) -> Result<BundleStatus, String> {
        // Build provider URL
        let provider_url = format!("{}/relay/v1/bundle-status", self.endpoint);
        
        // Prepare client with authentication
        let client = reqwest::Client::new();
        
        // Prepare request body
        let payload = serde_json::json!({
            "bundleHash": bundle_id
        });
        
        // Sign the payload with the Flashbots auth key
        let signature = self.signing_wallet
            .sign_message(format!("{:?}", payload))
            .await
            .map_err(|e| format!("Failed to sign payload: {}", e))?;
        
        // Send request
        let response = client.post(&provider_url)
            .header("X-Flashbots-Signature", format!("{}", signature))
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("Status check failed: {}", error_text));
        }
        
        // Parse response
        let response_json: serde_json::Value = response.json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;
        
        // Map response to BundleStatus
        let status = response_json.get("status")
            .and_then(|v| v.as_str())
            .ok_or("No status in response")?;
        
        match status {
            "PENDING" => Ok(BundleStatus::Submitted),
            "INCLUDED" => Ok(BundleStatus::Accepted),
            "FAILED" | "CANCELLED" => Ok(BundleStatus::Rejected),
            _ => Ok(BundleStatus::Expired),
        }
    }
    
    async fn simulate_bundle(&self, bundle: &MevBundle) -> Result<BundleSimulationResult, String> {
        // Build provider URL
        let provider_url = format!("{}/relay/v1/simulate", self.endpoint);
        
        // Prepare client with authentication
        let client = reqwest::Client::new();
        
        // Prepare request body
        let payload = serde_json::json!({
            "txs": bundle.transactions,
            "blockNumber": format!("0x{:x}", bundle.block_number),
            "stateBlockNumber": "latest"
        });
        
        // Sign the payload with the Flashbots auth key
        let signature = self.signing_wallet
            .sign_message(format!("{:?}", payload))
            .await
            .map_err(|e| format!("Failed to sign payload: {}", e))?;
        
        // Send request
        let response = client.post(&provider_url)
            .header("X-Flashbots-Signature", format!("{}", signature))
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Simulation request failed: {}", e))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("Simulation failed: {}", error_text));
        }
        
        // Parse response
        let response_json: serde_json::Value = response.json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;
        
        // Extract simulation results
        let success = response_json.get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        
        // Extract gas used
        let gas_used = response_json.get("gasUsed")
            .and_then(|v| v.as_str())
            .and_then(|s| u64::from_str_radix(&s[2..], 16).ok())
            .unwrap_or(0);
        
        // Extract profit (if available)
        let profit = response_json.get("coinbaseDiff")
            .and_then(|v| v.as_str())
            .and_then(|s| {
                if s.starts_with("0x") {
                    u64::from_str_radix(&s[2..], 16).ok()
                } else {
                    None
                }
            })
            .map(|v| v as f64 / 1e18) // Convert wei to ETH
            .unwrap_or(0.0);
        
        // Extract error if simulation failed
        let error = if !success {
            response_json.get("error")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        };
        
        // Prepare extra data
        let mut extra_data = std::collections::HashMap::new();
        if let Some(state_diff) = response_json.get("stateDiff") {
            extra_data.insert("state_diff".to_string(), state_diff.to_string());
        }
        
        if let Some(calls) = response_json.get("calls") {
            extra_data.insert("calls".to_string(), calls.to_string());
        }
        
        Ok(BundleSimulationResult {
            success,
            gas_used,
            actual_profit: profit,
            error,
            extra_data,
        })
    }
    
    async fn cancel_bundle(&self, bundle_id: &str) -> Result<bool, String> {
        // Flashbots doesn't support cancellation, bundles are automatically 
        // expired if not included in the target block
        Ok(true)
    }
    
    fn get_provider_info(&self) -> (String, String) {
        ("flashbots".to_string(), self.endpoint.clone())
    }
}

/// MEV Bundle Manager
pub struct BundleManager {
    /// Providers cho bundle submission
    providers: Vec<Arc<dyn BundleProvider>>,
    /// Bundles đang quản lý
    bundles: RwLock<Vec<MevBundle>>,
    /// Cấu hình MEV
    config: RwLock<MevConfig>,
    /// Đang chạy hay không
    running: RwLock<bool>,
}

impl BundleManager {
    /// Tạo mới bundle manager
    pub fn new(config: MevConfig) -> Self {
        Self {
            providers: Vec::new(),
            bundles: RwLock::new(Vec::new()),
            config: RwLock::new(config),
            running: RwLock::new(false),
        }
    }
    
    /// Thêm provider
    pub fn add_provider(&mut self, provider: Arc<dyn BundleProvider>) {
        self.providers.push(provider);
    }
    
    /// Khởi động monitor
    pub async fn start_monitoring(&self) {
        let mut running = self.running.write().await;
        if *running {
            return;
        }
        
        *running = true;
        
        // Clone self reference for async task
        let self_clone = self.clone();
        
        // Start monitoring task
        tokio::spawn(async move {
            self_clone.monitor_bundles().await;
        });
    }
    
    /// Dừng monitor
    pub async fn stop_monitoring(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }
    
    /// Tạo và gửi bundle mới
    pub async fn create_and_submit_bundle(
        &self, 
        chain_id: u64,
        transactions: Vec<String>,
        expected_profit: f64,
        target_block: u64,
    ) -> Result<String, String> {
        // Kiểm tra cấu hình
        let config = self.config.read().await;
        if !config.enabled {
            return Err("MEV is disabled in configuration".to_string());
        }
        
        // Check if we have providers for this chain
        if self.providers.is_empty() {
            return Err("No bundle providers available".to_string());
        }
        
        // Create bundle
        let bundle_id = Uuid::new_v4().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let bundle = MevBundle {
            id: bundle_id.clone(),
            chain_id,
            block_number: target_block,
            created_at: now,
            status: BundleStatus::Preparing,
            transactions,
            expected_profit,
            profit_share_percent: PROFIT_SHARE_PERCENT,
            options: std::collections::HashMap::new(),
        };
        
        // Add to managed bundles
        {
            let mut bundles = self.bundles.write().await;
            bundles.push(bundle.clone());
        }
        
        // Submit to all providers (for redundancy)
        let mut submission_errors = Vec::new();
        
        for provider in &self.providers {
            match provider.simulate_bundle(&bundle).await {
                Ok(sim_result) => {
                    if sim_result.success {
                        // If simulation succeeds, submit the bundle
                        match provider.submit_bundle(&bundle).await {
                            Ok(_) => {
                                // Update bundle status to submitted
                                self.update_bundle_status(&bundle_id, BundleStatus::Submitted).await;
                                
                                // Successful submission to at least one provider
                                info!(
                                    "Bundle {} submitted successfully to provider {:?}, expected profit: {} ETH", 
                                    bundle_id, 
                                    provider.get_provider_info().0,
                                    expected_profit
                                );
                                
                                return Ok(bundle_id);
                            },
                            Err(e) => {
                                submission_errors.push(format!(
                                    "Failed to submit to {}: {}", 
                                    provider.get_provider_info().0, 
                                    e
                                ));
                            }
                        }
                    } else {
                        submission_errors.push(format!(
                            "Simulation failed on {}: {}", 
                            provider.get_provider_info().0, 
                            sim_result.error.unwrap_or_else(|| "Unknown error".to_string())
                        ));
                    }
                },
                Err(e) => {
                    submission_errors.push(format!(
                        "Simulation error on {}: {}", 
                        provider.get_provider_info().0, 
                        e
                    ));
                }
            }
        }
        
        // If we reach here, all providers failed
        self.update_bundle_status(&bundle_id, BundleStatus::Rejected).await;
        
        Err(format!("All providers failed: {}", submission_errors.join("; ")))
    }
    
    /// Monitor và cập nhật trạng thái bundles
    async fn monitor_bundles(&self) {
        while *self.running.read().await {
            // Get bundles that need status update
            let bundles = self.bundles.read().await;
            let active_bundles: Vec<MevBundle> = bundles.iter()
                .filter(|b| b.status == BundleStatus::Submitted)
                .cloned()
                .collect();
            drop(bundles); // Release lock
            
            for bundle in active_bundles {
                // Check status with all providers
                for provider in &self.providers {
                    if let Ok(status) = provider.check_bundle_status(&bundle.id).await {
                        if status != BundleStatus::Submitted {
                            // Clone status để tránh bị move
                            let status_clone = status.clone();
                            
                            // Update status if changed
                            self.update_bundle_status(&bundle.id, status).await;
                            
                            // Log status change (sử dụng bản clone)
                            info!(
                                "Bundle {} status changed to {:?}", 
                                bundle.id, 
                                status_clone
                            );
                            
                            break; // Stop checking with other providers
                        }
                    }
                }
            }
            
            // Clean up expired bundles
            self.cleanup_expired_bundles().await;
            
            // Sleep before next check
            sleep(Duration::from_secs(2)).await;
        }
    }
    
    /// Update trạng thái bundle
    async fn update_bundle_status(&self, bundle_id: &str, status: BundleStatus) {
        let mut bundles = self.bundles.write().await;
        if let Some(bundle) = bundles.iter_mut().find(|b| b.id == bundle_id) {
            bundle.status = status;
        }
    }
    
    /// Dọn dẹp bundles đã hết hạn
    async fn cleanup_expired_bundles(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let mut bundles = self.bundles.write().await;
        bundles.retain(|bundle| {
            // Keep if bundle is not too old
            let age_secs = now - bundle.created_at;
            let is_active = bundle.status == BundleStatus::Preparing 
                || bundle.status == BundleStatus::Submitted;
                
            // Keep if still active and not too old
            if is_active && age_secs > 120 { // 2 minutes
                // Mark as expired
                return false;
            }
            
            // Keep if finalized less than 10 minutes ago
            if !is_active && age_secs > 600 { // 10 minutes
                return false;
            }
            
            true
        });
    }
    
    /// Lấy danh sách bundles theo trạng thái
    pub async fn get_bundles_by_status(&self, status: Option<BundleStatus>) -> Vec<MevBundle> {
        let bundles = self.bundles.read().await;
        
        if let Some(filter_status) = status {
            bundles.iter()
                .filter(|b| b.status == filter_status)
                .cloned()
                .collect()
        } else {
            bundles.clone()
        }
    }
    
    /// Clone self reference cho async tasks
    fn clone(&self) -> Self {
        let running_value = *self.running.read().blocking_lock();
        Self {
            providers: self.providers.clone(),
            bundles: RwLock::new(Vec::new()),
            config: RwLock::new(MevConfig::default()),
            running: RwLock::new(running_value),
        }
    }
}

// Thêm các providers khác như Blocknative, Eden Network, v.v. 