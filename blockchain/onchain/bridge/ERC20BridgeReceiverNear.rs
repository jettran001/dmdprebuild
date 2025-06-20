use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, UnorderedMap, Vector};
use near_sdk::json_types::{U128, U64};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, log, near_bindgen, AccountId, Balance, PanicOnDefault, Promise, PromiseOrValue};
use near_sdk::{serde_json, Gas, Timestamp};
use std::collections::HashMap;

// LayerZero chain IDs (đồng bộ với bridge_interface.sol)
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

// Token types
const TOKEN_TYPE_ERC20: u8 = 1;

// Bridge operation status
const STATUS_PENDING: &str = "Pending";
const STATUS_COMPLETED: &str = "Completed";
const STATUS_FAILED: &str = "Failed";

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

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct BridgePayload {
    pub sender: String,
    pub destination: String,
    pub amount: U128,
    pub timestamp: u64,
    pub nonce: u64,
    pub token_address: Option<String>,
    pub token_type: u8,
    pub extra_data: Option<String>,
    pub bridge_id: Option<String>,
    pub unwrapper: Option<String>,
    pub need_unwrap: Option<bool>,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct BridgeOperation {
    pub bridge_id: String,
    pub sender: String,
    pub token_address: Option<String>,
    pub source_chain_id: u16,
    pub destination_address: String,
    pub amount: U128,
    pub timestamp: u64,
    pub completed: bool,
    pub status: String,
}

/// ERC20BridgeReceiverNear contract
/// Handles receiving ERC20 tokens from other chains and manages corresponding minting
/// on the NEAR blockchain
#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct ERC20BridgeReceiver {
    /// Contract owner
    owner_id: AccountId,
    
    /// Admin accounts with special permissions
    admins: Vec<AccountId>,
    
    /// Mapping of source chain IDs to their trusted source addresses
    trusted_sources: LookupMap<u16, String>,
    
    /// Mapping of chain IDs to whether they are supported
    supported_chains: LookupMap<u16, bool>,
    
    /// Token contract that will be minted/burned
    token_contract: AccountId,
    
    /// Mapping of transaction IDs to whether they have been processed
    processed_txs: LookupMap<String, bool>,
    
    /// Maximum amount allowed per transaction
    max_transaction_amount: Balance,
    
    /// Daily limit for transactions from each chain
    daily_limit: Balance,
    
    /// Tracking daily volumes for each chain
    daily_volumes: LookupMap<String, Balance>, // key: chain_id + day
    
    /// Failed transactions that couldn't be processed
    failed_transactions: UnorderedMap<u64, FailedTransaction>,
    
    /// The next available ID for failed transactions
    next_failed_tx_id: u64,
    
    /// Failed transaction IDs by receiver
    failed_txs_by_receiver: LookupMap<AccountId, Vec<u64>>,
    
    /// Locked token amounts by chain ID
    locked_by_chain: LookupMap<u16, Balance>,
    
    /// Whether the bridge is paused
    paused: bool,
    
    /// Nonce for each chain to prevent replay attacks
    processed_nonces: LookupMap<String, bool>, // chain_id + nonce
    
    /// Tracking operations by ID
    bridge_operations: LookupMap<Vec<u8>, BridgeOperation>,
    
    /// List of all operation IDs
    all_operations: Vector<Vec<u8>>,
    
    /// User operations
    user_operations: LookupMap<AccountId, Vec<Vec<u8>>>,
}

/// Structure for tracking failed transactions
#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct FailedTransaction {
    /// Source chain ID
    pub source_chain_id: u16,
    
    /// Receiver account ID
    pub receiver: AccountId,
    
    /// Token amount
    pub amount: Balance,
    
    /// Transaction ID or nonce from source chain
    pub tx_id: String,
    
    /// Error message describing why it failed
    pub error: String,
    
    /// Timestamp when transaction was received
    pub timestamp: Timestamp,
    
    /// Number of retry attempts
    pub retry_count: u8,
    
    /// Last retry timestamp
    pub last_retry: Timestamp,
}

#[near_bindgen]
impl ERC20BridgeReceiver {
    /// Initialize contract with default values
    #[init]
    pub fn new(
        owner_id: AccountId,
        token_contract: AccountId,
        max_transaction_amount: U128,
        daily_limit: U128,
    ) -> Self {
        let mut this = Self {
            owner_id,
            admins: Vec::new(),
            trusted_sources: LookupMap::new(b"t_sources"),
            supported_chains: LookupMap::new(b"s_chains"),
            token_contract,
            processed_txs: LookupMap::new(b"p_txs"),
            max_transaction_amount: max_transaction_amount.0,
            daily_limit: daily_limit.0,
            daily_volumes: LookupMap::new(b"d_volumes"),
            failed_transactions: UnorderedMap::new(b"f_txs"),
            next_failed_tx_id: 0,
            failed_txs_by_receiver: LookupMap::new(b"f_txs_rcv"),
            locked_by_chain: LookupMap::new(b"l_chain"),
            paused: false,
            processed_nonces: LookupMap::new(b"p_nonces"),
            bridge_operations: LookupMap::new(b"b_ops"),
            all_operations: Vector::new(b"all_ops"),
            user_operations: LookupMap::new(b"user_ops"),
        };
        this
    }
    
    /// Only owner modifier
    fn assert_owner(&self) {
        assert_eq!(env::predecessor_account_id(), self.owner_id, "Owner only");
    }
    
    /// Admin or owner modifier
    fn assert_admin_or_owner(&self) {
        let caller = env::predecessor_account_id();
        assert!(
            caller == self.owner_id || self.admins.contains(&caller),
            "Admin or owner only"
        );
    }
    
    /// Check if bridge is not paused
    fn assert_not_paused(&self) {
        assert!(!self.paused, "Bridge is paused");
    }
    
    /// Add an admin
    pub fn add_admin(&mut self, admin_id: AccountId) {
        self.assert_owner();
        if !self.admins.contains(&admin_id) {
            self.admins.push(admin_id);
        }
    }
    
    /// Remove an admin
    pub fn remove_admin(&mut self, admin_id: &AccountId) {
        self.assert_owner();
        self.admins.retain(|id| id != admin_id);
    }
    
    /// Set trusted source for a chain
    pub fn set_trusted_source(&mut self, chain_id: u16, source_address: String) {
        self.assert_admin_or_owner();
        self.trusted_sources.insert(&chain_id, &source_address);
    }
    
    /// Set whether a chain is supported
    pub fn set_chain_support(&mut self, chain_id: u16, supported: bool) {
        self.assert_admin_or_owner();
        self.supported_chains.insert(&chain_id, &supported);
    }
    
    /// Pause/unpause the bridge
    pub fn set_paused(&mut self, paused: bool) {
        self.assert_admin_or_owner();
        self.paused = paused;
    }
    
    /// Update transaction limits
    pub fn update_limits(&mut self, max_transaction: U128, daily_limit: U128) {
        self.assert_admin_or_owner();
        self.max_transaction_amount = max_transaction.0;
        self.daily_limit = daily_limit.0;
    }
    
    /// Receive bridge data from LayerZero
    /// This function is called by the LayerZero endpoint when receiving cross-chain messages
    pub fn lz_receive(&mut self, source_chain_id: u16, source_address: String, payload: String, nonce: U64) -> PromiseOrValue<bool> {
        self.assert_not_paused();
        
        // Kiểm tra trusted source
        if !self.trusted_sources.contains_key(&source_chain_id) {
            env::log_str(&format!("Untrusted source chain: {}", source_chain_id));
            return PromiseOrValue::Value(false);
        }
        
        let trusted_source = self.trusted_sources.get(&source_chain_id).unwrap();
        if trusted_source != source_address {
            env::log_str(&format!("Untrusted source address: {} expected: {}", source_address, trusted_source));
            return PromiseOrValue::Value(false);
        }

        // Cải thiện kiểm tra nonce theo chuỗi
        let nonce_key = format!("{}:{}", source_chain_id, nonce.0);
        if self.processed_nonces.contains_key(&nonce_key) {
            env::log_str(&format!("Nonce already processed: {}", nonce_key));
            return PromiseOrValue::Value(false);
        }
        
        // Parse payload - thêm xử lý cho định dạng mới
        let bridge_payload = match self.parse_payload(&payload) {
            Ok(p) => p,
            Err(e) => {
                env::log_str(&format!("Failed to parse payload: {}", e));
                return PromiseOrValue::Value(false);
            }
        };
        
        // Kiểm tra thời gian hết hạn
        let current_timestamp = env::block_timestamp();
        let expiry_window: u64 = 3600 * 1_000_000_000; // 1 hour
        if current_timestamp > bridge_payload.timestamp + expiry_window {
            env::log_str("Transaction expired");
            return PromiseOrValue::Value(false);
        }

        // Validate receiver
        let receiver_str = bridge_payload.destination.clone();
        let receiver_result = AccountId::try_from(receiver_str.clone());
        if receiver_result.is_err() {
            env::log_str(&format!("Invalid receiver account ID: {}", receiver_str));
            return PromiseOrValue::Value(false);
        }
        let receiver = receiver_result.unwrap();
        
        // Validate amount
        let amount = bridge_payload.amount.0;
        if amount == 0 {
            env::log_str("Amount cannot be zero");
            return PromiseOrValue::Value(false);
        }
        
        if amount > self.max_transaction_amount {
            env::log_str(&format!("Amount exceeds max transaction limit: {} > {}", amount, self.max_transaction_amount));
            return PromiseOrValue::Value(false);
        }
        
        // Check daily limit
        let current_day = current_timestamp / (24 * 60 * 60 * 1_000_000_000);
        let daily_key = format!("{}:{}", source_chain_id, current_day);
        let mut daily_volume = self.daily_volumes.get(&daily_key).unwrap_or(0);
        
        if daily_volume + amount > self.daily_limit {
            env::log_str(&format!("Daily limit exceeded: {} + {} > {}", daily_volume, amount, self.daily_limit));
            return PromiseOrValue::Value(false);
        }
        
        // Update daily volume
        daily_volume += amount;
        self.daily_volumes.insert(&daily_key, &daily_volume);
        
        // Tạo bridge_id nhất quán với bridge_interface.sol
        let bridge_id = if let Some(id) = bridge_payload.bridge_id.clone() {
            id
        } else {
            // Tạo ID mới nếu không có sẵn
            self.create_bridge_id(
                &bridge_payload.sender,
                source_chain_id,
                &receiver,
                amount,
                bridge_payload.timestamp,
                nonce.0
            )
        };
        
        // Ghi nhận bridge operation trước khi mint
        self.record_bridge_operation(
            bridge_id.clone().into_bytes(),
            receiver.clone(),
            self.token_contract.clone(),
            source_chain_id,
            receiver_str.clone().into_bytes(),
            amount,
            "PENDING".to_string()
        );
        
        // Đánh dấu nonce đã được xử lý
        self.processed_nonces.insert(&nonce_key, &true);
        
        // Tạo transaction ID
        let tx_id = format!("{}:{}:{}", source_chain_id, nonce.0, current_timestamp);
        if self.processed_txs.contains_key(&tx_id) {
            env::log_str(&format!("Transaction already processed: {}", tx_id));
            return PromiseOrValue::Value(false);
        }
        
        // Đánh dấu transaction đã được xử lý
        self.processed_txs.insert(&tx_id, &true);
        
        // Mint tokens cho receiver
        let mint_promise = self.mint_tokens(
            receiver.clone(),
            amount,
            bridge_id.clone().into_bytes(),
            source_chain_id,
            nonce.0
        );
        
        // Kiểm tra nếu cần auto-unwrap
        let need_unwrap = bridge_payload.need_unwrap.unwrap_or(false);
        if need_unwrap {
            env::log_str(&format!("Auto-unwrap requested for {} tokens to {}", amount, receiver));
            // Thêm logic auto-unwrap ở đây nếu cần
            // Ví dụ: Thực hiện cross-contract call đến contract unwrap
        }
        
        // Trả về callback để xử lý kết quả
        PromiseOrValue::Promise(mint_promise.then(
            Self::ext(env::current_account_id())
                .with_static_gas(Gas(5_000_000_000_000))
                .on_mint_callback(
                    receiver.clone(),
                    U128(amount),
                    bridge_id.clone().into_bytes(),
                    source_chain_id,
                    U64(nonce.0)
                )
        ))
    }
    
    /// Create a unique bridge operation ID
    fn create_bridge_id(
        &self,
        sender: &str,
        source_chain_id: u16,
        receiver: &AccountId,
        amount: Balance,
        timestamp: u64,
        nonce: u64,
    ) -> Vec<u8> {
        // Combine all elements to create a unique identifier
        let mut data = Vec::new();
        data.extend_from_slice(sender.as_bytes());
        data.extend_from_slice(&source_chain_id.to_le_bytes());
        data.extend_from_slice(receiver.as_bytes());
        data.extend_from_slice(&amount.to_le_bytes());
        data.extend_from_slice(&timestamp.to_le_bytes());
        data.extend_from_slice(&nonce.to_le_bytes());
        
        // Use NEAR's hash function
        env::sha256(&data)
    }
    
    /// Record bridge operation for tracking
    pub fn record_bridge_operation(
        &mut self,
        bridge_id: Vec<u8>,
        sender: AccountId,
        token: AccountId,
        destination_chain_id: u16,
        destination_address: Vec<u8>,
        amount: Balance,
        status: String,
    ) {
        let operation = BridgeOperation {
            bridge_id: base64::encode(&bridge_id),
            sender: sender.clone(),
            token: token.clone(),
            source_chain_id: destination_chain_id,
            destination_address: base64::encode(&destination_address),
            amount: U128(amount),
            timestamp: env::block_timestamp(),
            completed: false,
            status,
        };
        
        // Store the operation
        self.bridge_operations.insert(&bridge_id, &operation);
        self.all_operations.push(&bridge_id);
        
        // Add to user operations
        let mut user_ops = self.user_operations.get(&sender).unwrap_or_else(|| Vec::new());
        user_ops.push(bridge_id);
        self.user_operations.insert(&sender, &user_ops);
        
        log!("Bridge operation recorded: {}", base64::encode(&bridge_id));
    }
    
    /// Update bridge operation status
    pub fn update_bridge_status(&mut self, bridge_id: Vec<u8>, status: String) {
        let mut operation = self.bridge_operations.get(&bridge_id).expect("Bridge operation not found");
        operation.status = status;
        
        if status == "COMPLETED" {
            operation.completed = true;
        }
        
        self.bridge_operations.insert(&bridge_id, &operation);
        log!("Bridge status updated: {} -> {}", base64::encode(&bridge_id), status);
    }
    
    /// Get bridge operation details
    pub fn get_bridge_operation(&self, bridge_id: Vec<u8>) -> Option<BridgeOperation> {
        self.bridge_operations.get(&bridge_id)
    }
    
    /// Mint tokens to receiver (cross-contract call to token contract)
    fn mint_tokens(
        &mut self, 
        receiver: AccountId, 
        amount: Balance, 
        bridge_id: Vec<u8>,
        source_chain_id: u16,
        nonce: u64
    ) -> Promise {
        // Cross-contract call to token contract mint function
        // This is a simplified version - actual implementation would depend on token contract interface
        let mint_call = Promise::new(self.token_contract.clone())
            .function_call(
                "mint".to_string(),
                serde_json::to_vec(&json!({
                    "account_id": receiver.to_string(),
                    "amount": U128(amount)
                })).unwrap(),
                0,
                Gas(30_000_000_000_000)
            );
            
        // Set up callback for handling the result
        mint_call.then(
            Self::ext(env::current_account_id())
                .with_static_gas(Gas(30_000_000_000_000))
                .on_mint_callback(
                    receiver.clone(),
                    U128(amount),
                    bridge_id.clone(),
                    source_chain_id,
                    U64(nonce)
                )
        )
    }
    
    /// Callback after mint attempt
    #[private]
    pub fn on_mint_callback(
        &mut self,
        receiver: AccountId,
        amount: U128,
        bridge_id: Vec<u8>,
        source_chain_id: u16,
        nonce: U64,
    ) -> bool {
        // Xác minh callback đến từ chính hợp đồng này
        assert_eq!(
            env::predecessor_account_id(),
            env::current_account_id(),
            "Callback can only be called from the contract itself"
        );
        
        // Lấy kết quả từ promise gọi trước đó
        let is_success = match env::promise_result(0) {
            PromiseResult::Successful(_) => true,
            _ => false,
        };
        
        // Cập nhật trạng thái của bridge operation
        if is_success {
            self.update_bridge_status(bridge_id.clone(), "COMPLETED".to_string());
            env::log_str(&format!(
                "Successfully minted {} tokens to {} from chain {} with nonce {}",
                amount.0, receiver, source_chain_id, nonce.0
            ));
        } else {
            self.update_bridge_status(bridge_id.clone(), "FAILED".to_string());
            
            // Ghi lại giao dịch thất bại để thử lại sau
            let tx_id = format!("{}:{}", source_chain_id, nonce.0);
            self.record_failed_transaction(
                source_chain_id,
                receiver.to_string(),
                amount.0,
                tx_id,
                "Mint failed".to_string()
            );
            
            env::log_str(&format!(
                "Failed to mint {} tokens to {} from chain {} with nonce {}",
                amount.0, receiver, source_chain_id, nonce.0
            ));
        }
        
        is_success
    }
    
    /// Record a failed transaction
    pub fn record_failed_transaction(
        &mut self,
        source_chain_id: u16,
        receiver_str: String,
        amount: Balance,
        tx_id: String,
        error: String,
    ) -> u64 {
        let receiver = match AccountId::try_from(receiver_str.clone()) {
            Ok(account) => account,
            Err(_) => AccountId::new_unchecked("system.near".to_string()), // Use a system account for invalid receivers
        };
        
        let failed_tx = FailedTransaction {
            source_chain_id,
            receiver: receiver.clone(),
            amount,
            tx_id,
            error,
            timestamp: env::block_timestamp(),
            retry_count: 0,
            last_retry: 0,
        };
        
        let tx_id = self.next_failed_tx_id;
        self.next_failed_tx_id += 1;
        
        // Store failed transaction
        self.failed_transactions.insert(&tx_id, &failed_tx);
        
        // Add to receiver's failed transactions
        let mut receiver_failed_txs = self.failed_txs_by_receiver.get(&receiver).unwrap_or_else(|| Vec::new());
        receiver_failed_txs.push(tx_id);
        self.failed_txs_by_receiver.insert(&receiver, &receiver_failed_txs);
        
        log!("Recorded failed transaction #{} for {}", tx_id, receiver);
        tx_id
    }
    
    /// Retry a failed transaction
    pub fn retry_failed_transaction(&mut self, tx_id: u64) -> PromiseOrValue<bool> {
        self.assert_not_paused();
        self.assert_admin_or_owner();
        
        // Get failed transaction
        let mut failed_tx = self.failed_transactions.get(&tx_id).expect("Failed transaction not found");
        
        // Check if enough time has passed since last retry
        let current_time = env::block_timestamp();
        let retry_window = 600 * 1_000_000_000; // 10 minutes in nanoseconds
        
        if failed_tx.last_retry > 0 && current_time < failed_tx.last_retry + retry_window {
            log!("Too soon to retry. Please wait at least 10 minutes between retries.");
            return PromiseOrValue::Value(false);
        }
        
        // Check if max retries reached
        if failed_tx.retry_count >= 5 {
            log!("Maximum retry attempts (5) reached for transaction #{}", tx_id);
            return PromiseOrValue::Value(false);
        }
        
        // Update retry info
        failed_tx.retry_count += 1;
        failed_tx.last_retry = current_time;
        self.failed_transactions.insert(&tx_id, &failed_tx);
        
        // Create bridge ID for this retry
        let bridge_id = self.create_bridge_id(
            "retry",
            failed_tx.source_chain_id,
            &failed_tx.receiver,
            failed_tx.amount,
            current_time,
            tx_id,
        );
        
        // Record bridge operation for this retry
        self.record_bridge_operation(
            bridge_id.clone(),
            failed_tx.receiver.clone(),
            self.token_contract.clone(),
            failed_tx.source_chain_id,
            "retry".as_bytes().to_vec(),
            failed_tx.amount,
            "RETRY".to_string(),
        );
        
        // Try to mint tokens again
        self.mint_tokens(
            failed_tx.receiver.clone(),
            failed_tx.amount,
            bridge_id,
            failed_tx.source_chain_id,
            tx_id,
        );
        
        PromiseOrValue::Value(true)
    }
    
    /// Get list of failed transactions
    pub fn get_failed_transactions(&self, from_index: u64, limit: u64) -> Vec<(u64, FailedTransaction)> {
        let mut result = Vec::new();
        let total = self.next_failed_tx_id;
        
        let from = from_index.min(total);
        let to = (from + limit).min(total);
        
        for i in from..to {
            if let Some(tx) = self.failed_transactions.get(&i) {
                result.push((i, tx));
            }
        }
        
        result
    }
    
    /// Get failed transactions for a specific receiver
    pub fn get_user_failed_transactions(&self, receiver: AccountId) -> Vec<(u64, FailedTransaction)> {
        let tx_ids = self.failed_txs_by_receiver.get(&receiver).unwrap_or_else(|| Vec::new());
        let mut result = Vec::new();
        
        for tx_id in tx_ids {
            if let Some(tx) = self.failed_transactions.get(&tx_id) {
                result.push((tx_id, tx));
            }
        }
        
        result
    }
    
    /// Get bridge operations for a specific user
    pub fn get_user_bridge_operations(&self, user: AccountId) -> Vec<(Vec<u8>, BridgeOperation)> {
        let op_ids = self.user_operations.get(&user).unwrap_or_else(|| Vec::new());
        let mut result = Vec::new();
        
        for op_id in op_ids {
            if let Some(op) = self.bridge_operations.get(&op_id) {
                result.push((op_id, op));
            }
        }
        
        result
    }
    
    /// Get all bridge operations with pagination
    pub fn get_all_bridge_operations(&self, from_index: u64, limit: u64) -> Vec<(Vec<u8>, BridgeOperation)> {
        let mut result = Vec::new();
        let total = self.all_operations.len();
        
        let from = from_index.min(total);
        let to = (from + limit).min(total);
        
        for i in from..to {
            let op_id = self.all_operations.get(i).unwrap();
            if let Some(op) = self.bridge_operations.get(&op_id) {
                result.push((op_id, op));
            }
        }
        
        result
    }

    // Hàm để xử lý các định dạng payload khác nhau từ bridge_interface.sol
    fn parse_payload(&self, payload: &str) -> Result<BridgePayload, String> {
<<<<<<< HEAD
        // Rate limit: chỉ cho phép tối đa 5 lần parse thất bại mỗi block
        let block_index = env::block_index();
        let key = format!("parse_failures:{}", block_index);
        let mut fail_count = env::storage_read(key.as_bytes()).map(|v| u64::from_le_bytes(v.try_into().unwrap())).unwrap_or(0);
        if fail_count > 5 {
            env::log_str("[SECURITY] Too many payload parse failures in this block, possible attack detected");
            return Err("Rate limit exceeded for payload parsing".to_string());
        }
        // Try parse as new format (with checksum/integrity)
        match near_sdk::serde_json::from_str::<BridgePayload>(payload) {
            Ok(parsed) => {
                // Validate integrity if bridge_id or checksum exists
                if let Some(ref bridge_id) = parsed.bridge_id {
                    if bridge_id.len() < 8 {
                        env::log_str("[SECURITY] Invalid bridge_id length in payload");
                        fail_count += 1;
                        env::storage_write(key.as_bytes(), &fail_count.to_le_bytes());
                        return Err("Invalid bridge_id length".to_string());
                    }
                }
                // (Nếu có trường checksum, validate checksum ở đây)
                // Log success
                env::log_str("[INFO] Parsed payload as new format (BridgePayload)");
                return Ok(parsed);
            },
            Err(json_err) => {
                env::log_str(&format!("[WARN] Failed to parse as new format: {}, trying legacy format", json_err));
                // Try legacy format
                let legacy_result = near_sdk::serde_json::from_str::<LegacyPayload>(payload);
                match legacy_result {
                    Ok(legacy) => {
                        env::log_str("[INFO] Parsed payload as legacy format");
=======
        // Thử parse theo định dạng JSON mới
        match near_sdk::serde_json::from_str::<BridgePayload>(payload) {
            Ok(parsed) => Ok(parsed),
            Err(json_err) => {
                // Nếu không parse được theo định dạng mới, thử định dạng cũ
                env::log_str(&format!("Failed to parse as new format: {}, trying legacy format", json_err));
                
                // Parse legacy format
                let legacy_result = near_sdk::serde_json::from_str::<LegacyPayload>(payload);
                match legacy_result {
                    Ok(legacy) => {
                        // Chuyển đổi từ định dạng cũ sang mới
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
                        Ok(BridgePayload {
                            sender: legacy.sender,
                            destination: legacy.destination,
                            amount: legacy.amount,
                            timestamp: legacy.timestamp,
                            nonce: legacy.nonce,
                            token_address: None,
<<<<<<< HEAD
                            token_type: TOKEN_TYPE_ERC20, // Default ERC20
=======
                            token_type: TOKEN_TYPE_ERC20, // Mặc định là ERC20
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
                            extra_data: None,
                            bridge_id: None,
                            unwrapper: None,
                            need_unwrap: None,
                        })
                    },
                    Err(legacy_err) => {
<<<<<<< HEAD
                        fail_count += 1;
                        env::storage_write(key.as_bytes(), &fail_count.to_le_bytes());
                        env::log_str(&format!("[SECURITY] Failed to parse payload in any known format. New format error: {}, Legacy format error: {}", json_err, legacy_err));
=======
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
                        Err(format!("Failed to parse payload in any known format. New format error: {}, Legacy format error: {}", json_err, legacy_err))
                    }
                }
            }
        }
    }
    
    // Định nghĩa struct cho định dạng cũ để hỗ trợ backward compatibility
    #[derive(Deserialize)]
    #[serde(crate = "near_sdk::serde")]
    struct LegacyPayload {
        pub sender: String,
        pub destination: String,
        pub amount: U128,
        pub timestamp: u64,
        pub nonce: u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::{testing_env, Balance, VMContext};
    
    const NEAR: Balance = 1_000_000_000_000_000_000_000_000;
    
    fn get_context(predecessor_account_id: AccountId) -> VMContext {
        let mut builder = VMContextBuilder::new();
        builder
            .current_account_id(accounts(0))
            .signer_account_id(predecessor_account_id.clone())
            .predecessor_account_id(predecessor_account_id);
        builder.build()
    }
    
    #[test]
    fn test_new() {
        let context = get_context(accounts(1));
        testing_env!(context);
        
        let contract = ERC20BridgeReceiver::new(
            accounts(1),
            accounts(2),
            U128(1000 * NEAR),
            U128(10000 * NEAR),
        );
        
        assert_eq!(contract.owner_id, accounts(1));
        assert_eq!(contract.token_contract, accounts(2));
        assert_eq!(contract.max_transaction_amount, 1000 * NEAR);
        assert_eq!(contract.daily_limit, 10000 * NEAR);
        assert_eq!(contract.paused, false);
    }
    
    #[test]
    fn test_admin_functions() {
        let mut context = get_context(accounts(1));
        testing_env!(context.clone());
        
        let mut contract = ERC20BridgeReceiver::new(
            accounts(1),
            accounts(2),
            U128(1000 * NEAR),
            U128(10000 * NEAR),
        );
        
        // Test adding admin
        contract.add_admin(accounts(3));
        assert!(contract.admins.contains(&accounts(3)));
        
        // Test removing admin
        contract.remove_admin(&accounts(3));
        assert!(!contract.admins.contains(&accounts(3)));
        
        // Test setting trusted source
        contract.set_trusted_source(1, "0x123456".to_string());
        assert_eq!(contract.trusted_sources.get(&1).unwrap(), "0x123456");
        
        // Test chain support
        contract.set_chain_support(1, true);
        assert!(contract.supported_chains.get(&1).unwrap());
        
        // Test pausing
        contract.set_paused(true);
        assert!(contract.paused);
        
        // Test updating limits
        contract.update_limits(U128(2000 * NEAR), U128(20000 * NEAR));
        assert_eq!(contract.max_transaction_amount, 2000 * NEAR);
        assert_eq!(contract.daily_limit, 20000 * NEAR);
    }
}
