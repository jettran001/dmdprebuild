pub mod dmd_bsc_bridge;
pub mod solana_contract;
pub mod bsc_contract;
pub mod near_contract;

use async_trait::async_trait;
use anyhow::Result;
use ethers::types::U256;

// Tái export DmdChain để các module khác có thể sử dụng
pub use near_contract::smartcontract::DmdChain;

#[async_trait]
pub trait TokenInterface: Send + Sync + 'static {
    async fn get_balance(&self, address: &str) -> Result<U256>;
    async fn transfer(&self, private_key: &str, to: &str, amount: U256) -> Result<String>;
    async fn total_supply(&self) -> Result<U256>;
    fn decimals(&self) -> u8;
    fn token_name(&self) -> String;
    fn token_symbol(&self) -> String;
    async fn get_total_supply(&self) -> Result<f64>;
    async fn get_balance(&self, account: &str) -> Result<f64>;
    async fn bridge_to(&self, to_chain: &str, from_account: &str, to_account: &str, amount: f64) -> Result<String>;
}

#[async_trait]
pub trait BridgeInterface: Send + Sync + 'static {
    async fn bridge_tokens(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String>;
    async fn check_bridge_status(&self, tx_hash: &str) -> Result<String>;
    async fn estimate_bridge_fee(&self, to_address: &str, amount: U256) -> Result<(U256, U256)>;
}
