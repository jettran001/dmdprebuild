//! # DMD BSC Bridge Module
//! 
//! Module này triển khai bridge giữa BSC và các blockchain khác sử dụng LayerZero.
//! Hỗ trợ chuyển token DMD qua các blockchain sử dụng chuẩn ERC-1155.

use std::sync::Arc;
use std::str::FromStr;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use anyhow::{Result, anyhow};
use ethers::types::{U256, Address, Bytes};
use ethers::providers::{Provider, Http};
use ethers::middleware::SignerMiddleware;
use ethers::signers::{LocalWallet, Signer};
use tracing::{info, warn, error};
use serde_json::json;

use crate::smartcontracts::{BridgeInterface, dmd_token::DmdChain};

/// Địa chỉ của contract ERC-1155 trên BSC
const BSC_CONTRACT_ADDRESS: &str = "0x16a7FA783c2FF584B234e13825960F95244Fc7bC";

/// LayerZero endpoint trên BSC
const LZ_ENDPOINT_BSC: &str = "0x3c2269811836af69497E5F486A85D7316753cf62";

/// Mapping LayerZero chain ID cho các chain khác
const LZ_CHAIN_ID_ETH: u16 = 101;
const LZ_CHAIN_ID_BSC: u16 = 102;
const LZ_CHAIN_ID_AVALANCHE: u16 = 106;
const LZ_CHAIN_ID_POLYGON: u16 = 109;
const LZ_CHAIN_ID_ARBITRUM: u16 = 110;
const LZ_CHAIN_ID_OPTIMISM: u16 = 111;
const LZ_CHAIN_ID_FANTOM: u16 = 112;
const LZ_CHAIN_ID_NEAR: u16 = 115; // Giả định

/// Thông tin cấu hình cho DMD Bridge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmdBscBridgeConfig {
    /// RPC URL của BSC
    pub rpc_url: String,
    /// Địa chỉ contract trên BSC
    pub contract_address: String,
    /// Địa chỉ LayerZero endpoint
    pub lz_endpoint: String,
    /// Gas limit mặc định cho giao dịch
    pub default_gas_limit: U256,
    /// Timeout cho request (ms)
    pub timeout_ms: u64,
}

impl Default for DmdBscBridgeConfig {
    fn default() -> Self {
        Self {
            rpc_url: "https://bsc-dataseed.binance.org".to_string(),
            contract_address: BSC_CONTRACT_ADDRESS.to_string(),
            lz_endpoint: LZ_ENDPOINT_BSC.to_string(),
            default_gas_limit: U256::from(300_000),
            timeout_ms: 30000,
        }
    }
}

impl DmdBscBridgeConfig {
    /// Tạo cấu hình mới
    pub fn new() -> Self {
        Self::default()
    }

    /// Thiết lập RPC URL
    pub fn with_rpc_url(mut self, rpc_url: &str) -> Self {
        self.rpc_url = rpc_url.to_string();
        self
    }

    /// Thiết lập địa chỉ contract
    pub fn with_contract_address(mut self, contract_address: &str) -> Self {
        self.contract_address = contract_address.to_string();
        self
    }
}

/// Bridge để chuyển DMD token giữa BSC và các blockchain khác
/// Sử dụng LayerZero Protocol cho cross-chain messaging
pub struct DmdBscBridge {
    /// Cấu hình bridge
    config: DmdBscBridgeConfig,
    /// Provider cho BSC
    provider: Arc<Provider<Http>>,
}

impl DmdBscBridge {
    /// Tạo bridge mới
    pub async fn new(config: Option<DmdBscBridgeConfig>) -> Result<Self> {
        let config = config.unwrap_or_default();
        
        // Tạo HTTP provider
        let provider = Provider::<Http>::try_from(&config.rpc_url)?;
        let provider = Arc::new(provider);
        
        Ok(Self {
            config,
            provider,
        })
    }
    
    /// Lấy LayerZero chain ID cho một chain
    fn get_lz_chain_id(chain: DmdChain) -> Result<u16> {
        match chain {
            DmdChain::Ethereum => Ok(LZ_CHAIN_ID_ETH),
            DmdChain::Bsc => Ok(LZ_CHAIN_ID_BSC),
            DmdChain::Avalanche => Ok(LZ_CHAIN_ID_AVALANCHE),
            DmdChain::Polygon => Ok(LZ_CHAIN_ID_POLYGON),
            DmdChain::Arbitrum => Ok(LZ_CHAIN_ID_ARBITRUM),
            DmdChain::Optimism => Ok(LZ_CHAIN_ID_OPTIMISM),
            DmdChain::Fantom => Ok(LZ_CHAIN_ID_FANTOM),
            DmdChain::Near => Ok(LZ_CHAIN_ID_NEAR),
            DmdChain::Solana => Err(anyhow!("LayerZero không hỗ trợ Solana")),
        }
    }
    
    /// Chuyển đổi địa chỉ sang định dạng cho LayerZero
    fn format_address_for_lz(address: &str, chain: DmdChain) -> Result<Bytes> {
        match chain {
            DmdChain::Near => {
                // Cho NEAR, chúng ta cần mã hóa địa chỉ dạng văn bản thành bytes
                let bytes = address.as_bytes();
                Ok(Bytes::from(bytes.to_vec()))
            },
            _ => {
                // Cho các chain EVM, chuyển đổi từ chuỗi hex sang bytes
                let address = address.trim_start_matches("0x");
                let bytes = hex::decode(address)?;
                Ok(Bytes::from(bytes))
            }
        }
    }
    
    /// Tạo wallet từ private key
    fn create_wallet(private_key: &str) -> Result<LocalWallet> {
        let private_key = private_key.trim_start_matches("0x");
        let wallet = LocalWallet::from_str(private_key)?;
        Ok(wallet)
    }

    /// Tạo client cho giao dịch
    async fn create_client(&self, private_key: &str) -> Result<SignerMiddleware<Arc<Provider<Http>>, LocalWallet>> {
        let wallet = Self::create_wallet(private_key)?;
        let client = SignerMiddleware::new(self.provider.clone(), wallet);
        Ok(client)
    }
    
    /// Ước tính phí giao dịch và phí LayerZero
    async fn estimate_fees(
        &self, 
        target_chain: DmdChain,
        to_address: &str, 
        amount: U256
    ) -> Result<(U256, U256)> {
        // Lấy giá gas hiện tại
        let gas_price = self.provider.get_gas_price().await?;
        
        // Gas cơ bản cho giao dịch bridge
        let base_gas = self.config.default_gas_limit;
        
        // LayerZero chain ID của chain đích
        let lz_chain_id = Self::get_lz_chain_id(target_chain)?;
        
        // Mã hóa địa chỉ đích cho LayerZero
        let to_address_bytes = Self::format_address_for_lz(to_address, target_chain)?;
        
        // Phí LayerZero
        // Trong thực tế cần gọi estimateFees trên contract
        // Giá trị này là ước tính
        let lz_fee = match target_chain {
            DmdChain::Ethereum => U256::from(0.01 * 1e18 as u64), // ~0.01 BNB
            DmdChain::Near => U256::from(0.015 * 1e18 as u64),    // ~0.015 BNB
            _ => U256::from(0.005 * 1e18 as u64),                 // ~0.005 BNB
        };
        
        // Phí giao dịch
        let tx_fee = gas_price * base_gas;
        
        Ok((tx_fee, lz_fee))
    }
    
    /// Tạo payload cho hàm bridgeToOtherChain
    fn create_bridge_payload(
        &self,
        target_chain: DmdChain,
        to_address: &str,
        amount: U256,
        token_id: u32
    ) -> Result<Vec<u8>> {
        // LayerZero chain ID của chain đích
        let lz_chain_id = Self::get_lz_chain_id(target_chain)?;
        
        // Mã hóa địa chỉ đích cho LayerZero
        let to_address_bytes = Self::format_address_for_lz(to_address, target_chain)?;
        
        // 1. Function selector cho bridgeToOtherChain (4 bytes): 0xd73f4284
        let mut payload = hex::decode("d73f4284")?;
        
        // 2. LayerZero chain ID (32 bytes)
        let mut chain_id_bytes = vec![0u8; 32];
        chain_id_bytes[30] = ((lz_chain_id >> 8) & 0xFF) as u8;
        chain_id_bytes[31] = (lz_chain_id & 0xFF) as u8;
        payload.extend_from_slice(&chain_id_bytes);
        
        // 3. Địa chỉ đích (32 bytes - left padded)
        let mut address_bytes = vec![0u8; 32];
        let to_bytes = to_address_bytes.to_vec();
        let start_idx = 32 - to_bytes.len();
        address_bytes[start_idx..].copy_from_slice(&to_bytes);
        payload.extend_from_slice(&address_bytes);
        
        // 4. Token ID (32 bytes)
        let mut token_id_bytes = vec![0u8; 32];
        token_id_bytes[31] = (token_id & 0xFF) as u8;
        token_id_bytes[30] = ((token_id >> 8) & 0xFF) as u8;
        token_id_bytes[29] = ((token_id >> 16) & 0xFF) as u8;
        token_id_bytes[28] = ((token_id >> 24) & 0xFF) as u8;
        payload.extend_from_slice(&token_id_bytes);
        
        // 5. Số lượng token (32 bytes)
        let amount_bytes = amount.to_be_bytes();
        payload.extend_from_slice(&amount_bytes);
        
        Ok(payload)
    }
}

#[async_trait]
impl BridgeInterface for DmdBscBridge {
    async fn bridge_tokens(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
        // Đối với DMD token, chúng ta sử dụng token ID 0 (fungible token trong ERC-1155)
        let token_id = 0u32;
        
        // Chain đích mặc định là NEAR (có thể thay đổi theo yêu cầu)
        let target_chain = DmdChain::Near;
        
        info!(
            "Bridge {} DMD từ BSC đến {} qua LayerZero, địa chỉ đích: {}",
            amount, target_chain.name(), to_address
        );
        
        // Tạo client với private key
        let client = self.create_client(private_key).await?;
        
        // Lấy địa chỉ người gửi
        let from_address = client.address();
        
        // Tạo payload cho hàm bridgeToOtherChain
        let payload = self.create_bridge_payload(target_chain, to_address, amount, token_id)?;
        
        // Ước tính phí
        let (tx_fee, lz_fee) = self.estimate_fees(target_chain, to_address, amount).await?;
        
        // Tổng phí (tx_fee + lz_fee)
        let total_fee = tx_fee + lz_fee;
        
        // Tạo địa chỉ contract
        let contract_address = Address::from_str(&self.config.contract_address)?;
        
        // Tạo transaction
        let tx = ethers::types::TransactionRequest::new()
            .to(contract_address)
            .from(from_address)
            .value(total_fee)
            .data(payload)
            .gas(self.config.default_gas_limit);
        
        // Gửi transaction
        let pending_tx = client.send_transaction(tx, None).await?;
        
        // Lấy hash của transaction
        let tx_hash = format!("0x{:x}", pending_tx.tx_hash());
        
        info!("Bridge transaction sent: {}", tx_hash);
        
        Ok(tx_hash)
    }

    async fn check_bridge_status(&self, tx_hash: &str) -> Result<String> {
        // Lấy thông tin giao dịch từ hash
        let tx_hash = ethers::types::H256::from_str(tx_hash.trim_start_matches("0x"))?;
        
        // Kiểm tra receipt
        match self.provider.get_transaction_receipt(tx_hash).await? {
            Some(receipt) => {
                if receipt.status.unwrap_or_default().as_u64() == 1 {
                    // Kiểm tra sự kiện MessageSent từ LayerZero
                    for log in receipt.logs {
                        if log.topics.len() >= 2 {
                            // Kiểm tra topic đầu tiên có phải là sự kiện MessageSent không
                            // Topic[0] = event signature: keccak256("MessageSent(uint16,bytes,bytes)")
                            let message_sent_sig = "0x8c5261668696ce22758910d05bab8f186d6eb247ceac2af2e82c7dc17669b036";
                            
                            if log.topics[0].to_string() == message_sent_sig {
                                return Ok("InProgress".to_string());
                            }
                        }
                    }
                    
                    // Không tìm thấy sự kiện MessageSent
                    Ok("Failed".to_string())
                } else {
                    // Giao dịch thất bại
                    Ok("Failed".to_string())
                }
            },
            None => {
                // Giao dịch chưa được xác nhận
                Ok("Pending".to_string())
            }
        }
    }

    async fn estimate_bridge_fee(&self, to_address: &str, amount: U256) -> Result<(U256, U256)> {
        // Chain đích mặc định là NEAR (có thể thay đổi theo yêu cầu)
        let target_chain = DmdChain::Near;
        
        // Ước tính phí
        self.estimate_fees(target_chain, to_address, amount).await
    }
}

/// Thông tin cấu hình cho contract ERC-1155 trên BSC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmdBscContractInfo {
    /// ID của token DMD (fungible token)
    pub dmd_token_id: u32,
    /// Tổng cung của token DMD
    pub total_supply: U256,
    /// Số thập phân của token
    pub decimals: u8,
    /// URI metadata của token
    pub token_uri: String,
}

impl DmdBscBridge {
    /// Lấy thông tin về contract DMD trên BSC
    pub async fn get_contract_info(&self) -> Result<DmdBscContractInfo> {
        // Gọi JSON-RPC để lấy thông tin contract
        
        // 1. Tạo call data cho hàm uri(uint256)
        let uri_call_data = format!(
            "{{\"jsonrpc\":\"2.0\",\"method\":\"eth_call\",\"params\":[{{\"to\":\"{}\",\"data\":\"0x0e89341c0000000000000000000000000000000000000000000000000000000000000000\"}},\"latest\"],\"id\":1}}",
            self.config.contract_address
        );
        
        // 2. Tạo call data cho hàm totalSupply(uint256)
        let total_supply_call_data = format!(
            "{{\"jsonrpc\":\"2.0\",\"method\":\"eth_call\",\"params\":[{{\"to\":\"{}\",\"data\":\"0xbd85b0390000000000000000000000000000000000000000000000000000000000000000\"}},\"latest\"],\"id\":1}}",
            self.config.contract_address
        );
        
        // 3. Tạo call data cho hàm decimals()
        let decimals_call_data = format!(
            "{{\"jsonrpc\":\"2.0\",\"method\":\"eth_call\",\"params\":[{{\"to\":\"{}\",\"data\":\"0x313ce567\"}},\"latest\"],\"id\":1}}",
            self.config.contract_address
        );
        
        // Tạo HTTP client
        let client = reqwest::Client::new();
        
        // Gửi request để lấy URI
        let response = client
            .post(&self.config.rpc_url)
            .header("Content-Type", "application/json")
            .body(uri_call_data)
            .send()
            .await?;
        
        let uri_response: serde_json::Value = response.json().await?;
        let uri_hex = uri_response["result"].as_str().ok_or_else(|| anyhow!("Invalid URI response"))?;
        
        // Parse URI từ hex
        let uri_bytes = hex::decode(&uri_hex[2..])?;
        let uri = String::from_utf8(uri_bytes)?;
        
        // Gửi request để lấy total supply
        let response = client
            .post(&self.config.rpc_url)
            .header("Content-Type", "application/json")
            .body(total_supply_call_data)
            .send()
            .await?;
        
        let supply_response: serde_json::Value = response.json().await?;
        let supply_hex = supply_response["result"].as_str().ok_or_else(|| anyhow!("Invalid total supply response"))?;
        
        // Parse total supply từ hex
        let total_supply = U256::from_str(supply_hex)?;
        
        // Gửi request để lấy decimals
        let response = client
            .post(&self.config.rpc_url)
            .header("Content-Type", "application/json")
            .body(decimals_call_data)
            .send()
            .await?;
        
        let decimals_response: serde_json::Value = response.json().await?;
        let decimals_hex = decimals_response["result"].as_str().ok_or_else(|| anyhow!("Invalid decimals response"))?;
        
        // Parse decimals từ hex
        let decimals = u8::from_str_radix(&decimals_hex[2..], 16)?;
        
        Ok(DmdBscContractInfo {
            dmd_token_id: 0,
            total_supply,
            decimals,
            token_uri: uri,
        })
    }
    
    /// Mint NFT mới trên BSC (chỉ dành cho owner)
    pub async fn mint_nft(
        &self,
        private_key: &str,
        to_address: &str,
        token_id: u32,
        amount: U256,
        token_uri: &str
    ) -> Result<String> {
        // Tạo client với private key
        let client = self.create_client(private_key).await?;
        
        // Lấy địa chỉ người gửi
        let from_address = client.address();
        
        // Tạo địa chỉ contract
        let contract_address = Address::from_str(&self.config.contract_address)?;
        
        // Tạo payload cho hàm mintNFT(address,uint256,uint256,string)
        // Function selector: 0x731133e9
        let mut payload = hex::decode("731133e9")?;
        
        // 1. Địa chỉ người nhận
        let to_addr = Address::from_str(to_address)?;
        let to_bytes = to_addr.as_bytes();
        let mut to_padded = vec![0u8; 32];
        to_padded[12..].copy_from_slice(to_bytes);
        payload.extend_from_slice(&to_padded);
        
        // 2. Token ID
        let mut token_id_bytes = vec![0u8; 32];
        token_id_bytes[31] = (token_id & 0xFF) as u8;
        token_id_bytes[30] = ((token_id >> 8) & 0xFF) as u8;
        token_id_bytes[29] = ((token_id >> 16) & 0xFF) as u8;
        token_id_bytes[28] = ((token_id >> 24) & 0xFF) as u8;
        payload.extend_from_slice(&token_id_bytes);
        
        // 3. Số lượng
        let amount_bytes = amount.to_be_bytes();
        payload.extend_from_slice(&amount_bytes);
        
        // 4. Token URI
        // Cách mã hóa chuỗi URI còn thiếu, cần thêm vào
        let uri_bytes = token_uri.as_bytes();
        
        // Tạo transaction
        let tx = ethers::types::TransactionRequest::new()
            .to(contract_address)
            .from(from_address)
            .data(Bytes::from(payload))
            .gas(self.config.default_gas_limit);
        
        // Gửi transaction
        let pending_tx = client.send_transaction(tx, None).await?;
        
        // Lấy hash của transaction
        let tx_hash = format!("0x{:x}", pending_tx.tx_hash());
        
        Ok(tx_hash)
    }
    
    /// Lấy metadata URI của một token
    pub async fn get_token_uri(&self, token_id: u32) -> Result<String> {
        // Tạo call data cho hàm uri(uint256)
        let call_data = format!(
            "{{\"jsonrpc\":\"2.0\",\"method\":\"eth_call\",\"params\":[{{\"to\":\"{}\",\"data\":\"0x0e89341c{:064x}\"}},\"latest\"],\"id\":1}}",
            self.config.contract_address,
            token_id
        );
        
        // Tạo HTTP client
        let client = reqwest::Client::new();
        
        // Gửi request
        let response = client
            .post(&self.config.rpc_url)
            .header("Content-Type", "application/json")
            .body(call_data)
            .send()
            .await?;
        
        let json_response: serde_json::Value = response.json().await?;
        let uri_hex = json_response["result"].as_str().ok_or_else(|| anyhow!("Invalid URI response"))?;
        
        // Parse URI từ hex
        let uri_bytes = hex::decode(&uri_hex[2..])?;
        let uri = String::from_utf8(uri_bytes)?;
        
        Ok(uri)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_create_bridge() {
        let bridge = DmdBscBridge::new(None).await;
        assert!(bridge.is_ok(), "Không thể tạo bridge mặc định");
    }
    
    #[tokio::test]
    async fn test_lz_chain_id() {
        assert_eq!(DmdBscBridge::get_lz_chain_id(DmdChain::Ethereum).unwrap(), 101);
        assert_eq!(DmdBscBridge::get_lz_chain_id(DmdChain::Bsc).unwrap(), 102);
        assert!(DmdBscBridge::get_lz_chain_id(DmdChain::Solana).is_err());
    }
    
    #[tokio::test]
    async fn test_format_address() {
        let eth_address = "0x742d35Cc6634C0532925a3b844Bc454e4438f44e";
        let near_address = "example.near";
        
        let eth_bytes = DmdBscBridge::format_address_for_lz(eth_address, DmdChain::Ethereum).unwrap();
        let near_bytes = DmdBscBridge::format_address_for_lz(near_address, DmdChain::Near).unwrap();
        
        assert_eq!(eth_bytes.len(), 20);
        assert_eq!(near_bytes.len(), near_address.len());
    }
} 