use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::{env, near_bindgen, AccountId, Promise, Balance, Gas, PanicOnDefault};
use near_sdk::collections::{LookupMap, UnorderedMap};
use near_sdk::json_types::{U128};
use near_sdk::serde::{Deserialize, Serialize};

// Số lượng gas cho các operations
const GAS_FOR_RESOLVE_TRANSFER: Gas = Gas(10_000_000_000_000);
const GAS_FOR_NFT_TRANSFER_CALL: Gas = Gas(25_000_000_000_000);

// LayerZero chain IDs
const LAYERZERO_CHAIN_ID_ETH: u16 = 1;
const LAYERZERO_CHAIN_ID_BSC: u16 = 2;
const LAYERZERO_CHAIN_ID_SOLANA: u16 = 8;

// Identifier cho token DMD
const DMD_TOKEN_ID: u64 = 0;

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(crate = "near_sdk::serde")]
pub enum DmdChain {
    NEAR,
    Ethereum,
    BSC,
    Solana,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct BridgeMessage {
    pub source_chain: DmdChain,
    pub destination_chain: DmdChain,
    pub sender: String,
    pub receiver: String,
    pub amount: U128,
    pub timestamp: u64,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct LockedTokens {
    pub ethereum: Balance,
    pub bsc: Balance,
    pub solana: Balance,
    pub other_chains: UnorderedMap<String, Balance>,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct AdminControl {
    pub owner: AccountId,
    pub admins: Vec<AccountId>,
    pub bridge_paused: bool,
    pub supported_chains: Vec<DmdChain>,
    pub auto_pause_threshold: Balance,
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct ERC1155Unwrapper {
    // Token metadata
    pub token_name: String,
    pub token_symbol: String,
    
    // Token tổng cung cấp
    pub total_supply: Balance,
    
    // Token đã bị lock cho cross-chain
    pub locked_tokens: LockedTokens,
    
    // Số dư của mỗi tài khoản
    pub accounts: LookupMap<AccountId, Balance>,
    
    // Lịch sử bridge
    pub bridge_history: UnorderedMap<String, BridgeMessage>,
    
    // Quản lý admin
    pub admin_control: AdminControl,
    
    // LayerZero endpoint
    pub lz_endpoint: AccountId,
    
    // Trusted remote addresses (mapping LayerZero chain ID -> address)
    pub trusted_remotes: LookupMap<u16, String>,
}

#[near_bindgen]
impl ERC1155Unwrapper {
    #[init]
    pub fn new(
        token_name: String,
        token_symbol: String,
        total_supply: U128,
        owner_id: AccountId,
        lz_endpoint: AccountId,
    ) -> Self {
        let mut this = Self {
            token_name,
            token_symbol,
            total_supply: total_supply.0,
            locked_tokens: LockedTokens {
                ethereum: 0,
                bsc: 0,
                solana: 0,
                other_chains: UnorderedMap::new(b"o".to_vec()),
            },
            accounts: LookupMap::new(b"a".to_vec()),
            bridge_history: UnorderedMap::new(b"b".to_vec()),
            admin_control: AdminControl {
                owner: owner_id.clone(),
                admins: vec![owner_id.clone()],
                bridge_paused: false,
                supported_chains: vec![DmdChain::Ethereum, DmdChain::BSC, DmdChain::Solana],
                auto_pause_threshold: total_supply.0 / 10, // 10% của tổng supply
            },
            lz_endpoint,
            trusted_remotes: LookupMap::new(b"t".to_vec()),
        };
        
        // Khởi tạo số dư ban đầu cho owner
        this.accounts.insert(&owner_id, &total_supply.0);
        
        this
    }
    
    /// Nhận tin từ bridge và unwrap token (chỉ có thể gọi từ LayerZero endpoint)
    pub fn lz_receive(
        &mut self,
        src_chain_id: u16,
        src_address: String,
        nonce: u64,
        payload: Vec<u8>,
    ) {
        // Chỉ endpoint mới có thể gọi
        assert_eq!(
            env::predecessor_account_id(),
            self.lz_endpoint,
            "Only LayerZero endpoint can call this function"
        );
        
        // Kiểm tra trusted remote
        let trusted_remote = self.trusted_remotes.get(&src_chain_id).expect("Chain not supported");
        assert_eq!(src_address, trusted_remote, "Source not trusted");
        
        // Không thực hiện khi bridge bị tạm dừng
        assert!(!self.admin_control.bridge_paused, "Bridge is paused");
        
        // Giải mã payload
        let (sender, receiver_id_str, amount): (String, String, u128) = 
            near_sdk::serde_json::from_slice(&payload).expect("Invalid payload");
        
        let receiver_id = receiver_id_str.parse::<AccountId>().expect("Invalid receiver");
        
        // Unlock tokens từ bridge
        self.unwrap_from_chain(src_chain_id, &receiver_id, amount);
        
        // Tạo bridge message để lưu lịch sử
        let source_chain = match src_chain_id {
            LAYERZERO_CHAIN_ID_ETH => DmdChain::Ethereum,
            LAYERZERO_CHAIN_ID_BSC => DmdChain::BSC,
            LAYERZERO_CHAIN_ID_SOLANA => DmdChain::Solana,
            _ => panic!("Unsupported chain")
        };
        
        let message = BridgeMessage {
            source_chain,
            destination_chain: DmdChain::NEAR,
            sender,
            receiver: receiver_id.to_string(),
            amount: U128(amount),
            timestamp: env::block_timestamp(),
        };
        
        // Lưu lịch sử
        let history_key = format!("{}_{}", nonce, src_chain_id);
        self.bridge_history.insert(&history_key, &message);
    }
    
    /// Unwrap token từ một chain cụ thể
    fn unwrap_from_chain(&mut self, chain_id: u16, receiver: &AccountId, amount: Balance) {
        // Xác định chain và kiểm tra số lượng locked tokens
        match chain_id {
            LAYERZERO_CHAIN_ID_ETH => {
                assert!(self.locked_tokens.ethereum >= amount, "Insufficient locked tokens");
                self.locked_tokens.ethereum -= amount;
            },
            LAYERZERO_CHAIN_ID_BSC => {
                assert!(self.locked_tokens.bsc >= amount, "Insufficient locked tokens");
                self.locked_tokens.bsc -= amount;
            },
            LAYERZERO_CHAIN_ID_SOLANA => {
                assert!(self.locked_tokens.solana >= amount, "Insufficient locked tokens");
                self.locked_tokens.solana -= amount;
            },
            _ => panic!("Unsupported chain")
        }
        
        // Cộng token cho người nhận
        let current_balance = self.accounts.get(receiver).unwrap_or(0);
        self.accounts.insert(receiver, &(current_balance + amount));
    }
    
    /// Wrap token để chuyển sang chain khác
    pub fn wrap_to_chain(&mut self, chain_id: u16, receiver: String, amount: U128) {
        // Không thực hiện khi bridge bị tạm dừng
        assert!(!self.admin_control.bridge_paused, "Bridge is paused");
        
        let sender = env::predecessor_account_id();
        let amount_u128 = amount.0;
        
        // Kiểm tra số dư
        let sender_balance = self.accounts.get(&sender).unwrap_or(0);
        assert!(sender_balance >= amount_u128, "Insufficient balance");
        
        // Kiểm tra chain được hỗ trợ
        let destination_chain = match chain_id {
            LAYERZERO_CHAIN_ID_ETH => DmdChain::Ethereum,
            LAYERZERO_CHAIN_ID_BSC => DmdChain::BSC,
            LAYERZERO_CHAIN_ID_SOLANA => DmdChain::Solana,
            _ => panic!("Unsupported chain")
        };
        
        assert!(
            self.admin_control.supported_chains.contains(&destination_chain),
            "Chain not supported"
        );
        
        // Lock tokens
        match chain_id {
            LAYERZERO_CHAIN_ID_ETH => {
                self.locked_tokens.ethereum += amount_u128;
            },
            LAYERZERO_CHAIN_ID_BSC => {
                self.locked_tokens.bsc += amount_u128;
            },
            LAYERZERO_CHAIN_ID_SOLANA => {
                self.locked_tokens.solana += amount_u128;
            },
            _ => panic!("Unsupported chain")
        }
        
        // Trừ số dư của người gửi
        self.accounts.insert(&sender, &(sender_balance - amount_u128));
        
        // Payload cho LayerZero
        let payload = near_sdk::serde_json::to_vec(&(
            sender.to_string(),
            receiver,
            amount_u128
        )).expect("Failed to serialize payload");
        
        // Gọi endpoint để gửi message
        // Code gọi endpoint LayerZero sẽ được implement tùy thuộc vào contract LayerZero trên NEAR
    }
    
    /// Bridge sang BSC
    pub fn bridge_to_bsc(&mut self, receiver: String, amount: U128) {
        self.wrap_to_chain(LAYERZERO_CHAIN_ID_BSC, receiver, amount);
    }
    
    /// Bridge sang Ethereum
    pub fn bridge_to_ethereum(&mut self, receiver: String, amount: U128) {
        self.wrap_to_chain(LAYERZERO_CHAIN_ID_ETH, receiver, amount);
    }
    
    /// Bridge sang Solana
    pub fn bridge_to_solana(&mut self, receiver: String, amount: U128) {
        self.wrap_to_chain(LAYERZERO_CHAIN_ID_SOLANA, receiver, amount);
    }
    
    /// Lấy số lượng tokens đã lock cho một chain
    pub fn get_locked_tokens(&self, chain: DmdChain) -> U128 {
        match chain {
            DmdChain::NEAR => U128(0), // Không có lock trên NEAR
            DmdChain::Ethereum => U128(self.locked_tokens.ethereum),
            DmdChain::BSC => U128(self.locked_tokens.bsc),
            DmdChain::Solana => U128(self.locked_tokens.solana),
        }
    }
    
    /// Lấy tổng số tokens đã lock
    pub fn get_total_locked(&self) -> U128 {
        let mut total = self.locked_tokens.ethereum + self.locked_tokens.bsc + self.locked_tokens.solana;
        
        // Thêm các chain khác
        for (_, amount) in self.locked_tokens.other_chains.iter() {
            total += amount;
        }
        
        U128(total)
    }
    
    /// Lấy lượng token đang lưu hành (total supply - locked)
    pub fn get_circulating_supply(&self) -> U128 {
        let total_locked = self.get_total_locked().0;
        U128(self.total_supply - total_locked)
    }
    
    /// Cài đặt trusted remote cho một chain
    pub fn set_trusted_remote(&mut self, chain_id: u16, remote_address: String) {
        assert_eq!(
            env::predecessor_account_id(),
            self.admin_control.owner,
            "Only owner can set trusted remote"
        );
        
        self.trusted_remotes.insert(&chain_id, &remote_address);
    }
    
    /// Cài đặt trạng thái của bridge
    pub fn set_bridge_status(&mut self, paused: bool) {
        assert!(
            self.admin_control.admins.contains(&env::predecessor_account_id()),
            "Only admin can set bridge status"
        );
        
        self.admin_control.bridge_paused = paused;
    }
    
    /// Lấy trạng thái của bridge
    pub fn is_bridge_paused(&self) -> bool {
        self.admin_control.bridge_paused
    }
}
