use async_trait::async_trait;
use anyhow::Result;
use ethers::types::U256;
use std::sync::Arc;

use crate::common::chain_types::DmdChain;
use crate::smartcontracts::TransactionStatus;

/// Cấu trúc dữ liệu cho thông tin phân phối token trên các blockchain
#[derive(Debug, Clone)]
pub struct TokenDistribution {
    /// Chain ID
    pub chain: DmdChain,
    
    /// Tổng cung token trên chain
    pub total_supply: f64,
    
    /// Số lượng token đã bridge ra khỏi chain
    pub bridged_out: f64,
    
    /// Số lượng token đã bridge vào chain
    pub bridged_in: f64,
    
    /// Tổng số transaction trên chain
    pub transaction_count: u64,
    
    /// Số lượng token đang lock (ví dụ: trong staking, farming)
    pub locked_amount: f64,
}

/// Cấu trúc dữ liệu tổng hợp về tổng cung token trên tất cả các blockchain
#[derive(Debug, Clone)]
pub struct MultiChainSupply {
    /// Tổng cung token trên tất cả các chain
    pub total_supply: f64,
    
    /// Phân phối token theo từng chain
    pub distributions: Vec<TokenDistribution>,
    
    /// Thời gian cập nhật (timestamp)
    pub updated_at: u64,
}

/// Interface cho TokenHub - trung tâm quản lý token trên tất cả các blockchain
#[async_trait]
pub trait TokenHub: Send + Sync + 'static {
    /// Lấy thông tin về token trên một blockchain cụ thể
    async fn get_token_info(&self, chain: DmdChain) -> Result<TokenDistribution>;
    
    /// Lấy thông tin tổng hợp về token trên tất cả các blockchain
    async fn get_distribution_stats(&self) -> Result<MultiChainSupply>;
    
    /// Transfer token trên cùng một blockchain
    async fn transfer(&self, private_key: &str, to_address: &str, chain: DmdChain, amount: U256) -> Result<String>;
    
    /// Bridge token từ blockchain này sang blockchain khác
    async fn bridge(&self, private_key: &str, to_address: &str, from_chain: DmdChain, to_chain: DmdChain, amount: U256) -> Result<String>;
    
    /// Kiểm tra trạng thái giao dịch bridge
    async fn check_bridge_status(&self, tx_hash: &str, from_chain: DmdChain, to_chain: DmdChain) -> Result<TransactionStatus>;
    
    /// Ước tính phí bridge
    async fn estimate_bridge_fee(&self, from_chain: DmdChain, to_chain: DmdChain, amount: U256) -> Result<(U256, U256)>;
    
    /// Lấy số dư token của một địa chỉ trên một blockchain
    async fn get_balance(&self, address: &str, chain: DmdChain) -> Result<U256>;
    
    /// Lấy số dư token của một địa chỉ trên một blockchain dưới dạng số thập phân
    async fn get_balance_decimal(&self, address: &str, chain: DmdChain) -> Result<f64>;
    
    /// Đồng bộ hóa dữ liệu giữa các blockchain
    async fn sync_data(&self) -> Result<()>;
    
    /// Lấy số lượng số thập phân của token trên một blockchain
    async fn get_decimals(&self, chain: DmdChain) -> Result<u8>;
    
    /// Lấy tên của token trên một blockchain
    async fn get_token_name(&self, chain: DmdChain) -> Result<String>;
    
    /// Lấy ký hiệu của token trên một blockchain
    async fn get_token_symbol(&self, chain: DmdChain) -> Result<String>;
} 