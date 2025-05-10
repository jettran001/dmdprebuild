use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::{env, near_bindgen, AccountId, Promise, Balance, Gas, PanicOnDefault};
use near_sdk::collections::{LookupMap, UnorderedMap};
use near_sdk::json_types::{U128};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::json_types::{U128};
use near_sdk::serde::{Deserialize, Serialize};

// Gas amounts for operations
const GAS_FOR_RESOLVE_TRANSFER: Gas = Gas(10_000_000_000_000);
const GAS_FOR_NFT_TRANSFER_CALL: Gas = Gas(25_000_000_000_000);
const GAS_FOR_RETRY_OPERATION: Gas = Gas(15_000_000_000_000);

// LayerZero chain IDs - updated to match bridge_interface.sol
const LAYERZERO_CHAIN_ID_ETH: u16 = 101;
const LAYERZERO_CHAIN_ID_BSC: u16 = 102;
const LAYERZERO_CHAIN_ID_POLYGON: u16 = 109;
const LAYERZERO_CHAIN_ID_AVALANCHE: u16 = 106;
const LAYERZERO_CHAIN_ID_ARBITRUM: u16 = 110;
const LAYERZERO_CHAIN_ID_OPTIMISM: u16 = 111;
const LAYERZERO_CHAIN_ID_BASE: u16 = 184;
const LAYERZERO_CHAIN_ID_FANTOM: u16 = 112;
const LAYERZERO_CHAIN_ID_MOONBEAM: u16 = 126;
const LAYERZERO_CHAIN_ID_MOONRIVER: u16 = 167;
const LAYERZERO_CHAIN_ID_SOLANA: u16 = 168;
const LAYERZERO_CHAIN_ID_NEAR: u16 = 118;

// Identifier for DMD token
const DMD_TOKEN_ID: u64 = 0;

// Expiry window for transactions
const EXPIRY_WINDOW: u64 = 3600 * 1000000000; // 1 hour in nanoseconds

// Token types matching bridge_interface.sol
const TOKEN_TYPE_ERC20: u8 = 1;
const TOKEN_TYPE_ERC1155: u8 = 2; 
const TOKEN_TYPE_NATIVE: u8 = 3;
const TOKEN_TYPE_ERC721: u8 = 4;

// Retry timeout
const RETRY_TIMEOUT: u64 = 86400 * 1000000000; // 1 day in nanoseconds
const MAX_RETRY_ATTEMPTS: u8 = 5; // Maximum retry attempts

// Bridge operation status (đồng bộ với bridge_interface.sol)
const STATUS_PENDING: &str = "Pending";
const STATUS_COMPLETED: &str = "Completed";
const STATUS_FAILED: &str = "Failed";
const STATUS_RETRYING: &str = "Retrying";
const STATUS_EXPIRED: &str = "Expired";

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(crate = "near_sdk::serde")]
pub enum DmdChain {
    NEAR,
    Ethereum,
    BSC,
    Polygon,
    Avalanche,
    Arbitrum,
    Optimism,
    Base,
    Fantom,
    Moonbeam,
    Moonriver,
    Solana,
}

// Updated to match bridge_interface.sol payload structure
#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct BridgePayload {
    pub sender: String,
    pub destination: String,
    pub amount: U128,
    pub timestamp: u64,
    pub nonce: u64,
    pub token_address: Option<String>,     // ERC20 token address or None for native token
    pub token_type: u8,                    // 1=ERC20, 2=ERC1155, 3=Native, 4=ERC721
    pub unwrapper: Option<String>,         // Address of unwrapper (if auto-unwrap needed)
    pub need_unwrap: Option<bool>,         // Flag to indicate unwrap on destination
    pub extra_data: Option<String>,        // Additional data for future compatibility
}

// Struct để lưu trữ bridge operation (đồng bộ với bridge_interface.sol)
#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct BridgeOperation {
    pub bridge_id: String,
    pub sender: String,
    pub token_address: Option<String>,
    pub destination_chain_id: u16,
    pub destination_address: String,
    pub amount: U128,
    pub timestamp: u64,
    pub completed: bool,
    pub status: String,
    pub related_op_id: Option<String>,
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
    pub nonce: u64,
    pub signature: Option<String>,
    pub status: TransactionStatus,
    pub token_type: u8,               // Added for token_type support
    pub token_address: Option<String>, // Added for token_address support
    pub bridge_id: Option<String>,     // Added for consistent tracking
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(crate = "near_sdk::serde")]
pub enum TransactionStatus {
    Pending,
    Completed,
    Failed(String),
    Retrying,
    Expired,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct LockedTokens {
    pub ethereum: Balance,
    pub bsc: Balance,
    pub polygon: Balance,
    pub avalanche: Balance,
    pub arbitrum: Balance,
    pub optimism: Balance,
    pub base: Balance,
    pub fantom: Balance,
    pub moonbeam: Balance,
    pub moonriver: Balance,
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
    pub max_retry_attempts: u8,
    pub transaction_limit: Balance,
    pub daily_limit: Balance,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct FailedTransaction {
    pub transaction_id: String,
    pub source_chain: DmdChain,
    pub receiver: AccountId,
    pub amount: Balance,
    pub error: String,
    pub timestamp: u64,
    pub retry_count: u8,
    pub last_retry: u64,
    pub token_type: u8,               // Added for token_type support
    pub token_address: Option<String>, // Added for token_address support
    pub bridge_id: Option<String>,     // Added for consistent tracking
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct ERC1155Unwrapper {
    // Token metadata
    pub token_name: String,
    pub token_symbol: String,
    
    // Total token supply
    pub total_supply: Balance,
    
    // Locked tokens for cross-chain
    pub locked_tokens: LockedTokens,
    
    // User balances
    pub accounts: LookupMap<AccountId, Balance>,
    
    // Bridge history
    pub bridge_history: UnorderedMap<String, BridgeMessage>,
    
    // Bridge operations (đồng bộ với bridge_interface.sol)
    pub bridge_operations: UnorderedMap<String, BridgeOperation>,
    
    // Admin control
    pub admin_control: AdminControl,
    
    // LayerZero endpoint
    pub lz_endpoint: AccountId,
    
    // Trusted remote addresses (mapping LayerZero chain ID -> address)
    pub trusted_remotes: LookupMap<u16, String>,
    
    // Processed nonces (for replay protection)
    pub processed_nonces: LookupMap<String, bool>,
    
    // Failed transactions
    pub failed_transactions: UnorderedMap<String, FailedTransaction>,
    
    // Current nonce for outgoing transactions
    pub current_nonce: u64,
    
    // Supported token types
    pub supported_token_types: Vec<u8>,
    
    // Supported token addresses
    pub supported_tokens: LookupMap<String, bool>,
    
    // Daily volumes
    pub daily_volumes: LookupMap<String, Balance>,
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
                polygon: 0,
                avalanche: 0,
                arbitrum: 0,
                optimism: 0,
                base: 0,
                fantom: 0,
                moonbeam: 0,
                moonriver: 0,
                solana: 0,
                other_chains: UnorderedMap::new(b"o".to_vec()),
            },
            accounts: LookupMap::new(b"a".to_vec()),
            bridge_history: UnorderedMap::new(b"b".to_vec()),
            bridge_operations: UnorderedMap::new(b"bo".to_vec()),
            admin_control: AdminControl {
                owner: owner_id.clone(),
                admins: vec![owner_id.clone()],
                bridge_paused: false,
                supported_chains: vec![
                    DmdChain::Ethereum, 
                    DmdChain::BSC, 
                    DmdChain::Polygon,
                    DmdChain::Avalanche,
                    DmdChain::Arbitrum,
                    DmdChain::Optimism,
                ],
                auto_pause_threshold: total_supply.0 / 10, // 10% of total supply
                max_retry_attempts: 3,
                transaction_limit: total_supply.0 / 100, // 1% of total supply per tx
                daily_limit: total_supply.0 / 20, // 5% of total supply per day
            },
            lz_endpoint,
            trusted_remotes: LookupMap::new(b"t".to_vec()),
            processed_nonces: LookupMap::new(b"p".to_vec()),
            failed_transactions: UnorderedMap::new(b"f".to_vec()),
            current_nonce: 0,
            supported_token_types: vec![TOKEN_TYPE_ERC20, TOKEN_TYPE_ERC1155, TOKEN_TYPE_NATIVE],
            supported_tokens: LookupMap::new(b"st".to_vec()),
            daily_volumes: LookupMap::new(b"dv".to_vec()),
        };
        
        // Initialize owner balance
        this.accounts.insert(&owner_id, &total_supply.0);
        
        this
    }
    
    /**
     * @dev Generates a unique bridge ID similar to bridge_interface.sol
     */
    fn generate_bridge_id(&self, sender: &str, chain_id: u16, amount: Balance, nonce: u64, timestamp: u64) -> String {
        let input = format!("{}{}{}{}{}{}", sender, chain_id, amount, nonce, timestamp, env::block_index());
        format!("{:x}", env::sha256(input.as_bytes()))
    }
    
    /**
     * @dev Records a bridge operation, synchronized with bridge_interface.sol
     */
    fn record_bridge_operation(
        &mut self, 
        bridge_id: String, 
        sender: String,
        token_address: Option<String>,
        destination_chain_id: u16,
        destination_address: String,
        amount: U128,
        status: String
    ) {
        let operation = BridgeOperation {
            bridge_id: bridge_id.clone(),
            sender,
            token_address,
            destination_chain_id,
            destination_address,
            amount,
            timestamp: env::block_timestamp(),
            completed: status == STATUS_COMPLETED,
            status,
            related_op_id: None,
        };
        
        self.bridge_operations.insert(&bridge_id, &operation);
    }
    
    /**
     * @dev Updates a bridge operation status
     */
    fn update_bridge_status(&mut self, bridge_id: &str, status: String, completed: bool) {
        if let Some(mut operation) = self.bridge_operations.get(bridge_id) {
            operation.status = status;
            operation.completed = completed;
            self.bridge_operations.insert(bridge_id, &operation);
        }
    }
    
    /**
     * @dev Handles incoming message from LayerZero endpoint
     * Updated to match bridge_interface.sol payload format
     */
    pub fn lz_receive(
        &mut self,
        src_chain_id: u16,
        src_address: String,
        nonce: u64,
        payload: Vec<u8>,
    ) {
        // Verify not paused
        if self.admin_control.bridge_paused {
            env::panic_str("Bridge is paused");
        }
        
        // Verify trusted remote
        if !self.trusted_remotes.contains_key(&src_chain_id) {
            env::panic_str("Untrusted source chain");
        }
        
        let trusted_remote = self.trusted_remotes.get(&src_chain_id).unwrap();
        if trusted_remote != src_address {
            env::panic_str("Untrusted source address");
        }
        
        // Verify nonce not processed already (replay protection)
        let nonce_key = format!("{}:{}", src_chain_id, nonce);
        if self.processed_nonces.contains_key(&nonce_key) {
            env::panic_str("Nonce already processed");
        }
        self.processed_nonces.insert(&nonce_key, &true);
        
        // Decode payload - match new format from bridge_interface.sol
        let bridge_payload = match self.decode_payload(&payload) {
            Ok(payload) => payload,
            Err(error) => {
                // Record failed transaction with detailed error
                self.record_failed_transaction(
                    format!("failed_decode_{}_{}_{}", src_chain_id, nonce, env::block_timestamp()),
                    src_chain_id,
                    "unknown_receiver".to_string(),
                    0,
                    format!("Payload decode error: {}", error),
                    TOKEN_TYPE_ERC20,
                    None
                );
                env::panic_str(&format!("Failed to decode payload: {}", error));
            }
        };
        
        // Check timestamp expiration - reject if too old
        if env::block_timestamp() > bridge_payload.timestamp + EXPIRY_WINDOW {
            // Record expired transaction
            let bridge_id = self.generate_bridge_id(
                &bridge_payload.sender, 
                src_chain_id, 
                bridge_payload.amount.0, 
                nonce, 
                bridge_payload.timestamp
            );
            
            self.record_bridge_operation(
                bridge_id.clone(),
                bridge_payload.sender.clone(),
                bridge_payload.token_address.clone(),
                src_chain_id,
                bridge_payload.destination.clone(),
                bridge_payload.amount,
                STATUS_EXPIRED.to_string()
            );
            
            env::panic_str("Transaction expired, too old");
        }
        
        // Check rate limits
        if bridge_payload.amount.0 > self.admin_control.transaction_limit {
            env::panic_str("Amount exceeds transaction limit");
        }
        
        // Check token support
        let token_type = bridge_payload.token_type;
        if !self.supported_token_types.contains(&token_type) {
            env::panic_str("Unsupported token type");
        }
        
        if let Some(token_address) = &bridge_payload.token_address {
            if !self.is_token_supported(token_address.clone()) {
                env::panic_str("Unsupported token address");
            }
        }
        
        // Get receiver account
        let receiver_str = bridge_payload.destination.clone();
        let receiver = match AccountId::try_from(receiver_str.clone()) {
            Ok(account) => account,
            Err(_) => {
                // Record failed transaction with invalid receiver
                    self.record_failed_transaction(
                    format!("failed_invalid_receiver_{}_{}_{}", src_chain_id, nonce, env::block_timestamp()),
                        src_chain_id,
                    receiver_str.clone(),
                        bridge_payload.amount.0,
                        "Invalid receiver account ID".to_string(),
                    token_type,
                    bridge_payload.token_address.clone()
                );
                env::panic_str("Invalid receiver account ID");
            }
        };
        
        // Generate bridge ID consistent with the Solidity contract
        let bridge_id = self.generate_bridge_id(
            &bridge_payload.sender, 
                    src_chain_id, 
                    bridge_payload.amount.0,
            nonce, 
            bridge_payload.timestamp
        );
        
        // Record the bridge operation
        self.record_bridge_operation(
            bridge_id.clone(),
            bridge_payload.sender.clone(),
            bridge_payload.token_address.clone(),
            src_chain_id,
            receiver_str.clone(),
            bridge_payload.amount,
            STATUS_PENDING.to_string()
        );
        
        // Try to process the unwrap
        match self.try_unwrap_from_chain(src_chain_id, &receiver, bridge_payload.amount.0, token_type) {
            Ok(_) => {
                // Record success
                self.update_bridge_status(&bridge_id, STATUS_COMPLETED.to_string(), true);
                
                // Update bridge history
                let chain = self.get_chain_from_id(src_chain_id);
                let bridge_message = BridgeMessage {
                    source_chain: chain,
                    destination_chain: DmdChain::NEAR,
                    sender: bridge_payload.sender,
                    receiver: receiver.to_string(),
                    amount: bridge_payload.amount,
                    timestamp: env::block_timestamp(),
                    nonce,
                    signature: None,
                    status: TransactionStatus::Completed,
                    token_type,
                    token_address: bridge_payload.token_address,
                    bridge_id: Some(bridge_id),
                };
                
                self.bridge_history.insert(&format!("{}:{}", src_chain_id, nonce), &bridge_message);
                
                // Check if auto-unwrap is requested
                if let (Some(true), Some(unwrapper)) = (bridge_payload.need_unwrap, bridge_payload.unwrapper) {
                    // Process auto-unwrap if requested
                    match self.auto_unwrap(&receiver, bridge_payload.amount.0, &unwrapper, &bridge_id) {
                        Ok(_) => {
                            env::log_str(&format!(
                                "Auto-unwrap successful for {} tokens to {}", 
                                bridge_payload.amount.0, receiver
                            ));
                        },
                        Err(error) => {
                            env::log_str(&format!(
                                "Auto-unwrap failed for {} tokens to {}: {}", 
                                bridge_payload.amount.0, receiver, error
                            ));
                            
                            // Note: We still consider the bridge operation successful even if auto-unwrap fails
                            // The tokens are still received in the wrapped form
                        }
                    }
                }
            },
            Err(error) => {
                // Record failure
                self.update_bridge_status(&bridge_id, format!("{}: {}", STATUS_FAILED, error), false);
                
                // Record failed transaction for retry
                    self.record_failed_transaction(
                    bridge_id.clone(),
                        src_chain_id,
                    receiver.to_string(),
                        bridge_payload.amount.0,
                        error.clone(),
                    token_type,
                    bridge_payload.token_address.clone()
                    );
                
                // Update bridge history
                let chain = self.get_chain_from_id(src_chain_id);
                let bridge_message = BridgeMessage {
                    source_chain: chain,
                    destination_chain: DmdChain::NEAR,
                    sender: bridge_payload.sender,
                    receiver: receiver.to_string(),
                    amount: bridge_payload.amount,
                    timestamp: env::block_timestamp(),
                    nonce,
                    signature: None,
                    status: TransactionStatus::Failed(error.clone()),
                    token_type,
                    token_address: bridge_payload.token_address,
                    bridge_id: Some(bridge_id),
                };
                
                self.bridge_history.insert(&format!("{}:{}", src_chain_id, nonce), &bridge_message);
                
                env::panic_str(&format!("Failed to unwrap token: {}", error));
            }
        }
    }
    
    /**
     * @dev Decodes payload from the bridge_interface.sol format
     * Updated to support new payload format with unwrapper details
     */
    fn decode_payload(&self, payload: &[u8]) -> Result<BridgePayload, String> {
        if payload.len() < 64 {
            return Err("Payload too short".to_string());
        }
        
        // First try to parse as JSON (preferred format)
        let payload_str = String::from_utf8(payload.to_vec())
            .map_err(|_| "Invalid UTF-8 in payload".to_string())?;
            
        match near_sdk::serde_json::from_str::<BridgePayload>(&payload_str) {
            Ok(decoded) => Ok(decoded),
            Err(json_err) => {
                // If JSON parsing fails, try to parse as binary ABI encoded data
                env::log_str(&format!("JSON parsing failed: {}, trying binary format", json_err));
                self.decode_binary_payload(payload)
            }
        }
    }
    
    /**
     * @dev Decode payload in binary format (compatible with bridge_interface.sol)
     * Updated to support multiple payload formats from different versions
     */
    fn decode_binary_payload(&self, payload: &[u8]) -> Result<BridgePayload, String> {
        if payload.len() < 64 {
            return Err("Payload too short".to_string());
        }

        // Try to parse as ABI encoded payload (format from bridge_interface.sol)
        // Expected format (abi.encode):
        // [0-31]: address sender
        // [32-63]: bytes destination
        // [64-95]: uint256 amount
        // [96-127]: uint256 timestamp
        // [128-159]: address unwrapper (optional)
        // [160-191]: bool needUnwrap (optional)
        // [192-223]: uint8 tokenType (optional)
        
        // Read sender (first 32 bytes, but we need last 20 bytes of it)
        let sender_offset = if payload.len() >= 32 { 32 - 20 } else { 0 };
        let mut sender_bytes = [0u8; 20];
        for i in 0..20 {
            if sender_offset + i < payload.len() {
                sender_bytes[i] = payload[sender_offset + i];
            }
        }
        let sender = format!("0x{}", hex::encode(sender_bytes));
        
        // Read destination (next 32 bytes contain offset to bytes array, then length, then actual bytes)
        // For simplicity, we'll try to extract an address-like value
        let mut dest_bytes = [0u8; 20];
        if payload.len() >= 64 {
            // Try to get destination from second parameter (assuming it's an address in bytes format)
            // This is simplified logic; in production, handle dynamic types properly
            for i in 0..20 {
                if 32 + i < payload.len() {
                    dest_bytes[i] = payload[32 + i];
                }
            }
        }
        let destination = format!("0x{}", hex::encode(dest_bytes));
        
        // Read amount (next 32 bytes)
        let mut amount: u128 = 0;
        if payload.len() >= 96 {
            let mut amount_bytes = [0u8; 32];
            amount_bytes.copy_from_slice(&payload[64..96]);
            // Convert big-endian bytes to u128
            for i in 0..32 {
                amount = (amount << 8) | (amount_bytes[i] as u128);
            }
        }
        
        // Read timestamp (next 32 bytes if available)
        let timestamp = if payload.len() >= 128 {
            let mut timestamp_bytes = [0u8; 32];
            timestamp_bytes.copy_from_slice(&payload[96..128]);
            // Convert big-endian bytes to u64
            let mut ts: u64 = 0;
            for i in 0..32 {
                ts = (ts << 8) | (timestamp_bytes[i] as u64);
            }
            ts
        } else {
            env::block_timestamp() // Default to current timestamp if not provided
        };
        
        // Read unwrapper address (next 32 bytes if available)
        let unwrapper = if payload.len() >= 160 {
            let mut unwrapper_bytes = [0u8; 20];
            // Extract the last 20 bytes from the 32-byte field
            for i in 0..20 {
                unwrapper_bytes[i] = payload[128 + 12 + i];
            }
            // Check if non-zero
            if unwrapper_bytes.iter().any(|&b| b != 0) {
                Some(format!("0x{}", hex::encode(unwrapper_bytes)))
            } else {
                None
            }
        } else {
            None
        };
        
        // Read needUnwrap flag (next 32 bytes if available)
        let need_unwrap = if payload.len() >= 192 {
            // In Solidity, bool is represented as uint8 padded to 32 bytes
            // The value is at the last byte
            Some(payload[191] != 0)
        } else {
            None
        };
        
        // Read token type (next 32 bytes if available)
        let token_type = if payload.len() >= 224 {
            // Token type is stored as uint8 padded to 32 bytes
            // The value is at the last byte
            payload[223]
        } else {
            TOKEN_TYPE_ERC20 // Default to ERC20 if not specified
        };
        
        // Determine nonce (use a random value if not provided)
        let nonce = env::block_index(); // Use block index as nonce
        
        // Default token address to None
        let token_address = None;
        
        Ok(BridgePayload {
            sender,
            destination,
            amount: U128(amount),
            timestamp,
            nonce,
            token_address,
            token_type,
            unwrapper,
            need_unwrap,
            extra_data: None,
        })
    }
    
    /**
     * @dev Tries to unwrap token from a specific chain
     * Updated to match bridge_interface.sol
     */
    fn try_unwrap_from_chain(&mut self, chain_id: u16, receiver: &AccountId, amount: Balance, token_type: u8) -> Result<(), String> {
        // Check if chain is supported
        if !self.is_chain_supported(chain_id) {
            return Err("Chain not supported".to_string());
        }
        
        // Check amount against rate limits and transaction limits
        if amount > self.admin_control.transaction_limit {
            return Err("Amount exceeds transaction limit".to_string());
        }
        
        // Check daily rate limit
        let current_day = env::block_timestamp() / (24 * 60 * 60 * 1_000_000_000);
        let day_key = format!("day:{}", current_day);
        let mut daily_volume = self.daily_volumes.get(&day_key).unwrap_or(0);
        
        if daily_volume + amount > self.admin_control.daily_limit {
            return Err("Daily limit exceeded".to_string());
        }
        
        // Update daily volume
        daily_volume += amount;
        self.daily_volumes.insert(&day_key, &daily_volume);
        
        // Check token type support
        if !self.supported_token_types.contains(&token_type) {
            return Err(format!("Unsupported token type: {}", token_type));
        }
        
        // Update balances
        let receiver_balance = self.accounts.get(receiver).unwrap_or(0);
        self.accounts.insert(receiver, &(receiver_balance + amount));
        
        // Update locked tokens tracking
        let mut locked_tokens = self.locked_tokens.clone();
        match self.get_chain_from_id(chain_id) {
            DmdChain::Ethereum => {
                if locked_tokens.ethereum < amount {
                    return Err("Insufficient locked tokens for Ethereum".to_string());
                }
                locked_tokens.ethereum -= amount;
            },
            DmdChain::BSC => {
                if locked_tokens.bsc < amount {
                    return Err("Insufficient locked tokens for BSC".to_string());
                }
                locked_tokens.bsc -= amount;
            },
            DmdChain::Polygon => {
                if locked_tokens.polygon < amount {
                    return Err("Insufficient locked tokens for Polygon".to_string());
                }
                locked_tokens.polygon -= amount;
            },
            DmdChain::Avalanche => {
                if locked_tokens.avalanche < amount {
                    return Err("Insufficient locked tokens for Avalanche".to_string());
                }
                locked_tokens.avalanche -= amount;
            },
            DmdChain::Arbitrum => {
                if locked_tokens.arbitrum < amount {
                    return Err("Insufficient locked tokens for Arbitrum".to_string());
                }
                locked_tokens.arbitrum -= amount;
            },
            DmdChain::Optimism => {
                if locked_tokens.optimism < amount {
                    return Err("Insufficient locked tokens for Optimism".to_string());
                }
                locked_tokens.optimism -= amount;
            },
            DmdChain::Base => {
                if locked_tokens.base < amount {
                    return Err("Insufficient locked tokens for Base".to_string());
                }
                locked_tokens.base -= amount;
            },
            DmdChain::Fantom => {
                if locked_tokens.fantom < amount {
                    return Err("Insufficient locked tokens for Fantom".to_string());
                }
                locked_tokens.fantom -= amount;
            },
            DmdChain::Moonbeam => {
                if locked_tokens.moonbeam < amount {
                    return Err("Insufficient locked tokens for Moonbeam".to_string());
                }
                locked_tokens.moonbeam -= amount;
            },
            DmdChain::Moonriver => {
                if locked_tokens.moonriver < amount {
                    return Err("Insufficient locked tokens for Moonriver".to_string());
                }
                locked_tokens.moonriver -= amount;
            },
            DmdChain::Solana => {
                if locked_tokens.solana < amount {
                    return Err("Insufficient locked tokens for Solana".to_string());
                }
                locked_tokens.solana -= amount;
            },
            _ => {
                let chain_key = format!("chain_{}", chain_id);
                let current = locked_tokens.other_chains.get(&chain_key).unwrap_or(0);
                if current < amount {
                    return Err(format!("Insufficient locked tokens for chain {}", chain_id));
                }
                locked_tokens.other_chains.insert(&chain_key, &(current - amount));
            }
        }
        self.locked_tokens = locked_tokens;
        
        // Log success
        env::log_str(&format!(
            "Unwrapped {} tokens for {} from chain {} with token type {}",
            amount, receiver, chain_id, token_type
        ));
        
        Ok(())
    }
    
    /**
     * @dev New function to handle auto-unwrap for tokens
     * Compatible with bridgeAndUnwrap in bridge_interface.sol
     */
    fn auto_unwrap(&mut self, receiver: &AccountId, amount: Balance, unwrapper: &str, bridge_id: &str) -> Result<(), String> {
        // Log the auto-unwrap attempt
        env::log_str(&format!(
            "Auto-unwrap requested for {} tokens to {} via unwrapper {}",
            amount, receiver, unwrapper
        ));
        
        // Here you would implement the actual unwrap mechanism based on your token system
        // For example, you might call a cross-contract call to the unwrapper contract
        
        // For now, we'll update the bridge operation status
        let unwrap_op_id = format!("auto_unwrap_{}", bridge_id);
        
        // Record the unwrap operation
        self.record_bridge_operation(
            unwrap_op_id.clone(),
            receiver.to_string(),
            None, // No specific token address for unwrap
            0, // No destination chain for unwrap
            receiver.to_string(),
            U128(amount),
            "AUTO_UNWRAP_STARTED".to_string()
        );
        
        // Simulate unwrap success for this example
        // In a real implementation, you would handle actual token unwrapping
        self.update_bridge_status(&unwrap_op_id, "AUTO_UNWRAP_COMPLETED".to_string(), true);
        
        Ok(())
    }
    
    /**
     * @dev Records a failed transaction for later retry
     * Updated to match bridge_interface.sol tracking system
     */
    fn record_failed_transaction(
        &mut self,
        transaction_id: String,
        source_chain_id: u16,
        receiver_str: String,
        amount: Balance,
        error: String,
        token_type: u8,
        token_address: Option<String>
    ) {
        let source_chain = self.get_chain_from_id(source_chain_id);
        
        let receiver = match AccountId::try_from(receiver_str.clone()) {
            Ok(account) => account,
            Err(_) => {
                // If receiver is invalid, use owner as fallback
                self.admin_control.owner.clone()
            }
        };
        
        let failed_tx = FailedTransaction {
            transaction_id: transaction_id.clone(),
            source_chain,
            receiver: receiver.clone(),
            amount,
            error: error.clone(),
            timestamp: env::block_timestamp(),
            retry_count: 0,
            last_retry: 0,
            token_type,
            token_address,
            bridge_id: Some(transaction_id.clone())
        };
        
        self.failed_transactions.insert(&transaction_id, &failed_tx);
        
        // Create a related bridge operation for tracking
        if !self.bridge_operations.contains_key(&transaction_id) {
            self.record_bridge_operation(
                transaction_id.clone(),
                receiver.to_string(),
                token_address.clone(),
                source_chain_id,
                receiver.to_string(),
                U128(amount),
                format!("FAILED: {}", error)
            );
        } else {
            // Update existing bridge operation
            self.update_bridge_status(&transaction_id, format!("FAILED: {}", error), false);
        }
        
        env::log_str(&format!(
            "Recorded failed transaction: {}, chain: {:?}, receiver: {}, amount: {}, error: {}",
            transaction_id, source_chain, receiver_str, amount, error
        ));
    }
    
    /**
     * @dev Retry a failed transaction
     * Updated to match bridge_interface.sol retry system
     */
    pub fn retry_failed_transaction(&mut self, transaction_id: String) -> bool {
        // Check if bridge is paused
        if self.admin_control.bridge_paused {
            env::log_str("Bridge is paused, cannot retry transactions");
            return false;
        }
        
        // Check if transaction exists
        if !self.failed_transactions.contains_key(&transaction_id) {
            env::log_str(&format!("Failed transaction not found: {}", transaction_id));
            return false;
        }
        
        // Get the transaction
        let mut failed_tx = self.failed_transactions.get(&transaction_id).unwrap();
        
        // Check if enough time passed since last retry
        if failed_tx.last_retry > 0 && env::block_timestamp() - failed_tx.last_retry < RETRY_TIMEOUT {
            env::log_str(&format!("Retry timeout not elapsed for transaction: {}", transaction_id));
            return false;
        }
        
        // Check if retry count exceeds max
        if failed_tx.retry_count >= self.admin_control.max_retry_attempts {
            env::log_str(&format!("Max retry attempts ({}) exceeded for transaction: {}", 
                self.admin_control.max_retry_attempts, transaction_id));
            return false;
        }
        
        // Update retry information
        failed_tx.retry_count += 1;
        failed_tx.last_retry = env::block_timestamp();
        self.failed_transactions.insert(&transaction_id, &failed_tx);
        
        // Update bridge operation status
        if let Some(bridge_id) = &failed_tx.bridge_id {
            self.update_bridge_status(bridge_id, STATUS_RETRYING.to_string(), false);
        }
        
        // Try to process the transaction
        let chain_id = match failed_tx.source_chain {
            DmdChain::Ethereum => LAYERZERO_CHAIN_ID_ETH,
            DmdChain::BSC => LAYERZERO_CHAIN_ID_BSC, 
            DmdChain::Polygon => LAYERZERO_CHAIN_ID_POLYGON,
            DmdChain::Avalanche => LAYERZERO_CHAIN_ID_AVALANCHE,
            DmdChain::Arbitrum => LAYERZERO_CHAIN_ID_ARBITRUM,
            DmdChain::Optimism => LAYERZERO_CHAIN_ID_OPTIMISM,
            DmdChain::Base => LAYERZERO_CHAIN_ID_BASE,
            DmdChain::Fantom => LAYERZERO_CHAIN_ID_FANTOM,
            DmdChain::Moonbeam => LAYERZERO_CHAIN_ID_MOONBEAM,
            DmdChain::Moonriver => LAYERZERO_CHAIN_ID_MOONRIVER,
            DmdChain::Solana => LAYERZERO_CHAIN_ID_SOLANA,
            _ => 0 // Unknown chain, should never happen
        };
        
        match self.try_unwrap_from_chain(chain_id, &failed_tx.receiver, failed_tx.amount, failed_tx.token_type) {
            Ok(_) => {
                // Mark transaction as processed
                let mut updated_tx = self.failed_transactions.get(&transaction_id).unwrap();
                // We keep the transaction record but mark it as successful
                updated_tx.error = "Retry successful".to_string();
                self.failed_transactions.insert(&transaction_id, &updated_tx);
                
                // Update bridge operation status
                if let Some(bridge_id) = &failed_tx.bridge_id {
                    self.update_bridge_status(bridge_id, STATUS_COMPLETED.to_string(), true);
                }
                
                env::log_str(&format!("Successfully retried transaction: {}", transaction_id));
                true
            },
            Err(error) => {
                // Update error message but keep transaction for future retry
                let mut updated_tx = self.failed_transactions.get(&transaction_id).unwrap();
                updated_tx.error = error.clone();
                self.failed_transactions.insert(&transaction_id, &updated_tx);
                
                // Update bridge operation status
                if let Some(bridge_id) = &failed_tx.bridge_id {
                    self.update_bridge_status(bridge_id, format!("RETRY_FAILED: {}", error), false);
                }
                
                env::log_str(&format!("Failed to retry transaction: {}, error: {}", transaction_id, error));
                false
            }
        }
    }
    
    /**
     * @dev Automatically retry multiple failed transactions
     * Updated to be more robust and compatible with bridge_interface.sol
     */
    pub fn auto_retry_failed_transactions(&mut self, limit: u64) -> u64 {
        if self.admin_control.bridge_paused {
            return 0;
        }
        
        let mut success_count: u64 = 0;
        let mut retry_count: u64 = 0;
        
        // Get all failed transactions
        let mut transactions: Vec<(String, FailedTransaction)> = Vec::new();
        
        // Collect all failed transactions
        for i in self.failed_transactions.keys() {
            if let Some(tx) = self.failed_transactions.get(&i) {
                transactions.push((i.clone(), tx));
            }
            
            // Limit the number of transactions to process
            if transactions.len() >= limit as usize {
                break;
            }
        }
        
        // Sort by timestamp (oldest first)
        transactions.sort_by(|a, b| a.1.timestamp.cmp(&b.1.timestamp));
        
        // Retry eligible transactions
        for (tx_id, tx) in transactions {
            // Skip if already retried too recently
            if tx.last_retry > 0 && env::block_timestamp() - tx.last_retry < RETRY_TIMEOUT {
                continue;
            }
            
            // Skip if max retries reached
            if tx.retry_count >= self.admin_control.max_retry_attempts {
                continue;
            }
            
            retry_count += 1;
            
            // Try to retry the transaction
            if self.retry_failed_transaction(tx_id) {
                success_count += 1;
            }
            
            // Don't exceed the limit
            if retry_count >= limit {
                break;
            }
        }
        
        env::log_str(&format!("Auto-retried {} transactions, {} successful", retry_count, success_count));
        
        success_count
    }
    
    /**
     * @dev Get failed transactions with pagination
     * Updated to be compatible with bridge_interface.sol
     */
    pub fn get_failed_transactions(&self, from_index: u64, limit: u64) -> Vec<FailedTransaction> {
        let keys: Vec<String> = self.failed_transactions.keys().collect();
        let total = keys.len() as u64;
        
        let from = from_index.min(total);
        let to = (from + limit).min(total);
        
        let mut result = Vec::new();
        
        for i in from..to {
            let key = &keys[i as usize];
            if let Some(tx) = self.failed_transactions.get(key) {
                result.push(tx);
            }
        }
        
        result
    }
    
    /**
     * @dev Get bridge operation by ID
     * Added to match bridge_interface.sol getBridgeOperation
     */
    pub fn get_bridge_operation_by_id(&self, bridge_id: String) -> Option<BridgeOperation> {
        self.bridge_operations.get(&bridge_id)
    }
    
    /**
     * @dev Helper to get chain from LayerZero ID
     */
    fn get_chain_from_id(&self, chain_id: u16) -> DmdChain {
        match chain_id {
            LAYERZERO_CHAIN_ID_ETH => DmdChain::Ethereum,
            LAYERZERO_CHAIN_ID_BSC => DmdChain::BSC,
            LAYERZERO_CHAIN_ID_POLYGON => DmdChain::Polygon,
            LAYERZERO_CHAIN_ID_AVALANCHE => DmdChain::Avalanche,
            LAYERZERO_CHAIN_ID_ARBITRUM => DmdChain::Arbitrum,
            LAYERZERO_CHAIN_ID_OPTIMISM => DmdChain::Optimism,
            LAYERZERO_CHAIN_ID_BASE => DmdChain::Base,
            LAYERZERO_CHAIN_ID_FANTOM => DmdChain::Fantom,
            LAYERZERO_CHAIN_ID_MOONBEAM => DmdChain::Moonbeam,
            LAYERZERO_CHAIN_ID_MOONRIVER => DmdChain::Moonriver,
            LAYERZERO_CHAIN_ID_SOLANA => DmdChain::Solana,
            _ => DmdChain::NEAR
        }
    }
    
    /**
     * @dev Check if chain is supported
     */
    fn is_chain_supported(&self, chain_id: u16) -> bool {
        let chain = self.get_chain_from_id(chain_id);
        self.admin_control.supported_chains.contains(&chain)
    }
    
    /**
     * @dev Validate destination address format
     */
    fn validate_destination_address(&self, chain_id: u16, address: &str) {
        match chain_id {
            LAYERZERO_CHAIN_ID_ETH | LAYERZERO_CHAIN_ID_BSC | LAYERZERO_CHAIN_ID_POLYGON | 
            LAYERZERO_CHAIN_ID_AVALANCHE | LAYERZERO_CHAIN_ID_ARBITRUM | LAYERZERO_CHAIN_ID_OPTIMISM |
            LAYERZERO_CHAIN_ID_BASE | LAYERZERO_CHAIN_ID_FANTOM | LAYERZERO_CHAIN_ID_MOONBEAM |
            LAYERZERO_CHAIN_ID_MOONRIVER => {
                // EVM address validation - should be 0x + 40 hex chars
                assert!(address.starts_with("0x"), "EVM address must start with 0x");
                assert!(address.len() == 42, "EVM address must be 42 characters long");
                
                // Check if hex after 0x
                for c in address[2..].chars() {
                    assert!(c.is_digit(16), "EVM address contains invalid characters");
                }
            },
            LAYERZERO_CHAIN_ID_SOLANA => {
                // Basic Solana address validation
                assert!(address.len() >= 32, "Solana address too short");
                assert!(address.len() <= 44, "Solana address too long");
            },
            LAYERZERO_CHAIN_ID_NEAR => {
                // NEAR account validation
                assert!(AccountId::try_from(address.to_string()).is_ok(), "Invalid NEAR account ID");
            },
            _ => {
                assert!(false, "Unsupported chain for address validation");
            }
        }
    }
    
    /// Wraps tokens to a specified chain
    /// This function locks tokens on NEAR and initiates a bridge to the specified chain
    /// @param chain_id - The LayerZero chain ID of the destination chain
    /// @param receiver - The address on the destination chain that will receive the tokens
    /// @param amount - The amount of tokens to bridge
    pub fn wrap_to_chain(&mut self, chain_id: u16, receiver: String, amount: U128) {
        // Check if bridge is paused
        require!(!self.admin_control.bridge_paused, "Bridge is currently paused");
        
        // Check if chain is supported
        require!(self.is_chain_supported(chain_id), "Destination chain not supported");
        
        // Validate transaction amount is within limits
        require!(
            amount.0 <= self.admin_control.transaction_limit,
            "Transaction exceeds daily limit"
        );
        
        // Check daily rate limit
        let current_day = env::block_timestamp() / (24 * 60 * 60 * 1_000_000_000);
        let day_key = format!("day:{}", current_day);
        let daily_volume = self.daily_volumes.get(&day_key).unwrap_or(0);
        
        require!(
            daily_volume + amount.0 <= self.admin_control.daily_limit,
            "Daily limit exceeded"
        );
        
        // Update daily volume
        self.daily_volumes.insert(&day_key, &(daily_volume + amount.0));
        
        // Get sender account
        let sender = env::predecessor_account_id();
        
        // Validate receiver address format based on chain
        self.validate_destination_address(chain_id, &receiver);
        
        // Check if sender has enough balance
        let sender_balance = self.accounts.get(&sender).unwrap_or(0);
        require!(
            sender_balance >= amount.0,
            format!("Insufficient balance: {} < {}", sender_balance, amount.0)
        );
        
        // Update sender balance
        self.accounts.insert(&sender, &(sender_balance - amount.0));
        
        // Update locked tokens for the destination chain
        let mut locked_tokens = self.locked_tokens.clone();
        match self.get_chain_from_id(chain_id) {
            DmdChain::Ethereum => locked_tokens.ethereum += amount.0,
            DmdChain::BSC => locked_tokens.bsc += amount.0,
            DmdChain::Polygon => locked_tokens.polygon += amount.0,
            DmdChain::Avalanche => locked_tokens.avalanche += amount.0,
            DmdChain::Arbitrum => locked_tokens.arbitrum += amount.0,
            DmdChain::Optimism => locked_tokens.optimism += amount.0,
            DmdChain::Base => locked_tokens.base += amount.0,
            DmdChain::Fantom => locked_tokens.fantom += amount.0,
            DmdChain::Moonbeam => locked_tokens.moonbeam += amount.0,
            DmdChain::Moonriver => locked_tokens.moonriver += amount.0,
            DmdChain::Solana => locked_tokens.solana += amount.0,
            _ => {
                let chain_name = format!("chain_{}", chain_id);
                let current = locked_tokens.other_chains.get(&chain_name).unwrap_or(0);
                locked_tokens.other_chains.insert(&chain_name, &(current + amount.0));
            }
        }
        self.locked_tokens = locked_tokens;
        
        // Increment nonce
        self.current_nonce += 1;
        let nonce = self.current_nonce;
        
        let timestamp = env::block_timestamp();
        
        // Generate bridge ID (hash of sender, chain ID, amount, nonce, timestamp)
        let bridge_id = self.generate_bridge_id(
            &sender.to_string(),
            chain_id,
            amount.0,
            nonce,
            timestamp
        );
        
        // Record the bridge operation
        self.record_bridge_operation(
            bridge_id.clone(),
            sender.to_string(),
            None, // No token address for native DMD
            chain_id,
            receiver.clone(),
            amount,
            "Initiated".to_string()
        );
        
        // Create a bridge message for history
        let bridge_message = BridgeMessage {
            source_chain: DmdChain::NEAR,
            destination_chain: self.get_chain_from_id(chain_id),
            sender: sender.to_string(),
            receiver: receiver.clone(),
            amount,
            timestamp,
            nonce,
            signature: None,
            status: TransactionStatus::Pending,
            token_type: TOKEN_TYPE_ERC1155, // Using ERC1155 as token type
            token_address: None,            // No token address for native DMD
            bridge_id: Some(bridge_id.clone()),
        };
        
        self.bridge_history.insert(&bridge_id, &bridge_message);
        
        // Create payload for LayerZero that matches the format in bridge_interface.sol
        // This format aligns with the BridgePayload struct in bridge_interface.sol
        let payload = format!(
            "{{\"bridgeId\":\"{}\",\"sender\":\"{}\",\"destination\":\"{}\",\"amount\":\"{}\",\"timestamp\":{},\"nonce\":{},\"token_type\":{},\"token_address\":null,\"unwrapper\":null,\"need_unwrap\":false}}",
            bridge_id, sender, receiver, amount.0, timestamp, nonce, TOKEN_TYPE_ERC1155
        );
        
        env::log_str(&format!(
            "Initiating bridge to chain {} for {} tokens to receiver {} with nonce {}, bridge ID {}",
            chain_id, amount.0, receiver, nonce, bridge_id
        ));
        
        // In a production implementation, we would make a cross-contract call to the LayerZero endpoint here
        // The cross-contract call would include the destination chain ID, trusted remote, payload, and fees
        env::log_str(&format!("LayerZero payload: {}", payload));
    }
}
