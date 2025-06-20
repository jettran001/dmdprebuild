//! Module ERC721 - Triển khai interface cho ERC721 NFTs
//!
//! Module này cung cấp triển khai cho ERC721 NFT contracts, cho phép:
//! - Truy vấn thông tin NFT: tên, symbol, URI, tokenId
//! - Kiểm tra quyền sở hữu NFT
//! - Phê duyệt và chuyển NFT
//! - Theo dõi sự kiện Transfer và Approval
//!
//! ## Ví dụ:
//! ```rust
//! use wallet::defi::erc721::{Erc721Contract, Erc721ContractBuilder};
//! use ethers::types::Address;
//!
//! #[tokio::main]
//! async fn main() {
//!     let contract = Erc721ContractBuilder::new()
//!         .with_address("0x123...".parse().unwrap())
//!         .with_chain_id(1)
//!         .with_name("My NFT Collection")
//!         .build()
//!         .await
//!         .unwrap();
//!
//!     let owner = contract.owner_of(1).await.unwrap();
//!     println!("Owner of token #1: {}", owner);
//! }
//! ```

use std::sync::Arc;
use ethers::abi::{Token, Tokenize};
use ethers::contract::{Contract, ContractError as EthersContractError};
use ethers::providers::{Provider, Http, Middleware};
use ethers::types::{Address, TransactionReceipt, U256, H256, Log};
use ethers::signers::{Signer, LocalWallet};
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use tracing::{info, warn, error, debug};

use crate::defi::contracts::{ContractInterface, ContractError, ContractType, ContractMetadata};
use crate::walletmanager::chain::ChainType;
use crate::defi::blockchain::get_provider;

/// ERC721 standard ABI
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
    "inputs": [],
    "name": "symbol",
    "outputs": [{"name": "", "type": "string"}],
    "payable": false,
    "stateMutability": "view",
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
    "inputs": [{"name": "tokenId", "type": "uint256"}],
    "name": "getApproved",
    "outputs": [{"name": "", "type": "address"}],
    "payable": false,
    "stateMutability": "view",
    "type": "function"
  },
  {
    "constant": false,
    "inputs": [{"name": "to", "type": "address"}, {"name": "approved", "type": "bool"}],
    "name": "setApprovalForAll",
    "outputs": [],
    "payable": false,
    "stateMutability": "nonpayable",
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
    "constant": false,
    "inputs": [{"name": "from", "type": "address"}, {"name": "to", "type": "address"}, {"name": "tokenId", "type": "uint256"}],
    "name": "transferFrom",
    "outputs": [],
    "payable": false,
    "stateMutability": "nonpayable",
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
    "constant": false,
    "inputs": [{"name": "from", "type": "address"}, {"name": "to", "type": "address"}, {"name": "tokenId", "type": "uint256"}, {"name": "data", "type": "bytes"}],
    "name": "safeTransferFrom",
    "outputs": [],
    "payable": false,
    "stateMutability": "nonpayable",
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

/// Thông tin NFT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftInfo {
    /// Tên bộ sưu tập
    pub name: String,
    /// Ký hiệu bộ sưu tập
    pub symbol: String,
}

/// NFT Transfer Event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferEvent {
    /// Địa chỉ nguồn
    pub from: Address,
    /// Địa chỉ đích
    pub to: Address,
    /// Token ID
    pub token_id: U256,
}

/// NFT Approval Event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalEvent {
    /// Chủ sở hữu
    pub owner: Address,
    /// Địa chỉ được phê duyệt
    pub approved: Address,
    /// Token ID
    pub token_id: U256,
}

/// NFT ApprovalForAll Event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalForAllEvent {
    /// Chủ sở hữu
    pub owner: Address,
    /// Operator
    pub operator: Address,
    /// Approved
    pub approved: bool,
}

/// ERC721 Contract
pub struct Erc721Contract {
    /// Metadata
    metadata: ContractMetadata,
    /// Contract
    contract: Contract<Provider<Http>>,
    /// Provider
    provider: Provider<Http>,
    /// Wallet
    wallet: Option<LocalWallet>,
    /// Thông tin NFT (cache)
    nft_info: Option<NftInfo>,
}

impl Erc721Contract {
    /// Tạo contract mới từ metadata
    pub async fn new(metadata: ContractMetadata) -> Result<Self, ContractError> {
        // Kiểm tra contract_type
        if metadata.contract_type != ContractType::ERC721 {
            return Err(ContractError::UnsupportedContract(
                format!("Expected ERC721 contract, got {:?}", metadata.contract_type)
            ));
        }
        
        // Lấy provider
        let provider_result = get_provider(metadata.chain_id);
        let provider = match provider_result {
            Ok(p) => p,
            Err(e) => return Err(ContractError::ConnectionError(format!("Failed to get provider: {}", e))),
        };
        
        // Lấy ABI
        let abi_json = metadata.abi_json.clone().unwrap_or_else(|| ERC721_ABI.to_string());
        
        // Tạo contract
        let contract = Contract::new(
            metadata.address,
            abi_json.parse().map_err(|e| ContractError::AbiError(format!("Failed to parse ABI: {}", e)))?,
            provider.clone(),
        );
        
        Ok(Self {
            metadata,
            contract,
            provider,
            wallet: None,
            nft_info: None,
        })
    }
    
    /// Thêm wallet
    pub fn with_wallet(mut self, wallet: LocalWallet) -> Self {
        self.wallet = Some(wallet);
        self
    }
    
    /// Lấy thông tin NFT (cache)
    pub async fn get_nft_info(&mut self) -> Result<NftInfo, ContractError> {
        if let Some(info) = &self.nft_info {
            return Ok(info.clone());
        }
        
        let name: String = self.call("name", vec![]).await?;
        let symbol: String = self.call("symbol", vec![]).await?;
        
        let info = NftInfo {
            name,
            symbol,
        };
        
        self.nft_info = Some(info.clone());
        Ok(info)
    }
    
    /// Lấy chủ sở hữu của token ID
    pub async fn owner_of(&self, token_id: U256) -> Result<Address, ContractError> {
        self.call("ownerOf", vec![Token::Uint(token_id)]).await
    }
    
    /// Lấy URI của token
    pub async fn token_uri(&self, token_id: U256) -> Result<String, ContractError> {
        self.call("tokenURI", vec![Token::Uint(token_id)]).await
    }
    
    /// Lấy số lượng NFT của một địa chỉ
    pub async fn balance_of(&self, owner: Address) -> Result<U256, ContractError> {
        self.call("balanceOf", vec![Token::Address(owner)]).await
    }
    
    /// Phê duyệt địa chỉ thao tác với token
    pub async fn approve(&self, to: Address, token_id: U256) -> Result<TransactionReceipt, ContractError> {
        self.send_transaction(
            "approve",
            vec![Token::Address(to), Token::Uint(token_id)],
            None,
        ).await
    }
    
    /// Lấy địa chỉ được phê duyệt cho token
    pub async fn get_approved(&self, token_id: U256) -> Result<Address, ContractError> {
        self.call("getApproved", vec![Token::Uint(token_id)]).await
    }
    
    /// Phê duyệt tất cả token cho operator
    pub async fn set_approval_for_all(&self, operator: Address, approved: bool) -> Result<TransactionReceipt, ContractError> {
        self.send_transaction(
            "setApprovalForAll",
            vec![Token::Address(operator), Token::Bool(approved)],
            None,
        ).await
    }
    
    /// Kiểm tra operator có được phê duyệt cho owner
    pub async fn is_approved_for_all(&self, owner: Address, operator: Address) -> Result<bool, ContractError> {
        self.call("isApprovedForAll", vec![Token::Address(owner), Token::Address(operator)]).await
    }
    
    /// Chuyển token
    pub async fn transfer_from(&self, from: Address, to: Address, token_id: U256) -> Result<TransactionReceipt, ContractError> {
        // Kiểm tra địa chỉ hợp lệ (không thể là địa chỉ zero)
        if to == Address::zero() {
            return Err(ContractError::CallError("Cannot transfer to zero address".into()));
        }
        
        // Kiểm tra ownership (chỉ owner hoặc approved address mới có thể transfer)
        let owner = self.owner_of(token_id).await?;
        let sender = match &self.wallet {
            Some(wallet) => wallet.address(),
            None => return Err(ContractError::SignatureError("No wallet provided for transaction".into())),
        };
        
        // Kiểm tra quyền
        if owner != sender {
            // Nếu không phải owner, kiểm tra xem có được approve không
            let approved = self.get_approved(token_id).await?;
            let is_approved_for_all = self.is_approved_for_all(owner, sender).await?;
            
            if approved != sender && !is_approved_for_all {
                return Err(ContractError::CallError(
                    "Not authorized to transfer this token".into()
                ));
            }
        }
        
        // Log cho mục đích audit
        info!("Transferring NFT token_id={} from={:?} to={:?}", token_id, from, to);
        
        // Thực hiện giao dịch với retry mechanism
        let max_retries = 3;
        let mut attempt = 0;
        
        loop {
            attempt += 1;
            match self.send_transaction(
                "transferFrom",
                vec![Token::Address(from), Token::Address(to), Token::Uint(token_id)],
                None,
            ).await {
                Ok(receipt) => {
                    info!("NFT transfer successful: tx_hash={}", receipt.transaction_hash);
                    return Ok(receipt);
                },
                Err(e) => {
                    if attempt >= max_retries {
                        error!("Failed to transfer NFT after {} attempts: {}", max_retries, e);
                        return Err(e);
                    }
                    
                    warn!("Transfer attempt {} failed: {}. Retrying...", attempt, e);
                    tokio::time::sleep(tokio::time::Duration::from_millis(500 * attempt as u64)).await;
                }
            }
        }
    }
    
    /// Chuyển token an toàn
    pub async fn safe_transfer_from(&self, from: Address, to: Address, token_id: U256) -> Result<TransactionReceipt, ContractError> {
        // Kiểm tra địa chỉ hợp lệ (không thể là địa chỉ zero)
        if to == Address::zero() {
            return Err(ContractError::CallError("Cannot transfer to zero address".into()));
        }
        
        // Kiểm tra ownership (chỉ owner hoặc approved address mới có thể transfer)
        let owner = self.owner_of(token_id).await?;
        let sender = match &self.wallet {
            Some(wallet) => wallet.address(),
            None => return Err(ContractError::SignatureError("No wallet provided for transaction".into())),
        };
        
        // Kiểm tra quyền
        if owner != sender {
            // Nếu không phải owner, kiểm tra xem có được approve không
            let approved = self.get_approved(token_id).await?;
            let is_approved_for_all = self.is_approved_for_all(owner, sender).await?;
            
            if approved != sender && !is_approved_for_all {
                return Err(ContractError::CallError(
                    "Not authorized to transfer this token".into()
                ));
            }
        }
        
        // Log cho mục đích audit
        info!("Safely transferring NFT token_id={} from={:?} to={:?}", token_id, from, to);
        
        // Thực hiện giao dịch với retry mechanism
        let max_retries = 3;
        let mut attempt = 0;
        
        loop {
            attempt += 1;
            match self.send_transaction(
                "safeTransferFrom",
                vec![Token::Address(from), Token::Address(to), Token::Uint(token_id)],
                None,
            ).await {
                Ok(receipt) => {
                    info!("Safe NFT transfer successful: tx_hash={}", receipt.transaction_hash);
                    return Ok(receipt);
                },
                Err(e) => {
                    if attempt >= max_retries {
                        error!("Failed to safely transfer NFT after {} attempts: {}", max_retries, e);
                        return Err(e);
                    }
                    
                    warn!("Safe transfer attempt {} failed: {}. Retrying...", attempt, e);
                    tokio::time::sleep(tokio::time::Duration::from_millis(500 * attempt as u64)).await;
                }
            }
        }
    }
    
    /// Giải mã event Transfer
    pub fn decode_transfer_event(&self, log: &Log) -> Result<TransferEvent, ContractError> {
        // Kiểm tra log topics
        if log.topics.len() != 4 {
            return Err(ContractError::CallError("Invalid log format for Transfer event".into()));
        }
        
        // Kiểm tra event signature
        let transfer_signature = H256::from(ethers::utils::keccak256("Transfer(address,address,uint256)"));
        if log.topics[0] != transfer_signature {
            return Err(ContractError::CallError("Log is not a Transfer event".into()));
        }
        
        // Parse addresses và token ID
        let from = Address::from(log.topics[1]);
        let to = Address::from(log.topics[2]);
        let token_id = U256::from(log.topics[3]);
        
        Ok(TransferEvent { from, to, token_id })
    }
    
    /// Giải mã event Approval
    pub fn decode_approval_event(&self, log: &Log) -> Result<ApprovalEvent, ContractError> {
        // Kiểm tra log topics
        if log.topics.len() != 4 {
            return Err(ContractError::CallError("Invalid log format for Approval event".into()));
        }
        
        // Kiểm tra event signature
        let approval_signature = H256::from(ethers::utils::keccak256("Approval(address,address,uint256)"));
        if log.topics[0] != approval_signature {
            return Err(ContractError::CallError("Log is not an Approval event".into()));
        }
        
        // Parse addresses và token ID
        let owner = Address::from(log.topics[1]);
        let approved = Address::from(log.topics[2]);
        let token_id = U256::from(log.topics[3]);
        
        Ok(ApprovalEvent { owner, approved, token_id })
    }
    
    /// Giải mã event ApprovalForAll
    pub fn decode_approval_for_all_event(&self, log: &Log) -> Result<ApprovalForAllEvent, ContractError> {
        // Kiểm tra log topics
        if log.topics.len() != 3 || log.data.0.len() < 32 {
            return Err(ContractError::CallError("Invalid log format for ApprovalForAll event".into()));
        }
        
        // Kiểm tra event signature
        let approval_signature = H256::from(ethers::utils::keccak256("ApprovalForAll(address,address,bool)"));
        if log.topics[0] != approval_signature {
            return Err(ContractError::CallError("Log is not an ApprovalForAll event".into()));
        }
        
        // Parse addresses
        let owner = Address::from(log.topics[1]);
        let operator = Address::from(log.topics[2]);
        
        // Parse approved
        let approved = log.data.0[31] != 0;
        
        Ok(ApprovalForAllEvent { owner, operator, approved })
    }
}

#[async_trait]
impl ContractInterface for Erc721Contract {
    fn metadata(&self) -> &ContractMetadata {
        &self.metadata
    }
    
    async fn call<T: Send + Sync + 'static>(&self, function: &str, params: Vec<Token>) -> Result<T, ContractError> {
        self.contract.method::<_, T>(function, params.clone())
            .map_err(|e| ContractError::CallError(format!("Failed to prepare method call: {}", e)))?
            .call()
            .await
            .map_err(|e| ContractError::CallError(format!("Method call failed: {}", e)))
    }
    
    async fn send_transaction(
        &self, 
        function: &str, 
        params: Vec<Token>, 
        value: Option<U256>
    ) -> Result<TransactionReceipt, ContractError> {
        let wallet = self.wallet.clone().ok_or_else(|| {
            ContractError::SignatureError("No wallet provided for transaction".into())
        })?;
        
        let client = self.provider.with_signer(wallet);
        
        let mut tx = self.contract.method::<_, bool>(function, params.clone())
            .map_err(|e| ContractError::CallError(format!("Failed to prepare transaction: {}", e)))?;
            
        if let Some(v) = value {
            tx = tx.value(v);
        }
        
        let pending_tx = tx.send()
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to send transaction: {}", e)))?;
            
        pending_tx.await
            .map_err(|e| ContractError::CallError(format!("Failed to confirm transaction: {}", e)))
    }
    
    fn encode_function_data(&self, function: &str, params: Vec<Token>) -> Result<Vec<u8>, ContractError> {
        self.contract.encode(function, params)
            .map_err(|e| ContractError::AbiError(format!("Failed to encode function data: {}", e)))
    }
    
    fn decode_logs<T: Send + Sync + 'static>(&self, logs: &[Log]) -> Result<Vec<T>, ContractError> {
        if std::any::TypeId::of::<T>() == std::any::TypeId::of::<TransferEvent>() {
            let events: Vec<TransferEvent> = logs.iter()
                .filter_map(|log| {
                    match self.decode_transfer_event(log) {
                        Ok(event) => Some(event),
                        Err(e) => {
                            error!("Failed to decode transfer event: {}", e);
                            None
                        }
                    }
                })
                .collect();
            
            // Chuyển đổi an toàn sử dụng trait Any
            let mut result = Vec::with_capacity(events.len());
            for event in events {
                // Chuyển đổi an toàn, sử dụng Result thay vì panic
                let boxed = Box::new(event);
                let any = boxed as Box<dyn std::any::Any>;
                
                match any.downcast::<T>() {
                    Ok(t) => result.push(*t),
                    Err(_) => {
                        // Log lỗi thay vì panic
                        error!("Failed to downcast TransferEvent to requested type, skipping event");
                        // Không thêm event này vào kết quả
                    }
                }
            }
            
            Ok(result)
        } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<ApprovalEvent>() {
            let events: Vec<ApprovalEvent> = logs.iter()
                .filter_map(|log| {
                    match self.decode_approval_event(log) {
                        Ok(event) => Some(event),
                        Err(e) => {
                            error!("Failed to decode approval event: {}", e);
                            None
                        }
                    }
                })
                .collect();
            
            // Chuyển đổi an toàn sử dụng trait Any
            let mut result = Vec::with_capacity(events.len());
            for event in events {
                // Chuyển đổi an toàn, sử dụng Result thay vì panic
                let boxed = Box::new(event);
                let any = boxed as Box<dyn std::any::Any>;
                
                match any.downcast::<T>() {
                    Ok(t) => result.push(*t),
                    Err(_) => {
                        // Log lỗi thay vì panic
                        error!("Failed to downcast ApprovalEvent to requested type, skipping event");
                        // Không thêm event này vào kết quả
                    }
                }
            }
            
            Ok(result)
        } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<ApprovalForAllEvent>() {
            let events: Vec<ApprovalForAllEvent> = logs.iter()
                .filter_map(|log| {
                    match self.decode_approval_for_all_event(log) {
                        Ok(event) => Some(event),
                        Err(e) => {
                            error!("Failed to decode approval for all event: {}", e);
                            None
                        }
                    }
                })
                .collect();
            
            // Chuyển đổi an toàn sử dụng trait Any
            let mut result = Vec::with_capacity(events.len());
            for event in events {
                // Chuyển đổi an toàn, sử dụng Result thay vì panic
                let boxed = Box::new(event);
                let any = boxed as Box<dyn std::any::Any>;
                
                match any.downcast::<T>() {
                    Ok(t) => result.push(*t),
                    Err(_) => {
                        // Log lỗi thay vì panic
                        error!("Failed to downcast ApprovalForAllEvent to requested type, skipping event");
                        // Không thêm event này vào kết quả
                    }
                }
            }
            
            Ok(result)
        } else {
            Err(ContractError::CallError(format!(
                "Unsupported event type for decode_logs"
            )))
        }
    }
    
    async fn check_permission(&self, address: Address, permission: &str) -> Result<bool, ContractError> {
        match permission {
            "owner" => {
                // Kiểm tra xem địa chỉ có phải là owner của contract
                match self.contract.method::<_, Address>("owner", ()).call().await {
                    Ok(owner) => Ok(owner == address),
                    Err(_) => {
                        // Không tìm thấy hàm owner, trả về false
                        Ok(false)
                    }
                }
            },
            "minter" => {
                // Kiểm tra quyền mint
                match self.contract.method::<_, bool>("isMinter", [address]).call().await {
                    Ok(is_minter) => Ok(is_minter),
                    Err(_) => {
                        // Không tìm thấy hàm isMinter, trả về false
                        Ok(false)
                    }
                }
            },
            _ => Ok(false),
        }
    }
}

/// Builder cho ERC721Contract
pub struct Erc721ContractBuilder {
    address: Option<Address>,
    chain_id: Option<u64>,
    chain_type: Option<ChainType>,
    name: Option<String>,
    abi_json: Option<String>,
    verified: bool,
    wallet: Option<LocalWallet>,
}

impl Erc721ContractBuilder {
    /// Tạo builder mới
    pub fn new() -> Self {
        Self {
            address: None,
            chain_id: None,
            chain_type: Some(ChainType::EVM),
            name: None,
            abi_json: None,
            verified: false,
            wallet: None,
        }
    }
    
    /// Thêm địa chỉ
    pub fn with_address(mut self, address: Address) -> Self {
        self.address = Some(address);
        self
    }
    
    /// Thêm chain ID
    pub fn with_chain_id(mut self, chain_id: u64) -> Self {
        self.chain_id = Some(chain_id);
        self
    }
    
    /// Thêm loại chain
    pub fn with_chain_type(mut self, chain_type: ChainType) -> Self {
        self.chain_type = Some(chain_type);
        self
    }
    
    /// Thêm tên
    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }
    
    /// Thêm ABI JSON
    pub fn with_abi_json(mut self, abi_json: String) -> Self {
        self.abi_json = Some(abi_json);
        self
    }
    
    /// Thêm trạng thái verified
    pub fn with_verified(mut self, verified: bool) -> Self {
        self.verified = verified;
        self
    }
    
    /// Thêm wallet để ký giao dịch
    pub fn with_wallet(mut self, wallet: LocalWallet) -> Self {
        self.wallet = Some(wallet);
        self
    }
    
    /// Xây dựng contract
    pub async fn build(self) -> Result<Erc721Contract, ContractError> {
        let address = self.address.ok_or_else(|| {
            ContractError::CallError("Address is required".into())
        })?;
        
        let chain_id = self.chain_id.ok_or_else(|| {
            ContractError::CallError("Chain ID is required".into())
        })?;
        
        let name = self.name.unwrap_or_else(|| "ERC721 Collection".to_string());
        
        let mut metadata = ContractMetadata::new(
            name,
            address,
            chain_id,
            ContractType::ERC721,
        );
        
        metadata.chain_type = self.chain_type.unwrap_or(ChainType::EVM);
        metadata.verified = self.verified;
        
        if let Some(abi_json) = self.abi_json {
            metadata.abi_json = Some(abi_json);
        }
        
        let mut contract = Erc721Contract::new(metadata).await?;
        
        if let Some(wallet) = self.wallet {
            contract = contract.with_wallet(wallet);
        }
        
        Ok(contract)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_erc721_contract_builder() {
        let builder = Erc721ContractBuilder::new()
            .with_address(Address::zero())
            .with_chain_id(1)
            .with_name("Test NFT Collection".to_string())
            .with_verified(true);
        
        assert!(builder.address.is_some());
        assert_eq!(builder.address.unwrap(), Address::zero());
        assert!(builder.chain_id.is_some());
        assert_eq!(builder.chain_id.unwrap(), 1);
        assert!(builder.name.is_some());
        assert_eq!(builder.name.unwrap(), "Test NFT Collection");
        assert!(builder.verified);
    }
} 