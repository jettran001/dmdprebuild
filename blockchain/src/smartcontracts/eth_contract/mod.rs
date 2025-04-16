use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use ethers::{
    abi::{AbiDecode, AbiEncode, ParamType, Token},
    contract::abigen,
    core::types::{
        transaction::eip2718::TypedTransaction, Address, Bytes, TransactionReceipt, H256, U256,
    },
    middleware::SignerMiddleware,
    prelude::rand,
    providers::{Http, Middleware, Provider},
    signers::{LocalWallet, Signer},
    types::BlockNumber,
};
use log::{debug, error, info, warn};
use once_cell::sync::{Lazy, OnceCell};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{Mutex, RwLock};

use crate::prelude::*;
use crate::smartcontracts::DmdTokenProvider;

// Define các hằng số và cấu trúc dữ liệu cho ETH contract
const DEFAULT_GAS_LIMIT: u64 = 21000;
const DEFAULT_PRIORITY_FEE: u64 = 2_000_000_000; // 2 Gwei
const DEFAULT_REQUEST_TIMEOUT: u64 = 30; // 30 giây
static MAX_RETRIES: u32 = 3;
static RETRY_DELAY: Duration = Duration::from_secs(2);

// Cache cho total supply và thời gian hết hạn
static TOTAL_SUPPLY_CACHE: OnceCell<Mutex<Option<(U256, Instant)>>> = OnceCell::new();
/// Thời gian cache cho các request (giây)
const CACHE_DURATION: u64 = 1800; // 30 phút thay vì 5 phút

// Struct chứa thông tin về DMD Token trên Ethereum
#[derive(Debug, Clone)]
pub struct DmdEthContract {
    contract_address: Address,
    contract_description: String,
}

// Cấu hình cho ETH contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EthContractConfig {
    pub rpc_url: String,
    pub contract_address: Address,
    pub default_gas_limit: u64,
    pub gas_price: Option<U256>,
    pub request_timeout: Duration,
}

// Triển khai mặc định cho EthContractConfig
impl Default for EthContractConfig {
    fn default() -> Self {
        Self {
            rpc_url: "https://ethereum.publicnode.com".to_string(),
            contract_address: Address::default(),
            default_gas_limit: DEFAULT_GAS_LIMIT,
            gas_price: None,
            request_timeout: Duration::from_secs(DEFAULT_REQUEST_TIMEOUT),
        }
    }
}

// Provider cho Ethereum contract
#[derive(Debug)]
pub struct EthContractProvider {
    config: EthContractConfig,
    provider: Provider<Http>,
    dmd_contract: DmdEthContract,
}

// Implement EthContractProvider
impl EthContractProvider {
    // Khởi tạo provider mới
    pub fn new(config: EthContractConfig) -> Result<Self> {
        let provider = Provider::<Http>::try_from(&config.rpc_url)
            .map_err(|e| anyhow!("Failed to create Ethereum provider: {}", e))?;

        let dmd_contract = DmdEthContract {
            contract_address: config.contract_address,
            contract_description: "DiamondToken (ETH)".to_string(),
        };

        // Khởi tạo cache cho total supply
        let _ = TOTAL_SUPPLY_CACHE.get_or_init(|| Mutex::new(None));

        Ok(Self {
            config,
            provider,
            dmd_contract,
        })
    }

    // Tạo wallet từ private key
    pub fn create_wallet_from_private_key(&self, private_key: &str) -> Result<LocalWallet> {
        let wallet = private_key
            .parse::<LocalWallet>()
            .map_err(|e| anyhow!("Failed to create wallet from private key: {}", e))?;

        Ok(wallet)
    }

    // Gửi giao dịch transfer ERC-1155
    pub async fn send_transfer_transaction(
        &self,
        wallet: LocalWallet,
        to: Address,
        amount: U256,
    ) -> Result<H256> {
        let client = SignerMiddleware::new(self.provider.clone(), wallet.clone());
        let client = Arc::new(client);

        let data = self.encode_safe_transfer_from_fn(wallet.address(), to, 0u8.into(), amount)?;

        let tx = TypedTransaction::Legacy(ethers::types::TransactionRequest {
            from: Some(wallet.address()),
            to: Some(ethers::types::NameOrAddress::Address(self.dmd_contract.contract_address)),
            gas: Some(U256::from(1_000_000)), // Giá trị gas giới hạn cao hơn cho ERC-1155
            gas_price: self.config.gas_price,
            value: None,
            data: Some(data),
            nonce: None,
            chain_id: None,
        });

        let pending_tx = client
            .send_transaction(tx, None)
            .await
            .map_err(|e| anyhow!("Failed to send transfer transaction: {}", e))?;

        let tx_hash = pending_tx.tx_hash();
        Ok(tx_hash)
    }

    // Hàm mã hóa cho gọi hàm safeTransferFrom
    fn encode_safe_transfer_from_fn(
        &self,
        from: Address,
        to: Address,
        id: U256,
        amount: U256,
    ) -> Result<Bytes> {
        // Mã hóa dữ liệu đầu vào cho hàm safeTransferFrom(address,address,uint256,uint256,bytes)
        let function_signature = "safeTransferFrom(address,address,uint256,uint256,bytes)";
        let selector = &ethers::utils::keccak256(function_signature.as_bytes())[0..4];

        let tokens = vec![
            Token::Address(from),
            Token::Address(to),
            Token::Uint(id),
            Token::Uint(amount),
            Token::Bytes(vec![]), // empty bytes for the data parameter
        ];

        let encoded_params = ethers::abi::encode(&tokens);
        let mut encoded_data = Vec::with_capacity(selector.len() + encoded_params.len());
        encoded_data.extend_from_slice(selector);
        encoded_data.extend_from_slice(&encoded_params);

        Ok(encoded_data.into())
    }

    // Hàm mã hóa cho gọi hàm bridge
    fn encode_bridge_to_near_fn(
        &self,
        to_near_address: String,
        id: U256,
        amount: U256,
    ) -> Result<Bytes> {
        // Mã hóa dữ liệu đầu vào cho hàm bridgeToNEAR(bytes,uint256,uint256)
        let function_signature = "bridgeToNEAR(bytes,uint256,uint256)";
        let selector = &ethers::utils::keccak256(function_signature.as_bytes())[0..4];

        // Chuyển địa chỉ NEAR thành bytes
        let to_near_bytes = to_near_address.as_bytes().to_vec();

        let tokens = vec![
            Token::Bytes(to_near_bytes),
            Token::Uint(id),
            Token::Uint(amount),
        ];

        let encoded_params = ethers::abi::encode(&tokens);
        let mut encoded_data = Vec::with_capacity(selector.len() + encoded_params.len());
        encoded_data.extend_from_slice(selector);
        encoded_data.extend_from_slice(&encoded_params);

        Ok(encoded_data.into())
    }

    // Gửi giao dịch bridge sang NEAR
    pub async fn send_bridge_transaction(
        &self,
        wallet: LocalWallet,
        to_near_address: String,
        amount: U256,
    ) -> Result<H256> {
        let client = SignerMiddleware::new(self.provider.clone(), wallet.clone());
        let client = Arc::new(client);

        // Lấy phí bridge
        let (fee, _) = self.estimate_bridge_fee(to_near_address.clone(), amount).await?;

        let data = self.encode_bridge_to_near_fn(to_near_address, 0u8.into(), amount)?;

        let tx = TypedTransaction::Legacy(ethers::types::TransactionRequest {
            from: Some(wallet.address()),
            to: Some(ethers::types::NameOrAddress::Address(self.dmd_contract.contract_address)),
            gas: Some(U256::from(500_000)), // Gas limit cao hơn cho bridge operation
            gas_price: self.config.gas_price,
            value: Some(fee), // Phí native token (ETH) để bridge
            data: Some(data),
            nonce: None,
            chain_id: None,
        });

        let pending_tx = client
            .send_transaction(tx, None)
            .await
            .map_err(|e| anyhow!("Failed to send bridge transaction: {}", e))?;

        let tx_hash = pending_tx.tx_hash();
        Ok(tx_hash)
    }

    // Lấy total supply với caching
    pub async fn get_total_supply(&self) -> Result<U256> {
        let cache = TOTAL_SUPPLY_CACHE.get().unwrap().lock().await;
        
        // Kiểm tra cache có hợp lệ không
        if let Some((supply, timestamp)) = *cache {
            if timestamp.elapsed() < Duration::from_secs(CACHE_DURATION) {
                return Ok(supply);
            }
        }
        drop(cache); // Drop lock để có thể mutable lock để cập nhật cache

        // Nếu cache không hợp lệ, gọi lại API
        for retry in 0..MAX_RETRIES {
            match self.query_total_supply().await {
                Ok(supply) => {
                    // Cập nhật cache
                    let mut cache = TOTAL_SUPPLY_CACHE.get().unwrap().lock().await;
                    *cache = Some((supply, Instant::now()));
                    return Ok(supply);
                }
                Err(e) => {
                    if retry < MAX_RETRIES - 1 {
                        warn!("Failed to get total supply, retrying: {}", e);
                        tokio::time::sleep(RETRY_DELAY).await;
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Err(anyhow!("Failed to get total supply after multiple retries"))
    }

    // Gọi RPC để lấy total supply
    async fn query_total_supply(&self) -> Result<U256> {
        // Gọi hàm totalSupply() trên contract
        let function_signature = "totalSupply()";
        let selector = &ethers::utils::keccak256(function_signature.as_bytes())[0..4];

        let tx = ethers::providers::Middleware::call(
            &self.provider,
            ethers::types::TransactionRequest {
                to: Some(ethers::types::NameOrAddress::Address(self.dmd_contract.contract_address)),
                data: Some(selector.to_vec().into()),
                ..Default::default()
            },
            None,
        )
        .await
        .map_err(|e| anyhow!("Failed to call totalSupply function: {}", e))?;

        // Decode kết quả
        let result = U256::from_big_endian(&tx);
        Ok(result)
    }

    // Lấy số dư của một địa chỉ
    pub async fn get_balance(&self, address: Address) -> Result<U256> {
        // Gọi hàm balanceOfDMD(address) trên contract
        let function_signature = "balanceOfDMD(address)";
        let selector = &ethers::utils::keccak256(function_signature.as_bytes())[0..4];

        // Mã hóa địa chỉ
        let address_token = Token::Address(address);
        let encoded_address = ethers::abi::encode(&[address_token]);

        // Kết hợp selector và địa chỉ đã mã hóa
        let mut encoded_data = Vec::with_capacity(selector.len() + encoded_address.len());
        encoded_data.extend_from_slice(selector);
        encoded_data.extend_from_slice(&encoded_address);

        // Gọi RPC
        let tx = ethers::providers::Middleware::call(
            &self.provider,
            ethers::types::TransactionRequest {
                to: Some(ethers::types::NameOrAddress::Address(self.dmd_contract.contract_address)),
                data: Some(encoded_data.into()),
                ..Default::default()
            },
            None,
        )
        .await
        .map_err(|e| anyhow!("Failed to call balanceOfDMD function: {}", e))?;

        // Decode kết quả
        let result = U256::from_big_endian(&tx);
        Ok(result)
    }

    // Ước tính phí bridge
    pub async fn estimate_bridge_fee(
        &self,
        to_near_address: String,
        amount: U256,
    ) -> Result<(U256, U256)> {
        // Gọi hàm estimateBridgeFeeToNEAR(bytes,uint256,uint256) trên contract
        let function_signature = "estimateBridgeFeeToNEAR(bytes,uint256,uint256)";
        let selector = &ethers::utils::keccak256(function_signature.as_bytes())[0..4];

        // Mã hóa tham số
        let to_near_bytes = to_near_address.as_bytes().to_vec();
        let tokens = vec![
            Token::Bytes(to_near_bytes),
            Token::Uint(0u8.into()), // ID = 0 cho DMD token
            Token::Uint(amount),
        ];
        let encoded_params = ethers::abi::encode(&tokens);

        // Kết hợp selector và tham số đã mã hóa
        let mut encoded_data = Vec::with_capacity(selector.len() + encoded_params.len());
        encoded_data.extend_from_slice(selector);
        encoded_data.extend_from_slice(&encoded_params);

        // Gọi RPC
        let tx = ethers::providers::Middleware::call(
            &self.provider,
            ethers::types::TransactionRequest {
                to: Some(ethers::types::NameOrAddress::Address(self.dmd_contract.contract_address)),
                data: Some(encoded_data.into()),
                ..Default::default()
            },
            None,
        )
        .await
        .map_err(|e| anyhow!("Failed to call estimateBridgeFee function: {}", e))?;

        // Hàm trả về 2 giá trị uint256 (nativeFee, zroFee)
        // Mỗi uint256 là 32 bytes nên cần tách thành 2 phần
        if tx.len() >= 64 {
            let native_fee = U256::from_big_endian(&tx[0..32]);
            let zro_fee = U256::from_big_endian(&tx[32..64]);
            Ok((native_fee, zro_fee))
        } else {
            Err(anyhow!(
                "Invalid response length for estimateBridgeFee: {} bytes",
                tx.len()
            ))
        }
    }

    // Kiểm tra trạng thái giao dịch
    pub async fn check_bridge_status(&self, tx_hash: H256) -> Result<TransactionStatus> {
        // Kiểm tra receipt
        match self.provider.get_transaction_receipt(tx_hash).await {
            Ok(Some(receipt)) => {
                if receipt.status == Some(1.into()) {
                    Ok(TransactionStatus::Confirmed)
                } else {
                    Ok(TransactionStatus::Failed)
                }
            }
            Ok(None) => Ok(TransactionStatus::Pending),
            Err(e) => Err(anyhow!("Failed to check transaction status: {}", e)),
        }
    }
}

// Implement trait DmdTokenProvider cho EthContractProvider
#[async_trait]
impl DmdTokenProvider for EthContractProvider {
    async fn transfer(&self, from_private_key: String, to: String, amount: U256) -> Result<String> {
        let wallet = self.create_wallet_from_private_key(&from_private_key)?;
        let to_address = Address::from_str(&to)
            .map_err(|e| anyhow!("Invalid ETH address: {}", e))?;

        let tx_hash = self.send_transfer_transaction(wallet, to_address, amount).await?;
        Ok(format!("0x{}", hex::encode(tx_hash.as_bytes())))
    }

    async fn total_supply(&self) -> Result<U256> {
        self.get_total_supply().await
    }

    async fn balance_of(&self, address: String) -> Result<U256> {
        let eth_address = Address::from_str(&address)
            .map_err(|e| anyhow!("Invalid ETH address: {}", e))?;
        self.get_balance(eth_address).await
    }

    async fn decimals(&self) -> Result<u8> {
        // ERC-1155 DMD token có 18 decimals
        Ok(18)
    }

    async fn token_name(&self) -> Result<String> {
        Ok("Diamond Token".to_string())
    }

    async fn token_symbol(&self) -> Result<String> {
        Ok("DMD".to_string())
    }

    async fn bridge_to_near(
        &self,
        from_private_key: String,
        to_near_account: String,
        amount: U256,
    ) -> Result<String> {
        let wallet = self.create_wallet_from_private_key(&from_private_key)?;
        
        // Gửi giao dịch bridge
        let tx_hash = self.send_bridge_transaction(wallet, to_near_account, amount).await?;
        Ok(format!("0x{}", hex::encode(tx_hash.as_bytes())))
    }

    async fn estimate_bridge_fee(&self, to_chain: Chain, amount: U256) -> Result<U256> {
        // Hiện tại chỉ hỗ trợ bridge sang NEAR
        if to_chain != Chain::Near {
            return Err(anyhow!("Bridging to {} is not supported yet", to_chain));
        }

        // Sử dụng địa chỉ mẫu để estimate
        let sample_near_address = "sample.near".to_string();
        let (fee, _) = self.estimate_bridge_fee(sample_near_address, amount).await?;
        Ok(fee)
    }

    async fn check_bridge_status(&self, tx_hash: String) -> Result<TransactionStatus> {
        let hash = H256::from_str(&tx_hash)
            .map_err(|e| anyhow!("Invalid transaction hash: {}", e))?;
        self.check_bridge_status(hash).await
    }
} 