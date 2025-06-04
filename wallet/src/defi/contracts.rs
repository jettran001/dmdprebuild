//! Module contracts - Quản lý tất cả các smart contract cho Wallet và DeFi
//!
//! Module này cung cấp các định nghĩa và triển khai các smart contract cần thiết cho ví và DeFi.
//! Nó tương tác chặt chẽ với module DeFi và Blockchain, cung cấp giao diện để tương tác với 
//! các smart contract trên các blockchain khác nhau.
//!
//! ## Các loại contract hỗ trợ:
//! - ERC20: Tokens chuẩn
//! - ERC721/ERC1155: NFTs
//! - DEX Router: Uniswap, PancakeSwap, ...
//! - Staking và Farming contracts
//! - Multisig Wallet contracts
//! - Proxy contracts (EIP-1967)
//!
//! ## Flow:
//! ```
//! Wallet/DeFi -> Contract Manager -> Smart Contract -> Blockchain
//! ```

use std::sync::Arc;
use std::collections::HashMap;
use async_trait::async_trait;
use ethers::{
    providers::{Middleware, Provider, Http},
    contract::{Contract, ContractError as EthersContractError},
    types::{Address, U256, H160, TransactionReceipt, Log, H256},
    signers::{Signer, LocalWallet},
    abi::{Token, Tokenize},
};
use thiserror::Error;
use serde::{Serialize, Deserialize};
use tokio::sync::RwLock;
use once_cell::sync::Lazy;
use tracing::{info, warn, error, debug};

use crate::defi::error::DefiError;
use crate::defi::blockchain::get_provider;
use crate::defi::constants::{
    ROUTER_CONTRACT_ADDRESS, FARM_CONTRACT_ADDRESS, 
    DMD_TOKEN_ADDRESS, CONTRACT_CALL_TIMEOUT_MS
};
use crate::walletmanager::chain::{ChainType, ChainConfig};

pub mod erc20;
pub mod dex;
pub mod staking;
pub mod farm;

/// Lỗi liên quan đến contract
#[derive(Error, Debug)]
pub enum ContractError {
    #[error("Lỗi kết nối: {0}")]
    ConnectionError(String),
    
    #[error("Lỗi gọi contract: {0}")]
    CallError(String),
    
    #[error("Contract không được hỗ trợ: {0}")]
    UnsupportedContract(String),
    
    #[error("Blockchain không được hỗ trợ: {0}")]
    UnsupportedChain(String),
    
    #[error("Lỗi chữ ký giao dịch: {0}")]
    SignatureError(String),
    
    #[error("Contract không tồn tại: {0}")]
    NonExistentContract(String),
    
    #[error("Không đủ quyền: {0}")]
    InsufficientPermission(String),
    
    #[error("Lỗi ABI: {0}")]
    AbiError(String),
    
    #[error("Lỗi khác: {0}")]
    OtherError(String),
}

/// Metadata của contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractMetadata {
    /// Tên contract
    pub name: String,
    /// Địa chỉ contract
    pub address: Address,
    /// Chain ID
    pub chain_id: u64,
    /// Loại blockchain
    pub chain_type: ChainType,
    /// Đường dẫn ABI
    pub abi_path: Option<String>,
    /// ABI JSON
    pub abi_json: Option<String>,
    /// Contract đã được verified chưa
    pub verified: bool,
    /// Loại contract
    pub contract_type: ContractType,
    /// Địa chỉ implementation (nếu là proxy)
    pub implementation: Option<Address>,
    /// Đã audit chưa
    pub audited: bool,
    /// Công ty audit
    pub audit_firm: Option<String>,
    /// Tags
    pub tags: Vec<String>,
}

impl ContractMetadata {
    /// Tạo metadata mới với các giá trị tối thiểu
    pub fn new(name: String, address: Address, chain_id: u64, contract_type: ContractType) -> Self {
        Self {
            name,
            address,
            chain_id,
            chain_type: ChainType::EVM,
            abi_path: None,
            abi_json: None,
            verified: false,
            contract_type,
            implementation: None,
            audited: false,
            audit_firm: None,
            tags: Vec::new(),
        }
    }

    /// Thêm ABI JSON
    pub fn with_abi_json(mut self, abi_json: String) -> Self {
        self.abi_json = Some(abi_json);
        self
    }

    /// Thêm ABI path
    pub fn with_abi_path(mut self, abi_path: String) -> Self {
        self.abi_path = Some(abi_path);
        self
    }

    /// Đặt loại blockchain
    pub fn with_chain_type(mut self, chain_type: ChainType) -> Self {
        self.chain_type = chain_type;
        self
    }

    /// Đặt trạng thái verified
    pub fn with_verified(mut self, verified: bool) -> Self {
        self.verified = verified;
        self
    }
}

/// Enum định nghĩa các loại contract
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ContractType {
    /// Token ERC20
    ERC20,
    /// NFT ERC721
    ERC721,
    /// Multi-token ERC1155
    ERC1155,
    /// Router DEX
    DexRouter,
    /// Farm contract
    Farm,
    /// Staking contract
    Staking,
    /// Vault contract
    Vault,
    /// Multisig wallet
    Multisig,
    /// Proxy contract
    Proxy,
    /// Custom contract
    Custom(String),
}

/// Interface cho contract
#[async_trait]
pub trait ContractInterface: Send + Sync + 'static {
    /// Trả về metadata của contract
    fn metadata(&self) -> &ContractMetadata;
    
    /// Trả về địa chỉ của contract
    fn address(&self) -> Address {
        self.metadata().address
    }
    
    /// Trả về chain ID
    fn chain_id(&self) -> u64 {
        self.metadata().chain_id
    }
    
    /// Trả về tên contract
    fn name(&self) -> &str {
        &self.metadata().name
    }
    
    /// Trả về loại contract
    fn contract_type(&self) -> &ContractType {
        &self.metadata().contract_type
    }
    
    /// Gọi hàm read-only
    async fn call<T: Send + Sync + 'static>(&self, function: &str, params: Vec<Token>) 
        -> Result<T, ContractError>;
    
    /// Gửi transaction
    async fn send_transaction(&self, function: &str, params: Vec<Token>, value: Option<U256>) 
        -> Result<TransactionReceipt, ContractError>;
    
    /// Encode function data
    fn encode_function_data(&self, function: &str, params: Vec<Token>)
        -> Result<Vec<u8>, ContractError>;
    
    /// Decode logs
    fn decode_logs<T: Send + Sync + 'static>(&self, logs: &[Log])
        -> Result<Vec<T>, ContractError>;
        
    /// Kiểm tra quyền của địa chỉ
    async fn check_permission(&self, address: Address, permission: &str) -> Result<bool, ContractError>;
}

/// Registry quản lý các contract
pub struct ContractRegistry {
    /// Contracts theo địa chỉ
    contracts: RwLock<HashMap<Address, Arc<dyn ContractInterface>>>,
    /// Contracts theo tên
    contracts_by_name: RwLock<HashMap<String, Vec<Arc<dyn ContractInterface>>>>,
    /// Contracts theo chain ID
    contracts_by_chain: RwLock<HashMap<u64, Vec<Arc<dyn ContractInterface>>>>,
    /// Contracts theo loại
    contracts_by_type: RwLock<HashMap<ContractType, Vec<Arc<dyn ContractInterface>>>>,
    /// Lock cho các thao tác đồng bộ
    sync_lock: tokio::sync::Mutex<()>,
}

impl ContractRegistry {
    /// Tạo registry mới
    pub fn new() -> Self {
        Self {
            contracts: RwLock::new(HashMap::new()),
            contracts_by_name: RwLock::new(HashMap::new()),
            contracts_by_chain: RwLock::new(HashMap::new()),
            contracts_by_type: RwLock::new(HashMap::new()),
            sync_lock: tokio::sync::Mutex::new(()),
        }
    }
    
    /// Đăng ký contract
    pub async fn register_contract(&self, contract: Arc<dyn ContractInterface>) -> Result<(), ContractError> {
        let _guard = self.sync_lock.lock().await;
        
        let metadata = contract.metadata();
        let address = metadata.address;
        let name = metadata.name.clone();
        let chain_id = metadata.chain_id;
        let contract_type = metadata.contract_type.clone();
        
        // Đăng ký theo địa chỉ
        let mut contracts = self.contracts.write().await;
        if contracts.contains_key(&address) {
            return Err(ContractError::OtherError(format!("Contract already registered at address {}", address)));
        }
        contracts.insert(address, contract.clone());
        
        // Đăng ký theo tên
        let mut contracts_by_name = self.contracts_by_name.write().await;
        contracts_by_name.entry(name.clone())
            .or_insert_with(Vec::new)
            .push(contract.clone());
            
        // Đăng ký theo chain ID
        let mut contracts_by_chain = self.contracts_by_chain.write().await;
        contracts_by_chain.entry(chain_id)
            .or_insert_with(Vec::new)
            .push(contract.clone());
            
        // Đăng ký theo loại
        let mut contracts_by_type = self.contracts_by_type.write().await;
        contracts_by_type.entry(contract_type)
            .or_insert_with(Vec::new)
            .push(contract);
            
        Ok(())
    }
    
    /// Lấy contract theo địa chỉ
    pub async fn get_contract(&self, address: Address) -> Result<Arc<dyn ContractInterface>, ContractError> {
        let contracts = self.contracts.read().await;
        contracts.get(&address).cloned().ok_or_else(|| {
            ContractError::NonExistentContract(format!("Contract {} không tồn tại", address))
        })
    }
    
    /// Get contracts by name
    pub async fn get_contracts_by_name(&self, name: &str) -> Result<Vec<Arc<dyn ContractInterface>>, ContractError> {
        let contracts_by_name = self.contracts_by_name.read().await;
        match contracts_by_name.get(name).cloned() {
            Some(contracts) if !contracts.is_empty() => Ok(contracts),
            _ => Err(ContractError::NonExistentContract(format!("Contract with name '{}' does not exist", name))),
        }
    }
    
    /// Get contracts by chain ID
    pub async fn get_contracts_by_chain(&self, chain_id: u64) -> Result<Vec<Arc<dyn ContractInterface>>, ContractError> {
        let contracts_by_chain = self.contracts_by_chain.read().await;
        match contracts_by_chain.get(&chain_id).cloned() {
            Some(contracts) if !contracts.is_empty() => Ok(contracts),
            _ => Err(ContractError::NonExistentContract(format!("Contract with chain_id '{}' does not exist", chain_id))),
        }
    }
    
    /// Get contracts by type
    pub async fn get_contracts_by_type(&self, contract_type: &ContractType) -> Result<Vec<Arc<dyn ContractInterface>>, ContractError> {
        let contracts_by_type = self.contracts_by_type.read().await;
        match contracts_by_type.get(contract_type).cloned() {
            Some(contracts) if !contracts.is_empty() => Ok(contracts),
            _ => Err(ContractError::NonExistentContract(format!("Contract with type '{:?}' does not exist", contract_type))),
        }
    }
    
    /// Tìm kiếm contracts theo các tiêu chí
    pub async fn search_contracts(&self, query: &ContractSearchQuery) -> Vec<Arc<dyn ContractInterface>> {
        let contracts = self.contracts.read().await;
        let mut results = Vec::new();
        
        for contract in contracts.values() {
            let metadata = contract.metadata();
            
            // Kiểm tra name
            if let Some(name) = &query.name {
                if !metadata.name.contains(name) {
                    continue;
                }
            }
            
            // Kiểm tra contract_type
            if let Some(contract_type) = &query.contract_type {
                if &metadata.contract_type != contract_type {
                    continue;
                }
            }
            
            // Kiểm tra chain_id
            if let Some(chain_id) = query.chain_id {
                if metadata.chain_id != chain_id {
                    continue;
                }
            }
            
            // Kiểm tra address
            if let Some(address) = query.address {
                if metadata.address != address {
                    continue;
                }
            }
            
            // Kiểm tra verified
            if query.verified_only && !metadata.verified {
                continue;
            }
            
            // Kiểm tra audited
            if query.audited_only && !metadata.audited {
                continue;
            }
            
            results.push(contract.clone());
        }
        
        results
    }
    
    /// Đếm số lượng contracts
    pub async fn count_contracts(&self) -> usize {
        let contracts = self.contracts.read().await;
        contracts.len()
    }
}

/// Tham số tìm kiếm contract
#[derive(Debug, Clone)]
pub struct ContractSearchQuery {
    /// Tên contract (substring)
    pub name: Option<String>,
    /// Loại contract
    pub contract_type: Option<ContractType>,
    /// Chain ID
    pub chain_id: Option<u64>,
    /// Địa chỉ contract (chính xác)
    pub address: Option<Address>,
    /// Chỉ lấy contracts đã verified
    pub verified_only: bool,
    /// Chỉ lấy contracts đã audit
    pub audited_only: bool,
    /// Giới hạn số lượng kết quả
    pub limit: Option<usize>,
    /// Sắp xếp theo
    pub sort_by: Option<SortField>,
    /// Thứ tự sắp xếp
    pub sort_order: Option<SortOrder>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Name,
    ChainId,
    Type,
    Verified,
    Audited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

impl Default for ContractSearchQuery {
    fn default() -> Self {
        Self {
            name: None,
            contract_type: None,
            chain_id: None,
            address: None,
            verified_only: false,
            audited_only: false,
            limit: None,
            sort_by: None,
            sort_order: None,
        }
    }
}

impl ContractSearchQuery {
    /// Tạo query mới
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Thêm điều kiện name
    pub fn with_name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self
    }
    
    /// Thêm điều kiện contract_type
    pub fn with_type(mut self, contract_type: ContractType) -> Self {
        self.contract_type = Some(contract_type);
        self
    }
    
    /// Thêm điều kiện chain_id
    pub fn with_chain_id(mut self, chain_id: u64) -> Self {
        self.chain_id = Some(chain_id);
        self
    }
    
    /// Thêm điều kiện address
    pub fn with_address(mut self, address: Address) -> Self {
        self.address = Some(address);
        self
    }
    
    /// Chỉ lấy contracts đã verified
    pub fn verified_only(mut self) -> Self {
        self.verified_only = true;
        self
    }
    
    /// Chỉ lấy contracts đã audit
    pub fn audited_only(mut self) -> Self {
        self.audited_only = true;
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn sort_by(mut self, field: SortField, order: SortOrder) -> Self {
        self.sort_by = Some(field);
        self.sort_order = Some(order);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.name.is_none() &&
        self.contract_type.is_none() &&
        self.chain_id.is_none() &&
        self.address.is_none() &&
        !self.verified_only &&
        !self.audited_only
    }
}

/// Contract Manager cũ - giữ lại để tương thích với mã hiện tại
pub struct ContractManager {
    /// Contracts theo loại
    contracts: RwLock<HashMap<ContractType, Arc<dyn ContractInterface>>>,
    /// Providers theo chain ID
    providers: RwLock<HashMap<u64, Arc<Provider<Http>>>>,
    /// Chain IDs
    chain_ids: Vec<u64>,
    /// Wallet
    wallet: Option<LocalWallet>,
    /// Registry chung
    registry: Arc<ContractRegistry>,
}

impl ContractManager {
    /// Tạo manager mới
    pub fn new(chain_ids: Vec<u64>, wallet: Option<LocalWallet>) -> Self {
        Self {
            contracts: RwLock::new(HashMap::new()),
            providers: RwLock::new(HashMap::new()),
            chain_ids,
            wallet,
            registry: Arc::new(ContractRegistry::new()),
        }
    }
    
    /// Thêm provider mới cho chain
    pub async fn add_provider(&self, chain_id: u64, rpc_url: &str) -> Result<(), ContractError> {
        let provider = Provider::<Http>::try_from(rpc_url)
            .map_err(|e| ContractError::ConnectionError(format!("Failed to create provider: {}", e)))?;
            
        let mut providers = self.providers.write().await;
        providers.insert(chain_id, Arc::new(provider));
        
        Ok(())
    }
    
    /// Lấy provider cho chain
    pub async fn get_provider(&self, chain_id: u64) -> Result<Arc<Provider<Http>>, ContractError> {
        let providers = self.providers.read().await;
        providers.get(&chain_id)
            .cloned()
            .ok_or_else(|| ContractError::ConnectionError(format!("No provider found for chain {}", chain_id)))
    }
    
    /// Đăng ký contract mới
    pub async fn register_contract(
        &self,
        contract_type: ContractType,
        metadata: ContractMetadata,
    ) -> Result<(), ContractError> {
        // Kiểm tra địa chỉ contract
        if metadata.address == Address::zero() {
            return Err(ContractError::OtherError("Invalid contract address: zero address".into()));
        }
        
        // Kiểm tra chain ID
        if !self.chain_ids.contains(&metadata.chain_id) {
            return Err(ContractError::UnsupportedChain(
                format!("Chain {} not supported", metadata.chain_id)
            ));
        }
        
        // Tạo contract
        let contract = self.registry
            .create_contract(metadata)
            .await?;
            
        // Đăng ký contract
        self.registry.register_contract(contract).await?;
        
        Ok(())
    }
    
    /// Lấy contract theo loại
    pub async fn get_contract(&self, contract_type: ContractType) -> Result<Arc<dyn ContractInterface>, ContractError> {
        let contracts = self.contracts.read().await;
        contracts.get(&contract_type)
            .cloned()
            .ok_or_else(|| ContractError::NonExistentContract(
                format!("No contract found for type {:?}", contract_type)
            ))
    }
    
    /// Thiết lập wallet mới
    pub fn set_wallet(&mut self, wallet: LocalWallet) {
        self.wallet = Some(wallet);
    }
    
    /// Lấy registry chung
    pub fn registry(&self) -> Arc<ContractRegistry> {
        self.registry.clone()
    }
}

/// Factory để tạo các loại contract
pub struct ContractFactory {
    /// Registry để đăng ký contracts
    registry: Arc<ContractRegistry>,
}

impl ContractFactory {
    /// Tạo factory mới
    pub fn new(registry: Arc<ContractRegistry>) -> Self {
        Self { registry }
    }
    
    /// Tạo contract dựa vào metadata
    pub async fn create_contract(
        &self, 
        metadata: ContractMetadata
    ) -> Result<Arc<dyn ContractInterface>, ContractError> {
        match metadata.contract_type {
            ContractType::ERC20 => self.create_erc20_contract(metadata).await,
            ContractType::ERC721 => Err(ContractError::UnsupportedContract("ERC721 contract chưa được hỗ trợ".to_string())),
            ContractType::ERC1155 => Err(ContractError::UnsupportedContract("ERC1155 contract chưa được hỗ trợ".to_string())),
            ContractType::DexRouter => self.create_dex_router_contract(metadata).await,
            ContractType::Farm => Err(ContractError::UnsupportedContract("Farm contract chưa được hỗ trợ".to_string())),
            ContractType::Staking => self.create_staking_contract(metadata).await,
            ContractType::Vault => Err(ContractError::UnsupportedContract("Vault contract chưa được hỗ trợ".to_string())),
            ContractType::Multisig => Err(ContractError::UnsupportedContract("Multisig contract chưa được hỗ trợ".to_string())),
            ContractType::Proxy => Err(ContractError::UnsupportedContract("Proxy contract chưa được hỗ trợ".to_string())),
            ContractType::Custom(_) => Err(ContractError::UnsupportedContract("Custom contract chưa được hỗ trợ".to_string())),
        }
    }
    
    /// Tạo ERC20 contract
    pub async fn create_erc20_contract(&self, metadata: ContractMetadata) -> Result<Arc<dyn ContractInterface>, ContractError> {
        erc20::Erc20Contract::new(metadata).await
            .map(|c| Arc::new(c) as Arc<dyn ContractInterface>)
    }
    
    /// Tạo DexRouter contract
    pub async fn create_dex_router_contract(&self, metadata: ContractMetadata) -> Result<Arc<dyn ContractInterface>, ContractError> {
        // TODO: Triển khai DexRouter contract
        Err(ContractError::UnsupportedContract("DexRouter contract chưa được triển khai đầy đủ".to_string()))
    }
    
    /// Tạo Staking contract
    pub async fn create_staking_contract(&self, metadata: ContractMetadata) -> Result<Arc<dyn ContractInterface>, ContractError> {
        // TODO: Triển khai Staking contract
        Err(ContractError::UnsupportedContract("Staking contract chưa được triển khai đầy đủ".to_string()))
    }
}

/// Singleton của contract registry
static CONTRACT_REGISTRY: Lazy<Arc<ContractRegistry>> = Lazy::new(|| {
    Arc::new(ContractRegistry::new())
});

/// Lấy instance của contract registry
pub fn get_contract_registry() -> Arc<ContractRegistry> {
    CONTRACT_REGISTRY.clone()
}

/// Lấy instance của contract factory
pub fn get_contract_factory() -> ContractFactory {
    ContractFactory::new(get_contract_registry())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_contract_registry_new() {
        let registry = ContractRegistry::new();
        // Registry should be empty at start
        // Chưa thể test count_contracts vì nó là async
    }
    
    #[test]
    fn test_contract_search_query() {
        let query = ContractSearchQuery::new()
            .with_name("Token")
            .with_type(ContractType::ERC20)
            .with_chain_id(1)
            .verified_only()
            .audited_only();
        
        assert_eq!(query.name, Some("Token".to_string()));
        assert_eq!(query.contract_type, Some(ContractType::ERC20));
        assert_eq!(query.chain_id, Some(1));
        assert!(query.verified_only);
        assert!(query.audited_only);
    }
    
    #[test]
    fn test_contract_type() {
        let types = vec![
            ContractType::ERC20,
            ContractType::ERC721,
            ContractType::ERC1155,
            ContractType::DexRouter,
            ContractType::Farm,
            ContractType::Staking,
            ContractType::Vault,
            ContractType::Multisig,
            ContractType::Proxy,
            ContractType::Custom("Test".to_string()),
        ];
        
        // Chỉ kiểm tra rằng enum có thể được sử dụng
        for t in types {
            match t {
                ContractType::Custom(s) => assert_eq!(s, "Test"),
                _ => {}
            }
        }
    }
} 