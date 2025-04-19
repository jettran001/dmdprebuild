//! # NEAR DMD Token Contract
//! 
//! Contract này triển khai DMD token trên NEAR theo chuẩn NEP-141 (Fungible Token) và
//! tích hợp với LayerZero để bridge token giữa NEAR và các blockchain khác.
//!
//! DMD là một omni token được triển khai trên nhiều blockchain bao gồm:
//! - NEAR Protocol (Main Chain)
//! - Solana (Sub Chain)
//! - Binance Smart Chain (BSC) (Sub Chain)
//! - Ethereum (Sub Chain)

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
const LAYERZERO_CHAIN_ID_SUI: u16 = 110;
const LAYERZERO_CHAIN_ID_PI: u16 = 111;

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

/// Cấu trúc theo dõi token đã lock cho bridge
#[derive(BorshDeserialize, BorshSerialize, Debug, Clone)]
pub struct LockedTokens {
    /// Số lượng token đã lock để bridge sang BSC
    pub locked_on_bsc: Balance,
    /// Số lượng token đã lock để bridge sang Solana
    pub locked_on_solana: Balance,
    /// Số lượng token đã lock để bridge sang Ethereum
    pub locked_on_ethereum: Balance,
    /// Số lượng token đã lock để bridge sang Polygon
    pub locked_on_polygon: Balance,
    /// Số lượng token đã lock để bridge sang SUI
    pub locked_on_sui: Balance,
    /// Số lượng token đã lock để bridge sang Pi Network
    pub locked_on_pi: Balance,
    /// Số lượng token đã lock để bridge sang các chain khác
    pub locked_on_others: Balance,
}

impl Default for LockedTokens {
    fn default() -> Self {
        Self {
            locked_on_bsc: 0,
            locked_on_solana: 0,
            locked_on_ethereum: 0,
            locked_on_polygon: 0,
            locked_on_sui: 0,
            locked_on_pi: 0,
            locked_on_others: 0,
        }
    }
}

/// Cấu trúc điều khiển admin cho bridge
#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub struct AdminControl {
    /// Danh sách admin
    pub admins: UnorderedMap<AccountId, bool>,
    /// Owner (cao nhất)
    pub owner: AccountId,
    /// Tạm dừng bridge
    pub bridge_paused: bool,
    /// Danh sách chain được cấp phép
    pub supported_chains: UnorderedMap<u16, bool>,
    /// Ngưỡng số lượng token để tự động tạm dừng bridge
    pub auto_pause_threshold: Balance,
}

impl Default for AdminControl {
    fn default() -> Self {
        let mut supported_chains = UnorderedMap::new(b"sc");
        // Mặc định hỗ trợ các chain chính
        supported_chains.insert(&LAYERZERO_CHAIN_ID_BSC, &true);
        supported_chains.insert(&LAYERZERO_CHAIN_ID_ETHEREUM, &true);
        supported_chains.insert(&LAYERZERO_CHAIN_ID_SOLANA, &true);
        
        Self {
            admins: UnorderedMap::new(b"a"),
            owner: "dmd.near".parse().unwrap(),
            bridge_paused: false,
            supported_chains,
            auto_pause_threshold: 50_000_000 * 10u128.pow(18), // 50 triệu DMD
        }
    }
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
    /// Theo dõi token đã lock cho bridge
    pub locked_tokens: LockedTokens,
    /// Điều khiển admin
    pub admin_control: AdminControl,
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
            owner_id: owner_id.clone(),
            metadata,
            storage_usage_per_account: 0,
            layerzero_endpoint,
            trusted_remotes: UnorderedMap::new(b"t"),
            base_fee: 0,
            config,
            locked_tokens: LockedTokens::default(),
            admin_control: AdminControl::default(),
        };
        
        // Đặt owner cho admin control
        this.admin_control.owner = owner_id.clone();
        
        // Khởi tạo tài khoản sở hữu với tổng cung ban đầu
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
    
    /// Kiểm tra xem chain có được hỗ trợ không
    pub fn is_supported_chain(&self, chain_id: u16) -> bool {
        self.admin_control.supported_chains.get(&chain_id).unwrap_or(false)
    }
    
    /// Wrap token sang chain khác
    pub fn wrap_to_chain(&mut self, chain_id: u16, to_address: Vec<u8>, amount: U128) -> Promise {
        assert!(!self.admin_control.bridge_paused, "Bridge đang bị tạm dừng");
        assert!(self.is_supported_chain(chain_id), "Chain không được hỗ trợ");
        
        let account_id = env::predecessor_account_id();
        let amount_u128 = amount.0;
        
        // Kiểm tra số dư
        let balance = self.accounts.get(&account_id).unwrap_or(0);
        assert!(balance >= amount_u128, "Không đủ token để wrap");
        
        // Kiểm tra số lượng token đã lock, nếu vượt quá ngưỡng thì tự động tạm dừng
        let total_locked = self.get_total_locked().0;
        if total_locked + amount_u128 > self.admin_control.auto_pause_threshold {
            self.admin_control.bridge_paused = true;
            log!("Tự động tạm dừng bridge do vượt quá ngưỡng");
        }
        
        // Giảm số dư của người gửi
        self.accounts.insert(&account_id, &(balance - amount_u128));
        
        // Cập nhật số lượng token đã lock cho chain tương ứng
        match chain_id {
            LAYERZERO_CHAIN_ID_BSC => self.locked_tokens.locked_on_bsc += amount_u128,
            LAYERZERO_CHAIN_ID_SOLANA => self.locked_tokens.locked_on_solana += amount_u128,
            LAYERZERO_CHAIN_ID_ETHEREUM => self.locked_tokens.locked_on_ethereum += amount_u128,
            LAYERZERO_CHAIN_ID_POLYGON => self.locked_tokens.locked_on_polygon += amount_u128,
            LAYERZERO_CHAIN_ID_SUI => self.locked_tokens.locked_on_sui += amount_u128,
            LAYERZERO_CHAIN_ID_PI => self.locked_tokens.locked_on_pi += amount_u128,
            _ => self.locked_tokens.locked_on_others += amount_u128,
        }
        
        // Log sự kiện
        log!("Wrap {} DMD từ {} (NEAR) đến {} ({}) chain_id: {}", 
             amount_u128, account_id, hex::encode(&to_address), 
             self.get_chain_name(chain_id), chain_id);
        
        // Gửi message qua LayerZero
        let payload = format!("{{\"from\":\"{}\",\"to\":\"{}\",\"amount\":{},\"action\":\"wrap\"}}", 
                     account_id, hex::encode(&to_address), amount_u128).into_bytes();
        
        ext_layerzero::send(
            chain_id,
            to_address,
            payload,
            account_id.clone(),
            U64(300000),  // Giá trị mặc định cho gas to destination
            U64(300000),  // Giá trị mặc định cho gas to lz receive
            &self.layerzero_endpoint,
            self.base_fee + self.estimate_bridge_fee(chain_id).0,  // Attach đủ token để trả phí
            env::prepaid_gas() - env::used_gas() - 10_000_000_000_000,  // Giữ lại một phần gas
        )
    }

    /// Unwrap token từ chain khác về NEAR
    pub fn unwrap_from_chain(&mut self, from_chain_id: u16, from_address: Vec<u8>, to_account: AccountId, amount: U128) {
        // Xác minh người gọi là LayerZero endpoint
        assert_eq!(env::predecessor_account_id(), self.layerzero_endpoint, "Chỉ LayerZero endpoint mới được phép gọi");
        
        // Xác minh trusted remote
        let trusted_remote = self.trusted_remotes.get(&from_chain_id);
        assert!(trusted_remote.is_some(), "Remote address không tin cậy");
        assert_eq!(trusted_remote.unwrap(), from_address, "Remote address không trùng khớp");
        
        let amount_u128 = amount.0;
        
        // Unlock token từ chain tương ứng
        match from_chain_id {
            LAYERZERO_CHAIN_ID_BSC => {
                assert!(self.locked_tokens.locked_on_bsc >= amount_u128, "Số lượng token lock không đủ");
                self.locked_tokens.locked_on_bsc -= amount_u128;
            },
            LAYERZERO_CHAIN_ID_SOLANA => {
                assert!(self.locked_tokens.locked_on_solana >= amount_u128, "Số lượng token lock không đủ");
                self.locked_tokens.locked_on_solana -= amount_u128;
            },
            LAYERZERO_CHAIN_ID_ETHEREUM => {
                assert!(self.locked_tokens.locked_on_ethereum >= amount_u128, "Số lượng token lock không đủ");
                self.locked_tokens.locked_on_ethereum -= amount_u128;
            },
            LAYERZERO_CHAIN_ID_POLYGON => {
                assert!(self.locked_tokens.locked_on_polygon >= amount_u128, "Số lượng token lock không đủ");
                self.locked_tokens.locked_on_polygon -= amount_u128;
            },
            LAYERZERO_CHAIN_ID_SUI => {
                assert!(self.locked_tokens.locked_on_sui >= amount_u128, "Số lượng token lock không đủ");
                self.locked_tokens.locked_on_sui -= amount_u128;
            },
            LAYERZERO_CHAIN_ID_PI => {
                assert!(self.locked_tokens.locked_on_pi >= amount_u128, "Số lượng token lock không đủ");
                self.locked_tokens.locked_on_pi -= amount_u128;
            },
            _ => {
                assert!(self.locked_tokens.locked_on_others >= amount_u128, "Số lượng token lock không đủ");
                self.locked_tokens.locked_on_others -= amount_u128;
            }
        }
        
        // Tăng số dư của người nhận
        let receiver_balance = self.accounts.get(&to_account).unwrap_or(0);
        self.accounts.insert(&to_account, &(receiver_balance + amount_u128));
        
        log!("Unwrap {} DMD từ chain {} đến {} (NEAR)", 
             amount_u128, from_chain_id, to_account);
    }
    
    /// Bridge token sang BSC (gọi wrap_to_chain với chain_id của BSC)
    pub fn bridge_to_bsc(&mut self, bsc_address: Vec<u8>, amount: U128) -> Promise {
        self.wrap_to_chain(LAYERZERO_CHAIN_ID_BSC, bsc_address, amount)
    }

    /// Bridge token sang Ethereum
    pub fn bridge_to_ethereum(&mut self, eth_address: Vec<u8>, amount: U128) -> Promise {
        self.wrap_to_chain(LAYERZERO_CHAIN_ID_ETHEREUM, eth_address, amount)
    }

    /// Bridge token sang Solana
    pub fn bridge_to_solana(&mut self, solana_address: Vec<u8>, amount: U128) -> Promise {
        self.wrap_to_chain(LAYERZERO_CHAIN_ID_SOLANA, solana_address, amount)
    }
    
    /// Bridge token sang Polygon
    pub fn bridge_to_polygon(&mut self, polygon_address: Vec<u8>, amount: U128) -> Promise {
        self.wrap_to_chain(LAYERZERO_CHAIN_ID_POLYGON, polygon_address, amount)
    }
    
    /// Bridge token sang SUI
    pub fn bridge_to_sui(&mut self, sui_address: Vec<u8>, amount: U128) -> Promise {
        self.wrap_to_chain(LAYERZERO_CHAIN_ID_SUI, sui_address, amount)
    }
    
    /// Bridge token sang Pi Network
    pub fn bridge_to_pi(&mut self, pi_address: Vec<u8>, amount: U128) -> Promise {
        self.wrap_to_chain(LAYERZERO_CHAIN_ID_PI, pi_address, amount)
    }
    
    /// Lấy tên của chain từ chain ID
    pub fn get_chain_name(&self, chain_id: u16) -> String {
        match chain_id {
            LAYERZERO_CHAIN_ID_ETHEREUM => "Ethereum".to_string(),
            LAYERZERO_CHAIN_ID_BSC => "BSC".to_string(),
            LAYERZERO_CHAIN_ID_AVALANCHE => "Avalanche".to_string(),
            LAYERZERO_CHAIN_ID_POLYGON => "Polygon".to_string(),
            LAYERZERO_CHAIN_ID_SOLANA => "Solana".to_string(),
            LAYERZERO_CHAIN_ID_FANTOM => "Fantom".to_string(),
            LAYERZERO_CHAIN_ID_NEAR => "NEAR".to_string(),
            LAYERZERO_CHAIN_ID_ARBITRUM => "Arbitrum".to_string(),
            LAYERZERO_CHAIN_ID_OPTIMISM => "Optimism".to_string(),
            LAYERZERO_CHAIN_ID_SUI => "SUI".to_string(),
            LAYERZERO_CHAIN_ID_PI => "Pi Network".to_string(),
            _ => format!("Unknown Chain ({})", chain_id),
        }
    }
    
    // === Admin Control Functions ===
    
    /// Thêm admin mới
    pub fn add_admin(&mut self, account_id: AccountId) {
        self.assert_owner();
        self.admin_control.admins.insert(&account_id, &true);
        log!("Đã thêm admin mới: {}", account_id);
    }
    
    /// Xóa admin
    pub fn remove_admin(&mut self, account_id: AccountId) {
        self.assert_owner();
        self.admin_control.admins.remove(&account_id);
        log!("Đã xóa admin: {}", account_id);
    }
    
    /// Tạm dừng bridge
    pub fn pause_bridge(&mut self) {
        self.assert_admin();
        self.admin_control.bridge_paused = true;
        log!("Bridge đã bị tạm dừng");
    }
    
    /// Mở lại bridge
    pub fn unpause_bridge(&mut self) {
        self.assert_admin();
        self.admin_control.bridge_paused = false;
        log!("Bridge đã được mở lại");
    }
    
    /// Thêm chain được hỗ trợ
    pub fn add_supported_chain(&mut self, chain_id: u16) {
        self.assert_admin();
        self.admin_control.supported_chains.insert(&chain_id, &true);
        log!("Đã thêm chain mới: {}", chain_id);
    }
    
    /// Xóa chain được hỗ trợ
    pub fn remove_supported_chain(&mut self, chain_id: u16) {
        self.assert_admin();
        self.admin_control.supported_chains.remove(&chain_id);
        log!("Đã xóa chain: {}", chain_id);
    }
    
    /// Cập nhật ngưỡng tự động tạm dừng
    pub fn update_auto_pause_threshold(&mut self, new_threshold: U128) {
        self.assert_admin();
        self.admin_control.auto_pause_threshold = new_threshold.0;
        log!("Đã cập nhật ngưỡng tự động tạm dừng: {}", new_threshold.0);
    }
    
    /// Kiểm tra nếu người gọi là owner
    fn assert_owner(&self) {
        assert_eq!(
            env::predecessor_account_id(), 
            self.admin_control.owner, 
            "Chỉ owner mới được phép thực hiện"
        );
    }
    
    /// Kiểm tra nếu người gọi là admin hoặc owner
    fn assert_admin(&self) {
        let account_id = env::predecessor_account_id();
        assert!(
            account_id == self.admin_control.owner || 
            self.admin_control.admins.get(&account_id).unwrap_or(false),
            "Chỉ admin mới được phép thực hiện"
        );
    }
    
    /// Kiểm tra nếu bridge đang bị tạm dừng
    pub fn is_bridge_paused(&self) -> bool {
        self.admin_control.bridge_paused
    }
    
    /// Kiểm tra nếu một tài khoản là admin
    pub fn is_admin(&self, account_id: AccountId) -> bool {
        account_id == self.admin_control.owner || 
        self.admin_control.admins.get(&account_id).unwrap_or(false)
    }
    
    /// Lấy danh sách các chain được hỗ trợ
    pub fn get_supported_chains(&self) -> Vec<u16> {
        self.admin_control.supported_chains
            .keys()
            .collect()
    }
    
    /// Lấy ngưỡng tự động tạm dừng hiện tại
    pub fn get_auto_pause_threshold(&self) -> U128 {
        U128(self.admin_control.auto_pause_threshold)
    }
    
    /// Lấy thông tin về token đã lock
    pub fn get_locked_tokens(&self) -> LockedTokens {
        self.locked_tokens.clone()
    }

    /// Lấy tổng số token đã lock
    pub fn get_total_locked(&self) -> U128 {
        U128(
            self.locked_tokens.locked_on_bsc +
            self.locked_tokens.locked_on_solana +
            self.locked_tokens.locked_on_ethereum +
            self.locked_tokens.locked_on_polygon +
            self.locked_tokens.locked_on_sui +
            self.locked_tokens.locked_on_pi +
            self.locked_tokens.locked_on_others
        )
    }

    /// Lấy tổng số token đang lưu thông (total supply - locked)
    pub fn get_circulating_supply(&self) -> U128 {
        U128(self.total_supply - self.get_total_locked().0)
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