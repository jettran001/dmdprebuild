//! Module ERC1155 - Triển khai interface cho ERC1155 Multi-Token Standard
//!
//! Module này cung cấp triển khai cho ERC1155 Multi-Token contracts, cho phép:
//! - Truy vấn thông tin về token: URI, số lượng
//! - Chuyển một hoặc nhiều token
//! - Phê duyệt giao dịch cho tất cả token
//! - Theo dõi sự kiện Transfer và ApprovalForAll
//!
//! ## Ví dụ:
//! ```rust
//! use wallet::defi::erc1155::{Erc1155Contract, Erc1155ContractBuilder};
//! use ethers::types::{Address, U256};
//!
//! #[tokio::main]
//! async fn main() {
//!     let contract = Erc1155ContractBuilder::new()
//!         .with_address("0x123...".parse().unwrap())
//!         .with_chain_id(1)
//!         .with_name("My Multi-token Collection")
//!         .build()
//!         .await
//!         .unwrap();
//!
//!     let balance = contract.balance_of("0x456...".parse().unwrap(), 1).await.unwrap();
//!     println!("Balance of token #1: {}", balance);
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

/// ERC1155 standard ABI
const ERC1155_ABI: &str = r#"
[
  {
    "constant": true,
    "inputs": [{"name": "account", "type": "address"}, {"name": "id", "type": "uint256"}],
    "name": "balanceOf",
    "outputs": [{"name": "", "type": "uint256"}],
    "payable": false,
    "stateMutability": "view",
    "type": "function"
  },
  {
    "constant": true,
    "inputs": [{"name": "accounts", "type": "address[]"}, {"name": "ids", "type": "uint256[]"}],
    "name": "balanceOfBatch",
    "outputs": [{"name": "", "type": "uint256[]"}],
    "payable": false,
    "stateMutability": "view",
    "type": "function"
  },
  {
    "constant": true,
    "inputs": [{"name": "account", "type": "address"}, {"name": "operator", "type": "address"}],
    "name": "isApprovedForAll",
    "outputs": [{"name": "", "type": "bool"}],
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
    "inputs": [{"name": "from", "type": "address"}, {"name": "to", "type": "address"}, {"name": "id", "type": "uint256"}, {"name": "amount", "type": "uint256"}, {"name": "data", "type": "bytes"}],
    "name": "safeTransferFrom",
    "outputs": [],
    "payable": false,
    "stateMutability": "nonpayable",
    "type": "function"
  },
  {
    "constant": false,
    "inputs": [{"name": "from", "type": "address"}, {"name": "to", "type": "address"}, {"name": "ids", "type": "uint256[]"}, {"name": "amounts", "type": "uint256[]"}, {"name": "data", "type": "bytes"}],
    "name": "safeBatchTransferFrom",
    "outputs": [],
    "payable": false,
    "stateMutability": "nonpayable",
    "type": "function"
  },
  {
    "constant": true,
    "inputs": [{"name": "id", "type": "uint256"}],
    "name": "uri",
    "outputs": [{"name": "", "type": "string"}],
    "payable": false,
    "stateMutability": "view",
    "type": "function"
  },
  {
    "anonymous": false,
    "inputs": [
      {"indexed": true, "name": "operator", "type": "address"},
      {"indexed": true, "name": "from", "type": "address"},
      {"indexed": true, "name": "to", "type": "address"},
      {"indexed": false, "name": "id", "type": "uint256"},
      {"indexed": false, "name": "value", "type": "uint256"}
    ],
    "name": "TransferSingle",
    "type": "event"
  },
  {
    "anonymous": false,
    "inputs": [
      {"indexed": true, "name": "operator", "type": "address"},
      {"indexed": true, "name": "from", "type": "address"},
      {"indexed": true, "name": "to", "type": "address"},
      {"indexed": false, "name": "ids", "type": "uint256[]"},
      {"indexed": false, "name": "values", "type": "uint256[]"}
    ],
    "name": "TransferBatch",
    "type": "event"
  },
  {
    "anonymous": false,
    "inputs": [
      {"indexed": true, "name": "account", "type": "address"},
      {"indexed": true, "name": "operator", "type": "address"},
      {"indexed": false, "name": "approved", "type": "bool"}
    ],
    "name": "ApprovalForAll",
    "type": "event"
  },
  {
    "anonymous": false,
    "inputs": [
      {"indexed": false, "name": "value", "type": "string"},
      {"indexed": true, "name": "id", "type": "uint256"}
    ],
    "name": "URI",
    "type": "event"
  }
]
"#;

/// ERC1155 TransferSingle Event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferSingleEvent {
    /// Operator
    pub operator: Address,
    /// Từ địa chỉ
    pub from: Address,
    /// Đến địa chỉ
    pub to: Address,
    /// Token ID
    pub id: U256,
    /// Số lượng token
    pub value: U256,
}

/// ERC1155 TransferBatch Event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferBatchEvent {
    /// Operator
    pub operator: Address,
    /// Từ địa chỉ
    pub from: Address,
    /// Đến địa chỉ
    pub to: Address,
    /// Danh sách token ID
    pub ids: Vec<U256>,
    /// Danh sách số lượng
    pub values: Vec<U256>,
}

/// ERC1155 ApprovalForAll Event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalForAllEvent {
    /// Chủ sở hữu
    pub account: Address,
    /// Operator
    pub operator: Address,
    /// Trạng thái phê duyệt
    pub approved: bool,
}

/// ERC1155 URI Event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UriEvent {
    /// URI
    pub value: String,
    /// Token ID
    pub id: U256,
}

/// ERC1155 Contract
pub struct Erc1155Contract {
    /// Metadata
    metadata: ContractMetadata,
    /// Contract
    contract: Contract<Provider<Http>>,
    /// Provider
    provider: Provider<Http>,
    /// Wallet
    wallet: Option<LocalWallet>,
}

impl Erc1155Contract {
    /// Tạo contract mới từ metadata
    pub async fn new(metadata: ContractMetadata) -> Result<Self, ContractError> {
        // Kiểm tra contract_type
        if metadata.contract_type != ContractType::ERC1155 {
            return Err(ContractError::UnsupportedContract(
                format!("Expected ERC1155 contract, got {:?}", metadata.contract_type)
            ));
        }
        
        // Lấy provider
        let provider_result = get_provider(metadata.chain_id);
        let provider = match provider_result {
            Ok(p) => p,
            Err(e) => return Err(ContractError::ConnectionError(format!("Failed to get provider: {}", e))),
        };
        
        // Lấy ABI
        let abi_json = metadata.abi_json.clone().unwrap_or_else(|| ERC1155_ABI.to_string());
        
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
        })
    }
    
    /// Thêm wallet
    pub fn with_wallet(mut self, wallet: LocalWallet) -> Self {
        self.wallet = Some(wallet);
        self
    }
    
    /// Lấy số lượng token theo ID cho một địa chỉ
    pub async fn balance_of(&self, account: Address, id: U256) -> Result<U256, ContractError> {
        self.call("balanceOf", vec![Token::Address(account), Token::Uint(id)]).await
    }
    
    /// Lấy số lượng nhiều token theo ID cho nhiều địa chỉ
    pub async fn balance_of_batch(
        &self, 
        accounts: Vec<Address>, 
        ids: Vec<U256>
    ) -> Result<Vec<U256>, ContractError> {
        // Chuyển accounts và ids thành tokens
        let accounts_tokens = Token::Array(accounts.into_iter().map(Token::Address).collect());
        let ids_tokens = Token::Array(ids.into_iter().map(Token::Uint).collect());
        
        self.call("balanceOfBatch", vec![accounts_tokens, ids_tokens]).await
    }
    
    /// Kiểm tra operator có được phê duyệt cho account
    pub async fn is_approved_for_all(&self, account: Address, operator: Address) -> Result<bool, ContractError> {
        self.call("isApprovedForAll", vec![Token::Address(account), Token::Address(operator)]).await
    }
    
    /// Đặt trạng thái phê duyệt cho operator
    pub async fn set_approval_for_all(&self, operator: Address, approved: bool) -> Result<TransactionReceipt, ContractError> {
        self.send_transaction(
            "setApprovalForAll",
            vec![Token::Address(operator), Token::Bool(approved)],
            None,
        ).await
    }
    
    /// Chuyển token an toàn
    pub async fn safe_transfer_from(
        &self, 
        from: Address, 
        to: Address, 
        id: U256,
        amount: U256,
        data: Vec<u8>
    ) -> Result<TransactionReceipt, ContractError> {
        // Kiểm tra địa chỉ
        if to == Address::zero() {
            return Err(ContractError::CallError("Cannot transfer to zero address".into()));
        }
        
        // Kiểm tra số lượng
        if amount == U256::zero() {
            return Err(ContractError::CallError("Cannot transfer zero amount".into()));
        }
        
        // Kiểm tra quyền
        let sender = match &self.wallet {
            Some(wallet) => wallet.address(),
            None => return Err(ContractError::SignatureError("No wallet provided for transaction".into())),
        };
        
        if from != sender {
            let is_approved = self.is_approved_for_all(from, sender).await?;
            if !is_approved {
                return Err(ContractError::CallError(
                    format!("Not approved to transfer tokens on behalf of {:?}", from)
                ));
            }
        }
        
        // Kiểm tra số dư
        let balance = self.balance_of(from, id).await?;
        if balance < amount {
            return Err(ContractError::CallError(
                format!("Insufficient balance: has {:?}, needs {:?}", balance, amount)
            ));
        }
        
        // Log giao dịch
        info!("Transferring ERC1155 token: id={}, amount={}, from={:?}, to={:?}", 
            id, amount, from, to);
        
        // Thực hiện giao dịch với retry
        let max_retries = 3;
        let mut attempt = 0;
        
        loop {
            attempt += 1;
            match self.send_transaction(
                "safeTransferFrom",
                vec![
                    Token::Address(from), 
                    Token::Address(to), 
                    Token::Uint(id),
                    Token::Uint(amount),
                    Token::Bytes(data.clone()),
                ],
                None,
            ).await {
                Ok(receipt) => {
                    info!("ERC1155 token transfer successful: tx_hash={}", receipt.transaction_hash);
                    return Ok(receipt);
                },
                Err(e) => {
                    if attempt >= max_retries {
                        error!("Failed to transfer ERC1155 token after {} attempts: {}", max_retries, e);
                        return Err(e);
                    }
                    
                    warn!("Transfer attempt {} failed: {}. Retrying...", attempt, e);
                    tokio::time::sleep(tokio::time::Duration::from_millis(500 * attempt as u64)).await;
                }
            }
        }
    }
    
    /// Chuyển nhiều token an toàn
    pub async fn safe_batch_transfer_from(
        &self, 
        from: Address, 
        to: Address, 
        ids: Vec<U256>,
        amounts: Vec<U256>,
        data: Vec<u8>
    ) -> Result<TransactionReceipt, ContractError> {
        // Chuyển ids và amounts thành tokens
        let ids_tokens = Token::Array(ids.into_iter().map(Token::Uint).collect());
        let amounts_tokens = Token::Array(amounts.into_iter().map(Token::Uint).collect());
        
        self.send_transaction(
            "safeBatchTransferFrom",
            vec![
                Token::Address(from), 
                Token::Address(to), 
                ids_tokens,
                amounts_tokens,
                Token::Bytes(data),
            ],
            None,
        ).await
    }
    
    /// Lấy URI của token
    pub async fn token_uri(&self, id: U256) -> Result<String, ContractError> {
        self.call("uri", vec![Token::Uint(id)]).await
    }
    
    /// Giải mã event TransferSingle
    pub fn decode_transfer_single_event(&self, log: &Log) -> Result<TransferSingleEvent, ContractError> {
        // Kiểm tra log topics
        if log.topics.len() != 4 || log.data.0.len() < 64 {
            return Err(ContractError::CallError("Invalid log format for TransferSingle event".into()));
        }
        
        // Kiểm tra event signature
        let transfer_signature = H256::from(ethers::utils::keccak256("TransferSingle(address,address,address,uint256,uint256)"));
        if log.topics[0] != transfer_signature {
            return Err(ContractError::CallError("Log is not a TransferSingle event".into()));
        }
        
        // Parse addresses
        let operator = Address::from(log.topics[1]);
        let from = Address::from(log.topics[2]);
        let to = Address::from(log.topics[3]);
        
        // Parse token ID và số lượng
        let id = U256::from_big_endian(&log.data.0[0..32]);
        let value = U256::from_big_endian(&log.data.0[32..64]);
        
        Ok(TransferSingleEvent { operator, from, to, id, value })
    }
    
    /// Giải mã event TransferBatch
    pub fn decode_transfer_batch_event(&self, log: &Log) -> Result<TransferBatchEvent, ContractError> {
        // Kiểm tra log topics
        if log.topics.len() != 4 {
            return Err(ContractError::CallError("Invalid log format for TransferBatch event".into()));
        }
        
        // Kiểm tra event signature
        let transfer_signature = H256::from(ethers::utils::keccak256("TransferBatch(address,address,address,uint256[],uint256[])"));
        if log.topics[0] != transfer_signature {
            return Err(ContractError::CallError("Log is not a TransferBatch event".into()));
        }
        
        // Parse addresses
        let operator = Address::from(log.topics[1]);
        let from = Address::from(log.topics[2]);
        let to = Address::from(log.topics[3]);
        
        // Sử dụng ethers ABI coder để giải mã chính xác data
        // Data format: offset của mảng ids, offset của mảng values, 
        // kích thước mảng ids, các ids, kích thước mảng values, các values
        
        if log.data.0.len() < 64 {
            return Err(ContractError::CallError("Invalid data length for TransferBatch event".into()));
        }
        
        // Giải mã data theo cách an toàn
        let mut ids = Vec::new();
        let mut values = Vec::new();
        
        // Đọc offset của mảng ids (thường là 0x40 = 64)
        let ids_offset = U256::from_big_endian(&log.data.0[0..32]).as_usize();
        
        // Đọc offset của mảng values
        let values_offset = U256::from_big_endian(&log.data.0[32..64]).as_usize();
        
        if ids_offset >= log.data.0.len() || values_offset >= log.data.0.len() {
            return Err(ContractError::CallError("Invalid offsets in TransferBatch event data".into()));
        }
        
        // Đọc kích thước mảng ids
        let ids_start = ids_offset;
        if ids_start + 32 > log.data.0.len() {
            return Err(ContractError::CallError("Invalid ids array start position".into()));
        }
        
        let ids_length = U256::from_big_endian(&log.data.0[ids_start..ids_start+32]).as_usize();
        
        // Đọc các ids
        for i in 0..ids_length {
            let pos = ids_start + 32 + i * 32;
            if pos + 32 > log.data.0.len() {
                return Err(ContractError::CallError(format!("Invalid id position at index {}", i)));
            }
            
            let id = U256::from_big_endian(&log.data.0[pos..pos+32]);
            ids.push(id);
        }
        
        // Đọc kích thước mảng values
        let values_start = values_offset;
        if values_start + 32 > log.data.0.len() {
            return Err(ContractError::CallError("Invalid values array start position".into()));
        }
        
        let values_length = U256::from_big_endian(&log.data.0[values_start..values_start+32]).as_usize();
        
        // Kiểm tra hai mảng có cùng kích thước không
        if ids_length != values_length {
            return Err(ContractError::CallError(
                format!("Mismatched array lengths: ids={}, values={}", ids_length, values_length)
            ));
        }
        
        // Đọc các values
        for i in 0..values_length {
            let pos = values_start + 32 + i * 32;
            if pos + 32 > log.data.0.len() {
                return Err(ContractError::CallError(format!("Invalid value position at index {}", i)));
            }
            
            let value = U256::from_big_endian(&log.data.0[pos..pos+32]);
            values.push(value);
        }
        
        debug!("Decoded TransferBatch event: operator={:?}, from={:?}, to={:?}, ids_count={}, values_count={}",
            operator, from, to, ids.len(), values.len());
        
        Ok(TransferBatchEvent { operator, from, to, ids, values })
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
        let account = Address::from(log.topics[1]);
        let operator = Address::from(log.topics[2]);
        
        // Parse approved
        let approved = log.data.0[31] != 0;
        
        Ok(ApprovalForAllEvent { account, operator, approved })
    }
}

#[async_trait]
impl ContractInterface for Erc1155Contract {
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
        if std::any::TypeId::of::<T>() == std::any::TypeId::of::<TransferSingleEvent>() {
            let events: Vec<TransferSingleEvent> = logs.iter()
                .filter_map(|log| {
                    match self.decode_transfer_single_event(log) {
                        Ok(event) => Some(event),
                        Err(e) => {
                            error!("Failed to decode transfer single event: {}", e);
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
                        error!("Failed to downcast TransferSingleEvent to requested type, skipping event");
                        // Không thêm event này vào kết quả
                    }
                }
            }
            
            Ok(result)
        } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<TransferBatchEvent>() {
            let events: Vec<TransferBatchEvent> = logs.iter()
                .filter_map(|log| {
                    match self.decode_transfer_batch_event(log) {
                        Ok(event) => Some(event),
                        Err(e) => {
                            error!("Failed to decode transfer batch event: {}", e);
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
                        error!("Failed to downcast TransferBatchEvent to requested type, skipping event");
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
        } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<UriEvent>() {
            let events: Vec<UriEvent> = logs.iter()
                .filter_map(|log| {
                    match self.decode_uri_event(log) {
                        Ok(event) => Some(event),
                        Err(e) => {
                            error!("Failed to decode URI event: {}", e);
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
                        error!("Failed to downcast UriEvent to requested type, skipping event");
                        // Không thêm event này vào kết quả
                    }
                }
            }
            
            Ok(result)
        } else {
            Err(ContractError::CallError(format!(
                "Unsupported event type for decode_logs: {:?}",
                std::any::type_name::<T>()
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
            _ => Ok(false),
        }
    }
}

/// Builder cho ERC1155Contract
pub struct Erc1155ContractBuilder {
    address: Option<Address>,
    chain_id: Option<u64>,
    chain_type: Option<ChainType>,
    name: Option<String>,
    abi_json: Option<String>,
    verified: bool,
    wallet: Option<LocalWallet>,
}

impl Erc1155ContractBuilder {
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
    pub async fn build(self) -> Result<Erc1155Contract, ContractError> {
        let address = self.address.ok_or_else(|| {
            ContractError::CallError("Address is required".into())
        })?;
        
        let chain_id = self.chain_id.ok_or_else(|| {
            ContractError::CallError("Chain ID is required".into())
        })?;
        
        let name = self.name.unwrap_or_else(|| "ERC1155 Collection".to_string());
        
        let mut metadata = ContractMetadata::new(
            name,
            address,
            chain_id,
            ContractType::ERC1155,
        );
        
        metadata.chain_type = self.chain_type.unwrap_or(ChainType::EVM);
        metadata.verified = self.verified;
        
        if let Some(abi_json) = self.abi_json {
            metadata.abi_json = Some(abi_json);
        }
        
        let mut contract = Erc1155Contract::new(metadata).await?;
        
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
    fn test_erc1155_contract_builder() {
        let builder = Erc1155ContractBuilder::new()
            .with_address(Address::zero())
            .with_chain_id(1)
            .with_name("Test Multi-token Collection".to_string())
            .with_verified(true);
        
        assert!(builder.address.is_some());
        assert_eq!(builder.address.unwrap(), Address::zero());
        assert!(builder.chain_id.is_some());
        assert_eq!(builder.chain_id.unwrap(), 1);
        assert!(builder.name.is_some());
        assert_eq!(builder.name.unwrap(), "Test Multi-token Collection");
        assert!(builder.verified);
    }
} 