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
//! use wallet::defi::erc20::{Erc20Contract, Erc20ContractBuilder};
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

use crate::defi::contracts::{ContractInterface, ContractError, ContractType, ContractMetadata};
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
        // Kiểm tra contract_type
        if metadata.contract_type != ContractType::ERC20 {
            return Err(ContractError::UnsupportedContract(
                format!("Expected ERC20 contract, got {:?}", metadata.contract_type)
            ));
        }
        
        // Lấy provider cho chain
        let provider_result = get_provider(metadata.chain_id);
        let provider = match provider_result {
            Ok(p) => p,
            Err(e) => return Err(ContractError::ConnectionError(format!("Failed to get provider: {}", e))),
        };
        
        // Lấy ABI
        let abi_json = metadata.abi_json.clone().unwrap_or_else(|| ERC20_ABI.to_string());
        
        // Tạo contract
        let contract = Contract::new(
            metadata.address,
            abi_json.parse().map_err(|e| ContractError::AbiError(format!("Failed to parse ABI: {}", e)))?,
            provider.clone(),
        );
        
        Ok(Self {
            metadata,
            contract,
            token_info: None,
            provider,
            wallet: None,
        })
    }
    
    /// Thêm wallet để ký giao dịch
    pub fn with_wallet(mut self, wallet: LocalWallet) -> Self {
        self.wallet = Some(wallet);
        self
    }
    
    /// Lấy thông tin token (cache)
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
    
    /// Lấy balance của address
    pub async fn balance_of(&self, address: &str) -> Result<U256, ContractError> {
        info!("Getting balance for address: {}", address);
        let address = self.provider.validate_address(address)
            .map_err(|e| ContractError::CallError(format!("Invalid address: {}", e)))?;
            
        let balance = self.contract
            .balance_of(address)
            .call()
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to get balance: {}", e)))?;
            
        info!("Balance for address {}: {}", address, balance);
        Ok(balance)
    }
    
    /// Lấy allowance
    pub async fn allowance(&self, owner: &str, spender: &str) -> Result<U256, ContractError> {
        info!("Getting allowance for owner: {} and spender: {}", owner, spender);
        let owner = self.provider.validate_address(owner)
            .map_err(|e| ContractError::CallError(format!("Invalid owner address: {}", e)))?;
        let spender = self.provider.validate_address(spender)
            .map_err(|e| ContractError::CallError(format!("Invalid spender address: {}", e)))?;
            
        let allowance = self.contract
            .allowance(owner, spender)
            .call()
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to get allowance: {}", e)))?;
            
        info!("Allowance for owner {} and spender {}: {}", owner, spender, allowance);
        Ok(allowance)
    }
    
    /// Transfer tokens
    pub async fn transfer(&self, to: &str, amount: U256) -> Result<TransactionReceipt, ContractError> {
        info!("Transferring {} tokens to {}", amount, to);
        let to = self.provider.validate_address(to)
            .map_err(|e| ContractError::CallError(format!("Invalid recipient address: {}", e)))?;
            
        let wallet = self.wallet.as_ref()
            .ok_or_else(|| ContractError::CallError("No wallet available for signing".to_string()))?;
            
        let receipt = self.contract
            .transfer(to, amount)
            .send()
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to transfer tokens: {}", e)))?;
            
        info!("Transfer successful. Transaction hash: {}", receipt.transaction_hash);
        Ok(receipt)
    }
    
    /// Approve tokens
    pub async fn approve(&self, spender: Address, amount: U256) -> Result<TransactionReceipt, ContractError> {
        self.send_transaction("approve", vec![
            Token::Address(spender),
            Token::Uint(amount),
        ], None).await
    }
    
    /// Transfer from tokens
    pub async fn transfer_from(
        &self, 
        from: Address, 
        to: Address, 
        amount: U256
    ) -> Result<TransactionReceipt, ContractError> {
        self.send_transaction("transferFrom", vec![
            Token::Address(from),
            Token::Address(to),
            Token::Uint(amount),
        ], None).await
    }
    
    /// Decode transfer event
    pub fn decode_transfer_event(&self, log: &Log) -> Result<TransferEvent, ContractError> {
        // Kiểm tra log topics
        if log.topics.len() < 3 {
            return Err(ContractError::CallError("Invalid log format for Transfer event".into()));
        }
        
        // Kiểm tra event signature
        let transfer_signature = H256::from(ethers::utils::keccak256("Transfer(address,address,uint256)"));
        if log.topics[0] != transfer_signature {
            return Err(ContractError::CallError("Log is not a Transfer event".into()));
        }
        
        // Parse addresses
        let from = Address::from(log.topics[1]);
        let to = Address::from(log.topics[2]);
        
        // Parse amount
        let value = if log.data.0.len() >= 32 {
            U256::from_big_endian(&log.data.0[0..32])
        } else {
            return Err(ContractError::CallError("Invalid data length for Transfer event".into()));
        };
        
        Ok(TransferEvent { from, to, value })
    }
    
    /// Decode approval event
    pub fn decode_approval_event(&self, log: &Log) -> Result<ApprovalEvent, ContractError> {
        // Kiểm tra log topics
        if log.topics.len() < 3 {
            return Err(ContractError::CallError("Invalid log format for Approval event".into()));
        }
        
        // Kiểm tra event signature
        let approval_signature = H256::from(ethers::utils::keccak256("Approval(address,address,uint256)"));
        if log.topics[0] != approval_signature {
            return Err(ContractError::CallError("Log is not an Approval event".into()));
        }
        
        // Parse addresses
        let owner = Address::from(log.topics[1]);
        let spender = Address::from(log.topics[2]);
        
        // Parse amount
        let value = if log.data.0.len() >= 32 {
            U256::from_big_endian(&log.data.0[0..32])
        } else {
            return Err(ContractError::CallError("Invalid data length for Approval event".into()));
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
                // Nhiều ERC20 có hàm owner() để kiểm tra
                match self.contract.method::<_, Address>("owner", ()).call().await {
                    Ok(owner) => Ok(owner == address),
                    Err(_) => {
                        // Thử với hàm getOwner()
                        match self.contract.method::<_, Address>("getOwner", ()).call().await {
                            Ok(owner) => Ok(owner == address),
                            Err(_) => {
                                // Không tìm thấy hàm owner, trả về false
                                Ok(false)
                            }
                        }
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
            "pauser" => {
                // Kiểm tra quyền pause
                match self.contract.method::<_, bool>("isPauser", [address]).call().await {
                    Ok(is_pauser) => Ok(is_pauser),
                    Err(_) => {
                        // Không tìm thấy hàm isPauser, trả về false
                        Ok(false)
                    }
                }
            },
            _ => Ok(false),
        }
    }
}

/// Builder cho ERC20Contract
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
    pub async fn build(self) -> Result<Erc20Contract, ContractError> {
        let address = self.address.ok_or_else(|| {
            ContractError::CallError("Address is required".into())
        })?;
        
        let chain_id = self.chain_id.ok_or_else(|| {
            ContractError::CallError("Chain ID is required".into())
        })?;
        
        let name = self.name.unwrap_or_else(|| "ERC20 Token".to_string());
        
        let mut metadata = ContractMetadata::new(
            name,
            address,
            chain_id,
            ContractType::ERC20,
        );
        
        metadata.chain_type = self.chain_type.unwrap_or(ChainType::EVM);
        metadata.verified = self.verified;
        
        if let Some(abi_json) = self.abi_json {
            metadata.abi_json = Some(abi_json);
        }
        
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
    use crate::walletlogic::WalletProvider;
    use mockall::mock;
    use mockall::predicate::*;
    use std::sync::Arc;

    // Tạo mock cho WalletProvider để test độc lập
    mock! {
        WalletMock {}
        impl WalletProvider for WalletMock {
            fn get_address(&self) -> Result<Address, Box<dyn std::error::Error>>;
            fn get_chain_id(&self) -> Result<u64, Box<dyn std::error::Error>>;
            fn get_balance(&self) -> Result<U256, Box<dyn std::error::Error>>;
            fn send_transaction(&self, tx: TransactionRequest) -> Result<H256, Box<dyn std::error::Error>>;
            fn sign_message(&self, message: &[u8]) -> Result<Signature, Box<dyn std::error::Error>>;
        }
    }

    #[tokio::test]
    async fn test_erc20_contract_builder() {
        // Arrange
        let address = Address::random();
        let chain_id = 1;
        let name = "Test Token";
        let abi_json = r#"[{"constant":true,"inputs":[],"name":"name","outputs":[{"name":"","type":"string"}],"payable":false,"stateMutability":"view","type":"function"}]"#;
        
        // Act
        let builder = Erc20ContractBuilder::new()
            .address(address)
            .chain_id(chain_id)
            .name(name)
            .verified(true)
            .abi_json(abi_json);
            
        let contract = builder.build();
        
        // Assert
        assert_eq!(contract.address, address);
        assert_eq!(contract.chain_id, chain_id);
        assert_eq!(contract.name, name);
        assert_eq!(contract.verified, true);
        assert_eq!(contract.abi_json, abi_json);
    }
    
    #[tokio::test]
    async fn test_erc20_contract_builder_default_values() {
        // Arrange
        let address = Address::random();
        
        // Act
        let builder = Erc20ContractBuilder::new()
            .address(address);
            
        let contract = builder.build();
        
        // Assert
        assert_eq!(contract.address, address);
        assert_eq!(contract.chain_id, 0); // Default value
        assert_eq!(contract.name, ""); // Default value
        assert_eq!(contract.verified, false); // Default value
        assert_eq!(contract.abi_json, ""); // Default value
    }
    
    #[tokio::test]
    async fn test_erc20_contract_methods() {
        // Arrange
        let mut mock_wallet = MockWalletMock::new();
        let wallet_address = Address::random();
        let token_address = Address::random();
        
        // Setup mock
        mock_wallet
            .expect_get_address()
            .returning(move || Ok(wallet_address));
            
        mock_wallet
            .expect_get_chain_id()
            .returning(|| Ok(1));
            
        // Create contract
        let wallet = Arc::new(mock_wallet) as Arc<dyn WalletProvider>;
        let contract = Erc20ContractBuilder::new()
            .address(token_address)
            .chain_id(1)
            .name("Test Token")
            .verified(true)
            .wallet(wallet.clone())
            .build();
            
        // Act & Assert - should not panic
        assert_eq!(contract.address, token_address);
        assert_eq!(contract.chain_id, 1);
    }
    
    #[tokio::test]
    async fn test_erc20_contract_integration() {
        // Skip trong môi trường test tự động
        if std::env::var("SKIP_INTEGRATION_TESTS").is_ok() {
            return;
        }
        
        // Integration test chỉ chạy khi có biến môi trường TEST_RPC_URL
        let rpc_url = match std::env::var("TEST_RPC_URL") {
            Ok(url) => url,
            Err(_) => return,
        };
        
        // Chuẩn bị provider
        let provider = Provider::<Http>::try_from(rpc_url).expect("could not instantiate HTTP Provider");
        
        // Địa chỉ của USDT token trên mainnet
        let usdt_address = "0xdAC17F958D2ee523a2206206994597C13D831ec7".parse::<Address>().unwrap();
        
        // Tạo contract
        let contract = Erc20ContractBuilder::new()
            .address(usdt_address)
            .chain_id(1)
            .name("USDT")
            .verified(true)
            .build();
            
        // Gọi các phương thức read-only trong môi trường thực
        // Lưu ý: Chỉ test các phương thức không thay đổi state
        let client = Arc::new(provider);
        
        // Tạm thời bỏ qua error nếu không kết nối được
        // trong môi trường test tự động
        let _ = contract.name(client.clone()).await;
        let _ = contract.symbol(client.clone()).await;
        let _ = contract.decimals(client.clone()).await;
        let _ = contract.total_supply(client.clone()).await;
    }
} 