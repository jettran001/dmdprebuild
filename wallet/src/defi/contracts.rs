// Contracts module - Quản lý kết nối và tương tác với các smart contract
//
// Module này chứa các định nghĩa và triển khai các contract cần thiết cho module DeFi.
// Nó cung cấp một lớp trừu tượng để tương tác với các smart contract trên các blockchain khác nhau.

use std::sync::Arc;
use async_trait::async_trait;
use ethers::{
    providers::{Middleware, Provider, Http},
    contract::{Contract, ContractError},
    types::{Address, U256, H160, TransactionReceipt},
    signers::{Signer, LocalWallet},
};
use thiserror::Error;
use serde::{Serialize, Deserialize};
use tokio::sync::RwLock;
use std::collections::HashMap;
use crate::defi::error::DefiError;
use crate::defi::constants::{
    ROUTER_CONTRACT_ADDRESS, FARM_CONTRACT_ADDRESS, 
    DMD_TOKEN_ADDRESS, CONTRACT_CALL_TIMEOUT_MS
};

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
}

/// Metadata của contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractMetadata {
    pub name: String,
    pub address: Address,
    pub chain_id: u64,
    pub abi_path: String,
    pub verified: bool,
}

/// Enum định nghĩa các loại contract
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ContractType {
    Router,
    Farm,
    Token,
    Staking,
    NFT,
}

/// Trait định nghĩa các phương thức chung cho tất cả các contract
#[async_trait]
pub trait ContractInterface: Send + Sync {
    /// Lấy địa chỉ của contract
    fn address(&self) -> Address;
    
    /// Lấy tên của contract
    fn name(&self) -> String;
    
    /// Gọi một hàm read-only trên contract
    async fn call<T: Send + Sync>(&self, function: &str, params: Vec<T>) -> Result<T, ContractError>;
    
    /// Gửi một giao dịch đến contract
    async fn send_transaction(&self, function: &str, params: Vec<Box<dyn std::any::Any + Send + Sync>>) 
        -> Result<TransactionReceipt, ContractError>;
    
    /// Kiểm tra xem contract có được xác minh hay không
    async fn is_verified(&self) -> bool;
}

/// Manager quản lý tất cả các contract
pub struct ContractManager {
    contracts: RwLock<HashMap<ContractType, Arc<dyn ContractInterface>>>,
    providers: RwLock<HashMap<u64, Arc<Provider<Http>>>>,
    chain_ids: Vec<u64>,
    wallet: Option<LocalWallet>,
}

impl ContractManager {
    /// Tạo một instance mới của ContractManager
    pub fn new(chain_ids: Vec<u64>, wallet: Option<LocalWallet>) -> Self {
        Self {
            contracts: RwLock::new(HashMap::new()),
            providers: RwLock::new(HashMap::new()),
            chain_ids,
            wallet,
        }
    }
    
    /// Thêm một provider mới cho một chain
    pub async fn add_provider(&self, chain_id: u64, rpc_url: &str) -> Result<(), ContractError> {
        let provider = Provider::<Http>::try_from(rpc_url)
            .map_err(|e| ContractError::ConnectionError(e.to_string()))?;
        
        let mut providers = self.providers.write().await;
        providers.insert(chain_id, Arc::new(provider));
        
        Ok(())
    }
    
    /// Lấy một provider cho một chain
    pub async fn get_provider(&self, chain_id: u64) -> Result<Arc<Provider<Http>>, ContractError> {
        let providers = self.providers.read().await;
        providers.get(&chain_id)
            .cloned()
            .ok_or_else(|| ContractError::UnsupportedChain(format!("Chain ID {} không được hỗ trợ", chain_id)))
    }
    
    /// Đăng ký một contract mới
    pub async fn register_contract(
        &self,
        contract_type: ContractType,
        metadata: ContractMetadata,
    ) -> Result<(), ContractError> {
        let provider = self.get_provider(metadata.chain_id).await?;
        
        let contract_impl = match contract_type {
            ContractType::Router => {
                let router = RouterContract::new(metadata, provider.clone(), self.wallet.clone());
                Arc::new(router) as Arc<dyn ContractInterface>
            },
            ContractType::Farm => {
                let farm = FarmContract::new(metadata, provider.clone(), self.wallet.clone());
                Arc::new(farm) as Arc<dyn ContractInterface>
            },
            ContractType::Token => {
                let token = TokenContract::new(metadata, provider.clone(), self.wallet.clone());
                Arc::new(token) as Arc<dyn ContractInterface>
            },
            ContractType::Staking => {
                let staking = StakingContract::new(metadata, provider.clone(), self.wallet.clone());
                Arc::new(staking) as Arc<dyn ContractInterface>
            },
            ContractType::NFT => {
                return Err(ContractError::UnsupportedContract("NFT contracts not implemented yet".to_string()));
            }
        };
        
        let mut contracts = self.contracts.write().await;
        contracts.insert(contract_type, contract_impl);
        
        Ok(())
    }
    
    /// Lấy một contract theo loại
    pub async fn get_contract(&self, contract_type: ContractType) -> Result<Arc<dyn ContractInterface>, ContractError> {
        let contracts = self.contracts.read().await;
        contracts.get(&contract_type)
            .cloned()
            .ok_or_else(|| ContractError::UnsupportedContract(format!("{:?} contract không được tìm thấy", contract_type)))
    }
    
    /// Thiết lập wallet mới
    pub fn set_wallet(&mut self, wallet: LocalWallet) {
        self.wallet = Some(wallet);
    }
}

/// Triển khai cho Router Contract (UniswapV2Router02)
pub struct RouterContract {
    metadata: ContractMetadata,
    provider: Arc<Provider<Http>>,
    wallet: Option<LocalWallet>,
}

impl RouterContract {
    pub fn new(
        metadata: ContractMetadata,
        provider: Arc<Provider<Http>>,
        wallet: Option<LocalWallet>,
    ) -> Self {
        Self {
            metadata,
            provider,
            wallet,
        }
    }
    
    /// Thêm thanh khoản vào pool
    pub async fn add_liquidity(
        &self,
        token_a: Address,
        token_b: Address,
        amount_a_desired: U256,
        amount_b_desired: U256,
        amount_a_min: U256,
        amount_b_min: U256,
        to: Address,
        deadline: U256,
    ) -> Result<TransactionReceipt, ContractError> {
        // Triển khai logic gọi hàm addLiquidity trên router contract
        // Trong trường hợp thực tế, sẽ sử dụng ethers để gọi contract
        
        Err(ContractError::CallError("Not implemented yet".to_string()))
    }
    
    /// Rút thanh khoản khỏi pool
    pub async fn remove_liquidity(
        &self,
        token_a: Address,
        token_b: Address,
        liquidity: U256,
        amount_a_min: U256,
        amount_b_min: U256,
        to: Address,
        deadline: U256,
    ) -> Result<TransactionReceipt, ContractError> {
        // Triển khai logic gọi hàm removeLiquidity trên router contract
        
        Err(ContractError::CallError("Not implemented yet".to_string()))
    }
}

#[async_trait]
impl ContractInterface for RouterContract {
    fn address(&self) -> Address {
        self.metadata.address
    }
    
    fn name(&self) -> String {
        self.metadata.name.clone()
    }
    
    async fn call<T: Send + Sync>(&self, function: &str, params: Vec<T>) -> Result<T, ContractError> {
        // Tạo contract instance từ abi
        let abi_json = tokio::fs::read_to_string(&self.metadata.abi_path)
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to read ABI file: {}", e)))?;
        
        let contract = Contract::new(
            self.metadata.address,
            serde_json::from_str(&abi_json).unwrap(),
            self.provider.clone(),
        );
        
        // Tạo tham số cho contract call
        let params_any: Vec<Box<dyn std::any::Any>> = params
            .into_iter()
            .map(|p| Box::new(p) as Box<dyn std::any::Any>)
            .collect();
        
        // Gọi hàm với timeout
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(CONTRACT_CALL_TIMEOUT_MS),
            contract.method::<_, T>(function, params_any).call(),
        )
        .await
        .map_err(|_| ContractError::CallError("Contract call timeout".to_string()))?
        .map_err(|e| ContractError::CallError(format!("Contract call error: {}", e)))?;
        
        Ok(result)
    }
    
    async fn send_transaction(&self, function: &str, params: Vec<Box<dyn std::any::Any + Send + Sync>>) 
        -> Result<TransactionReceipt, ContractError> {
        let wallet = self.wallet.clone().ok_or_else(|| 
            ContractError::SignatureError("No wallet provided for transaction".to_string())
        )?;
        
        // Tạo contract instance từ abi
        let abi_json = tokio::fs::read_to_string(&self.metadata.abi_path)
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to read ABI file: {}", e)))?;
        
        let client = self.provider.clone().with_signer(wallet);
        
        let contract = Contract::new(
            self.metadata.address,
            serde_json::from_str(&abi_json).unwrap(),
            client,
        );
        
        // Gửi transaction với timeout
        let tx = tokio::time::timeout(
            std::time::Duration::from_millis(CONTRACT_CALL_TIMEOUT_MS),
            contract.method::<_, ()>(function, params).send(),
        )
        .await
        .map_err(|_| ContractError::CallError("Transaction sending timeout".to_string()))?
        .map_err(|e| ContractError::CallError(format!("Transaction error: {}", e)))?;
        
        // Chờ transaction được xác nhận
        let receipt = tx.await
            .map_err(|e| ContractError::CallError(format!("Failed to get transaction receipt: {}", e)))?;
        
        Ok(receipt)
    }
    
    async fn is_verified(&self) -> bool {
        self.metadata.verified
    }
}

/// Triển khai cho Farm Contract
pub struct FarmContract {
    metadata: ContractMetadata,
    provider: Arc<Provider<Http>>,
    wallet: Option<LocalWallet>,
}

impl FarmContract {
    pub fn new(
        metadata: ContractMetadata,
        provider: Arc<Provider<Http>>,
        wallet: Option<LocalWallet>,
    ) -> Self {
        Self {
            metadata,
            provider,
            wallet,
        }
    }
    
    /// Stake LP token
    pub async fn stake(&self, pool_id: u64, amount: U256) -> Result<TransactionReceipt, ContractError> {
        // Triển khai logic gọi hàm stake trên farm contract
        
        Err(ContractError::CallError("Not implemented yet".to_string()))
    }
    
    /// Unstake LP token
    pub async fn unstake(&self, pool_id: u64, amount: U256) -> Result<TransactionReceipt, ContractError> {
        // Triển khai logic gọi hàm unstake trên farm contract
        
        Err(ContractError::CallError("Not implemented yet".to_string()))
    }
    
    /// Harvest rewards
    pub async fn harvest(&self, pool_id: u64) -> Result<TransactionReceipt, ContractError> {
        // Triển khai logic gọi hàm harvest trên farm contract
        
        Err(ContractError::CallError("Not implemented yet".to_string()))
    }
    
    /// Lấy thông tin APY của pool
    pub async fn get_pool_apy(&self, pool_id: u64) -> Result<U256, ContractError> {
        // Triển khai logic gọi hàm getPoolApy trên farm contract
        
        Err(ContractError::CallError("Not implemented yet".to_string()))
    }
}

#[async_trait]
impl ContractInterface for FarmContract {
    fn address(&self) -> Address {
        self.metadata.address
    }
    
    fn name(&self) -> String {
        self.metadata.name.clone()
    }
    
    async fn call<T: Send + Sync>(&self, function: &str, params: Vec<T>) -> Result<T, ContractError> {
        // Tạo contract instance từ abi
        let abi_json = tokio::fs::read_to_string(&self.metadata.abi_path)
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to read ABI file: {}", e)))?;
        
        let contract = Contract::new(
            self.metadata.address,
            serde_json::from_str(&abi_json).unwrap(),
            self.provider.clone(),
        );
        
        // Tạo tham số cho contract call
        let params_any: Vec<Box<dyn std::any::Any>> = params
            .into_iter()
            .map(|p| Box::new(p) as Box<dyn std::any::Any>)
            .collect();
        
        // Gọi hàm với timeout
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(CONTRACT_CALL_TIMEOUT_MS),
            contract.method::<_, T>(function, params_any).call(),
        )
        .await
        .map_err(|_| ContractError::CallError("Contract call timeout".to_string()))?
        .map_err(|e| ContractError::CallError(format!("Contract call error: {}", e)))?;
        
        Ok(result)
    }
    
    async fn send_transaction(&self, function: &str, params: Vec<Box<dyn std::any::Any + Send + Sync>>) 
        -> Result<TransactionReceipt, ContractError> {
        let wallet = self.wallet.clone().ok_or_else(|| 
            ContractError::SignatureError("No wallet provided for transaction".to_string())
        )?;
        
        // Tạo contract instance từ abi
        let abi_json = tokio::fs::read_to_string(&self.metadata.abi_path)
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to read ABI file: {}", e)))?;
        
        let client = self.provider.clone().with_signer(wallet);
        
        let contract = Contract::new(
            self.metadata.address,
            serde_json::from_str(&abi_json).unwrap(),
            client,
        );
        
        // Gửi transaction với timeout
        let tx = tokio::time::timeout(
            std::time::Duration::from_millis(CONTRACT_CALL_TIMEOUT_MS),
            contract.method::<_, ()>(function, params).send(),
        )
        .await
        .map_err(|_| ContractError::CallError("Transaction sending timeout".to_string()))?
        .map_err(|e| ContractError::CallError(format!("Transaction error: {}", e)))?;
        
        // Chờ transaction được xác nhận
        let receipt = tx.await
            .map_err(|e| ContractError::CallError(format!("Failed to get transaction receipt: {}", e)))?;
        
        Ok(receipt)
    }
    
    async fn is_verified(&self) -> bool {
        self.metadata.verified
    }
}

/// Triển khai cho Token Contract (ERC20)
pub struct TokenContract {
    metadata: ContractMetadata,
    provider: Arc<Provider<Http>>,
    wallet: Option<LocalWallet>,
}

impl TokenContract {
    pub fn new(
        metadata: ContractMetadata,
        provider: Arc<Provider<Http>>,
        wallet: Option<LocalWallet>,
    ) -> Self {
        Self {
            metadata,
            provider,
            wallet,
        }
    }
    
    /// Lấy số dư của một địa chỉ
    pub async fn balance_of(&self, address: Address) -> Result<U256, ContractError> {
        // Triển khai logic gọi hàm balanceOf trên token contract
        
        Err(ContractError::CallError("Not implemented yet".to_string()))
    }
    
    /// Phê duyệt số token cho một địa chỉ khác sử dụng
    pub async fn approve(&self, spender: Address, amount: U256) -> Result<TransactionReceipt, ContractError> {
        // Triển khai logic gọi hàm approve trên token contract
        
        Err(ContractError::CallError("Not implemented yet".to_string()))
    }
    
    /// Chuyển token cho một địa chỉ khác
    pub async fn transfer(&self, to: Address, amount: U256) -> Result<TransactionReceipt, ContractError> {
        // Triển khai logic gọi hàm transfer trên token contract
        
        Err(ContractError::CallError("Not implemented yet".to_string()))
    }
}

#[async_trait]
impl ContractInterface for TokenContract {
    fn address(&self) -> Address {
        self.metadata.address
    }
    
    fn name(&self) -> String {
        self.metadata.name.clone()
    }
    
    async fn call<T: Send + Sync>(&self, function: &str, params: Vec<T>) -> Result<T, ContractError> {
        // Tạo contract instance từ abi
        let abi_json = tokio::fs::read_to_string(&self.metadata.abi_path)
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to read ABI file: {}", e)))?;
        
        let contract = Contract::new(
            self.metadata.address,
            serde_json::from_str(&abi_json).unwrap(),
            self.provider.clone(),
        );
        
        // Tạo tham số cho contract call
        let params_any: Vec<Box<dyn std::any::Any>> = params
            .into_iter()
            .map(|p| Box::new(p) as Box<dyn std::any::Any>)
            .collect();
        
        // Gọi hàm với timeout
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(CONTRACT_CALL_TIMEOUT_MS),
            contract.method::<_, T>(function, params_any).call(),
        )
        .await
        .map_err(|_| ContractError::CallError("Contract call timeout".to_string()))?
        .map_err(|e| ContractError::CallError(format!("Contract call error: {}", e)))?;
        
        Ok(result)
    }
    
    async fn send_transaction(&self, function: &str, params: Vec<Box<dyn std::any::Any + Send + Sync>>) 
        -> Result<TransactionReceipt, ContractError> {
        let wallet = self.wallet.clone().ok_or_else(|| 
            ContractError::SignatureError("No wallet provided for transaction".to_string())
        )?;
        
        // Tạo contract instance từ abi
        let abi_json = tokio::fs::read_to_string(&self.metadata.abi_path)
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to read ABI file: {}", e)))?;
        
        let client = self.provider.clone().with_signer(wallet);
        
        let contract = Contract::new(
            self.metadata.address,
            serde_json::from_str(&abi_json).unwrap(),
            client,
        );
        
        // Gửi transaction với timeout
        let tx = tokio::time::timeout(
            std::time::Duration::from_millis(CONTRACT_CALL_TIMEOUT_MS),
            contract.method::<_, ()>(function, params).send(),
        )
        .await
        .map_err(|_| ContractError::CallError("Transaction sending timeout".to_string()))?
        .map_err(|e| ContractError::CallError(format!("Transaction error: {}", e)))?;
        
        // Chờ transaction được xác nhận
        let receipt = tx.await
            .map_err(|e| ContractError::CallError(format!("Failed to get transaction receipt: {}", e)))?;
        
        Ok(receipt)
    }
    
    async fn is_verified(&self) -> bool {
        self.metadata.verified
    }
}

/// Triển khai cho Staking Contract
pub struct StakingContract {
    metadata: ContractMetadata,
    provider: Arc<Provider<Http>>,
    wallet: Option<LocalWallet>,
}

impl StakingContract {
    pub fn new(
        metadata: ContractMetadata,
        provider: Arc<Provider<Http>>,
        wallet: Option<LocalWallet>,
    ) -> Self {
        Self {
            metadata,
            provider,
            wallet,
        }
    }
    
    /// Stake token
    pub async fn stake(&self, amount: U256, lock_time: u64) -> Result<TransactionReceipt, ContractError> {
        // Kiểm tra ví
        let wallet = self.wallet.clone().ok_or_else(|| 
            ContractError::SignatureError("No wallet provided for transaction".to_string())
        )?;
        
        // Tạo contract instance từ abi
        let abi_json = tokio::fs::read_to_string(&self.metadata.abi_path)
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to read ABI file: {}", e)))?;
        
        let client = self.provider.clone().with_signer(wallet);
        
        let contract = Contract::new(
            self.metadata.address,
            serde_json::from_str(&abi_json).unwrap(),
            client,
        );
        
        // Gọi hàm stake với timeout
        let tx = tokio::time::timeout(
            std::time::Duration::from_millis(CONTRACT_CALL_TIMEOUT_MS),
            contract.method::<_, ()>("stake", (amount, lock_time)).send(),
        )
        .await
        .map_err(|_| ContractError::CallError("Transaction sending timeout".to_string()))?
        .map_err(|e| ContractError::CallError(format!("Stake transaction error: {}", e)))?;
        
        // Chờ transaction được xác nhận
        let receipt = tx.await
            .map_err(|e| ContractError::CallError(format!("Failed to get stake transaction receipt: {}", e)))?;
            
        debug!(
            amount = %amount,
            lock_time = lock_time,
            tx_hash = %receipt.transaction_hash,
            "Successfully staked tokens"
        );
        
        Ok(receipt)
    }
    
    /// Unstake token
    pub async fn unstake(&self, stake_id: U256) -> Result<TransactionReceipt, ContractError> {
        // Kiểm tra ví
        let wallet = self.wallet.clone().ok_or_else(|| 
            ContractError::SignatureError("No wallet provided for transaction".to_string())
        )?;
        
        // Tạo contract instance từ abi
        let abi_json = tokio::fs::read_to_string(&self.metadata.abi_path)
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to read ABI file: {}", e)))?;
        
        let client = self.provider.clone().with_signer(wallet);
        
        let contract = Contract::new(
            self.metadata.address,
            serde_json::from_str(&abi_json).unwrap(),
            client,
        );
        
        // Kiểm tra xem có thể unstake không
        let can_unstake: bool = contract.method::<_, bool>("canUnstake", stake_id).call()
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to check if can unstake: {}", e)))?;
            
        if !can_unstake {
            return Err(ContractError::CallError("Cannot unstake yet, lock period has not ended".to_string()));
        }
        
        // Gọi hàm unstake với timeout
        let tx = tokio::time::timeout(
            std::time::Duration::from_millis(CONTRACT_CALL_TIMEOUT_MS),
            contract.method::<_, ()>("unstake", stake_id).send(),
        )
        .await
        .map_err(|_| ContractError::CallError("Transaction sending timeout".to_string()))?
        .map_err(|e| ContractError::CallError(format!("Unstake transaction error: {}", e)))?;
        
        // Chờ transaction được xác nhận
        let receipt = tx.await
            .map_err(|e| ContractError::CallError(format!("Failed to get unstake transaction receipt: {}", e)))?;
            
        debug!(
            stake_id = %stake_id,
            tx_hash = %receipt.transaction_hash,
            "Successfully unstaked tokens"
        );
        
        Ok(receipt)
    }
    
    /// Claim rewards
    pub async fn claim_rewards(&self, stake_id: U256) -> Result<TransactionReceipt, ContractError> {
        // Kiểm tra ví
        let wallet = self.wallet.clone().ok_or_else(|| 
            ContractError::SignatureError("No wallet provided for transaction".to_string())
        )?;
        
        // Tạo contract instance từ abi
        let abi_json = tokio::fs::read_to_string(&self.metadata.abi_path)
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to read ABI file: {}", e)))?;
        
        let client = self.provider.clone().with_signer(wallet);
        
        let contract = Contract::new(
            self.metadata.address,
            serde_json::from_str(&abi_json).unwrap(),
            client,
        );
        
        // Kiểm tra số lượng rewards có thể claim
        let pending_rewards: U256 = contract.method::<_, U256>("getPendingRewards", stake_id).call()
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to get pending rewards: {}", e)))?;
            
        if pending_rewards.is_zero() {
            return Err(ContractError::CallError("No rewards to claim".to_string()));
        }
        
        // Gọi hàm claimRewards với timeout
        let tx = tokio::time::timeout(
            std::time::Duration::from_millis(CONTRACT_CALL_TIMEOUT_MS),
            contract.method::<_, ()>("claimRewards", stake_id).send(),
        )
        .await
        .map_err(|_| ContractError::CallError("Transaction sending timeout".to_string()))?
        .map_err(|e| ContractError::CallError(format!("Claim rewards transaction error: {}", e)))?;
        
        // Chờ transaction được xác nhận
        let receipt = tx.await
            .map_err(|e| ContractError::CallError(format!("Failed to get claim rewards transaction receipt: {}", e)))?;
            
        debug!(
            stake_id = %stake_id,
            rewards = %pending_rewards,
            tx_hash = %receipt.transaction_hash,
            "Successfully claimed rewards"
        );
        
        Ok(receipt)
    }
    
    /// Lấy thông tin stake
    pub async fn get_stake_info(&self, stake_id: U256) -> Result<(U256, U256, U256, bool), ContractError> {
        // Tạo contract instance từ abi
        let abi_json = tokio::fs::read_to_string(&self.metadata.abi_path)
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to read ABI file: {}", e)))?;
        
        let contract = Contract::new(
            self.metadata.address,
            serde_json::from_str(&abi_json).unwrap(),
            self.provider.clone(),
        );
        
        // Gọi hàm getStakeInfo với timeout
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(CONTRACT_CALL_TIMEOUT_MS),
            contract.method::<_, (U256, U256, U256, bool)>("getStakeInfo", stake_id).call(),
        )
        .await
        .map_err(|_| ContractError::CallError("Contract call timeout".to_string()))?
        .map_err(|e| ContractError::CallError(format!("Failed to get stake info: {}", e)))?;
        
        debug!(
            stake_id = %stake_id,
            amount = %result.0,
            start_time = %result.1,
            end_time = %result.2,
            is_active = result.3,
            "Retrieved stake information"
        );
        
        Ok(result)
    }
}

#[async_trait]
impl ContractInterface for StakingContract {
    fn address(&self) -> Address {
        self.metadata.address
    }
    
    fn name(&self) -> String {
        self.metadata.name.clone()
    }
    
    async fn call<T: Send + Sync>(&self, function: &str, params: Vec<T>) -> Result<T, ContractError> {
        // Tạo contract instance từ abi
        let abi_json = tokio::fs::read_to_string(&self.metadata.abi_path)
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to read ABI file: {}", e)))?;
        
        let contract = Contract::new(
            self.metadata.address,
            serde_json::from_str(&abi_json).unwrap(),
            self.provider.clone(),
        );
        
        // Tạo tham số cho contract call
        let params_any: Vec<Box<dyn std::any::Any>> = params
            .into_iter()
            .map(|p| Box::new(p) as Box<dyn std::any::Any>)
            .collect();
        
        // Gọi hàm với timeout
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(CONTRACT_CALL_TIMEOUT_MS),
            contract.method::<_, T>(function, params_any).call(),
        )
        .await
        .map_err(|_| ContractError::CallError("Contract call timeout".to_string()))?
        .map_err(|e| ContractError::CallError(format!("Contract call error: {}", e)))?;
        
        Ok(result)
    }
    
    async fn send_transaction(&self, function: &str, params: Vec<Box<dyn std::any::Any + Send + Sync>>) 
        -> Result<TransactionReceipt, ContractError> {
        let wallet = self.wallet.clone().ok_or_else(|| 
            ContractError::SignatureError("No wallet provided for transaction".to_string())
        )?;
        
        // Tạo contract instance từ abi
        let abi_json = tokio::fs::read_to_string(&self.metadata.abi_path)
            .await
            .map_err(|e| ContractError::CallError(format!("Failed to read ABI file: {}", e)))?;
        
        let client = self.provider.clone().with_signer(wallet);
        
        let contract = Contract::new(
            self.metadata.address,
            serde_json::from_str(&abi_json).unwrap(),
            client,
        );
        
        // Gửi transaction với timeout
        let tx = tokio::time::timeout(
            std::time::Duration::from_millis(CONTRACT_CALL_TIMEOUT_MS),
            contract.method::<_, ()>(function, params).send(),
        )
        .await
        .map_err(|_| ContractError::CallError("Transaction sending timeout".to_string()))?
        .map_err(|e| ContractError::CallError(format!("Transaction error: {}", e)))?;
        
        // Chờ transaction được xác nhận
        let receipt = tx.await
            .map_err(|e| ContractError::CallError(format!("Failed to get transaction receipt: {}", e)))?;
        
        Ok(receipt)
    }
    
    async fn is_verified(&self) -> bool {
        self.metadata.verified
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defi::constants::CONTRACT_CALL_TIMEOUT_MS;
    use ethers::providers::{Provider, Http};
    use ethers::types::{U256, Address, TransactionReceipt, H256};
    use ethers::utils::hex;
    use std::str::FromStr;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use mockall::predicate::*;
    use mockall::mock;

    // Mock cho Provider
    mock! {
        pub MockProvider {}
        impl Clone for MockProvider {
            fn clone(&self) -> Self;
        }
        #[async_trait]
        impl Provider for MockProvider {
            fn get_chain_id(&self) -> U256;
            async fn get_block_number(&self) -> Result<U256, ethers::providers::ProviderError>;
            async fn get_balance(&self, address: Address, block: Option<u64>) 
                -> Result<U256, ethers::providers::ProviderError>;
        }
    }
    
    #[tokio::test]
    async fn test_contract_manager_creation() {
        let chain_ids = vec![1, 56]; // Ethereum and BSC
        let manager = ContractManager::new(chain_ids, None);
        assert!(manager.contracts.read().await.is_empty());
        assert_eq!(manager.chain_ids, vec![1, 56]);
        assert!(manager.wallet.is_none());
    }
    
    #[tokio::test]
    async fn test_contract_metadata_serialization() {
        let metadata = ContractMetadata {
            name: "TestRouter".to_string(),
            address: Address::zero(),
            chain_id: 1,
            abi_path: "./abis/router.json".to_string(),
            verified: true,
        };
        
        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: ContractMetadata = serde_json::from_str(&json).unwrap();
        
        assert_eq!(metadata.name, deserialized.name);
        assert_eq!(metadata.address, deserialized.address);
        assert_eq!(metadata.chain_id, deserialized.chain_id);
        assert_eq!(metadata.abi_path, deserialized.abi_path);
        assert_eq!(metadata.verified, deserialized.verified);
    }
    
    #[test]
    fn test_contract_error_display() {
        let error = ContractError::ConnectionError("Connection failed".to_string());
        assert_eq!(format!("{}", error), "Lỗi kết nối: Connection failed");
        
        let error = ContractError::CallError("Call failed".to_string());
        assert_eq!(format!("{}", error), "Lỗi gọi contract: Call failed");
        
        let error = ContractError::UnsupportedContract("NFT".to_string());
        assert_eq!(format!("{}", error), "Contract không được hỗ trợ: NFT");
        
        let error = ContractError::UnsupportedChain("Chain 999".to_string());
        assert_eq!(format!("{}", error), "Blockchain không được hỗ trợ: Chain 999");
        
        let error = ContractError::SignatureError("Signature failed".to_string());
        assert_eq!(format!("{}", error), "Lỗi chữ ký giao dịch: Signature failed");
    }
    
    #[test]
    fn test_contract_type_serialization() {
        let contract_type = ContractType::Router;
        let json = serde_json::to_string(&contract_type).unwrap();
        let deserialized: ContractType = serde_json::from_str(&json).unwrap();
        assert_eq!(contract_type, deserialized);
        
        let contract_type = ContractType::Farm;
        let json = serde_json::to_string(&contract_type).unwrap();
        let deserialized: ContractType = serde_json::from_str(&json).unwrap();
        assert_eq!(contract_type, deserialized);
        
        let contract_type = ContractType::Token;
        let json = serde_json::to_string(&contract_type).unwrap();
        let deserialized: ContractType = serde_json::from_str(&json).unwrap();
        assert_eq!(contract_type, deserialized);
        
        let contract_type = ContractType::Staking;
        let json = serde_json::to_string(&contract_type).unwrap();
        let deserialized: ContractType = serde_json::from_str(&json).unwrap();
        assert_eq!(contract_type, deserialized);
        
        let contract_type = ContractType::NFT;
        let json = serde_json::to_string(&contract_type).unwrap();
        let deserialized: ContractType = serde_json::from_str(&json).unwrap();
        assert_eq!(contract_type, deserialized);
    }
    
    #[tokio::test]
    async fn test_router_contract_methods() {
        // Tạo metadata
        let metadata = ContractMetadata {
            name: "TestRouter".to_string(),
            address: Address::from_str("0x1234567890123456789012345678901234567890").unwrap(),
            chain_id: 1,
            abi_path: "./abis/router.json".to_string(),
            verified: true,
        };
        
        // Giả lập provider
        let provider = Provider::<Http>::try_from("http://localhost:8545").unwrap();
        
        // Tạo router contract
        let router = RouterContract::new(
            metadata.clone(),
            Arc::new(provider),
            None,
        );
        
        // Kiểm tra các getters
        assert_eq!(router.address(), metadata.address);
        assert_eq!(router.name(), metadata.name);
        assert!(router.is_verified().await);
    }
    
    #[tokio::test]
    async fn test_token_contract_methods() {
        // Tạo metadata
        let metadata = ContractMetadata {
            name: "TestToken".to_string(),
            address: Address::from_str("0x1234567890123456789012345678901234567890").unwrap(),
            chain_id: 1,
            abi_path: "./abis/token.json".to_string(),
            verified: true,
        };
        
        // Giả lập provider
        let provider = Provider::<Http>::try_from("http://localhost:8545").unwrap();
        
        // Tạo token contract
        let token = TokenContract::new(
            metadata.clone(),
            Arc::new(provider),
            None,
        );
        
        // Kiểm tra các getters
        assert_eq!(token.address(), metadata.address);
        assert_eq!(token.name(), metadata.name);
        assert!(token.is_verified().await);
    }
    
    #[tokio::test]
    async fn test_farm_contract_methods() {
        // Tạo metadata
        let metadata = ContractMetadata {
            name: "TestFarm".to_string(),
            address: Address::from_str("0x1234567890123456789012345678901234567890").unwrap(),
            chain_id: 1,
            abi_path: "./abis/farm.json".to_string(),
            verified: true,
        };
        
        // Giả lập provider
        let provider = Provider::<Http>::try_from("http://localhost:8545").unwrap();
        
        // Tạo farm contract
        let farm = FarmContract::new(
            metadata.clone(),
            Arc::new(provider),
            None,
        );
        
        // Kiểm tra các getters
        assert_eq!(farm.address(), metadata.address);
        assert_eq!(farm.name(), metadata.name);
        assert!(farm.is_verified().await);
    }
    
    #[tokio::test]
    async fn test_staking_contract_methods() {
        // Tạo metadata
        let metadata = ContractMetadata {
            name: "TestStaking".to_string(),
            address: Address::from_str("0x1234567890123456789012345678901234567890").unwrap(),
            chain_id: 1,
            abi_path: "./abis/staking.json".to_string(),
            verified: true,
        };
        
        // Giả lập provider
        let provider = Provider::<Http>::try_from("http://localhost:8545").unwrap();
        
        // Tạo staking contract
        let staking = StakingContract::new(
            metadata.clone(),
            Arc::new(provider),
            None,
        );
        
        // Kiểm tra các getters
        assert_eq!(staking.address(), metadata.address);
        assert_eq!(staking.name(), metadata.name);
        assert!(staking.is_verified().await);
    }
    
    #[tokio::test]
    async fn test_contract_manager_add_provider() {
        let chain_ids = vec![1, 56];
        let manager = ContractManager::new(chain_ids, None);
        
        // Thêm provider cho Ethereum mainnet
        assert!(manager.add_provider(1, "http://localhost:8545").await.is_ok());
        
        // Kiểm tra xem provider đã được thêm chưa
        let provider = manager.get_provider(1).await;
        assert!(provider.is_ok());
        
        // Kiểm tra xem provider cho chain khác có tồn tại không
        let provider = manager.get_provider(999).await;
        assert!(provider.is_err());
        assert!(matches!(provider.unwrap_err(), ContractError::UnsupportedChain(_)));
    }
} 