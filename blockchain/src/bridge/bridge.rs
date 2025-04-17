//! Module triển khai chức năng bridge cho token DMD trên các blockchain.
//! Sử dụng mô hình hub-and-spoke với NEAR Protocol là hub trung tâm.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::blockchain::BlockchainProvider;
use crate::bridge::error::{BridgeError, BridgeResult, is_evm_chain};
use crate::smartcontracts::dmd_token::DmdChain;

/// Trạng thái của một giao dịch bridge
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeTransactionStatus {
    /// Giao dịch được tạo nhưng chưa bắt đầu
    Created,
    /// Giao dịch đang xử lý
    InProgress,
    /// Giao dịch hoàn thành
    Completed,
    /// Giao dịch thất bại
    Failed,
}

/// Thông tin một giao dịch bridge
#[derive(Debug, Clone)]
pub struct BridgeTransaction {
    /// ID giao dịch
    pub id: String,
    /// Chain nguồn
    pub source_chain: DmdChain,
    /// Chain đích
    pub target_chain: DmdChain,
    /// Địa chỉ nguồn
    pub source_address: String,
    /// Địa chỉ đích
    pub target_address: String,
    /// Số lượng token
    pub amount: u128,
    /// Hash giao dịch trên chain nguồn
    pub source_tx_hash: Option<String>,
    /// Hash giao dịch trên chain đích
    pub target_tx_hash: Option<String>,
    /// Trạng thái
    pub status: BridgeTransactionStatus,
    /// Thời gian tạo
    pub created_at: DateTime<Utc>,
    /// Thời gian cập nhật
    pub updated_at: DateTime<Utc>,
    /// Thông tin lỗi (nếu có)
    pub error: Option<String>,
}

/// Trait định nghĩa adapter cho bridge
#[async_trait]
pub trait BridgeAdapter: Send + Sync {
    /// Trả về tên của adapter
    fn get_adapter_name(&self) -> &str;
    
    /// Trả về danh sách các chain được hỗ trợ
    fn get_supported_chains(&self) -> Vec<DmdChain>;
    
    /// Kiểm tra xem adapter có hỗ trợ route này không
    fn supports_route(&self, source: &DmdChain, target: &DmdChain) -> bool;
    
    /// Chuyển token từ chain nguồn sang chain đích
    async fn transfer(
        &self,
        tx_id: &str,
        source_chain: &DmdChain,
        target_chain: &DmdChain,
        source_address: &str,
        target_address: &str,
        amount: u128,
    ) -> BridgeResult<String>;
    
    /// Kiểm tra trạng thái giao dịch
    async fn check_transaction_status(
        &self,
        tx_id: &str,
        source_chain: &DmdChain,
        target_chain: &DmdChain,
        source_tx_hash: &str,
    ) -> BridgeResult<BridgeTransactionStatus>;
}

/// Adapter sử dụng LayerZero cho các EVM chain
pub struct LayerZeroAdapter {
    /// Các chain được hỗ trợ
    supported_chains: HashSet<DmdChain>,
    /// Provider factory
    provider_factory: Arc<dyn BlockchainProvider>,
    /// Ánh xạ từ chain sang LayerZero chain ID
    chain_to_lz_id: HashMap<DmdChain, u16>,
}

impl LayerZeroAdapter {
    /// Tạo mới adapter LayerZero
    pub fn new(provider_factory: Arc<dyn BlockchainProvider>) -> Self {
        let mut supported_chains = HashSet::new();
        supported_chains.insert(DmdChain::Ethereum);
        supported_chains.insert(DmdChain::BinanceSmartChain);
        supported_chains.insert(DmdChain::Avalanche);
        supported_chains.insert(DmdChain::Polygon);
        supported_chains.insert(DmdChain::Arbitrum);
        supported_chains.insert(DmdChain::Optimism);
        supported_chains.insert(DmdChain::Base);
        
        let mut chain_to_lz_id = HashMap::new();
        chain_to_lz_id.insert(DmdChain::Ethereum, 101);
        chain_to_lz_id.insert(DmdChain::BinanceSmartChain, 102);
        chain_to_lz_id.insert(DmdChain::Avalanche, 106);
        chain_to_lz_id.insert(DmdChain::Polygon, 109);
        chain_to_lz_id.insert(DmdChain::Arbitrum, 110);
        chain_to_lz_id.insert(DmdChain::Optimism, 111);
        chain_to_lz_id.insert(DmdChain::Base, 184);
        
        Self {
            supported_chains,
            provider_factory,
            chain_to_lz_id,
        }
    }
    
    /// Trả về LayerZero ID của chain
    fn get_lz_chain_id(&self, chain: &DmdChain) -> BridgeResult<u16> {
        self.chain_to_lz_id.get(chain)
            .copied()
            .ok_or_else(|| BridgeError::UnsupportedChain(format!("Chain không hỗ trợ LayerZero: {:?}", chain)))
    }
}

#[async_trait]
impl BridgeAdapter for LayerZeroAdapter {
    fn get_adapter_name(&self) -> &str {
        "LayerZero"
    }
    
    fn get_supported_chains(&self) -> Vec<DmdChain> {
        self.supported_chains.iter().cloned().collect()
    }
    
    fn supports_route(&self, source: &DmdChain, target: &DmdChain) -> bool {
        self.supported_chains.contains(source) && self.supported_chains.contains(target)
    }
    
    async fn transfer(
        &self,
        tx_id: &str,
        source_chain: &DmdChain,
        target_chain: &DmdChain,
        source_address: &str,
        target_address: &str,
        amount: u128,
    ) -> BridgeResult<String> {
        if !self.supports_route(source_chain, target_chain) {
            return Err(BridgeError::UnsupportedRoute(
                format!("LayerZero không hỗ trợ route từ {:?} đến {:?}", source_chain, target_chain)
            ));
        }
        
        // Lấy LayerZero ID cho chain đích
        let target_lz_id = self.get_lz_chain_id(target_chain)?;
        
        // Lấy provider cho chain nguồn
        let provider = self.provider_factory.clone();
        
        // Khởi tạo giao dịch bridge (giả lập)
        // Trong triển khai thực tế, đây sẽ là một lệnh gọi hợp đồng LayerZero
        let tx_hash = format!("0x{:x}", Uuid::new_v4().as_u128());
        
        // Ghi log
        log::info!(
            "Bridge transaction created: ID={}, Source={:?}, Target={:?}, Amount={}, Hash={}",
            tx_id, source_chain, target_chain, amount, tx_hash
        );
        
        Ok(tx_hash)
    }
    
    async fn check_transaction_status(
        &self,
        tx_id: &str,
        source_chain: &DmdChain,
        target_chain: &DmdChain,
        source_tx_hash: &str,
    ) -> BridgeResult<BridgeTransactionStatus> {
        // Kiểm tra xem route có được hỗ trợ không
        if !self.supports_route(source_chain, target_chain) {
            return Err(BridgeError::UnsupportedRoute(
                format!("LayerZero không hỗ trợ route từ {:?} đến {:?}", source_chain, target_chain)
            ));
        }
        
        // Lấy provider
        let provider = self.provider_factory.clone();
        
        // Trong triển khai thực tế, sẽ kiểm tra trạng thái giao dịch trên bridge contract
        // Giả lập: Coi như đã hoàn thành
        
        Ok(BridgeTransactionStatus::Completed)
    }
}

/// Bridge manager quản lý các giao dịch bridge
pub struct BridgeManager {
    /// Danh sách các adapter
    adapters: Vec<Arc<dyn BridgeAdapter>>,
    /// Danh sách các giao dịch
    transactions: Arc<RwLock<HashMap<String, BridgeTransaction>>>,
}

impl BridgeManager {
    /// Tạo một bridge manager mới
    pub fn new(provider_factory: Arc<dyn BlockchainProvider>) -> Self {
        let adapters: Vec<Arc<dyn BridgeAdapter>> = vec![
            Arc::new(LayerZeroAdapter::new(provider_factory)),
        ];
        
        Self {
            adapters,
            transactions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Tạo giao dịch bridge mới
    pub fn create_transaction(
        &self,
        source_chain: DmdChain,
        target_chain: DmdChain,
        source_address: String,
        target_address: String,
        amount: u128,
    ) -> BridgeResult<String> {
        // Kiểm tra xem bridge có được hỗ trợ không
        if !crate::bridge::error::is_bridge_supported(&source_chain, &target_chain) {
            return Err(BridgeError::UnsupportedRoute(
                format!("Không hỗ trợ bridge từ {:?} đến {:?}", source_chain, target_chain)
            ));
        }
        
        // Tạo ID giao dịch
        let tx_id = Uuid::new_v4().to_string();
        
        // Tạo giao dịch mới
        let transaction = BridgeTransaction {
            id: tx_id.clone(),
            source_chain,
            target_chain,
            source_address,
            target_address,
            amount,
            source_tx_hash: None,
            target_tx_hash: None,
            status: BridgeTransactionStatus::Created,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            error: None,
        };
        
        // Lưu giao dịch
        let mut transactions = self.transactions.write().unwrap();
        transactions.insert(tx_id.clone(), transaction);
        
        Ok(tx_id)
    }
    
    /// Tìm adapter phù hợp cho bridge
    fn find_adapter(&self, source_chain: &DmdChain, target_chain: &DmdChain) -> BridgeResult<Arc<dyn BridgeAdapter>> {
        for adapter in &self.adapters {
            if adapter.supports_route(source_chain, target_chain) {
                return Ok(adapter.clone());
            }
        }
        
        Err(BridgeError::UnsupportedRoute(
            format!("Không tìm thấy adapter hỗ trợ route từ {:?} đến {:?}", source_chain, target_chain)
        ))
    }
    
    /// Thực hiện bridge
    pub async fn execute_bridge(&self, tx_id: &str) -> BridgeResult<String> {
        // Tìm giao dịch
        let transaction = {
            let transactions = self.transactions.read().unwrap();
            transactions.get(tx_id).cloned().ok_or_else(|| {
                BridgeError::TransactionNotFound(format!("Không tìm thấy giao dịch bridge: {}", tx_id))
            })?
        };
        
        // Kiểm tra trạng thái
        if transaction.status != BridgeTransactionStatus::Created {
            return Err(BridgeError::InvalidStatus(
                format!("Giao dịch không ở trạng thái Created: {:?}", transaction.status)
            ));
        }
        
        // Cập nhật trạng thái
        self.update_transaction_status(tx_id, BridgeTransactionStatus::InProgress, None)?;
        
        // Tìm adapter phù hợp
        let adapter = self.find_adapter(&transaction.source_chain, &transaction.target_chain)?;
        
        // Thực hiện bridge
        let source_tx_hash = adapter.transfer(
            tx_id,
            &transaction.source_chain,
            &transaction.target_chain,
            &transaction.source_address,
            &transaction.target_address,
            transaction.amount,
        ).await?;
        
        // Cập nhật hash giao dịch
        self.update_transaction_hash(tx_id, source_tx_hash.clone())?;
        
        Ok(source_tx_hash)
    }
    
    /// Kiểm tra trạng thái giao dịch bridge
    pub async fn check_transaction_status(&self, tx_id: &str) -> BridgeResult<BridgeTransactionStatus> {
        // Tìm giao dịch
        let transaction = {
            let transactions = self.transactions.read().unwrap();
            transactions.get(tx_id).cloned().ok_or_else(|| {
                BridgeError::TransactionNotFound(format!("Không tìm thấy giao dịch bridge: {}", tx_id))
            })?
        };
        
        // Kiểm tra nếu giao dịch đã hoàn thành hoặc thất bại
        if transaction.status == BridgeTransactionStatus::Completed || 
           transaction.status == BridgeTransactionStatus::Failed {
            return Ok(transaction.status);
        }
        
        // Kiểm tra nếu chưa có hash giao dịch
        let source_tx_hash = match &transaction.source_tx_hash {
            Some(hash) => hash.clone(),
            None => return Ok(BridgeTransactionStatus::InProgress),
        };
        
        // Tìm adapter phù hợp
        let adapter = self.find_adapter(&transaction.source_chain, &transaction.target_chain)?;
        
        // Kiểm tra trạng thái
        let status = adapter.check_transaction_status(
            tx_id,
            &transaction.source_chain,
            &transaction.target_chain,
            &source_tx_hash,
        ).await?;
        
        // Cập nhật trạng thái
        if status != BridgeTransactionStatus::InProgress {
            self.update_transaction_status(tx_id, status.clone(), None)?;
        }
        
        Ok(status)
    }
    
    /// Hoàn thành giao dịch
    pub fn complete_transaction(&self, tx_id: &str, target_tx_hash: String) -> BridgeResult<()> {
        // Tìm giao dịch
        let mut transactions = self.transactions.write().unwrap();
        let transaction = transactions.get_mut(tx_id).ok_or_else(|| {
            BridgeError::TransactionNotFound(format!("Không tìm thấy giao dịch bridge: {}", tx_id))
        })?;
        
        // Cập nhật thông tin
        transaction.status = BridgeTransactionStatus::Completed;
        transaction.target_tx_hash = Some(target_tx_hash);
        transaction.updated_at = Utc::now();
        
        Ok(())
    }
    
    /// Đánh dấu giao dịch thất bại
    pub fn fail_transaction(&self, tx_id: &str, error_message: &str) -> BridgeResult<()> {
        // Cập nhật trạng thái
        self.update_transaction_status(
            tx_id,
            BridgeTransactionStatus::Failed,
            Some(error_message.to_string()),
        )
    }
    
    /// Cập nhật trạng thái giao dịch
    fn update_transaction_status(
        &self,
        tx_id: &str,
        status: BridgeTransactionStatus,
        error: Option<String>,
    ) -> BridgeResult<()> {
        // Tìm giao dịch
        let mut transactions = self.transactions.write().unwrap();
        let transaction = transactions.get_mut(tx_id).ok_or_else(|| {
            BridgeError::TransactionNotFound(format!("Không tìm thấy giao dịch bridge: {}", tx_id))
        })?;
        
        // Cập nhật thông tin
        transaction.status = status;
        transaction.error = error;
        transaction.updated_at = Utc::now();
        
        Ok(())
    }
    
    /// Cập nhật hash giao dịch
    fn update_transaction_hash(&self, tx_id: &str, source_tx_hash: String) -> BridgeResult<()> {
        // Tìm giao dịch
        let mut transactions = self.transactions.write().unwrap();
        let transaction = transactions.get_mut(tx_id).ok_or_else(|| {
            BridgeError::TransactionNotFound(format!("Không tìm thấy giao dịch bridge: {}", tx_id))
        })?;
        
        // Cập nhật thông tin
        transaction.source_tx_hash = Some(source_tx_hash);
        transaction.updated_at = Utc::now();
        
        Ok(())
    }
    
    /// Lấy thông tin giao dịch
    pub fn get_transaction(&self, tx_id: &str) -> BridgeResult<BridgeTransaction> {
        // Tìm giao dịch
        let transactions = self.transactions.read().unwrap();
        transactions.get(tx_id).cloned().ok_or_else(|| {
            BridgeError::TransactionNotFound(format!("Không tìm thấy giao dịch bridge: {}", tx_id))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::predicate::*;
    use mockall::*;
    
    mock! {
        BlockchainProviderMock {}
        
        impl Clone for BlockchainProviderMock {
            fn clone(&self) -> Self;
        }
        
        impl BlockchainProvider for BlockchainProviderMock {}
    }
    
    #[test]
    fn test_is_evm_chain() {
        assert!(is_evm_chain(&DmdChain::Ethereum));
        assert!(is_evm_chain(&DmdChain::BinanceSmartChain));
        assert!(!is_evm_chain(&DmdChain::Near));
        assert!(!is_evm_chain(&DmdChain::Solana));
    }
    
    #[tokio::test]
    async fn test_bridge_manager_create_transaction() {
        let mock_provider = Arc::new(MockBlockchainProviderMock::new());
        let bridge_manager = BridgeManager::new(mock_provider);
        
        let result = bridge_manager.create_transaction(
            DmdChain::Ethereum,
            DmdChain::Near,
            "0x1234...".to_string(),
            "near.testnet".to_string(),
            1000,
        );
        
        assert!(result.is_ok());
        let tx_id = result.unwrap();
        
        // Kiểm tra giao dịch đã được tạo
        let tx = bridge_manager.get_transaction(&tx_id).unwrap();
        assert_eq!(tx.source_chain, DmdChain::Ethereum);
        assert_eq!(tx.target_chain, DmdChain::Near);
        assert_eq!(tx.amount, 1000);
        assert_eq!(tx.status, BridgeTransactionStatus::Created);
    }
    
    #[tokio::test]
    async fn test_bridge_manager_unsupported_route() {
        let mock_provider = Arc::new(MockBlockchainProviderMock::new());
        let bridge_manager = BridgeManager::new(mock_provider);
        
        let result = bridge_manager.create_transaction(
            DmdChain::Solana,
            DmdChain::Ethereum,
            "solana_address".to_string(),
            "0x1234...".to_string(),
            1000,
        );
        
        assert!(result.is_err());
        match result {
            Err(BridgeError::UnsupportedRoute(_)) => {}
            _ => panic!("Expected UnsupportedRoute error"),
        }
    }
} 