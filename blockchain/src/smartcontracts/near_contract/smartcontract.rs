//! # NEAR DMD Token Contract
//! 
//! Contract này triển khai DMD token trên NEAR theo chuẩn NEP-141 (Fungible Token) và
//! tích hợp với LayerZero để bridge token giữa NEAR và các blockchain khác.
//!
//! DMD là một omni token được triển khai trên nhiều blockchain bao gồm:
//! - NEAR Protocol
//! - Solana
//! - Binance Smart Chain (BSC)
//! - Ethereum

use std::convert::TryInto;
use near_sdk::{
    borsh::{self, BorshDeserialize, BorshSerialize},
    collections::{LookupMap, UnorderedMap},
    env, ext_contract, log, near_bindgen, AccountId, Balance, Promise, PromiseOrValue,
    PanicOnDefault, StorageUsage,
};
use near_sdk::json_types::{U128, U64};
use serde::{Serialize, Deserialize};

/// Chain ID mà DMD token hỗ trợ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DmdChain {
    /// Ethereum Mainnet
    Ethereum,
    /// Binance Smart Chain
    Bsc,
    /// Solana
    Solana,
    /// NEAR Protocol
    Near,
    /// Avalanche
    Avalanche,
    /// Polygon
    Polygon,
    /// Arbitrum
    Arbitrum,
    /// Optimism
    Optimism,
    /// Fantom
    Fantom,
}

impl DmdChain {
    /// Chuyển đổi từ chuỗi thành DmdChain
    pub fn from_string(chain: &str) -> Option<Self> {
        match chain.to_lowercase().as_str() {
            "ethereum" | "eth" => Some(DmdChain::Ethereum),
            "bsc" | "binance" => Some(DmdChain::Bsc),
            "solana" | "sol" => Some(DmdChain::Solana),
            "near" => Some(DmdChain::Near),
            "avalanche" => Some(DmdChain::Avalanche),
            "polygon" => Some(DmdChain::Polygon),
            "arbitrum" => Some(DmdChain::Arbitrum),
            "optimism" => Some(DmdChain::Optimism),
            "fantom" => Some(DmdChain::Fantom),
            _ => None,
        }
    }
    
    /// Lấy tên của chain
    pub fn name(&self) -> &'static str {
        match self {
            DmdChain::Ethereum => "Ethereum",
            DmdChain::Bsc => "Binance Smart Chain",
            DmdChain::Solana => "Solana",
            DmdChain::Near => "NEAR Protocol",
            DmdChain::Avalanche => "Avalanche",
            DmdChain::Polygon => "Polygon",
            DmdChain::Arbitrum => "Arbitrum",
            DmdChain::Optimism => "Optimism",
            DmdChain::Fantom => "Fantom",
        }
    }
    
    /// Các chain EVM
    pub fn is_evm(&self) -> bool {
        matches!(self, DmdChain::Ethereum | DmdChain::Bsc | DmdChain::Avalanche | DmdChain::Polygon | DmdChain::Arbitrum | DmdChain::Optimism | DmdChain::Fantom)
    }
}

// LayerZero Chain IDs
const LAYERZERO_CHAIN_ID_ETHEREUM: u16 = 1; // Ethereum
const LAYERZERO_CHAIN_ID_BSC: u16 = 2; // BSC
const LAYERZERO_CHAIN_ID_AVALANCHE: u16 = 3; // Avalanche
const LAYERZERO_CHAIN_ID_POLYGON: u16 = 4; // Polygon
const LAYERZERO_CHAIN_ID_SOLANA: u16 = 5; // Solana
const LAYERZERO_CHAIN_ID_FANTOM: u16 = 6; // Fantom
const LAYERZERO_CHAIN_ID_NEAR: u16 = 7; // NEAR
const LAYERZERO_CHAIN_ID_ARBITRUM: u16 = 8; // Arbitrum
const LAYERZERO_CHAIN_ID_OPTIMISM: u16 = 9; // Optimism

// Contract ID của LayerZero Endpoint trên NEAR
const LAYERZERO_ENDPOINT: &str = "layerzero.near"; // Ví dụ

/// Cấu trúc cho phí giao dịch
#[derive(BorshDeserialize, BorshSerialize)]
pub struct StorageBalance {
    pub total: Balance,
    pub available: Balance,
}

/// Cấu trúc cho phí bridge cross-chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainFees {
    /// Số gas ước tính
    pub gas: U128,
    /// Giá gas hiện tại
    pub gas_price: U128,
    /// Phí giao dịch (gas * gas_price)
    pub transaction_fee: U128,
    /// Phí protocol (LayerZero)
    pub protocol_fee: U128,
    /// Tổng phí (transaction_fee + protocol_fee)
    pub total_fee: U128,
}

/// Thông tin cấu hình cho DMD token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmdConfig {
    /// Chain ID
    pub chain: DmdChain,
    /// Địa chỉ contract của DMD token
    pub token_address: String,
    /// RPC URL của chain
    pub rpc_url: String,
    /// Explorer URL
    pub explorer_url: String,
    /// Timeout (ms)
    pub timeout_ms: u64,
    /// Số lần retry tối đa
    pub max_retries: u32,
}

impl DmdConfig {
    /// Tạo cấu hình mới với các giá trị mặc định
    pub fn new(chain: DmdChain) -> Self {
        let (token_address, rpc_url, explorer_url) = match chain {
            DmdChain::Ethereum => (
                "0x90fE084F877C65e1b577c7b2eA64B8D8dd1AB278", // Địa chỉ DMD trên Ethereum
                "https://ethereum.publicnode.com", // URL RPC công khai, không cần API key
                "https://etherscan.io"
            ),
            DmdChain::Bsc => (
                "0x16a7FA783c2FF584B234e13825960F95244Fc7bC", // Địa chỉ DMD trên BSC
                "https://bsc-dataseed.binance.org",
                "https://bscscan.com"
            ),
            DmdChain::Solana => (
                "DMDxaFj2u9SfJYYt3u12jUeyjiLnpCJsJ3A6s2qAZk86", // Địa chỉ DMD trên Solana
                "https://api.mainnet-beta.solana.com",
                "https://explorer.solana.com"
            ),
            DmdChain::Near => (
                "dmd.near", // Địa chỉ DMD trên NEAR
                "https://rpc.mainnet.near.org",
                "https://explorer.near.org"
            ),
            DmdChain::Avalanche => (
                "0xA384BcF68f32dCCC751bD8270f93F4Ab142AbFF1", // Địa chỉ DMD trên Avalanche
                "https://api.avax.network/ext/bc/C/rpc",
                "https://snowtrace.io"
            ),
            DmdChain::Polygon => (
                "0x4F6B88635c8B2FE88E8DD5F22938D454Bb758bF0", // Địa chỉ DMD trên Polygon
                "https://rpc.ankr.com/polygon",
                "https://polygonscan.com"
            ),
            DmdChain::Arbitrum => (
                "0x7E07e15D2a87A24492740D16f5bdF58c16db0c4E", // Địa chỉ DMD trên Arbitrum
                "https://rpc.ankr.com/arbitrum",
                "https://arbiscan.io"
            ),
            DmdChain::Optimism => (
                "0x28Ed5C13B02e91A4729C5F4Ec2c2DB07C54C78eA", // Địa chỉ DMD trên Optimism
                "https://rpc.ankr.com/optimism",
                "https://optimistic.etherscan.io"
            ),
            DmdChain::Fantom => (
                "0x3E9ADD579C92D2AB158E5B7F483A16E2b9A43261", // Địa chỉ DMD trên Fantom
                "https://rpc.ankr.com/fantom",
                "https://ftmscan.com"
            ),
        };

        Self {
            chain,
            token_address: token_address.to_string(),
            rpc_url: rpc_url.to_string(),
            explorer_url: explorer_url.to_string(),
            timeout_ms: 30000, // 30 giây
            max_retries: 3,
        }
    }
}

/// Thông tin token DMD
#[derive(Debug, Clone, Serialize, Deserialize, BorshDeserialize, BorshSerialize)]
pub struct DmdTokenInfo {
    /// Tên token
    pub name: String,
    /// Ký hiệu token
    pub symbol: String,
    /// Số thập phân (decimals)
    pub decimals: u8,
    /// Tổng cung
    pub total_supply: U128,
    /// Chain
    pub chain: DmdChain,
}

/// Metadata của token
#[derive(Debug, Clone, BorshDeserialize, BorshSerialize)]
pub struct TokenMetadata {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub icon: Option<String>,
    pub reference: Option<String>,
    pub reference_hash: Option<Vec<u8>>,
}

/// Định nghĩa interface cho LayerZero Endpoint
#[ext_contract(ext_layerzero)]
trait LayerZeroEndpoint {
    fn send(
        &mut self,
        dest_chain_id: u16,
        dest_address: Vec<u8>,
        payload: Vec<u8>,
        refund_address: AccountId,
        gas_for_dest: U64,
        gas_for_lzreceive: U64,
    ) -> Promise;
    
    fn estimate_fees(
        &self,
        dest_chain_id: u16,
        dest_address: Vec<u8>,
        payload: Vec<u8>,
    ) -> U128;
}

/// Cấu trúc chính của contract
#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct DmdToken {
    /// Tổng cung của token
    pub total_supply: Balance,
    /// Số dư của các tài khoản
    pub accounts: LookupMap<AccountId, Balance>,
    /// Chủ sở hữu contract
    pub owner_id: AccountId,
    /// Metadata của token
    pub metadata: TokenMetadata,
    /// Phí lưu trữ cho mỗi tài khoản
    pub storage_usage_per_account: StorageUsage,
    /// LayerZero endpoint
    pub layerzero_endpoint: AccountId,
    /// Các địa chỉ tin cậy từ xa
    pub trusted_remotes: UnorderedMap<u16, Vec<u8>>,
    /// Phí cơ bản cho bridge
    pub base_fee: Balance,
    /// Cấu hình của token
    pub config: DmdConfig,
}

#[near_bindgen]
impl DmdToken {
    /// Khởi tạo contract mới
    #[init]
    pub fn new(
        owner_id: AccountId,
        total_supply: U128,
        metadata: TokenMetadata,
        layerzero_endpoint: AccountId,
    ) -> Self {
        let config = DmdConfig::new(DmdChain::Near);
        
        let mut this = Self {
            total_supply: total_supply.0,
            accounts: LookupMap::new(b"a"),
            owner_id,
            metadata,
            storage_usage_per_account: 0,
            layerzero_endpoint,
            trusted_remotes: UnorderedMap::new(b"t"),
            base_fee: 0,
            config,
        };
        
        // Tính toán phí lưu trữ
        let initial_storage = env::storage_usage();
        let tmp_account_id = AccountId::new_unchecked("a".repeat(64));
        this.accounts.insert(&tmp_account_id, &0);
        this.storage_usage_per_account = env::storage_usage() - initial_storage;
        this.accounts.remove(&tmp_account_id);
        
        // Mint toàn bộ token cho owner
        this.accounts.insert(&owner_id, &total_supply.0);
        
        this
    }
    
    /// Trả về tổng cung của token
    pub fn total_supply(&self) -> U128 {
        U128(self.total_supply)
    }
    
    /// Trả về số dư của tài khoản
    pub fn balance_of(&self, account_id: AccountId) -> U128 {
        U128(self.accounts.get(&account_id).unwrap_or(0))
    }
    
    /// Lấy metadata của token
    pub fn metadata(&self) -> TokenMetadata {
        self.metadata.clone()
    }
    
    /// Lấy thông tin token
    pub fn token_info(&self) -> DmdTokenInfo {
        DmdTokenInfo {
            name: self.metadata.name.clone(),
            symbol: self.metadata.symbol.clone(),
            decimals: self.metadata.decimals,
            total_supply: U128(self.total_supply),
            chain: DmdChain::Near,
        }
    }
    
    /// Chuyển token cho người nhận
    pub fn transfer(&mut self, receiver_id: AccountId, amount: U128) -> Promise {
        let sender_id = env::predecessor_account_id();
        let amount = amount.0;
        
        // Kiểm tra số dư
        let sender_balance = self.accounts.get(&sender_id).unwrap_or(0);
        assert!(sender_balance >= amount, "Không đủ token");
        
        // Cập nhật số dư
        self.accounts.insert(&sender_id, &(sender_balance - amount));
        let receiver_balance = self.accounts.get(&receiver_id).unwrap_or(0);
        self.accounts.insert(&receiver_id, &(receiver_balance + amount));
        
        log!("Transfer {} from {} to {}", amount, sender_id, receiver_id);
        
        Promise::new(receiver_id)
    }
    
    /// Bridge token sang blockchain khác
    pub fn bridge_to_chain(&mut self, chain_id: u16, to_address: Vec<u8>, amount: U128) -> Promise {
        let sender_id = env::predecessor_account_id();
        let amount = amount.0;
        
        // Kiểm tra số dư
        let sender_balance = self.accounts.get(&sender_id).unwrap_or(0);
        assert!(sender_balance >= amount, "Không đủ token");
        
        // Kiểm tra chain ID
        assert!(
            chain_id == LAYERZERO_CHAIN_ID_ETHEREUM ||
            chain_id == LAYERZERO_CHAIN_ID_BSC ||
            chain_id == LAYERZERO_CHAIN_ID_AVALANCHE ||
            chain_id == LAYERZERO_CHAIN_ID_POLYGON ||
            chain_id == LAYERZERO_CHAIN_ID_SOLANA ||
            chain_id == LAYERZERO_CHAIN_ID_FANTOM ||
            chain_id == LAYERZERO_CHAIN_ID_ARBITRUM ||
            chain_id == LAYERZERO_CHAIN_ID_OPTIMISM,
            "Chain ID không được hỗ trợ"
        );
        
        // Giảm số dư của người gửi
        self.accounts.insert(&sender_id, &(sender_balance - amount));
        
        // Giảm tổng cung (vì token sẽ được mint ở chain đích)
        self.total_supply -= amount;
        
        // Tạo payload
        let mut payload = Vec::new();
        payload.push(0x01); // Loại message: bridge token
        payload.extend_from_slice(&env::predecessor_account_id().as_bytes()); // Địa chỉ người gửi
        payload.extend_from_slice(&amount.to_le_bytes()); // Số lượng
        
        // Lấy địa chỉ đích từ trusted remotes
        let dest_address = self
            .trusted_remotes
            .get(&chain_id)
            .expect("Chain endpoint not configured");
        
        log!("Bridging {} tokens to chain ID {}", amount, chain_id);
        
        // Gọi LayerZero endpoint
        ext_layerzero::send(
            chain_id,
            dest_address,
            payload,
            env::predecessor_account_id(),
            U64(0),
            U64(0),
            &self.layerzero_endpoint,
            env::attached_deposit(),
            env::prepaid_gas() / 2,
        )
    }
    
    /// Bridge token sang Ethereum
    pub fn bridge_to_ethereum(&mut self, eth_address: Vec<u8>, amount: U128) -> Promise {
        self.bridge_to_chain(LAYERZERO_CHAIN_ID_ETHEREUM, eth_address, amount)
    }
    
    /// Bridge token sang BSC
    pub fn bridge_to_bsc(&mut self, bsc_address: Vec<u8>, amount: U128) -> Promise {
        self.bridge_to_chain(LAYERZERO_CHAIN_ID_BSC, bsc_address, amount)
    }
    
    /// Bridge token sang Solana
    pub fn bridge_to_solana(&mut self, solana_address: Vec<u8>, amount: U128) -> Promise {
        self.bridge_to_chain(LAYERZERO_CHAIN_ID_SOLANA, solana_address, amount)
    }
    
    /// Nhận token từ blockchain khác (qua LayerZero)
    pub fn receive_from_chain(&mut self, from_chain_id: u16, from_address: Vec<u8>, to_account: AccountId, amount: U128) {
        // Trong triển khai thực tế, hàm này sẽ chỉ được gọi bởi LayerZero endpoint
        assert_eq!(env::predecessor_account_id(), self.layerzero_endpoint, "Unauthorized");
        
        let amount = amount.0;
        
        // Tăng tổng cung
        self.total_supply += amount;
        
        // Mint token cho người nhận
        let receiver_balance = self.accounts.get(&to_account).unwrap_or(0);
        self.accounts.insert(&to_account, &(receiver_balance + amount));
        
        log!("Received {} tokens from chain ID {}", amount, from_chain_id);
        log!("Tokens minted to: {}", to_account);
    }
    
    /// Thiết lập trusted remote address
    pub fn set_trusted_remote(&mut self, chain_id: u16, remote_address: Vec<u8>) {
        assert_eq!(env::predecessor_account_id(), self.owner_id, "Unauthorized");
        self.trusted_remotes.insert(&chain_id, &remote_address);
        log!("Set trusted remote for chain ID {}", chain_id);
    }
    
    /// Đặt phí cơ bản bridge
    pub fn set_base_fee(&mut self, fee: U128) {
        assert_eq!(env::predecessor_account_id(), self.owner_id, "Unauthorized");
        self.base_fee = fee.0;
        log!("Set base fee to {}", fee.0);
    }
    
    /// Ước tính phí bridge
    pub fn estimate_bridge_fee(&self, chain_id: u16) -> U128 {
        U128(self.base_fee)
    }
    
    /// Cập nhật cấu hình
    pub fn update_config(&mut self, config: DmdConfig) {
        assert_eq!(env::predecessor_account_id(), self.owner_id, "Unauthorized");
        assert_eq!(config.chain, DmdChain::Near, "Config phải cho chain NEAR");
        self.config = config;
        log!("Config updated");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::{testing_env, MockedBlockchain};
    
    fn get_context(predecessor_account_id: AccountId) -> VMContextBuilder {
        let mut builder = VMContextBuilder::new();
        builder
            .current_account_id(accounts(0))
            .signer_account_id(predecessor_account_id.clone())
            .predecessor_account_id(predecessor_account_id);
        builder
    }
    
    #[test]
    fn test_new() {
        let mut context = get_context(accounts(1));
        testing_env!(context.build());
        
        let metadata = TokenMetadata {
            name: "Diamond Token".to_string(),
            symbol: "DMD".to_string(),
            decimals: 18,
            icon: None,
            reference: None,
            reference_hash: None,
        };
        
        let contract = DmdToken::new(
            accounts(1),
            U128(1_000_000_000_000_000_000_000_000_000), // 1 billion with 18 decimals
            metadata,
            AccountId::new_unchecked("layerzero.near".to_string()),
        );
        
        assert_eq!(contract.total_supply().0, 1_000_000_000_000_000_000_000_000_000);
        assert_eq!(contract.balance_of(accounts(1)).0, 1_000_000_000_000_000_000_000_000_000);
    }
    
    #[test]
    fn test_bridge_flow() {
        let mut context = get_context(accounts(1));
        testing_env!(context.build());
        
        let metadata = TokenMetadata {
            name: "Diamond Token".to_string(),
            symbol: "DMD".to_string(),
            decimals: 18,
            icon: None,
            reference: None,
            reference_hash: None,
        };
        
        let mut contract = DmdToken::new(
            accounts(1),
            U128(1_000_000_000_000_000_000_000_000_000), // 1 billion with 18 decimals
            metadata,
            AccountId::new_unchecked("layerzero.near".to_string()),
        );
        
        // Thiết lập trusted remote cho BSC
        contract.set_trusted_remote(LAYERZERO_CHAIN_ID_BSC, vec![1, 2, 3, 4]);
        
        // Kiểm tra phí bridge
        let fee = contract.estimate_bridge_fee(LAYERZERO_CHAIN_ID_BSC);
        assert_eq!(fee.0, 0);
        
        // Thiết lập phí bridge
        contract.set_base_fee(U128(1_000_000_000_000_000_000)); // 1 NEAR
        let fee = contract.estimate_bridge_fee(LAYERZERO_CHAIN_ID_BSC);
        assert_eq!(fee.0, 1_000_000_000_000_000_000);
    }
} 