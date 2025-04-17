//! Các trait chính định nghĩa giao diện bridge

use async_trait::async_trait;
use ethers::types::U256;
use anyhow::Result;

use crate::smartcontracts::dmd_token::DmdChain;
use super::types::{BridgeTransaction, BridgeStatus, BridgeConfig};
use super::error::{BridgeError, BridgeResult};

/// Trait cho bridge hub (NEAR)
#[async_trait]
pub trait BridgeHub: Send + Sync + 'static {
    /// Khởi tạo hub với cấu hình cho trước
    async fn initialize(&mut self, config: BridgeConfig) -> BridgeResult<()>;
    
    /// Nhận token từ chain khác vào hub
    async fn receive_from_spoke(
        &self,
        from_chain: DmdChain,
        from_address: &str, 
        to_near_account: &str, 
        amount: U256
    ) -> BridgeResult<BridgeTransaction>;
    
    /// Gửi token từ hub sang chain khác
    async fn send_to_spoke(
        &self,
        private_key: &str,
        to_chain: DmdChain,
        to_address: &str,
        amount: U256
    ) -> BridgeResult<BridgeTransaction>;
    
    /// Kiểm tra trạng thái giao dịch bridge
    async fn check_transaction_status(&self, tx_id: &str) -> BridgeResult<BridgeStatus>;
    
    /// Lấy thông tin giao dịch bridge
    async fn get_transaction(&self, tx_id: &str) -> BridgeResult<BridgeTransaction>;
    
    /// Ước tính phí bridge
    async fn estimate_fee(&self, to_chain: DmdChain, amount: U256) -> BridgeResult<U256>;
    
    /// Lấy danh sách các chain được hỗ trợ
    fn get_supported_chains(&self) -> Vec<DmdChain>;
}

/// Trait cho bridge spoke (EVM chains và Solana)
#[async_trait]
pub trait BridgeSpoke: Send + Sync + 'static {
    /// Loại chain của spoke
    fn chain_type(&self) -> DmdChain;
    
    /// Khởi tạo spoke với cấu hình cho trước
    async fn initialize(&mut self, config: BridgeConfig) -> BridgeResult<()>;
    
    /// Gửi token từ spoke sang hub (wrap ERC-1155 -> ERC-20 -> NEP-141)
    async fn send_to_hub(
        &self,
        private_key: &str,
        near_account: &str,
        amount: U256
    ) -> BridgeResult<BridgeTransaction>;
    
    /// Nhận token từ hub (unwrap NEP-141 -> ERC-20 -> ERC-1155)
    async fn receive_from_hub(
        &self,
        tx_proof: &str,
        to_address: &str,
        amount: U256
    ) -> BridgeResult<BridgeTransaction>;
    
    /// Wrap token từ ERC-1155 sang ERC-20
    async fn wrap_erc1155_to_erc20(
        &self,
        private_key: &str,
        amount: U256
    ) -> BridgeResult<String>;
    
    /// Unwrap token từ ERC-20 sang ERC-1155
    async fn unwrap_erc20_to_erc1155(
        &self,
        private_key: &str,
        amount: U256
    ) -> BridgeResult<String>;
    
    /// Kiểm tra trạng thái giao dịch
    async fn check_transaction_status(&self, tx_hash: &str) -> BridgeResult<BridgeStatus>;
    
    /// Lấy thông tin giao dịch bridge
    async fn get_transaction(&self, tx_id: &str) -> BridgeResult<BridgeTransaction>;
    
    /// Ước tính phí bridge
    async fn estimate_fee(&self, amount: U256) -> BridgeResult<U256>;
    
    /// Kiểm tra xem spoke có hỗ trợ một địa chỉ cụ thể
    fn is_address_valid(&self, address: &str) -> bool;
}

/// Trait chung cho provider bridge
#[async_trait]
pub trait BridgeProvider: Send + Sync + 'static {
    /// Khởi tạo bridge provider
    async fn initialize(&mut self, config: BridgeConfig) -> BridgeResult<()>;
    
    /// Bridge token từ chain nguồn đến chain đích
    async fn bridge_token(
        &self,
        private_key: &str,
        from_chain: DmdChain,
        to_chain: DmdChain,
        to_address: &str,
        amount: U256
    ) -> BridgeResult<BridgeTransaction>;
    
    /// Kiểm tra trạng thái giao dịch bridge
    async fn check_transaction_status(&self, tx_id: &str) -> BridgeResult<BridgeStatus>;
    
    /// Lấy thông tin chi tiết của giao dịch bridge
    async fn get_transaction_details(&self, tx_id: &str) -> BridgeResult<BridgeTransaction>;
    
    /// Lấy lịch sử các giao dịch bridge cho một địa chỉ cụ thể
    async fn get_transaction_history(
        &self,
        address: &str,
        limit: u64,
        offset: u64
    ) -> BridgeResult<Vec<BridgeTransaction>>;
    
    /// Ước tính phí bridge
    async fn estimate_bridge_fee(
        &self,
        from_chain: DmdChain,
        to_chain: DmdChain,
        amount: U256
    ) -> BridgeResult<U256>;
    
    /// Trả về các chain được hỗ trợ
    fn get_supported_chains(&self) -> Vec<(DmdChain, DmdChain)>;
} 