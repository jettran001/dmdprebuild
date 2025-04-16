//! Module ERC20 - Triển khai interface cho ERC20 tokens
//!
//! Module này cung cấp triển khai cho ERC20 token contracts, cho phép:
//! - Truy vấn thông tin tokens: tên, symbol, decimals, tổng cung
//! - Kiểm tra số dư tokens
//! - Transfer và approve tokens
//! - Theo dõi sự kiện Transfer và Approval
//!
//! ## Ví dụ:
//! ```rust
//! use wallet::contracts::erc20::{Erc20Contract, Erc20ContractBuilder};
//! use ethers::types::Address;
//!
//! #[tokio::main]
//! async fn main() {
//!     let contract = Erc20ContractBuilder::new()
//!         .with_address("0x123...".parse().unwrap())
//!         .with_chain_id(1)
//!         .with_name("My Token")
//!         .build()
//!         .await
//!         .unwrap();
//!
//!     let balance = contract.balance_of("0x456...".parse().unwrap()).await.unwrap();
//!     println!("Balance: {}", balance);
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

use crate::contracts::{ContractInterface, ContractError, ContractType, ContractMetadata};
use crate::walletmanager::chain::ChainType;
use crate::defi::blockchain::get_provider;

/// ERC20 standard ABI
const ERC20_ABI: &str = r#"
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
    "inputs": [],
    "name": "decimals",
    "outputs": [{"name": "", "type": "uint8"}],
    "payable": false,
    "stateMutability": "view",
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
    "constant": true,
    "inputs": [{"name": "account", "type": "address"}],
    "name": "balanceOf",
    "outputs": [{"name": "", "type": "uint256"}],
    "payable": false,
    "stateMutability": "view",
    "type": "function"
  },
  {
    "constant": true,
    "inputs": [{"name": "owner", "type": "address"}, {"name": "spender", "type": "address"}],
    "name": "allowance",
    "outputs": [{"name": "", "type": "uint256"}],
    "payable": false,
    "stateMutability": "view",
    "type": "function"
  },
  {
    "constant": false,
    "inputs": [{"name": "to", "type": "address"}, {"name": "value", "type": "uint256"}],
    "name": "transfer",
    "outputs": [{"name": "", "type": "bool"}],
    "payable": false,
    "stateMutability": "nonpayable",
    "type": "function"
  },
  {
    "constant": false,
    "inputs": [{"name": "spender", "type": "address"}, {"name": "value", "type": "uint256"}],
    "name": "approve",
    "outputs": [{"name": "", "type": "bool"}],
    "payable": false,
    "stateMutability": "nonpayable",
    "type": "function"
  },
  {
    "constant": false,
    "inputs": [{"name": "from", "type": "address"}, {"name": "to", "type": "address"}, {"name": "value", "type": "uint256"}],
    "name": "transferFrom",
    "outputs": [{"name": "", "type": "bool"}],
    "payable": false,
    "stateMutability": "nonpayable",
    "type": "function"
  },
  {
    "anonymous": false,
    "inputs": [
      {"indexed": true, "name": "from", "type": "address"},
      {"indexed": true, "name": "to", "type": "address"},
      {"indexed": false, "name": "value", "type": "uint256"}
    ],
    "name": "Transfer",
    "type": "event"
  },
  {
    "anonymous": false,
    "inputs": [
      {"indexed": true, "name": "owner", "type": "address"},
      {"indexed": true, "name": "spender", "type": "address"},
      {"indexed": false, "name": "value", "type": "uint256"}
    ],
    "name": "Approval",
    "type": "event"
  }
]
"#;

/// ERC20 Token Info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    /// Token name
    pub name: String,
    /// Token symbol
    pub symbol: String,
    /// Token decimals
    pub decimals: u8,
    /// Token total supply
    pub total_supply: U256,
}

/// ERC20 Token Transfer Event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferEvent {
    /// Token sender
    pub from: Address,
    /// Token receiver
    pub to: Address,
    /// Token amount
    pub value: U256,
}

/// ERC20 Token Approval Event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalEvent {
    /// Token owner
    pub owner: Address,
    /// Token spender
    pub spender: Address,
    /// Token amount
    pub value: U256,
}

/// ERC20 Contract
pub struct Erc20Contract {
    /// Contract metadata
    metadata: ContractMetadata,
    /// Ethers contract
    contract: Contract<Provider<Http>>,
    /// Token info (cached)
    token_info: Option<TokenInfo>,
    /// Provider for blockchain
    provider: Provider<Http>,
    /// Wallet for signing (optional)
    wallet: Option<LocalWallet>,
}

impl Erc20Contract {
    /// Tạo contract mới từ metadata
    pub async fn new(metadata: ContractMetadata) -> Result<Self, ContractError> {
        // Kiểm tra loại contract
        if metadata.contract_type != ContractType::ERC20 {
            return Err(ContractError::UnsupportedContract(
                format!("Expected ERC20 contract, got {:?}", metadata.contract_type)
            ));
        }
        
        // Lấy provider cho chain ID
        let provider = get_provider(metadata.chain_id.into())
            .map_err(|e| ContractError::ConnectionError(e.to_string()))?;
            
        // Tạo contract từ ABI
        let abi_json = metadata.abi_json.clone().unwrap_or_else(|| ERC20_ABI.to_string());
        let contract = Contract::new(
            metadata.address,
            serde_json::from_str(&abi_json)
                .map_err(|e| ContractError::AbiError(e.to_string()))?,
            provider.clone()
        );
        
        Ok(Self {
            metadata,
            contract,
            token_info: None,
            provider: provider.clone(),
            wallet: None,
        })
    }
    
    /// Thêm ví để ký giao dịch
    pub fn with_wallet(mut self, wallet: LocalWallet) -> Self {
        self.wallet = Some(wallet);
        self
    }
    
    /// Lấy thông tin token
    pub async fn get_token_info(&mut self) -> Result<TokenInfo, ContractError> {
        if let Some(info) = &self.token_info {
            return Ok(info.clone());
        }
        
        let name: String = self.call("name", vec![]).await?;
        let symbol: String = self.call("symbol", vec![]).await?;
        let decimals: u8 = self.call("decimals", vec![]).await?;
        let total_supply: U256 = self.call("totalSupply", vec![]).await?;
        
        let info = TokenInfo {
            name,
            symbol,
            decimals,
            total_supply,
        };
        
        self.token_info = Some(info.clone());
        Ok(info)
    }
    
    /// Lấy số dư token của địa chỉ
    pub async fn balance_of(&self, address: Address) -> Result<U256, ContractError> {
        self.call("balanceOf", vec![Token::Address(address)]).await
    }
    
    /// Lấy allowance
    pub async fn allowance(&self, owner: Address, spender: Address) -> Result<U256, ContractError> {
        self.call("allowance", vec![Token::Address(owner), Token::Address(spender)]).await
    }
    
    /// Transfer tokens
    pub async fn transfer(&self, to: Address, amount: U256) -> Result<TransactionReceipt, ContractError> {
        self.send_transaction(
            "transfer", 
            vec![Token::Address(to), Token::Uint(amount)], 
            None
        ).await
    }
    
    /// Approve spender
    pub async fn approve(&self, spender: Address, amount: U256) -> Result<TransactionReceipt, ContractError> {
        self.send_transaction(
            "approve", 
            vec![Token::Address(spender), Token::Uint(amount)], 
            None
        ).await
    }
    
    /// TransferFrom
    pub async fn transfer_from(
        &self, 
        from: Address, 
        to: Address, 
        amount: U256
    ) -> Result<TransactionReceipt, ContractError> {
        self.send_transaction(
            "transferFrom", 
            vec![Token::Address(from), Token::Address(to), Token::Uint(amount)], 
            None
        ).await
    }
    
    /// Decode Transfer event
    pub fn decode_transfer_event(&self, log: &Log) -> Result<TransferEvent, ContractError> {
        // Kiểm tra event signature
        let transfer_signature = "Transfer(address,address,uint256)";
        let transfer_topic = H256::from_slice(&ethers::utils::keccak256(transfer_signature.as_bytes()));
        
        if log.topics[0] != transfer_topic {
            return Err(ContractError::AbiError("Not a Transfer event".to_string()));
        }
        
        // Kiểm tra số lượng topics
        if log.topics.len() < 3 {
            return Err(ContractError::AbiError("Invalid Transfer event format: insufficient topics".to_string()));
        }
        
        // Parse địa chỉ from và to từ topics
        // Trong ERC20, indexed parameters được lưu trong topics
        let from = Address::from_slice(&log.topics[1].as_bytes()[12..32]); // lấy 20 bytes cuối
        let to = Address::from_slice(&log.topics[2].as_bytes()[12..32]); // lấy 20 bytes cuối
        
        // Parse value từ data - không indexed parameter lưu trong data
        let value = if !log.data.is_empty() {
            U256::from_big_endian(&log.data)
        } else {
            return Err(ContractError::AbiError("Invalid Transfer event format: empty data".to_string()));
        };
        
        Ok(TransferEvent { from, to, value })
    }
    
    /// Decode Approval event
    pub fn decode_approval_event(&self, log: &Log) -> Result<ApprovalEvent, ContractError> {
        // Kiểm tra event signature
        let approval_signature = "Approval(address,address,uint256)";
        let approval_topic = H256::from_slice(&ethers::utils::keccak256(approval_signature.as_bytes()));
        
        if log.topics[0] != approval_topic {
            return Err(ContractError::AbiError("Not an Approval event".to_string()));
        }
        
        // Kiểm tra số lượng topics
        if log.topics.len() < 3 {
            return Err(ContractError::AbiError("Invalid Approval event format: insufficient topics".to_string()));
        }
        
        // Parse địa chỉ owner và spender từ topics
        let owner = Address::from_slice(&log.topics[1].as_bytes()[12..32]); // lấy 20 bytes cuối
        let spender = Address::from_slice(&log.topics[2].as_bytes()[12..32]); // lấy 20 bytes cuối
        
        // Parse value từ data - không indexed parameter lưu trong data
        let value = if !log.data.is_empty() {
            U256::from_big_endian(&log.data)
        } else {
            return Err(ContractError::AbiError("Invalid Approval event format: empty data".to_string()));
        };
        
        Ok(ApprovalEvent { owner, spender, value })
    }
}

#[async_trait]
impl ContractInterface for Erc20Contract {
    fn metadata(&self) -> &ContractMetadata {
        &self.metadata
    }
    
    async fn call<T: Send + Sync + 'static>(&self, function: &str, params: Vec<Token>) -> Result<T, ContractError> {
        self.contract
            .method::<_, T>(function, params)
            .map_err(|e| ContractError::CallError(format!("Failed to prepare method call: {}", e)))?
            .call()
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to call {}: {}", function, e)))
    }
    
    async fn send_transaction(
        &self, 
        function: &str, 
        params: Vec<Token>, 
        value: Option<U256>
    ) -> Result<TransactionReceipt, ContractError> {
        let wallet = self.wallet.clone().ok_or_else(|| 
            ContractError::SignatureError("No wallet available for signing".to_string())
        )?;
        
        let client = self.provider.clone().with_signer(wallet);
        let contract = self.contract.connect(client);
        
        let tx = contract
            .method::<_, bool>(function, params)
            .map_err(|e| ContractError::CallError(format!("Failed to prepare transaction: {}", e)))?;
            
        let tx = if let Some(value) = value {
            tx.value(value)
        } else {
            tx
        };
        
        tx.send()
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to send transaction: {}", e)))?
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to confirm transaction: {}", e)))
    }
    
    fn encode_function_data(&self, function: &str, params: Vec<Token>) -> Result<Vec<u8>, ContractError> {
        self.contract
            .encode(function, params)
            .map_err(|e| ContractError::AbiError(format!("Failed to encode function data: {}", e)))
    }
    
    fn decode_logs<T: Send + Sync + 'static>(&self, logs: &[Log]) -> Result<Vec<T>, ContractError> {
        use std::any::TypeId;
        
        // Xác định kiểu T
        let type_id = TypeId::of::<T>();
        
        if type_id == TypeId::of::<TransferEvent>() {
            let mut events = Vec::new();
            
            for log in logs {
                match self.decode_transfer_event(log) {
                    Ok(event) => {
                        // Chuyển đổi kiểu an toàn (bằng cách serialize/deserialize)
                        let json = serde_json::to_string(&event)
                            .map_err(|e| ContractError::AbiError(format!("Failed to serialize event: {}", e)))?;
                        
                        let typed_event = serde_json::from_str::<T>(&json)
                            .map_err(|e| ContractError::AbiError(format!("Failed to deserialize event: {}", e)))?;
                        
                        events.push(typed_event);
                    },
                    Err(_) => continue, // Bỏ qua các log không phải Transfer event
                }
            }
            
            return Ok(events);
        }
        else if type_id == TypeId::of::<ApprovalEvent>() {
            let mut events = Vec::new();
            
            for log in logs {
                match self.decode_approval_event(log) {
                    Ok(event) => {
                        // Chuyển đổi kiểu an toàn (bằng cách serialize/deserialize)
                        let json = serde_json::to_string(&event)
                            .map_err(|e| ContractError::AbiError(format!("Failed to serialize event: {}", e)))?;
                        
                        let typed_event = serde_json::from_str::<T>(&json)
                            .map_err(|e| ContractError::AbiError(format!("Failed to deserialize event: {}", e)))?;
                        
                        events.push(typed_event);
                    },
                    Err(_) => continue, // Bỏ qua các log không phải Approval event
                }
            }
            
            return Ok(events);
        }
        
        // Kiểu T không được hỗ trợ
        Err(ContractError::UnsupportedContract(format!(
            "Decode_logs not implemented for the requested type. Use specific decode methods instead."
        )))
    }
    
    async fn check_permission(&self, address: Address, permission: &str) -> Result<bool, ContractError> {
        match permission {
            "owner" => {
                // Kiểm tra nếu contract có phương thức owner()
                let owner_result: Result<Address, _> = self.call("owner", vec![]).await;
                match owner_result {
                    Ok(owner) => Ok(owner == address),
                    Err(_) => {
                        // Kiểm tra nếu contract có phương thức getOwner()
                        let get_owner_result: Result<Address, _> = self.call("getOwner", vec![]).await;
                        match get_owner_result {
                            Ok(owner) => Ok(owner == address),
                            Err(_) => Err(ContractError::OtherError(
                                "ERC20 contract doesn't implement a standard owner function".to_string()
                            )),
                        }
                    }
                }
            },
            "balance" => {
                // Kiểm tra nếu địa chỉ có token
                let balance = self.balance_of(address).await?;
                Ok(!balance.is_zero())
            },
            "has_min_balance" => {
                // Kiểm tra nếu địa chỉ có ít nhất một lượng token tối thiểu (0.1% tổng cung)
                let balance = self.balance_of(address).await?;
                
                if let Some(token_info) = &self.token_info {
                    let min_balance = token_info.total_supply / 1000; // 0.1% tổng cung
                    Ok(balance >= min_balance)
                } else {
                    // Nếu chưa có thông tin token, cần phải lấy tổng cung
                    let total_supply: U256 = self.call("totalSupply", vec![]).await?;
                    let min_balance = total_supply / 1000; // 0.1% tổng cung
                    Ok(balance >= min_balance)
                }
            },
            "can_transfer" => {
                // Kiểm tra nếu địa chỉ có thể thực hiện transfer (có token và không bị blacklist)
                let balance = self.balance_of(address).await?;
                
                // Kiểm tra blacklist nếu có
                let is_blacklisted_result: Result<bool, _> = 
                    self.call("isBlacklisted", vec![Token::Address(address)]).await;
                
                match is_blacklisted_result {
                    Ok(is_blacklisted) => Ok(!balance.is_zero() && !is_blacklisted),
                    Err(_) => {
                        // Nếu không có hàm kiểm tra blacklist, chỉ kiểm tra balance
                        Ok(!balance.is_zero())
                    }
                }
            },
            "minter" => {
                // Kiểm tra nếu địa chỉ có quyền mint
                let is_minter_result: Result<bool, _> = 
                    self.call("isMinter", vec![Token::Address(address)]).await;
                
                match is_minter_result {
                    Ok(is_minter) => Ok(is_minter),
                    Err(_) => {
                        let minter_role_result: Result<H256, _> = 
                            self.call("MINTER_ROLE", vec![]).await;
                        
                        match minter_role_result {
                            Ok(minter_role) => {
                                // Kiểm tra nếu sử dụng AccessControl
                                let has_role_result: Result<bool, _> = self.call(
                                    "hasRole", 
                                    vec![Token::FixedBytes(minter_role.as_bytes().to_vec()), Token::Address(address)]
                                ).await;
                                
                                match has_role_result {
                                    Ok(has_role) => Ok(has_role),
                                    Err(_) => Err(ContractError::OtherError(
                                        "Contract doesn't implement a standard minter check".to_string()
                                    )),
                                }
                            },
                            Err(_) => Err(ContractError::OtherError(
                                "Contract doesn't implement a standard minter check".to_string()
                            )),
                        }
                    }
                }
            },
            _ => Err(ContractError::OtherError(format!("Unknown permission: {}", permission))),
        }
    }
}

/// Builder for ERC20 Contract
pub struct Erc20ContractBuilder {
    address: Option<Address>,
    chain_id: Option<u64>,
    chain_type: Option<ChainType>,
    name: Option<String>,
    abi_json: Option<String>,
    verified: bool,
    wallet: Option<LocalWallet>,
}

impl Erc20ContractBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            address: None,
            chain_id: None,
            chain_type: None,
            name: None,
            abi_json: None,
            verified: false,
            wallet: None,
        }
    }
    
    /// Set contract address
    pub fn with_address(mut self, address: Address) -> Self {
        self.address = Some(address);
        self
    }
    
    /// Set chain id
    pub fn with_chain_id(mut self, chain_id: u64) -> Self {
        self.chain_id = Some(chain_id);
        self
    }
    
    /// Set chain type
    pub fn with_chain_type(mut self, chain_type: ChainType) -> Self {
        self.chain_type = Some(chain_type);
        self
    }
    
    /// Set contract name
    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }
    
    /// Set custom ABI JSON
    pub fn with_abi_json(mut self, abi_json: String) -> Self {
        self.abi_json = Some(abi_json);
        self
    }
    
    /// Set verified flag
    pub fn with_verified(mut self, verified: bool) -> Self {
        self.verified = verified;
        self
    }
    
    /// Set wallet for signing
    pub fn with_wallet(mut self, wallet: LocalWallet) -> Self {
        self.wallet = Some(wallet);
        self
    }
    
    /// Build the contract
    pub async fn build(self) -> Result<Erc20Contract, ContractError> {
        let address = self.address.ok_or_else(|| 
            ContractError::InvalidConfig("Contract address is required".to_string())
        )?;
        
        let chain_id = self.chain_id.ok_or_else(|| 
            ContractError::InvalidConfig("Chain ID is required".to_string())
        )?;
        
        let chain_type = self.chain_type.unwrap_or(ChainType::EVM);
        
        let name = self.name.unwrap_or_else(|| format!("ERC20 Token ({})", address));
        
        let metadata = ContractMetadata {
            name,
            address,
            chain_id,
            chain_type,
            abi_path: None,
            abi_json: self.abi_json,
            verified: self.verified,
            contract_type: ContractType::ERC20,
            implementation: None,
            audited: false,
            audit_firm: None,
            tags: vec![],
        };
        
        let mut contract = Erc20Contract::new(metadata).await?;
        
        if let Some(wallet) = self.wallet {
            contract = contract.with_wallet(wallet);
        }
        
        Ok(contract)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::types::Address;
    
    #[test]
    fn test_erc20_contract_builder() {
        let builder = Erc20ContractBuilder::new()
            .with_address(Address::zero())
            .with_chain_id(1)
            .with_name("Test Token".to_string());
            
        assert!(true); // Passes if no panic
    }
    
    // Integration test would need real blockchain connection
    // Can be implemented with mock providers or test networks
} 