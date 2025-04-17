pub mod dmd_bsc_bridge;
pub mod solana_contract;
pub mod bsc_contract;
pub mod near_contract;
pub mod arb_contract;
pub mod eth_contract;
pub mod polygon_contract;

use async_trait::async_trait;
use anyhow::Result;
use ethers::types::{U256, H256, Address};
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

// Tái export các module và interface quan trọng
pub use near_contract::smartcontract::DmdChain;
pub use eth_contract::EthContractProvider;
pub use bsc_contract::BscContractProvider;
pub use arb_contract::ArbContractProvider;
pub use solana_contract::SolanaContractProvider;
pub use polygon_contract::PolygonContractProvider;

// Tái export enum TransactionStatus từ eth_contract để đồng nhất cách xử lý
pub use eth_contract::TransactionStatus;

/// Interface chung cho tất cả các token
#[async_trait]
pub trait TokenInterface: Send + Sync + 'static {
    /// Lấy số dư token của một địa chỉ
    async fn get_balance(&self, address: &str) -> Result<U256>;
    
    /// Chuyển token từ một địa chỉ đến địa chỉ khác
    async fn transfer(&self, private_key: &str, to: &str, amount: U256) -> Result<String>;
    
    /// Lấy tổng cung token
    async fn total_supply(&self) -> Result<U256>;
    
    /// Lấy số lượng số thập phân của token
    fn decimals(&self) -> u8;
    
    /// Lấy tên của token
    fn token_name(&self) -> String;
    
    /// Lấy ký hiệu của token
    fn token_symbol(&self) -> String;
    
    /// Lấy tổng cung token dưới dạng số thập phân
    async fn get_total_supply(&self) -> Result<f64>;
    
    /// Lấy số dư token của một tài khoản dưới dạng số thập phân
    async fn get_balance_decimal(&self, account: &str) -> Result<f64>;
    
    /// Bridge token sang một blockchain khác
    async fn bridge_to(&self, to_chain: &str, from_account: &str, to_account: &str, amount: f64) -> Result<String>;
}

/// Interface cho các hoạt động bridge
#[async_trait]
pub trait BridgeInterface: Send + Sync + 'static {
    /// Bridge token đến một địa chỉ trên blockchain khác
    async fn bridge_tokens(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String>;
    
    /// Kiểm tra trạng thái của giao dịch bridge
    async fn check_bridge_status(&self, tx_hash: &str) -> Result<TransactionStatus>;
    
    /// Ước tính phí bridge
    async fn estimate_bridge_fee(&self, to_address: &str, amount: U256) -> Result<(U256, U256)>;
}

/// Factory để tạo provider phù hợp cho mỗi blockchain
pub struct TokenProviderFactory;

impl TokenProviderFactory {
    /// Tạo provider dựa trên chainId
    pub fn create_provider(chain_id: u64) -> Result<Box<dyn TokenInterface>> {
        match chain_id {
            1 => {
                // Ethereum Mainnet
                let config = eth_contract::EthContractConfig::default();
                let provider = eth_contract::EthContractProvider::new(config)?;
                Ok(Box::new(provider))
            },
            56 => {
                // Binance Smart Chain
                let config = bsc_contract::BscContractConfig::default();
                let provider = bsc_contract::BscContractProvider::new(config)?;
                Ok(Box::new(provider))
            },
            42161 => {
                // Arbitrum One
                let config = arb_contract::ArbContractConfig::default();
                let provider = arb_contract::ArbContractProvider::new(config)?;
                Ok(Box::new(provider))
            },
            137 => {
                // Polygon PoS Chain
                let config = polygon_contract::PolygonContractConfig::default();
                let provider = polygon_contract::PolygonContractProvider::new(config)?;
                Ok(Box::new(provider))
            },
            _ => Err(anyhow::anyhow!("Unsupported chain ID: {}", chain_id))
        }
    }
    
    /// Trả về danh sách các chain được hỗ trợ
    pub fn supported_chains() -> Vec<(u64, &'static str)> {
        vec![
            (1, "Ethereum"),
            (56, "BNB Smart Chain"),
            (42161, "Arbitrum One"),
            (137, "Polygon PoS"),
            (1313161554, "Aurora"),
            (43114, "Avalanche C-Chain"),
            (250, "Fantom Opera")
        ]
    }
    
    /// Kiểm tra xem một chain ID có được hỗ trợ không
    pub fn is_chain_supported(chain_id: u64) -> bool {
        Self::supported_chains().iter().any(|(id, _)| *id == chain_id)
    }
}
