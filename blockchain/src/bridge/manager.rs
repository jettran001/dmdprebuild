//! Module quản lý quá trình bridge giữa các blockchain

use crate::bridge::error::BridgeError;
use crate::bridge::transaction::{BridgeTransaction, BridgeTransactionRepository, BridgeTransactionStatus};
use crate::smartcontracts::dmd_token::DmdChain;
use crate::defi::blockchain::{BlockchainProvider, BlockchainProviderFactory};
use async_trait::async_trait;
use log::{debug, error, info, warn};
use std::sync::Arc;
use uuid::Uuid;

/// Kết quả của hoạt động bridge
pub type BridgeResult<T> = Result<T, BridgeError>;

/// Thông tin cấu hình bridge
pub struct BridgeConfig {
    /// Chain sử dụng làm hub
    pub hub_chain: DmdChain,
    /// Địa chỉ hợp đồng bridge trên hub
    pub hub_contract_address: String,
    /// Địa chỉ ví của bridge operator
    pub operator_address: String,
    /// Địa chỉ khóa riêng tư của operator (nếu có)
    pub operator_private_key: Option<String>,
    /// Thời gian tối đa chờ xác nhận (tính bằng giây)
    pub confirmation_timeout: u64,
    /// Số block cần để xác nhận trên mỗi chain
    pub confirmation_blocks: std::collections::HashMap<DmdChain, u64>,
    /// Phí bridge theo phần trăm
    pub fee_percentage: f64,
    /// Phí bridge tối thiểu
    pub min_fee: std::collections::HashMap<DmdChain, String>,
    /// Địa chỉ của hợp đồng ERC20 trên các chain EVM
    pub erc20_addresses: std::collections::HashMap<DmdChain, String>,
    /// Địa chỉ của hợp đồng ERC1155 trên các chain EVM
    pub erc1155_addresses: std::collections::HashMap<DmdChain, String>,
}

/// Trình quản lý bridge
pub struct BridgeManager {
    /// Cấu hình bridge
    config: BridgeConfig,
    /// Nhà máy cung cấp blockchain
    provider_factory: Arc<BlockchainProviderFactory>,
    /// Kho lưu trữ giao dịch
    transaction_repository: Arc<dyn BridgeTransactionRepository + Send + Sync>,
}

impl BridgeManager {
    /// Tạo bridge manager mới
    pub fn new(
        config: BridgeConfig,
        provider_factory: Arc<BlockchainProviderFactory>,
        transaction_repository: Arc<dyn BridgeTransactionRepository + Send + Sync>,
    ) -> Self {
        Self {
            config,
            provider_factory,
            transaction_repository,
        }
    }

    /// Tạo ID giao dịch mới
    fn generate_transaction_id(&self) -> String {
        Uuid::new_v4().to_string()
    }

    /// Tính phí bridge
    fn calculate_fee(&self, amount: &str, source_chain: &DmdChain, target_chain: &DmdChain) -> BridgeResult<String> {
        // Chuyển đổi amount từ string sang f64
        let amount_value = amount.parse::<f64>().map_err(|_| {
            BridgeError::InvalidAmount(format!("Không thể chuyển đổi số lượng '{}' thành số", amount))
        })?;

        // Tính phí dựa trên phần trăm
        let fee = amount_value * self.config.fee_percentage / 100.0;

        // Lấy phí tối thiểu cho chain đích
        let min_fee = self.config.min_fee.get(target_chain).ok_or_else(|| {
            BridgeError::ConfigError(format!("Không tìm thấy cấu hình phí tối thiểu cho chain {:?}", target_chain))
        })?;

        let min_fee_value = min_fee.parse::<f64>().map_err(|_| {
            BridgeError::ConfigError(format!("Cấu hình phí tối thiểu không hợp lệ: {}", min_fee))
        })?;

        // Lấy max của phí tính được và phí tối thiểu
        let final_fee = if fee < min_fee_value { min_fee_value } else { fee };

        // Chuyển đổi lại thành string, làm tròn xuống 6 chữ số thập phân
        Ok(format!("{:.6}", final_fee))
    }

    /// Khởi tạo giao dịch bridge từ chain nguồn đến chain đích
    pub async fn initiate_bridge(
        &self,
        source_address: String,
        target_address: String,
        source_chain: DmdChain,
        target_chain: DmdChain,
        amount: String,
        token_id: Option<String>,
    ) -> BridgeResult<BridgeTransaction> {
        // Kiểm tra chain nguồn và đích có được hỗ trợ không
        if !self.is_chain_supported(&source_chain) {
            return Err(BridgeError::UnsupportedChain(format!("Chain nguồn không được hỗ trợ: {:?}", source_chain)));
        }

        if !self.is_chain_supported(&target_chain) {
            return Err(BridgeError::UnsupportedChain(format!("Chain đích không được hỗ trợ: {:?}", target_chain)));
        }

        // Tính phí bridge
        let fee = self.calculate_fee(&amount, &source_chain, &target_chain)?;

        // Tạo ID giao dịch mới
        let transaction_id = self.generate_transaction_id();

        // Tạo đối tượng giao dịch
        let transaction = BridgeTransaction::new(
            transaction_id,
            source_address.clone(),
            target_address.clone(),
            source_chain.clone(),
            target_chain.clone(),
            amount.clone(),
            token_id.clone(),
            fee.clone(),
        );

        // Lưu giao dịch
        self.transaction_repository.save(&transaction).map_err(|err| {
            error!("Không thể lưu giao dịch bridge: {}", err);
            BridgeError::SystemError(format!("Không thể lưu giao dịch: {}", err))
        })?;

        // Ghi log
        info!(
            "Đã khởi tạo giao dịch bridge từ {:?} đến {:?}, ID: {}, Số lượng: {}, Phí: {}",
            source_chain, target_chain, transaction.id, amount, fee
        );

        Ok(transaction)
    }

    /// Kiểm tra chain có được hỗ trợ không
    fn is_chain_supported(&self, chain: &DmdChain) -> bool {
        match chain {
            DmdChain::Ethereum | DmdChain::BinanceSmartChain | DmdChain::Avalanche |
            DmdChain::Polygon | DmdChain::Arbitrum | DmdChain::Optimism | DmdChain::Base |
            DmdChain::Near => true,
            _ => false
        }
    }

    /// Lấy thông tin giao dịch bridge
    pub async fn get_transaction(&self, id: &str) -> BridgeResult<BridgeTransaction> {
        self.transaction_repository.find_by_id(id).map_err(|err| {
            error!("Không thể tìm giao dịch bridge: {}", err);
            BridgeError::SystemError(format!("Không thể tìm giao dịch: {}", err))
        })?.ok_or_else(|| {
            BridgeError::BridgeTransactionNotFound(format!("Không tìm thấy giao dịch bridge với ID: {}", id))
        })
    }

    /// Lấy danh sách giao dịch theo địa chỉ
    pub async fn get_transactions_by_address(&self, address: &str) -> BridgeResult<Vec<BridgeTransaction>> {
        // Tìm cả trong source address và target address
        let source_txs = self.transaction_repository.find_by_source_address(address).map_err(|err| {
            error!("Không thể tìm giao dịch theo địa chỉ nguồn: {}", err);
            BridgeError::SystemError(format!("Không thể tìm giao dịch: {}", err))
        })?;

        let target_txs = self.transaction_repository.find_by_target_address(address).map_err(|err| {
            error!("Không thể tìm giao dịch theo địa chỉ đích: {}", err);
            BridgeError::SystemError(format!("Không thể tìm giao dịch: {}", err))
        })?;

        // Kết hợp kết quả từ cả hai truy vấn
        let mut result = Vec::new();
        result.extend(source_txs);
        result.extend(target_txs.into_iter().filter(|tx| !result.iter().any(|t| t.id == tx.id)));

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    // Mock repository cho việc test
    struct MockBridgeTransactionRepository {
        transactions: Mutex<Vec<BridgeTransaction>>,
    }

    impl MockBridgeTransactionRepository {
        fn new() -> Self {
            Self {
                transactions: Mutex::new(Vec::new()),
            }
        }
    }

    impl BridgeTransactionRepository for MockBridgeTransactionRepository {
        fn save(&self, transaction: &BridgeTransaction) -> Result<(), String> {
            let mut transactions = self.transactions.lock().unwrap();
            
            // Kiểm tra xem giao dịch đã tồn tại chưa
            if let Some(idx) = transactions.iter().position(|t| t.id == transaction.id) {
                transactions[idx] = transaction.clone();
            } else {
                transactions.push(transaction.clone());
            }
            
            Ok(())
        }
        
        fn find_by_id(&self, id: &str) -> Result<Option<BridgeTransaction>, String> {
            let transactions = self.transactions.lock().unwrap();
            Ok(transactions.iter().find(|t| t.id == id).cloned())
        }
        
        fn find_by_source_tx_id(&self, tx_id: &str) -> Result<Option<BridgeTransaction>, String> {
            let transactions = self.transactions.lock().unwrap();
            Ok(transactions.iter().find(|t| t.source_tx_id.as_ref().map_or(false, |id| id == tx_id)).cloned())
        }
        
        fn find_by_hub_tx_id(&self, tx_id: &str) -> Result<Option<BridgeTransaction>, String> {
            let transactions = self.transactions.lock().unwrap();
            Ok(transactions.iter().find(|t| t.hub_tx_id.as_ref().map_or(false, |id| id == tx_id)).cloned())
        }
        
        fn find_by_target_tx_id(&self, tx_id: &str) -> Result<Option<BridgeTransaction>, String> {
            let transactions = self.transactions.lock().unwrap();
            Ok(transactions.iter().find(|t| t.target_tx_id.as_ref().map_or(false, |id| id == tx_id)).cloned())
        }
        
        fn find_by_source_address(&self, address: &str) -> Result<Vec<BridgeTransaction>, String> {
            let transactions = self.transactions.lock().unwrap();
            Ok(transactions.iter().filter(|t| t.source_address == address).cloned().collect())
        }
        
        fn find_by_target_address(&self, address: &str) -> Result<Vec<BridgeTransaction>, String> {
            let transactions = self.transactions.lock().unwrap();
            Ok(transactions.iter().filter(|t| t.target_address == address).cloned().collect())
        }
        
        fn find_by_status(&self, status: &BridgeTransactionStatus) -> Result<Vec<BridgeTransaction>, String> {
            let transactions = self.transactions.lock().unwrap();
            Ok(transactions.iter().filter(|t| &t.status == status).cloned().collect())
        }
    }

    // Mock BlockchainProviderFactory
    struct MockBlockchainProviderFactory;

    impl BlockchainProviderFactory {
        fn new_mock() -> Self {
            Self::default()
        }
    }

    #[test]
    fn test_calculate_fee() {
        // Tạo cấu hình bridge
        let mut min_fee = HashMap::new();
        min_fee.insert(DmdChain::Ethereum, "0.01".to_string());
        min_fee.insert(DmdChain::BinanceSmartChain, "0.005".to_string());
        
        let mut confirmation_blocks = HashMap::new();
        confirmation_blocks.insert(DmdChain::Ethereum, 12);
        confirmation_blocks.insert(DmdChain::BinanceSmartChain, 15);
        
        let config = BridgeConfig {
            hub_chain: DmdChain::Near,
            hub_contract_address: "hub.near".to_string(),
            operator_address: "operator.near".to_string(),
            operator_private_key: None,
            confirmation_timeout: 3600,
            confirmation_blocks,
            fee_percentage: 0.5,
            min_fee,
            erc20_addresses: HashMap::new(),
            erc1155_addresses: HashMap::new(),
        };
        
        let provider_factory = Arc::new(BlockchainProviderFactory::new_mock());
        let repository = Arc::new(MockBridgeTransactionRepository::new());
        
        let bridge_manager = BridgeManager::new(config, provider_factory, repository);
        
        // Test 1: Số lượng nhỏ, phí dưới mức tối thiểu
        let fee = bridge_manager.calculate_fee("1.0", &DmdChain::BinanceSmartChain, &DmdChain::Ethereum).unwrap();
        assert_eq!(fee, "0.010000"); // Phí tối thiểu cho Ethereum
        
        // Test 2: Số lượng lớn, phí vượt mức tối thiểu
        let fee = bridge_manager.calculate_fee("10.0", &DmdChain::Ethereum, &DmdChain::BinanceSmartChain).unwrap();
        assert_eq!(fee, "0.050000"); // 0.5% của 10.0 = 0.05, lớn hơn mức tối thiểu 0.005
    }
} 