//! DMD Token Facade - facade for the new hub-spoke model
//! Cung cấp khả năng tương thích ngược cho các ứng dụng hiện tại
//! Các ứng dụng mới nên sử dụng trực tiếp các module trong hub, evm, và non_evm

use anyhow::{anyhow, Result};
use ethers::types::U256;
use std::sync::Arc;

use crate::common::chain_types::DmdChain;
use crate::smartcontracts::hub::{DmdTokenHub, TokenHub, MultiChainSupply};
use crate::smartcontracts::TransactionStatus;

/// Re-export từ file dmd_token.rs cũ
pub type BridgeTransaction = crate::smartcontracts::hub::TokenDistribution;
pub type ChainConfig = crate::smartcontracts::hub::ChainConfig;

/// DMD Token - facade cho mô hình hub-spoke mới
#[derive(Clone)]
pub struct DmdToken {
    /// Hub xử lý tất cả các blockchain
    hub: Arc<DmdTokenHub>,
    
    /// Chain hiện tại
    current_chain: DmdChain,
}

impl DmdToken {
    /// Khởi tạo DmdToken mới
    pub fn new() -> Self {
        Self {
            hub: Arc::new(DmdTokenHub::new()),
            current_chain: DmdChain::Ethereum,
        }
    }
    
    /// Khởi tạo DmdToken mới với TTL tùy chỉnh
    pub fn with_cache_ttl(ttl: u64) -> Self {
        Self {
            hub: Arc::new(DmdTokenHub::with_cache_ttl(ttl)),
            current_chain: DmdChain::Ethereum,
        }
    }
    
    /// Lấy tổng cung token trên tất cả các blockchain
    pub async fn get_total_supply_multi_chain(&self) -> MultiChainSupply {
        match self.hub.get_distribution_stats().await {
            Ok(stats) => stats,
            Err(e) => {
                tracing::error!("Failed to get multi-chain supply: {}", e);
                MultiChainSupply {
                    total_supply: 0.0,
                    distributions: vec![],
                    updated_at: 0,
                }
            }
        }
    }
    
    /// Transfer token trên Solana
    pub async fn transfer_to_solana(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
        self.hub.transfer(private_key, to_address, DmdChain::Solana, amount).await
    }
    
    /// Transfer token trên NEAR
    pub async fn transfer_to_near(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
        self.hub.transfer(private_key, to_address, DmdChain::Near, amount).await
    }
    
    /// Transfer token trên Avalanche
    pub async fn transfer_to_avalanche(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
        self.hub.transfer(private_key, to_address, DmdChain::Avalanche, amount).await
    }
    
    /// Transfer token trên Polygon
    pub async fn transfer_to_polygon(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
        self.hub.transfer(private_key, to_address, DmdChain::Polygon, amount).await
    }
    
    /// Transfer token trên Arbitrum
    pub async fn transfer_to_arbitrum(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
        self.hub.transfer(private_key, to_address, DmdChain::Arbitrum, amount).await
    }
    
    /// Bridge token giữa các blockchain
    pub async fn bridge_tokens(&self, private_key: &str, to_address: &str, to_chain: DmdChain, amount: U256) -> Result<String> {
        self.hub.bridge(private_key, to_address, self.current_chain, to_chain, amount).await
    }
    
    /// Kiểm tra tính hợp lệ của các tham số bridge
    fn validate_bridge_parameters(&self, private_key: &str, to_address: &str, to_chain: DmdChain) -> Result<()> {
        if private_key.is_empty() {
            return Err(anyhow!("Private key cannot be empty"));
        }
        
        if to_address.is_empty() {
            return Err(anyhow!("Recipient address cannot be empty"));
        }
        
        if self.current_chain == to_chain {
            return Err(anyhow!("Source and destination chains cannot be the same: {}", to_chain.name()));
        }
        
        Ok(())
    }
    
    /// Chuyển đổi số dư từ U256 sang f64
    pub async fn convert_balance_to_f64(&self, balance: U256, chain: DmdChain) -> Result<f64> {
        // Lấy số lượng số thập phân
        let decimals = self.hub.get_decimals(chain).await?;
        
        // Chuyển đổi
        let balance_str = balance.to_string();
        let balance_f64 = balance_str.parse::<f64>().map_err(|e| {
            anyhow!("Failed to parse balance '{}' to f64: {}", balance_str, e)
        })?;
        
        // Chia cho 10^decimals
        let divisor = 10_f64.powi(decimals as i32);
        Ok(balance_f64 / divisor)
    }
    
    /// Đặt chain hiện tại
    pub fn set_current_chain(&mut self, chain: DmdChain) {
        self.current_chain = chain;
    }
    
    /// Lấy chain hiện tại
    pub fn get_current_chain(&self) -> DmdChain {
        self.current_chain
    }
    
    /// Lấy số decimals của token trên một chain
    pub async fn get_token_decimals(&self, chain: DmdChain) -> Result<u8> {
        self.hub.get_decimals(chain).await
    }
    
    /// Đồng bộ dữ liệu giữa các blockchain
    pub async fn sync_data(&self) -> Result<()> {
        self.hub.sync_data().await
    }
    
    /// Kiểm tra trạng thái giao dịch bridge
    pub async fn check_bridge_status(&self, tx_hash: &str, from_chain: DmdChain, to_chain: DmdChain) -> Result<TransactionStatus> {
        self.hub.check_bridge_status(tx_hash, from_chain, to_chain).await
    }
} 