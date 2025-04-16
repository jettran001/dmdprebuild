//! Module contracts - Quản lý các smart contract cho Wallet
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
use tokio::sync::RwLock;
use ethers::types::{Address, Transaction, TransactionReceipt, U256, H256};
use ethers::contract::{Contract, ContractError};
use ethers::providers::{Provider, Http, Middleware};
use ethers::signers::{Signer, LocalWallet};
use serde::{Serialize, Deserialize};
use thiserror::Error;
use once_cell::sync::Lazy;
use tracing::{info, warn, error, debug};

use crate::walletmanager::chain::{ChainType, ChainConfig};
use crate::error::WalletError;

pub mod erc20;
pub mod erc721;
pub mod erc1155;
pub mod dex;
pub mod staking;
pub mod multisig;
pub mod proxy;

/// Error types liên quan đến contract
#[derive(Debug, Error)]
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

/// Loại contract
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

/// Interface cho contract
#[async_trait::async_trait]
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
    async fn call<T: Send + Sync + 'static>(&self, function: &str, params: Vec<ethers::abi::Token>) 
        -> Result<T, ContractError>;
    
    /// Gửi transaction
    async fn send_transaction(&self, function: &str, params: Vec<ethers::abi::Token>, value: Option<U256>) 
        -> Result<TransactionReceipt, ContractError>;
    
    /// Encode function data
    fn encode_function_data(&self, function: &str, params: Vec<ethers::abi::Token>)
        -> Result<Vec<u8>, ContractError>;
    
    /// Decode logs
    fn decode_logs<T: Send + Sync + 'static>(&self, logs: &[ethers::types::Log])
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
}

impl ContractRegistry {
    /// Tạo registry mới
    pub fn new() -> Self {
        Self {
            contracts: RwLock::new(HashMap::new()),
            contracts_by_name: RwLock::new(HashMap::new()),
            contracts_by_chain: RwLock::new(HashMap::new()),
            contracts_by_type: RwLock::new(HashMap::new()),
        }
    }
    
    /// Đăng ký contract
    pub async fn register_contract(&self, contract: Arc<dyn ContractInterface>) -> Result<(), ContractError> {
        let metadata = contract.metadata();
        let address = metadata.address;
        let name = metadata.name.clone();
        let chain_id = metadata.chain_id;
        let contract_type = metadata.contract_type.clone();
        
        // Thêm vào map theo địa chỉ
        let mut contracts = self.contracts.write().await;
        contracts.insert(address, contract.clone());
        
        // Thêm vào map theo tên
        let mut contracts_by_name = self.contracts_by_name.write().await;
        contracts_by_name
            .entry(name)
            .or_insert_with(Vec::new)
            .push(contract.clone());
        
        // Thêm vào map theo chain
        let mut contracts_by_chain = self.contracts_by_chain.write().await;
        contracts_by_chain
            .entry(chain_id)
            .or_insert_with(Vec::new)
            .push(contract.clone());
        
        // Thêm vào map theo loại
        let mut contracts_by_type = self.contracts_by_type.write().await;
        contracts_by_type
            .entry(contract_type)
            .or_insert_with(Vec::new)
            .push(contract.clone());
        
        Ok(())
    }
    
    /// Lấy contract theo địa chỉ
    pub async fn get_contract(&self, address: Address) -> Result<Arc<dyn ContractInterface>, ContractError> {
        let contracts = self.contracts.read().await;
        contracts
            .get(&address)
            .cloned()
            .ok_or_else(|| ContractError::NonExistentContract(format!("Contract {} không tồn tại", address)))
    }
    
    /// Lấy contract theo tên
    pub async fn get_contracts_by_name(&self, name: &str) -> Vec<Arc<dyn ContractInterface>> {
        let contracts_by_name = self.contracts_by_name.read().await;
        contracts_by_name
            .get(name)
            .cloned()
            .unwrap_or_default()
    }
    
    /// Lấy contracts theo chain ID
    pub async fn get_contracts_by_chain(&self, chain_id: u64) -> Vec<Arc<dyn ContractInterface>> {
        let contracts_by_chain = self.contracts_by_chain.read().await;
        contracts_by_chain
            .get(&chain_id)
            .cloned()
            .unwrap_or_default()
    }
    
    /// Lấy contracts theo loại
    pub async fn get_contracts_by_type(&self, contract_type: &ContractType) -> Vec<Arc<dyn ContractInterface>> {
        let contracts_by_type = self.contracts_by_type.read().await;
        contracts_by_type
            .get(contract_type)
            .cloned()
            .unwrap_or_default()
    }
    
    /// Tìm kiếm contracts
    pub async fn search_contracts(&self, query: &ContractSearchQuery) -> Vec<Arc<dyn ContractInterface>> {
        let mut result = Vec::new();
        
        // Lọc theo chain ID nếu có
        let contracts_to_search = if let Some(chain_id) = query.chain_id {
            self.get_contracts_by_chain(chain_id).await
        } else {
            // Lấy tất cả contracts
            let contracts = self.contracts.read().await;
            contracts.values().cloned().collect()
        };
        
        // Lọc theo loại contract nếu có
        let contracts_to_search = if let Some(contract_type) = &query.contract_type {
            contracts_to_search
                .into_iter()
                .filter(|c| c.contract_type() == contract_type)
                .collect()
        } else {
            contracts_to_search
        };
        
        // Lọc theo tên
        if let Some(name) = &query.name {
            for contract in contracts_to_search {
                if contract.name().contains(name) {
                    result.push(contract);
                }
            }
        } else {
            result = contracts_to_search;
        }
        
        result
    }
    
    /// Đếm số lượng contracts
    pub async fn count_contracts(&self) -> usize {
        let contracts = self.contracts.read().await;
        contracts.len()
    }
}

/// Query tìm kiếm contracts
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
        }
    }
}

impl ContractSearchQuery {
    /// Tạo query mới
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Tìm theo tên
    pub fn with_name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self
    }
    
    /// Tìm theo loại
    pub fn with_type(mut self, contract_type: ContractType) -> Self {
        self.contract_type = Some(contract_type);
        self
    }
    
    /// Tìm theo chain ID
    pub fn with_chain_id(mut self, chain_id: u64) -> Self {
        self.chain_id = Some(chain_id);
        self
    }
    
    /// Tìm theo địa chỉ
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
}

/// Factory tạo các contracts
pub struct ContractFactory {
    /// Registry để đăng ký contracts
    registry: Arc<ContractRegistry>,
}

impl ContractFactory {
    /// Tạo factory mới
    pub fn new(registry: Arc<ContractRegistry>) -> Self {
        Self { registry }
    }
    
    /// Tạo contract ERC20
    pub async fn create_erc20_contract(&self, metadata: ContractMetadata) -> Result<Arc<dyn ContractInterface>, ContractError> {
        // Kiểm tra loại contract
        if metadata.contract_type != ContractType::ERC20 {
            return Err(ContractError::UnsupportedContract(
                format!("Expected ERC20 contract, got {:?}", metadata.contract_type)
            ));
        }
        
        // Import crate cần thiết
        use crate::contracts::erc20::Erc20Contract;
        
        // Tạo contract mới
        let contract = Erc20Contract::new(metadata).await?;
        
        // Đóng gói trong Arc<dyn ContractInterface>
        Ok(Arc::new(contract) as Arc<dyn ContractInterface>)
    }
    
    /// Tạo contract ERC721
    pub async fn create_erc721_contract(&self, metadata: ContractMetadata) -> Result<Arc<dyn ContractInterface>, ContractError> {
        // Kiểm tra loại contract
        if metadata.contract_type != ContractType::ERC721 {
            return Err(ContractError::UnsupportedContract(
                format!("Expected ERC721 contract, got {:?}", metadata.contract_type)
            ));
        }
        
        // ERC721 standard ABI
        const ERC721_ABI: &str = r#"
        [
            {
                "constant": true,
                "inputs": [],
                "name": "name",
                "outputs": [{"name": "", "type": "string"}],
                "payable": false,
                "stateMutability": "view",
                "type": "function"
            },
            {
                "constant": true,
                "inputs": [{"name": "tokenId", "type": "uint256"}],
                "name": "getApproved",
                "outputs": [{"name": "", "type": "address"}],
                "payable": false,
                "stateMutability": "view",
                "type": "function"
            },
            {
                "constant": false,
                "inputs": [{"name": "to", "type": "address"}, {"name": "tokenId", "type": "uint256"}],
                "name": "approve",
                "outputs": [],
                "payable": false,
                "stateMutability": "nonpayable",
                "type": "function"
            },
            {
                "constant": true,
                "inputs": [],
                "name": "totalSupply",
                "outputs": [{"name": "", "type": "uint256"}],
                "payable": false,
                "stateMutability": "view",
                "type": "function"
            },
            {
                "constant": false,
                "inputs": [{"name": "from", "type": "address"}, {"name": "to", "type": "address"}, {"name": "tokenId", "type": "uint256"}],
                "name": "transferFrom",
                "outputs": [],
                "payable": false,
                "stateMutability": "nonpayable",
                "type": "function"
            },
            {
                "constant": true,
                "inputs": [{"name": "owner", "type": "address"}, {"name": "index", "type": "uint256"}],
                "name": "tokenOfOwnerByIndex",
                "outputs": [{"name": "", "type": "uint256"}],
                "payable": false,
                "stateMutability": "view",
                "type": "function"
            },
            {
                "constant": false,
                "inputs": [{"name": "from", "type": "address"}, {"name": "to", "type": "address"}, {"name": "tokenId", "type": "uint256"}],
                "name": "safeTransferFrom",
                "outputs": [],
                "payable": false,
                "stateMutability": "nonpayable",
                "type": "function"
            },
            {
                "constant": true,
                "inputs": [{"name": "index", "type": "uint256"}],
                "name": "tokenByIndex",
                "outputs": [{"name": "", "type": "uint256"}],
                "payable": false,
                "stateMutability": "view",
                "type": "function"
            },
            {
                "constant": true,
                "inputs": [{"name": "tokenId", "type": "uint256"}],
                "name": "ownerOf",
                "outputs": [{"name": "", "type": "address"}],
                "payable": false,
                "stateMutability": "view",
                "type": "function"
            },
            {
                "constant": true,
                "inputs": [{"name": "owner", "type": "address"}],
                "name": "balanceOf",
                "outputs": [{"name": "", "type": "uint256"}],
                "payable": false,
                "stateMutability": "view",
                "type": "function"
            },
            {
                "constant": true,
                "inputs": [],
                "name": "symbol",
                "outputs": [{"name": "", "type": "string"}],
                "payable": false,
                "stateMutability": "view",
                "type": "function"
            },
            {
                "constant": false,
                "inputs": [{"name": "operator", "type": "address"}, {"name": "approved", "type": "bool"}],
                "name": "setApprovalForAll",
                "outputs": [],
                "payable": false,
                "stateMutability": "nonpayable",
                "type": "function"
            },
            {
                "constant": false,
                "inputs": [{"name": "from", "type": "address"}, {"name": "to", "type": "address"}, {"name": "tokenId", "type": "uint256"}, {"name": "data", "type": "bytes"}],
                "name": "safeTransferFrom",
                "outputs": [],
                "payable": false,
                "stateMutability": "nonpayable",
                "type": "function"
            },
            {
                "constant": true,
                "inputs": [{"name": "tokenId", "type": "uint256"}],
                "name": "tokenURI",
                "outputs": [{"name": "", "type": "string"}],
                "payable": false,
                "stateMutability": "view",
                "type": "function"
            },
            {
                "constant": true,
                "inputs": [{"name": "owner", "type": "address"}, {"name": "operator", "type": "address"}],
                "name": "isApprovedForAll",
                "outputs": [{"name": "", "type": "bool"}],
                "payable": false,
                "stateMutability": "view",
                "type": "function"
            },
            {
                "anonymous": false,
                "inputs": [
                    {"indexed": true, "name": "from", "type": "address"},
                    {"indexed": true, "name": "to", "type": "address"},
                    {"indexed": true, "name": "tokenId", "type": "uint256"}
                ],
                "name": "Transfer",
                "type": "event"
            },
            {
                "anonymous": false,
                "inputs": [
                    {"indexed": true, "name": "owner", "type": "address"},
                    {"indexed": true, "name": "approved", "type": "address"},
                    {"indexed": true, "name": "tokenId", "type": "uint256"}
                ],
                "name": "Approval",
                "type": "event"
            },
            {
                "anonymous": false,
                "inputs": [
                    {"indexed": true, "name": "owner", "type": "address"},
                    {"indexed": true, "name": "operator", "type": "address"},
                    {"indexed": false, "name": "approved", "type": "bool"}
                ],
                "name": "ApprovalForAll",
                "type": "event"
            }
        ]
        "#;
        
        // Lấy provider cho chain ID
        let provider = crate::defi::blockchain::get_provider(metadata.chain_id.into())
            .map_err(|e| ContractError::ConnectionError(e.to_string()))?;
            
        // Tạo contract từ ABI
        let abi_json = metadata.abi_json.clone().unwrap_or_else(|| ERC721_ABI.to_string());
        let contract = ethers::contract::Contract::new(
            metadata.address,
            serde_json::from_str(&abi_json)
                .map_err(|e| ContractError::AbiError(e.to_string()))?,
            provider.clone()
        );
        
        // Tạo một implementation đơn giản cho ERC721Interface (cho mục đích demo)
        struct Erc721Contract {
            metadata: ContractMetadata,
            contract: ethers::contract::Contract<ethers::providers::Provider<ethers::providers::Http>>,
            provider: ethers::providers::Provider<ethers::providers::Http>,
        }
        
        #[async_trait::async_trait]
        impl ContractInterface for Erc721Contract {
            fn metadata(&self) -> &ContractMetadata {
                &self.metadata
            }
            
            async fn call<T: Send + Sync + 'static>(&self, function: &str, params: Vec<ethers::abi::Token>) 
                -> Result<T, ContractError> {
                self.contract
                    .method::<_, T>(function, params)
                    .map_err(|e| ContractError::CallError(format!("Failed to prepare method call: {}", e)))?
                    .call()
                    .await
                    .map_err(|e| ContractError::CallError(format!("Failed to call {}: {}", function, e)))
            }
            
            async fn send_transaction(&self, function: &str, params: Vec<ethers::abi::Token>, value: Option<ethers::types::U256>) 
                -> Result<ethers::types::TransactionReceipt, ContractError> {
                Err(ContractError::OtherError("Wallet not configured for sending transactions".to_string()))
            }
            
            fn encode_function_data(&self, function: &str, params: Vec<ethers::abi::Token>)
                -> Result<Vec<u8>, ContractError> {
                self.contract
                    .encode(function, params)
                    .map_err(|e| ContractError::AbiError(format!("Failed to encode function data: {}", e)))
            }
            
            fn decode_logs<T: Send + Sync + 'static>(&self, _logs: &[ethers::types::Log])
                -> Result<Vec<T>, ContractError> {
                Err(ContractError::OtherError("Decode logs not implemented for ERC721 yet".to_string()))
            }
                
            async fn check_permission(&self, address: ethers::types::Address, permission: &str) -> Result<bool, ContractError> {
                match permission {
                    "owner" => {
                        // Kiểm tra nếu contract có phương thức owner()
                        let owner_result: Result<ethers::types::Address, _> = self.call("owner", vec![]).await;
                        match owner_result {
                            Ok(owner) => Ok(owner == address),
                            Err(_) => Err(ContractError::OtherError("Cannot determine owner".to_string())),
                        }
                    },
                    "balance" => {
                        // Kiểm tra nếu địa chỉ có token nào không
                        let balance: Result<ethers::types::U256, _> = self.call("balanceOf", vec![ethers::abi::Token::Address(address)]).await;
                        match balance {
                            Ok(balance) => Ok(!balance.is_zero()),
                            Err(e) => Err(e),
                        }
                    },
                    _ => Err(ContractError::OtherError(format!("Unknown permission: {}", permission))),
                }
            }
        }
        
        // Tạo contract mới
        let erc721_contract = Erc721Contract {
            metadata,
            contract,
            provider,
        };
        
        // Trả về contract được đóng gói trong Arc<dyn ContractInterface>
        Ok(Arc::new(erc721_contract) as Arc<dyn ContractInterface>)
    }
    
    /// Tạo contract DEX Router
    pub async fn create_dex_router_contract(&self, metadata: ContractMetadata) -> Result<Arc<dyn ContractInterface>, ContractError> {
        // Kiểm tra loại contract
        if metadata.contract_type != ContractType::DexRouter {
            return Err(ContractError::UnsupportedContract(
                format!("Expected DexRouter contract, got {:?}", metadata.contract_type)
            ));
        }
        
        // DEX Router standard ABI (Uniswap V2 compatible)
        const DEX_ROUTER_ABI: &str = r#"
        [
            {
                "inputs": [
                    {"internalType": "uint256", "name": "amountIn", "type": "uint256"},
                    {"internalType": "uint256", "name": "amountOutMin", "type": "uint256"},
                    {"internalType": "address[]", "name": "path", "type": "address[]"},
                    {"internalType": "address", "name": "to", "type": "address"},
                    {"internalType": "uint256", "name": "deadline", "type": "uint256"}
                ],
                "name": "swapExactTokensForTokens",
                "outputs": [{"internalType": "uint256[]", "name": "amounts", "type": "uint256[]"}],
                "stateMutability": "nonpayable",
                "type": "function"
            },
            {
                "inputs": [
                    {"internalType": "uint256", "name": "amountIn", "type": "uint256"},
                    {"internalType": "uint256", "name": "amountOutMin", "type": "uint256"},
                    {"internalType": "address[]", "name": "path", "type": "address[]"},
                    {"internalType": "address", "name": "to", "type": "address"},
                    {"internalType": "uint256", "name": "deadline", "type": "uint256"}
                ],
                "name": "swapExactTokensForETH",
                "outputs": [{"internalType": "uint256[]", "name": "amounts", "type": "uint256[]"}],
                "stateMutability": "nonpayable",
                "type": "function"
            },
            {
                "inputs": [
                    {"internalType": "uint256", "name": "amountOutMin", "type": "uint256"},
                    {"internalType": "address[]", "name": "path", "type": "address[]"},
                    {"internalType": "address", "name": "to", "type": "address"},
                    {"internalType": "uint256", "name": "deadline", "type": "uint256"}
                ],
                "name": "swapExactETHForTokens",
                "outputs": [{"internalType": "uint256[]", "name": "amounts", "type": "uint256[]"}],
                "stateMutability": "payable",
                "type": "function"
            },
            {
                "inputs": [
                    {"internalType": "uint256", "name": "amountOut", "type": "uint256"},
                    {"internalType": "uint256", "name": "amountInMax", "type": "uint256"},
                    {"internalType": "address[]", "name": "path", "type": "address[]"},
                    {"internalType": "address", "name": "to", "type": "address"},
                    {"internalType": "uint256", "name": "deadline", "type": "uint256"}
                ],
                "name": "swapTokensForExactTokens",
                "outputs": [{"internalType": "uint256[]", "name": "amounts", "type": "uint256[]"}],
                "stateMutability": "nonpayable",
                "type": "function"
            },
            {
                "inputs": [
                    {"internalType": "uint256", "name": "amountOut", "type": "uint256"},
                    {"internalType": "address[]", "name": "path", "type": "address[]"},
                    {"internalType": "address", "name": "to", "type": "address"},
                    {"internalType": "uint256", "name": "deadline", "type": "uint256"}
                ],
                "name": "swapETHForExactTokens",
                "outputs": [{"internalType": "uint256[]", "name": "amounts", "type": "uint256[]"}],
                "stateMutability": "payable",
                "type": "function"
            },
            {
                "inputs": [
                    {"internalType": "uint256", "name": "amountA", "type": "uint256"},
                    {"internalType": "uint256", "name": "amountB", "type": "uint256"},
                    {"internalType": "uint256", "name": "amountAMin", "type": "uint256"},
                    {"internalType": "uint256", "name": "amountBMin", "type": "uint256"},
                    {"internalType": "address", "name": "to", "type": "address"},
                    {"internalType": "uint256", "name": "deadline", "type": "uint256"}
                ],
                "name": "addLiquidity",
                "outputs": [
                    {"internalType": "uint256", "name": "amountA", "type": "uint256"},
                    {"internalType": "uint256", "name": "amountB", "type": "uint256"},
                    {"internalType": "uint256", "name": "liquidity", "type": "uint256"}
                ],
                "stateMutability": "nonpayable",
                "type": "function"
            },
            {
                "inputs": [
                    {"internalType": "address", "name": "token", "type": "address"},
                    {"internalType": "uint256", "name": "amountTokenDesired", "type": "uint256"},
                    {"internalType": "uint256", "name": "amountTokenMin", "type": "uint256"},
                    {"internalType": "uint256", "name": "amountETHMin", "type": "uint256"},
                    {"internalType": "address", "name": "to", "type": "address"},
                    {"internalType": "uint256", "name": "deadline", "type": "uint256"}
                ],
                "name": "addLiquidityETH",
                "outputs": [
                    {"internalType": "uint256", "name": "amountToken", "type": "uint256"},
                    {"internalType": "uint256", "name": "amountETH", "type": "uint256"},
                    {"internalType": "uint256", "name": "liquidity", "type": "uint256"}
                ],
                "stateMutability": "payable",
                "type": "function"
            },
            {
                "inputs": [
                    {"internalType": "address", "name": "tokenA", "type": "address"},
                    {"internalType": "address", "name": "tokenB", "type": "address"},
                    {"internalType": "uint256", "name": "amountIn", "type": "uint256"}
                ],
                "name": "getAmountsOut",
                "outputs": [{"internalType": "uint256[]", "name": "amounts", "type": "uint256[]"}],
                "stateMutability": "view",
                "type": "function"
            },
            {
                "inputs": [
                    {"internalType": "address", "name": "tokenA", "type": "address"},
                    {"internalType": "address", "name": "tokenB", "type": "address"},
                    {"internalType": "uint256", "name": "amountOut", "type": "uint256"}
                ],
                "name": "getAmountsIn",
                "outputs": [{"internalType": "uint256[]", "name": "amounts", "type": "uint256[]"}],
                "stateMutability": "view",
                "type": "function"
            },
            {
                "inputs": [],
                "name": "factory",
                "outputs": [{"internalType": "address", "name": "", "type": "address"}],
                "stateMutability": "view",
                "type": "function"
            },
            {
                "inputs": [],
                "name": "WETH",
                "outputs": [{"internalType": "address", "name": "", "type": "address"}],
                "stateMutability": "view",
                "type": "function"
            }
        ]
        "#;
        
        // Lấy provider cho chain ID
        let provider = crate::defi::blockchain::get_provider(metadata.chain_id.into())
            .map_err(|e| ContractError::ConnectionError(e.to_string()))?;
            
        // Tạo contract từ ABI
        let abi_json = metadata.abi_json.clone().unwrap_or_else(|| DEX_ROUTER_ABI.to_string());
        let contract = ethers::contract::Contract::new(
            metadata.address,
            serde_json::from_str(&abi_json)
                .map_err(|e| ContractError::AbiError(e.to_string()))?,
            provider.clone()
        );
        
        // Tạo một implementation đơn giản cho DEX Router
        struct DexRouterContract {
            metadata: ContractMetadata,
            contract: ethers::contract::Contract<ethers::providers::Provider<ethers::providers::Http>>,
            provider: ethers::providers::Provider<ethers::providers::Http>,
        }
        
        #[async_trait::async_trait]
        impl ContractInterface for DexRouterContract {
            fn metadata(&self) -> &ContractMetadata {
                &self.metadata
            }
            
            async fn call<T: Send + Sync + 'static>(&self, function: &str, params: Vec<ethers::abi::Token>) 
                -> Result<T, ContractError> {
                self.contract
                    .method::<_, T>(function, params)
                    .map_err(|e| ContractError::CallError(format!("Failed to prepare method call: {}", e)))?
                    .call()
                    .await
                    .map_err(|e| ContractError::CallError(format!("Failed to call {}: {}", function, e)))
            }
            
            async fn send_transaction(&self, function: &str, params: Vec<ethers::abi::Token>, value: Option<ethers::types::U256>) 
                -> Result<ethers::types::TransactionReceipt, ContractError> {
                Err(ContractError::OtherError("Wallet not configured for sending transactions".to_string()))
            }
            
            fn encode_function_data(&self, function: &str, params: Vec<ethers::abi::Token>)
                -> Result<Vec<u8>, ContractError> {
                self.contract
                    .encode(function, params)
                    .map_err(|e| ContractError::AbiError(format!("Failed to encode function data: {}", e)))
            }
            
            fn decode_logs<T: Send + Sync + 'static>(&self, _logs: &[ethers::types::Log])
                -> Result<Vec<T>, ContractError> {
                Err(ContractError::OtherError("Decode logs not implemented for DEX Router yet".to_string()))
            }
                
            async fn check_permission(&self, address: ethers::types::Address, permission: &str) -> Result<bool, ContractError> {
                match permission {
                    "owner" => {
                        // Kiểm tra nếu contract có phương thức owner()
                        let owner_result: Result<ethers::types::Address, _> = self.call("owner", vec![]).await;
                        match owner_result {
                            Ok(owner) => Ok(owner == address),
                            Err(_) => Err(ContractError::OtherError("Cannot determine owner".to_string())),
                        }
                    },
                    _ => Err(ContractError::OtherError(format!("Unknown permission: {}", permission))),
                }
            }
        }
        
        // Tạo contract mới
        let dex_router_contract = DexRouterContract {
            metadata,
            contract,
            provider,
        };
        
        // Trả về contract được đóng gói trong Arc<dyn ContractInterface>
        Ok(Arc::new(dex_router_contract) as Arc<dyn ContractInterface>)
    }
    
    /// Tạo contract Staking
    pub async fn create_staking_contract(&self, metadata: ContractMetadata) -> Result<Arc<dyn ContractInterface>, ContractError> {
        // TODO: Implement create_staking_contract
        Err(ContractError::OtherError("Not implemented yet".to_string()))
    }
    
    /// Tạo contract theo loại
    pub async fn create_contract(
        &self, 
        metadata: ContractMetadata
    ) -> Result<Arc<dyn ContractInterface>, ContractError> {
        let contract = match metadata.contract_type {
            ContractType::ERC20 => self.create_erc20_contract(metadata).await?,
            ContractType::ERC721 => self.create_erc721_contract(metadata).await?,
            ContractType::DexRouter => self.create_dex_router_contract(metadata).await?,
            ContractType::Staking => self.create_staking_contract(metadata).await?,
            _ => return Err(ContractError::UnsupportedContract(
                format!("Contract type {:?} not supported yet", metadata.contract_type)
            )),
        };
        
        // Đăng ký contract vào registry
        self.registry.register_contract(contract.clone()).await?;
        
        Ok(contract)
    }
}

/// Singleton Contract Registry
pub static CONTRACT_REGISTRY: Lazy<Arc<ContractRegistry>> = Lazy::new(|| {
    Arc::new(ContractRegistry::new())
});

/// Lấy contract registry
pub fn get_contract_registry() -> Arc<ContractRegistry> {
    CONTRACT_REGISTRY.clone()
}

/// Lấy contract factory
pub fn get_contract_factory() -> ContractFactory {
    ContractFactory::new(get_contract_registry())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_contract_registry_new() {
        let registry = ContractRegistry::new();
        assert!(true); // Kiểm tra khởi tạo thành công
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
        assert_eq!(query.verified_only, true);
        assert_eq!(query.audited_only, true);
    }
    
    #[test]
    fn test_contract_type() {
        let t1 = ContractType::ERC20;
        let t2 = ContractType::ERC721;
        let t3 = ContractType::Custom("MyToken".to_string());
        
        assert_ne!(t1, t2);
        assert_ne!(t1, t3);
        
        match t3 {
            ContractType::Custom(name) => assert_eq!(name, "MyToken"),
            _ => panic!("Expected Custom type"),
        }
    }
} 