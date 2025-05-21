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
    collections::{LookupMap, UnorderedMap, Vector},
    env, ext_contract, log, near_bindgen, AccountId, Balance, Promise, PromiseOrValue,
    PanicOnDefault, StorageUsage,
};
use near_sdk::json_types::{U128, U64};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

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

/// Thêm vào payload để xác định loại token
#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Debug)]
pub enum TokenType {
    ERC20,
    ERC1155,
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

/// Trạng thái của giao dịch bridge
#[derive(BorshDeserialize, BorshSerialize, Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BridgeStatus {
    /// Đang chờ xử lý
    Pending,
    /// Đã hoàn thành
    Completed,
    /// Thất bại với thông tin chi tiết
    Failed {
        /// Mã lỗi
        code: u32,
        /// Mô tả lỗi
        reason: String,
    },
    /// Đang thử lại
    Retrying,
    /// Đã hết thời gian
    TimedOut,
    /// Đã hoàn trả
    Refunded,
    /// Mắc kẹt
    Stuck {
        /// Thời gian mắc kẹt
        since: u64,
        /// Trạng thái trước đó
        previous_status: String,
    },
}

/// Thêm trường mới cho BridgeTransaction để theo dõi tốt hơn
#[derive(BorshDeserialize, BorshSerialize, Debug, Clone, Serialize, Deserialize)]
pub struct BridgeTransaction {
    /// ID giao dịch
    pub tx_id: String,
    /// Chain nguồn
    pub source_chain: DmdChain,
    /// Chain đích
    pub destination_chain: DmdChain,
    /// Địa chỉ gửi
    pub sender: AccountId,
    /// Địa chỉ nhận
    pub receiver: Vec<u8>,
    /// Số lượng token
    pub amount: Balance,
    /// Thời gian tạo giao dịch
    pub timestamp: u64,
    /// Nonce
    pub nonce: u64,
    /// Chữ ký xác thực (nếu có)
    pub signature: Option<Vec<u8>>,
    /// Trạng thái giao dịch
    pub status: BridgeStatus,
    /// Số lần thử lại
    pub retry_count: u8,
    /// Thời gian thử lại gần nhất
    pub last_retry: u64,
    /// Thời gian hết hạn
    pub expiry_time: u64,
    /// Refund destination (nếu giao dịch thất bại)
    pub refund_destination: Option<AccountId>,
    /// Thông tin bổ sung
    pub extra_data: Option<String>,
    
    // Thêm các trường mới
    /// Thời gian cập nhật cuối
    pub updated_at: u64,
    /// Các attempt đã thực hiện
    pub attempts: Vec<BridgeAttempt>,
    /// Thông tin về hệ thống xử lý
    pub processor_info: Option<String>,
    /// Thời gian timeout
    pub timeout_timestamp: u64,
    /// Flag tự động retry
    pub auto_retry_enabled: bool,
    /// Lịch sử trạng thái
    pub status_history: Vec<StatusChange>,
}

/// Thông tin về một lần thử bridge
#[derive(BorshDeserialize, BorshSerialize, Debug, Clone, Serialize, Deserialize)]
pub struct BridgeAttempt {
    /// Thời gian thử
    pub timestamp: u64,
    /// Trạng thái
    pub status: BridgeStatus,
    /// Hash giao dịch (nếu có)
    pub tx_hash: Option<String>,
    /// Thông tin lỗi (nếu có)
    pub error_info: Option<String>,
    /// Thông tin bổ sung
    pub extra_data: Option<String>,
}

/// Thay đổi trạng thái
#[derive(BorshDeserialize, BorshSerialize, Debug, Clone, Serialize, Deserialize)]
pub struct StatusChange {
    /// Thời gian thay đổi
    pub timestamp: u64,
    /// Trạng thái trước
    pub from_status: String,
    /// Trạng thái sau
    pub to_status: String,
    /// Lý do thay đổi
    pub reason: Option<String>,
    /// Người thực hiện thay đổi
    pub changed_by: Option<String>,
}

/// Bổ sung thêm thông tin cho FailedTransaction
#[derive(BorshDeserialize, BorshSerialize, Debug, Clone, Serialize, Deserialize)]
pub struct FailedTransaction {
    /// ID giao dịch
    pub tx_id: String,
    /// Chain nguồn
    pub source_chain: DmdChain,
    /// Địa chỉ người nhận refund
    pub refund_account: AccountId,
    /// Số lượng token
    pub amount: Balance,
    /// Lý do thất bại
    pub error: String,
    /// Thời gian thất bại
    pub timestamp: u64,
    /// Số lần thử lại
    pub retry_count: u8,
    /// Thời gian thử lại gần nhất
    pub last_retry: u64,
    /// Đã được hoàn trả chưa
    pub refunded: bool,
    /// Phí hoàn trả (nếu có)
    pub refund_fee: Option<Balance>,
    
    // Thêm các trường mới
    /// Mã lỗi cụ thể
    pub error_code: Option<u32>,
    /// Chi tiết lỗi kỹ thuật
    pub technical_details: Option<String>,
    /// Các bước đã thử
    pub resolution_attempts: Vec<ResolutionAttempt>,
    /// Nguyên nhân gốc rễ (nếu xác định được)
    pub root_cause: Option<String>,
    /// Biện pháp khắc phục được đề xuất
    pub suggested_resolution: Option<String>,
    /// Có tự động thử lại không
    pub auto_retry_enabled: bool,
}

/// Thông tin về một lần thử giải quyết
#[derive(BorshDeserialize, BorshSerialize, Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionAttempt {
    /// Thời gian thử
    pub timestamp: u64,
    /// Loại hành động
    pub action_type: String,
    /// Kết quả
    pub result: String,
    /// Thông tin chi tiết
    pub details: Option<String>,
    /// Người thực hiện
    pub executor: Option<String>,
}

/// Cấu trúc config của bridge
#[derive(BorshDeserialize, BorshSerialize, Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    /// Thời gian chờ tối đa cho mỗi giao dịch (giây)
    pub max_transaction_timeout: u64,
    /// Số lần retry tự động tối đa
    pub max_auto_retry_count: u8,
    /// Thời gian chờ giữa các lần retry (giây)
    pub retry_delay: u64,
    /// Hệ số tăng cho retry delay
    pub retry_backoff_factor: f64,
    /// Có tự động hoàn trả khi hết thời gian không
    pub auto_refund_on_timeout: bool,
    /// Số lượng tối đa các giao dịch đang chờ xử lý cho mỗi người dùng
    pub max_pending_transactions_per_user: u32,
    /// Có theo dõi giao dịch không
    pub enable_transaction_tracking: bool,
    /// Có cho phép người dùng hủy giao dịch không
    pub allow_user_cancellation: bool,
    /// Số trường cần xác nhận cho giao dịch cross-chain
    pub required_confirmations: u8,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            max_transaction_timeout: 3600, // 1 giờ
            max_auto_retry_count: 5,
            retry_delay: 300, // 5 phút
            retry_backoff_factor: 1.5,
            auto_refund_on_timeout: true,
            max_pending_transactions_per_user: 10,
            enable_transaction_tracking: true,
            allow_user_cancellation: true,
            required_confirmations: 1,
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
    /// Các giao dịch bridge đang xử lý
    pub bridge_transactions: UnorderedMap<String, BridgeTransaction>,
    
    /// Các giao dịch thất bại
    pub failed_transactions: UnorderedMap<String, FailedTransaction>,
    
    /// Các nonce đã xử lý (chống replay attack)
    pub processed_nonces: UnorderedMap<String, bool>,
    
    /// Nonce hiện tại cho outgoing transactions
    pub current_nonce: u64,
    
    /// Thời gian timeout cho bridge transaction
    pub bridge_timeout: u64,
    
    /// Số lần retry tối đa
    pub max_retry_attempts: u8,
    
    /// Allowlist cho địa chỉ đích trên các chain
    pub destination_allowlist: UnorderedMap<u16, UnorderedMap<String, bool>>,
    
    /// Flag kiểm tra địa chỉ đích
    pub enable_destination_check: bool,
}

impl Default for DmdToken {
    fn default() -> Self {
        env::panic_str("DmdToken should be initialized using new method")
    }
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
            bridge_transactions: UnorderedMap::new(b"bt"),
            failed_transactions: UnorderedMap::new(b"ft"),
            processed_nonces: UnorderedMap::new(b"pn"),
            current_nonce: 0,
            bridge_timeout: 30000, // 30 giây
            max_retry_attempts: 3,
            destination_allowlist: UnorderedMap::new(b"dl"),
            enable_destination_check: true,
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
    
    /// Xác thực chữ ký
    pub fn validate_signature(&self, chain_id: u16, sender: &str, receiver: &[u8], amount: u128, nonce: u64, signature: &[u8]) -> bool {
        // Trong môi trường production, đây sẽ là một hàm xác thực chữ ký thực sự
        // Sử dụng ed25519 hoặc ECDSA tùy thuộc vào chain
        
        // Kiểm tra xem chain có được hỗ trợ không
        if !self.is_supported_chain(chain_id) {
            return false;
        }
        
        // Lấy public key từ trusted remote
        if let Some(trusted_remote) = self.trusted_remotes.get(&chain_id) {
            // Tạo message cần được ký
            let message = format!("{}:{}:{}:{}", sender, hex::encode(receiver), amount, nonce);
            
            // Trong implementation thực tế, bạn sẽ xác thực chữ ký ở đây
            // Ví dụ: ed25519_dalek::verify(...)
            
            // Trả về true nếu chữ ký hợp lệ
            true
        } else {
            false
        }
    }
    
    /// Thêm địa chỉ vào allowlist
    pub fn add_destination_to_allowlist(&mut self, chain_id: u16, address: String) {
        self.assert_admin();
        
        if !self.destination_allowlist.contains_key(&chain_id) {
            let allowlist = UnorderedMap::new(format!("allowlist:{}", chain_id).as_bytes());
            self.destination_allowlist.insert(&chain_id, &allowlist);
        }
        
        let mut allowlist = self.destination_allowlist.get(&chain_id).unwrap();
        allowlist.insert(&address, &true);
        self.destination_allowlist.insert(&chain_id, &allowlist);
        
        log!("Added {} to allowlist for chain {}", address, chain_id);
    }
    
    /// Xóa địa chỉ khỏi allowlist
    pub fn remove_destination_from_allowlist(&mut self, chain_id: u16, address: String) {
        self.assert_admin();
        
        if let Some(mut allowlist) = self.destination_allowlist.get(&chain_id) {
            allowlist.remove(&address);
            self.destination_allowlist.insert(&chain_id, &allowlist);
            
            log!("Removed {} from allowlist for chain {}", address, chain_id);
        }
    }
    
    /// Kiểm tra xem địa chỉ có trong allowlist không
    pub fn is_destination_allowed(&self, chain_id: u16, address: &str) -> bool {
        if !self.enable_destination_check {
            return true;
        }
        
        if let Some(allowlist) = self.destination_allowlist.get(&chain_id) {
            allowlist.get(&address.to_string()).unwrap_or(false)
        } else {
            false
        }
    }
    
    /// Bật/tắt kiểm tra địa chỉ đích
    pub fn set_destination_check(&mut self, enabled: bool) {
        self.assert_admin();
        self.enable_destination_check = enabled;
        log!("Destination check {}", if enabled { "enabled" } else { "disabled" });
    }
    
    /// Tạo ID giao dịch bridge
    fn generate_bridge_tx_id(&mut self, chain_id: u16, receiver: &[u8], amount: u128) -> String {
        let account_id = env::predecessor_account_id();
        let timestamp = env::block_timestamp();
        self.current_nonce += 1;
        
        // Tạo ID duy nhất cho giao dịch
        format!("{}:{}:{}:{}:{}:{}", 
            account_id, 
            chain_id, 
            hex::encode(receiver), 
            amount, 
            timestamp, 
            self.current_nonce
        )
    }
    
    /// Cải thiện hàm bridge với encoding/decoding tốt hơn
    pub fn wrap_to_chain(&mut self, chain_id: u16, to_address: Vec<u8>, amount: U128, token_type: TokenType) -> Promise {
        assert!(!self.admin_control.bridge_paused, "Bridge is paused");
        let amount: Balance = amount.into();
        assert!(amount > 0, "Amount must be greater than 0");
        
        // Kiểm tra chain được hỗ trợ
        assert!(self.is_supported_chain(chain_id), "Chain not supported");
        
        // Kiểm tra địa chỉ đích
        let dest_hex = hex::encode(&to_address);
        assert!(
            !self.enable_destination_check || self.is_destination_allowed(chain_id, &dest_hex),
            "Destination address not in allowlist"
        );
        
        // Kiểm tra trusted remote đã được thiết lập
        assert!(
            self.trusted_remotes.get(&chain_id).is_some(),
            "Trusted remote not set for this chain"
        );
        
        // Tạo payload chuẩn
        let token_type_value = match token_type {
            TokenType::ERC20 => 0u8,
            TokenType::ERC721(token_id) => 1u8,
            TokenType::ERC1155(token_id) => 2u8,
        };
        
        let token_id = match token_type {
            TokenType::ERC20 => None,
            TokenType::ERC721(id) => Some(U64(id)),
            TokenType::ERC1155(id) => Some(U64(id)),
        };
        
        let account_id = env::predecessor_account_id();
        
        // Tạo ID giao dịch
        let tx_id = self.generate_bridge_tx_id(chain_id, &to_address, amount.0);
        
        // Tạo payload đúng chuẩn ABI
        let payload = BridgePayload {
            token_type: token_type_value,
            source_address: account_id.to_string(),
            destination_address: hex::encode(&to_address),
            amount,
            token_id,
            nonce: self.current_nonce,
            extra_data: None,
        };
        
        // Encode payload theo chuẩn Solidity ABI
        let encoded_payload = payload.to_solidity_abi();
        
        // Tạo đối tượng giao dịch bridge
        let bridge_tx = BridgeTransaction {
            tx_id: tx_id.clone(),
            source_chain: DmdChain::Near,
            destination_chain: self.get_chain_from_id(chain_id),
            sender: account_id.clone(),
            receiver: to_address.clone(),
            amount: amount.0,
            timestamp: env::block_timestamp(),
            nonce: self.current_nonce,
            signature: None,
            status: BridgeStatus::Pending,
            retry_count: 0,
            last_retry: 0,
            expiry_time: env::block_timestamp() + self.bridge_timeout,
            refund_destination: Some(account_id.clone()),
            extra_data: Some(payload.to_simple_string()),
            
            // Thêm các trường mới
            updated_at: 0,
            attempts: Vec::new(),
            processor_info: None,
            timeout_timestamp: 0,
            auto_retry_enabled: false,
            status_history: Vec::new(),
        };
        
        // Lưu thông tin giao dịch
        self.bridge_transactions.insert(&tx_id, &bridge_tx);
        
        // Gọi LayerZero endpoint với payload đã encode đúng chuẩn
        ext_layerzero::send(
            chain_id,
            to_address,
            encoded_payload,
            account_id,
            U64(100000000000000000), // gas for destination
            U64(100000000000000000), // gas for lzreceive
            self.layerzero_endpoint.clone(),
            0, // no deposit
            200000000000000, // gas
        ).then(
            Self::ext(env::current_account_id())
                .with_static_gas(200000000000000)
                .handle_wrap_to_chain_callback(tx_id, chain_id, amount.0)
        )
    }
    
    /// Xử lý callback sau khi gọi LayerZero
    #[private]
    pub fn handle_wrap_to_chain_callback(&mut self, tx_id: String, chain_id: u16, amount: Balance) -> bool {
        let transfer_succeeded = match env::promise_result(0) {
            PromiseResult::Successful(_) => true,
            _ => false,
        };
        
        if let Some(mut bridge_tx) = self.bridge_transactions.get(&tx_id) {
            if transfer_succeeded {
                bridge_tx.status = BridgeStatus::Completed;
                self.bridge_transactions.insert(&tx_id, &bridge_tx);
                log!("Bridge transaction completed: {}", tx_id);
                true
            } else {
                // Xử lý thất bại
                bridge_tx.status = BridgeStatus::Failed {
                    code: 500,
                    reason: "LayerZero transaction failed".to_string(),
                };
                self.bridge_transactions.insert(&tx_id, &bridge_tx);
                
                // Tạo một giao dịch thất bại để theo dõi và hoàn trả
                let failed_tx = FailedTransaction {
                    tx_id: tx_id.clone(),
                    source_chain: DmdChain::Near,
                    refund_account: bridge_tx.sender.clone(),
                    amount: bridge_tx.amount,
                    error: "LayerZero transaction failed".to_string(),
                    timestamp: env::block_timestamp(),
                    retry_count: 0,
                    last_retry: 0,
                    refunded: false,
                    refund_fee: None,
                    
                    // Thông tin mở rộng
                    error_code: Some(500),
                    technical_details: Some("LayerZero communication failure".to_string()),
                    resolution_attempts: vec![
                        ResolutionAttempt {
                            timestamp: env::block_timestamp() / 1_000_000_000,
                            action_type: "auto_retry".to_string(),
                            result: "failed".to_string(),
                            details: Some("LayerZero communication failure".to_string()),
                            executor: Some("system".to_string()),
                        }
                    ],
                    root_cause: Some("LayerZero communication failure".to_string()),
                    suggested_resolution: Some("Manual refund or admin retry required".to_string()),
                    auto_retry_enabled: false,
                };
                
                self.failed_transactions.insert(&tx_id, &failed_tx);
                
                // Hoàn trả token tự động
                self.process_refund(&tx_id);
                
                log!("Bridge transaction failed: {}", tx_id);
                false
            }
        } else {
            log!("Cannot find bridge transaction: {}", tx_id);
            false
        }
    }
    
    /// Xử lý hoàn trả token cho giao dịch thất bại
    pub fn process_refund(&mut self, tx_id: &str) -> bool {
        if let Some(mut failed_tx) = self.failed_transactions.get(&tx_id) {
            if failed_tx.refunded {
                return true; // Đã hoàn trả rồi
            }
            
            if let Some(refund_account) = &failed_tx.refund_account {
                // Hoàn trả token
                let balance = self.accounts.get(&refund_account).unwrap_or(0);
                self.accounts.insert(&refund_account, &(balance + failed_tx.amount));
                
                // Cập nhật locked tokens
                match self.get_chain_id(&failed_tx.source_chain) {
                    LAYERZERO_CHAIN_ID_ETHEREUM => self.locked_tokens.locked_on_ethereum -= failed_tx.amount,
                    LAYERZERO_CHAIN_ID_BSC => self.locked_tokens.locked_on_bsc -= failed_tx.amount,
                    LAYERZERO_CHAIN_ID_SOLANA => self.locked_tokens.locked_on_solana -= failed_tx.amount,
                    LAYERZERO_CHAIN_ID_POLYGON => self.locked_tokens.locked_on_polygon -= failed_tx.amount,
                    LAYERZERO_CHAIN_ID_SUI => self.locked_tokens.locked_on_sui -= failed_tx.amount,
                    LAYERZERO_CHAIN_ID_PI => self.locked_tokens.locked_on_pi -= failed_tx.amount,
                    _ => self.locked_tokens.locked_on_others -= failed_tx.amount,
                }
                
                // Cập nhật tổng cung
                self.total_supply += failed_tx.amount;
                
                // Cập nhật trạng thái
                failed_tx.refunded = true;
                self.failed_transactions.insert(&tx_id, &failed_tx);
                
                // Cập nhật trạng thái giao dịch bridge
                if let Some(mut bridge_tx) = self.bridge_transactions.get(&tx_id) {
                    bridge_tx.status = BridgeStatus::Refunded;
                    self.bridge_transactions.insert(&tx_id, &bridge_tx);
                }
                
                log!(
                    "Refunded {} tokens to {} for failed transaction {}",
                    failed_tx.amount,
                    refund_account,
                    tx_id
                );
                
                true
            } else {
                log!("No refund account for transaction {}", tx_id);
                false
            }
        } else {
            log!("Failed transaction not found: {}", tx_id);
            false
        }
    }
    
    /// Cải tiến hàm nhận token từ chain khác
    pub fn unwrap_from_chain(&mut self, from_chain_id: u16, from_address: Vec<u8>, to_account: AccountId, amount: U128) {
        self.assert_admin();
        assert!(!self.admin_control.bridge_paused, "Bridge is paused");
        
        let amount: Balance = amount.into();
        assert!(amount > 0, "Amount must be greater than 0");
        
        // Kiểm tra chain được hỗ trợ
        assert!(self.is_supported_chain(from_chain_id), "Chain not supported");
        
        // Kiểm tra trusted remote
        if let Some(trusted_remote) = self.trusted_remotes.get(&from_chain_id) {
            assert!(
                trusted_remote == from_address,
                "Source address is not trusted remote"
            );
        } else {
            env::panic_str("Trusted remote not set for this chain");
        }
        
        // Tạo khóa nonce để tránh replay attack
        let nonce_key = format!("{}:{}:{}", from_chain_id, hex::encode(&from_address), env::block_index());
        
        // Kiểm tra nonce đã được xử lý chưa
        assert!(
            !self.processed_nonces.contains_key(&nonce_key),
            "Transaction already processed (potential replay attack)"
        );
        
        // Kiểm tra tài khoản người nhận tồn tại
        assert!(
            env::is_valid_account_id(to_account.as_bytes()),
            "Invalid recipient account"
        );
        
        // Mint token cho người nhận
        let balance = self.accounts.get(&to_account).unwrap_or(0);
        self.accounts.insert(&to_account, &(balance + amount));
        
        // Cập nhật tổng cung
        self.total_supply += amount;
        
        // Cập nhật số lượng token đã unlock theo chain
        match from_chain_id {
            LAYERZERO_CHAIN_ID_ETHEREUM => self.locked_tokens.locked_on_ethereum -= amount,
            LAYERZERO_CHAIN_ID_BSC => self.locked_tokens.locked_on_bsc -= amount,
            LAYERZERO_CHAIN_ID_SOLANA => self.locked_tokens.locked_on_solana -= amount,
            LAYERZERO_CHAIN_ID_POLYGON => self.locked_tokens.locked_on_polygon -= amount,
            LAYERZERO_CHAIN_ID_SUI => self.locked_tokens.locked_on_sui -= amount,
            LAYERZERO_CHAIN_ID_PI => self.locked_tokens.locked_on_pi -= amount,
            _ => self.locked_tokens.locked_on_others -= amount,
        }
        
        // Đánh dấu nonce đã xử lý
        self.processed_nonces.insert(&nonce_key, &true);
        
        // Log sự kiện
        log!(
            "Unwrapped {} tokens from chain {} address {} to {}",
            amount,
            from_chain_id,
            hex::encode(&from_address),
            to_account
        );
    }
    
    /// Cải thiện hàm nhận message với decoding tốt hơn
    pub fn lz_receive(&mut self, src_chain_id: u16, src_address: Vec<u8>, nonce: u64, payload: Vec<u8>) {
        // Kiểm tra xem chain có được hỗ trợ không
        assert!(self.is_supported_chain(src_chain_id), "Chain not supported");
        assert!(!self.admin_control.bridge_paused, "Bridge is paused");
        
        // Kiểm tra trusted remote
        if let Some(trusted_remote) = self.trusted_remotes.get(&src_chain_id) {
            assert!(
                trusted_remote == src_address,
                "Source address is not trusted remote"
            );
        } else {
            env::panic_str("Trusted remote not set for this chain");
        }
        
        // Tạo khóa nonce để tránh replay attack
        let nonce_key = format!("{}:{}:{}", src_chain_id, hex::encode(&src_address), nonce);
        
        // Kiểm tra nonce đã được xử lý chưa
        assert!(
            !self.processed_nonces.contains_key(&nonce_key),
            "Transaction already processed (potential replay attack)"
        );
        
        // Thử decode payload theo cả 2 cách (mới và cũ) để đảm bảo tương thích
        let bridge_payload = match BridgePayload::from_solidity_abi(&payload) {
            Ok(p) => p,
            Err(_) => {
                // Nếu không thể decode theo chuẩn mới, thử decode theo chuẩn cũ
                self.process_legacy_payload(src_chain_id, &payload)
            }
        };
        
        // Lấy thông tin từ payload đã decode
        let account_id = match AccountId::try_from(bridge_payload.source_address) {
            Ok(id) => id,
            Err(_) => env::panic_str("Invalid account ID in payload"),
        };
        
        let amount = bridge_payload.amount.0;
        
        // Mint token cho người nhận
        let balance = self.accounts.get(&account_id).unwrap_or(0);
        self.accounts.insert(&account_id, &(balance + amount));
        
        // Cập nhật tổng cung
        self.total_supply += amount;
        
        // Đánh dấu nonce đã xử lý
        self.processed_nonces.insert(&nonce_key, &true);
        
        // Log sự kiện
        log!(
            "Received {} tokens from chain {} to {} (token type: {})",
            amount,
            src_chain_id,
            account_id,
            bridge_payload.token_type
        );
    }
    
    /// Xử lý payload định dạng cũ
    fn process_legacy_payload(&self, src_chain_id: u16, payload: &[u8]) -> BridgePayload {
        // Giải mã payload định dạng cũ
        let payload_str = String::from_utf8(payload.to_vec())
            .unwrap_or_else(|_| env::panic_str("Invalid UTF-8 in payload"));
        
        // Phân tích payload
        if payload_str.starts_with('{') && payload_str.ends_with('}') {
            // Định dạng JSON cũ
            let parts: Vec<&str> = payload_str
                .trim_start_matches('{')
                .trim_end_matches('}')
                .split(',')
                .collect();
            
            let mut from = "";
            let mut to = "";
            let mut amount = 0u128;
            let mut action = "";
            
            for part in parts {
                let kv: Vec<&str> = part.split(':').collect();
                if kv.len() == 2 {
                    let key = kv[0].trim_matches('"');
                    let value = kv[1].trim_matches('"');
                    
                    match key {
                        "from" => from = value,
                        "to" => to = value,
                        "amount" => amount = value.parse().unwrap_or(0),
                        "action" => action = value,
                        _ => {}
                    }
                }
            }
            
            // Token type based on action
            let token_type = match action {
                "erc20" => 0u8,
                "erc1155" => 2u8,
                _ => 0u8,
            };
            
            BridgePayload {
                token_type,
                source_address: from.to_string(),
                destination_address: to.to_string(),
                amount: U128(amount),
                token_id: None,
                nonce: 0, // No nonce in legacy format
                extra_data: None,
            }
        } else {
            // Định dạng chuỗi đơn giản
            let parts: Vec<&str> = payload_str.split(':').collect();
            
            if parts.len() >= 3 {
                let token_type = parts[0].parse::<u8>().unwrap_or(0);
                let account_id_str = parts[1];
                let amount = parts[2].parse::<u128>().unwrap_or(0);
                
                let token_id = if parts.len() >= 4 {
                    match parts[3].parse::<u64>() {
                        Ok(id) if id > 0 => Some(U64(id)),
                        _ => None,
                    }
                } else {
                    None
                };
                
                BridgePayload {
                    token_type,
                    source_address: account_id_str.to_string(),
                    destination_address: "".to_string(), // Not available in this format
                    amount: U128(amount),
                    token_id,
                    nonce: 0,
                    extra_data: None,
                }
            } else {
                env::panic_str("Invalid payload format");
            }
        }
    }
    
    /// Lấy thông tin các giao dịch thất bại
    pub fn get_failed_transactions(&self, from_index: u64, limit: u64) -> Vec<FailedTransaction> {
        let keys = self.failed_transactions.keys_as_vector();
        let start = from_index as u64;
        let end = min(start + limit as u64, keys.len());
        
        let mut result = Vec::new();
        for i in start..end {
            if let Some(tx_id) = keys.get(i) {
                if let Some(tx) = self.failed_transactions.get(&tx_id) {
                    result.push(tx);
                }
            }
        }
        
        result
    }
    
    /// Lấy thông tin các giao dịch bridge
    pub fn get_bridge_transactions(&self, from_index: u64, limit: u64) -> Vec<BridgeTransaction> {
        let keys = self.bridge_transactions.keys_as_vector();
        let start = from_index as u64;
        let end = min(start + limit as u64, keys.len());
        
        let mut result = Vec::new();
        for i in start..end {
            if let Some(tx_id) = keys.get(i) {
                if let Some(tx) = self.bridge_transactions.get(&tx_id) {
                    result.push(tx);
                }
            }
        }
        
        result
    }
    
    /// Lấy thông tin một giao dịch bridge cụ thể
    pub fn get_bridge_transaction(&self, tx_id: String) -> Option<BridgeTransaction> {
        self.bridge_transactions.get(&tx_id)
    }
    
    /// Lấy thông tin một giao dịch thất bại cụ thể
    pub fn get_failed_transaction(&self, tx_id: String) -> Option<FailedTransaction> {
        self.failed_transactions.get(&tx_id)
    }
    
    /// Kiểm tra và hoàn trả tự động các giao dịch đã hết thời gian
    pub fn process_timed_out_transactions(&mut self) -> u64 {
        let stuck_txs = self.monitor_stuck_transactions();
        
        // Lấy bridge config
        let bridge_config = self.get_bridge_config();
        
        // Nếu config cho phép auto refund, thực hiện hoàn trả
        if bridge_config.auto_refund_on_timeout {
            let mut refunded_count = 0;
            
            for tx_id in &stuck_txs {
                if self.cancel_and_refund_transaction(tx_id.clone()) {
                    refunded_count += 1;
                }
            }
            
            refunded_count
        } else {
            // Thử lại tự động nếu được bật
            self.auto_retry_stuck_transactions(stuck_txs.len() as u64)
        }
    }
    
    /// Chuyển đổi giữa DmdChain và chain ID
    fn get_chain_from_id(&self, chain_id: u16) -> DmdChain {
        match chain_id {
            LAYERZERO_CHAIN_ID_ETHEREUM => DmdChain::Ethereum,
            LAYERZERO_CHAIN_ID_BSC => DmdChain::Bsc,
            LAYERZERO_CHAIN_ID_AVALANCHE => DmdChain::Avalanche,
            LAYERZERO_CHAIN_ID_POLYGON => DmdChain::Polygon,
            LAYERZERO_CHAIN_ID_SOLANA => DmdChain::Solana,
            LAYERZERO_CHAIN_ID_FANTOM => DmdChain::Fantom,
            LAYERZERO_CHAIN_ID_NEAR => DmdChain::Near,
            LAYERZERO_CHAIN_ID_ARBITRUM => DmdChain::Arbitrum,
            LAYERZERO_CHAIN_ID_OPTIMISM => DmdChain::Optimism,
            _ => env::panic_str("Unsupported chain ID"),
        }
    }
    
    /// Chuyển đổi từ DmdChain sang chain ID
    fn get_chain_id(&self, chain: &DmdChain) -> u16 {
        match chain {
            DmdChain::Ethereum => LAYERZERO_CHAIN_ID_ETHEREUM,
            DmdChain::Bsc => LAYERZERO_CHAIN_ID_BSC,
            DmdChain::Avalanche => LAYERZERO_CHAIN_ID_AVALANCHE,
            DmdChain::Polygon => LAYERZERO_CHAIN_ID_POLYGON,
            DmdChain::Solana => LAYERZERO_CHAIN_ID_SOLANA,
            DmdChain::Fantom => LAYERZERO_CHAIN_ID_FANTOM,
            DmdChain::Near => LAYERZERO_CHAIN_ID_NEAR,
            DmdChain::Arbitrum => LAYERZERO_CHAIN_ID_ARBITRUM,
            DmdChain::Optimism => LAYERZERO_CHAIN_ID_OPTIMISM,
        }
    }
    
    /// Hàm để theo dõi và phát hiện các giao dịch bị mắc kẹt
    pub fn monitor_stuck_transactions(&mut self) -> Vec<String> {
        let now = env::block_timestamp();
        let mut stuck_tx_ids = Vec::new();
        
        // Lấy bridge config
        let bridge_config = self.get_bridge_config();
        
        // Duyệt qua các giao dịch đang pending
        let all_txs = self.bridge_transactions.keys_as_vector();
        for i in 0..all_txs.len() {
            if let Some(tx_id) = all_txs.get(i) {
                if let Some(mut tx) = self.bridge_transactions.get(&tx_id) {
                    // Chỉ kiểm tra các giao dịch đang pending
                    if matches!(tx.status, BridgeStatus::Pending) || matches!(tx.status, BridgeStatus::Retrying) {
                        // Kiểm tra xem giao dịch có bị stuck không
                        let elapsed_time = now - tx.timestamp;
                        let is_stuck = elapsed_time > bridge_config.max_transaction_timeout * 1_000_000_000; // Convert to nanoseconds
                        
                        if is_stuck {
                            // Đánh dấu giao dịch là stuck
                            let previous_status = format!("{:?}", tx.status);
                            tx.status = BridgeStatus::Stuck { 
                                since: now / 1_000_000_000, // Convert to seconds
                                previous_status,
                            };
                            
                            // Thêm vào status history
                            tx.status_history.push(StatusChange {
                                timestamp: now / 1_000_000_000,
                                from_status: previous_status,
                                to_status: "Stuck".to_string(),
                                reason: Some("Transaction timeout exceeded".to_string()),
                                changed_by: Some("system_monitor".to_string()),
                            });
                            
                            tx.updated_at = now / 1_000_000_000;
                            
                            // Cập nhật lại trong storage
                            self.bridge_transactions.insert(&tx_id, &tx);
                            
                            // Thêm vào danh sách giao dịch bị stuck
                            stuck_tx_ids.push(tx_id);
                            
                            // Log sự kiện
                            log!(
                                "Transaction {} detected as stuck, elapsed time: {} seconds",
                                tx_id,
                                elapsed_time / 1_000_000_000
                            );
                        }
                    }
                }
            }
        }
        
        stuck_tx_ids
    }
    
    /// Tự động thử lại các giao dịch bị stuck
    pub fn auto_retry_stuck_transactions(&mut self, max_transactions: u64) -> u64 {
        let mut retried_count = 0;
        let now = env::block_timestamp();
        
        // Lấy bridge config
        let bridge_config = self.get_bridge_config();
        
        // Duyệt qua các giao dịch bị stuck
        let all_txs = self.bridge_transactions.keys_as_vector();
        for i in 0..std::cmp::min(all_txs.len(), max_transactions as u64) {
            if let Some(tx_id) = all_txs.get(i) {
                if let Some(mut tx) = self.bridge_transactions.get(&tx_id) {
                    // Chỉ thử lại các giao dịch bị stuck và có auto_retry_enabled = true
                    if let BridgeStatus::Stuck { .. } = tx.status {
                        if tx.auto_retry_enabled && tx.retry_count < bridge_config.max_auto_retry_count {
                            // Tính toán thời gian delay giữa các lần retry
                            let retry_delay_ns = bridge_config.retry_delay as f64 
                                * bridge_config.retry_backoff_factor.powi(tx.retry_count as i32) 
                                * 1_000_000_000.0; // Chuyển sang nanoseconds
                                
                            // Kiểm tra xem đã đến thời gian retry chưa
                            if tx.last_retry + retry_delay_ns as u64 <= now {
                                // Thử lại giao dịch
                                tx.status = BridgeStatus::Retrying;
                                tx.retry_count += 1;
                                tx.last_retry = now;
                                tx.updated_at = now;
                                
                                // Thêm vào status history
                                tx.status_history.push(StatusChange {
                                    timestamp: now / 1_000_000_000,
                                    from_status: "Stuck".to_string(),
                                    to_status: "Retrying".to_string(),
                                    reason: Some(format!("Auto retry #{}", tx.retry_count)),
                                    changed_by: Some("auto_retry".to_string()),
                                });
                                
                                // Thêm vào attempts
                                tx.attempts.push(BridgeAttempt {
                                    timestamp: now / 1_000_000_000,
                                    status: BridgeStatus::Retrying,
                                    tx_hash: None,
                                    error_info: None,
                                    extra_data: Some(format!("Auto retry #{}", tx.retry_count)),
                                });
                                
                                // Cập nhật lại trong storage
                                self.bridge_transactions.insert(&tx_id, &tx);
                                
                                // Gọi hàm retry tương ứng dựa vào chain đích
                                match tx.destination_chain {
                                    DmdChain::Solana => {
                                        // Retry bridge to Solana
                                        self.retry_bridge_to_solana(&tx_id);
                                    },
                                    DmdChain::Ethereum | DmdChain::Bsc | DmdChain::Avalanche |
                                    DmdChain::Polygon | DmdChain::Arbitrum | DmdChain::Optimism |
                                    DmdChain::Fantom => {
                                        // Retry bridge to EVM chain
                                        self.retry_bridge_to_evm(&tx_id);
                                    },
                                    _ => {
                                        // Không hỗ trợ
                                        log!("Unsupported destination chain for retry: {:?}", tx.destination_chain);
                                    }
                                }
                                
                                retried_count += 1;
                                
                                // Log sự kiện
                                log!(
                                    "Auto retrying stuck transaction {}, attempt #{}",
                                    tx_id,
                                    tx.retry_count
                                );
                            }
                        }
                    }
                }
            }
        }
        
        retried_count
    }
    
    /// Thử lại giao dịch bridge tới Solana
    fn retry_bridge_to_solana(&mut self, tx_id: &str) {
        // Lấy thông tin giao dịch
        if let Some(tx) = self.bridge_transactions.get(tx_id) {
            // Gọi lại LayerZero endpoint
            ext_layerzero::send(
                LAYERZERO_CHAIN_ID_SOLANA,
                tx.receiver.clone(),
                self.generate_bridge_payload(&tx),
                env::current_account_id(),
                U64(5000000000000000), // Gas for destination: 5 NEAR
                U64(10000000000000000), // Gas for Receive: 10 NEAR
                &self.layerzero_endpoint,
                1, // 1 yoctoNEAR
                env::prepaid_gas() / 3,
            )
            .then(
                BridgeCallback::new()
                    .with_tx_id(tx_id.to_string())
                    .with_chain_id(LAYERZERO_CHAIN_ID_SOLANA)
                    .with_status(BridgeStatus::Pending)
                    .callback_contract()
                    .handle_retry_bridge_to_solana_callback(),
            );
        }
    }
    
    /// Callback cho retry_bridge_to_solana
    pub fn handle_retry_bridge_to_solana_callback(&mut self, tx_id: String, success: bool, error: Option<String>) {
        if let Some(mut tx) = self.bridge_transactions.get(&tx_id) {
            let now = env::block_timestamp();
            
            if success {
                // Bridge thành công
                tx.status = BridgeStatus::Completed;
                
                // Thêm vào status history
                tx.status_history.push(StatusChange {
                    timestamp: now / 1_000_000_000,
                    from_status: "Retrying".to_string(),
                    to_status: "Completed".to_string(),
                    reason: Some("Retry successful".to_string()),
                    changed_by: Some("system".to_string()),
                });
                
                // Thêm vào attempts
                tx.attempts.push(BridgeAttempt {
                    timestamp: now / 1_000_000_000,
                    status: BridgeStatus::Completed,
                    tx_hash: None, // Sẽ được cập nhật sau
                    error_info: None,
                    extra_data: Some("Retry completed successfully".to_string()),
                });
                
                tx.updated_at = now / 1_000_000_000;
                
                // Cập nhật lại trong storage
                self.bridge_transactions.insert(&tx_id, &tx);
                
                // Log sự kiện
                log!(
                    "Successfully retried bridge transaction {}",
                    tx_id
                );
            } else {
                // Bridge thất bại
                let error_info = error.unwrap_or("Unknown error".to_string());
                
                tx.status = BridgeStatus::Failed {
                    code: 500,
                    reason: error_info.clone(),
                };
                
                // Thêm vào status history
                tx.status_history.push(StatusChange {
                    timestamp: now / 1_000_000_000,
                    from_status: "Retrying".to_string(),
                    to_status: "Failed".to_string(),
                    reason: Some(format!("Retry failed: {}", error_info)),
                    changed_by: Some("system".to_string()),
                });
                
                // Thêm vào attempts
                tx.attempts.push(BridgeAttempt {
                    timestamp: now / 1_000_000_000,
                    status: BridgeStatus::Failed {
                        code: 500,
                        reason: error_info.clone(),
                    },
                    tx_hash: None,
                    error_info: Some(error_info.clone()),
                    extra_data: None,
                });
                
                tx.updated_at = now / 1_000_000_000;
                
                // Cập nhật lại trong storage
                self.bridge_transactions.insert(&tx_id, &tx);
                
                // Đánh dấu là failed transaction để có thể refund
                self.failed_transactions.insert(&tx_id, &FailedTransaction {
                    tx_id: tx_id.clone(),
                    source_chain: tx.source_chain,
                    refund_account: tx.sender.clone(),
                    amount: tx.amount,
                    error: error_info.clone(),
                    timestamp: now / 1_000_000_000,
                    retry_count: tx.retry_count,
                    last_retry: now / 1_000_000_000,
                    refunded: false,
                    refund_fee: None,
                    
                    // Thông tin mở rộng
                    error_code: Some(500),
                    technical_details: Some(format!("LayerZero retry failed: {}", error_info)),
                    resolution_attempts: vec![
                        ResolutionAttempt {
                            timestamp: now / 1_000_000_000,
                            action_type: "auto_retry".to_string(),
                            result: "failed".to_string(),
                            details: Some(error_info),
                            executor: Some("system".to_string()),
                        }
                    ],
                    root_cause: Some("LayerZero communication failure".to_string()),
                    suggested_resolution: Some("Manual refund or admin retry required".to_string()),
                    auto_retry_enabled: false,
                });
                
                // Log sự kiện
                log!(
                    "Failed to retry bridge transaction {}: {}",
                    tx_id,
                    error_info
                );
            }
        }
    }
    
    /// Thử lại giao dịch bridge tới EVM chain
    fn retry_bridge_to_evm(&mut self, tx_id: &str) {
        // Tương tự như retry_bridge_to_solana nhưng dành cho EVM chains
        // ...
        
        // NOTE: Triển khai tương tự như retry_bridge_to_solana
        // Cần xác định chain id cụ thể dựa vào destination_chain
    }
    
    /// Tạo bridge payload
    fn generate_bridge_payload(&self, tx: &BridgeTransaction) -> Vec<u8> {
        // Tạo payload theo định dạng chuẩn
        let bridge_payload = BridgePayload {
            token_type: 0, // ERC20
            source_address: tx.sender.to_string(),
            destination_address: hex::encode(&tx.receiver),
            amount: U128(tx.amount),
            token_id: None,
            nonce: tx.nonce,
            extra_data: tx.extra_data.clone(),
        };
        
        bridge_payload.to_solidity_abi()
    }
    
    /// Hủy giao dịch và hoàn trả token
    pub fn cancel_and_refund_transaction(&mut self, tx_id: String) -> bool {
        // Chỉ admin mới có thể gọi hàm này
        self.assert_admin();
        
        if let Some(mut tx) = self.bridge_transactions.get(&tx_id) {
            // Chỉ có thể hủy các giao dịch đang pending, retrying hoặc stuck
            if matches!(tx.status, BridgeStatus::Pending) 
               || matches!(tx.status, BridgeStatus::Retrying)
               || matches!(tx.status, BridgeStatus::Stuck { .. }) {
                
                let now = env::block_timestamp();
                
                // Chuyển trạng thái sang Refunded
                tx.status = BridgeStatus::Refunded;
                
                // Thêm vào status history
                tx.status_history.push(StatusChange {
                    timestamp: now / 1_000_000_000,
                    from_status: format!("{:?}", tx.status),
                    to_status: "Refunded".to_string(),
                    reason: Some("Manually cancelled by admin".to_string()),
                    changed_by: Some(env::predecessor_account_id().to_string()),
                });
                
                tx.updated_at = now / 1_000_000_000;
                
                // Cập nhật lại trong storage
                self.bridge_transactions.insert(&tx_id, &tx);
                
                // Hoàn trả token cho người dùng
                let balance = self.accounts.get(&tx.sender).unwrap_or(0);
                self.accounts.insert(&tx.sender, &(balance + tx.amount));
                
                // Log sự kiện
                log!(
                    "Transaction {} cancelled and refunded. Amount: {} refunded to {}",
                    tx_id,
                    tx.amount,
                    tx.sender
                );
                
                // Thêm vào failed transactions để theo dõi
                self.failed_transactions.insert(&tx_id, &FailedTransaction {
                    tx_id: tx_id.clone(),
                    source_chain: tx.source_chain,
                    refund_account: tx.sender.clone(),
                    amount: tx.amount,
                    error: "Manually cancelled by admin".to_string(),
                    timestamp: now / 1_000_000_000,
                    retry_count: tx.retry_count,
                    last_retry: tx.last_retry / 1_000_000_000,
                    refunded: true,
                    refund_fee: None,
                    
                    // Thông tin mở rộng
                    error_code: Some(400),
                    technical_details: Some("Transaction manually cancelled by admin".to_string()),
                    resolution_attempts: vec![
                        ResolutionAttempt {
                            timestamp: now / 1_000_000_000,
                            action_type: "manual_cancel".to_string(),
                            result: "success".to_string(),
                            details: Some("Full refund processed".to_string()),
                            executor: Some(env::predecessor_account_id().to_string()),
                        }
                    ],
                    root_cause: Some("Administrative decision".to_string()),
                    suggested_resolution: None,
                    auto_retry_enabled: false,
                });
                
                return true;
            }
        }
        
        false
    }
    
    /// Cho phép người dùng hủy giao dịch của họ
    pub fn user_cancel_transaction(&mut self, tx_id: String) -> bool {
        // Chỉ sender mới có thể hủy giao dịch của họ
        if let Some(tx) = self.bridge_transactions.get(&tx_id) {
        assert_eq!(
            env::predecessor_account_id(), 
                tx.sender,
                "Only the sender can cancel their transaction"
            );
            
            // Lấy bridge config
            let bridge_config = self.get_bridge_config();
            
            // Kiểm tra xem có cho phép người dùng hủy giao dịch không
        assert!(
                bridge_config.allow_user_cancellation,
                "User cancellation is not allowed"
            );
            
            // Chỉ có thể hủy các giao dịch đang pending, retrying hoặc stuck
            if matches!(tx.status, BridgeStatus::Pending) 
               || matches!(tx.status, BridgeStatus::Retrying)
               || matches!(tx.status, BridgeStatus::Stuck { .. }) {
                
                // Gọi hàm hủy và hoàn trả token
                return self.cancel_and_refund_transaction(tx_id);
            }
        }
        
        false
    }
    
    /// Lấy bridge config
    pub fn get_bridge_config(&self) -> BridgeConfig {
        // Trong triển khai thực tế, nên lưu config trong storage
        // Ở đây sử dụng giá trị mặc định cho đơn giản
        BridgeConfig::default()
    }
    
    /// API để người dùng truy vấn trạng thái các giao dịch bridge của mình
    pub fn get_user_transactions(&self, account_id: AccountId, from_index: u64, limit: u64) -> Vec<BridgeTransaction> {
        self.get_user_bridge_transactions(account_id, from_index, limit)
    }
    
    /// API để lấy chi tiết một giao dịch bridge
    pub fn get_transaction_details(&self, tx_id: String) -> Option<BridgeTransaction> {
        self.get_bridge_transaction_details(tx_id)
    }
    
    /// API để lấy số lượng giao dịch bridge của người dùng
    pub fn get_user_transaction_count(&self, account_id: AccountId) -> u64 {
        let all_txs = self.bridge_transactions.keys_as_vector();
        let mut count = 0;
        
        for i in 0..all_txs.len() {
            if let Some(tx_id) = all_txs.get(i) {
                if let Some(tx) = self.bridge_transactions.get(&tx_id) {
                    if tx.sender == account_id {
                        count += 1;
                    }
                }
            }
        }
        
        count
    }
    
    /// API để xem các giao dịch bị stuck
    pub fn get_stuck_transactions(&self, from_index: u64, limit: u64) -> Vec<BridgeTransaction> {
        let all_txs = self.bridge_transactions.keys_as_vector();
        let mut stuck_txs = Vec::new();
        let mut count = 0;
        
        for i in 0..all_txs.len() {
            if stuck_txs.len() >= limit as usize {
                break;
            }
            
            if let Some(tx_id) = all_txs.get(i) {
                if let Some(tx) = self.bridge_transactions.get(&tx_id) {
                    if let BridgeStatus::Stuck { .. } = tx.status {
                        if count >= from_index {
                            stuck_txs.push(tx);
                        }
                        count += 1;
                    }
                }
            }
        }
        
        stuck_txs
    }
    
    /// Lấy thông tin chi tiết về các giao dịch bridge của người dùng
    pub fn get_user_bridge_transactions(&self, account_id: AccountId, from_index: u64, limit: u64) -> Vec<BridgeTransaction> {
        // Lấy tất cả transactions
        let all_txs = self.bridge_transactions.keys_as_vector();
        let total = all_txs.len();
        
        let mut result = Vec::new();
        let mut count = 0;
        
        // Lọc theo user và giới hạn số lượng
        for i in 0..total {
            if let Some(tx_id) = all_txs.get(i) {
                if let Some(tx) = self.bridge_transactions.get(&tx_id) {
                    if tx.sender == account_id {
                        if count >= from_index && result.len() < limit as usize {
                            result.push(tx);
                        }
                        count += 1;
                    }
                }
            }
        }
        
        result
    }
    
    /// Lấy thông tin về một giao dịch bridge
    pub fn get_bridge_transaction_details(&self, tx_id: String) -> Option<BridgeTransaction> {
        self.bridge_transactions.get(&tx_id)
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